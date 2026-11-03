//! Ending sessions in the committed revocation order (spec B.4 rev 4/5).
//!
//! Inside the caller's transaction, after its locks and re-checks:
//!
//! 1. `UPDATE sessions SET ended_at, end_reason` (the row is kept, DC4);
//! 2. the session's elevation rows are deleted;
//! 3. each id is written to the Valkey denylist (TTL ≥ the access-JWT lifetime) — a failed write is
//!    logged and never stops the transition, since the commit is what ends the session;
//! 4. `pg_notify("kaiku_session_ended", …)`, delivered only on commit (DC5);
//! 5. the caller commits.
//!
//! After commit, [`spawn_session_end_listener`] republishes the notice as
//! [`SessionNotice`] on `user:{id}`, which closes the sessions' WebSocket and
//! voice connections; their ≤ 60 s re-check of the committed row is the
//! backstop. A denylist entry whose transaction rolls back only over-revokes
//! until it expires — the safe direction.

use std::time::Duration;

use fred::prelude::*;
use sqlx::postgres::PgListener;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use super::ctx::AuthCtx;
use super::error::AuthResult;

/// Denylist TTL beyond the access-token lifetime.
const DENYLIST_LEEWAY_SECS: i64 = 60;

/// The `NOTIFY` channel of ended sessions. Payload:
/// `{"user_id": uuid, "session_ids": [uuid], "all": bool}`, at most
/// [`NOTIFY_CHUNK`] ids per notification (several for a larger end; `all`
/// on the first).
pub const SESSION_END_CHANNEL: &str = "kaiku_session_ended";

/// Session ids per `NOTIFY` payload: `PostgreSQL` limits a payload to 8000
/// bytes, and 100 ids (39 bytes each in JSON) stay well below it.
pub const NOTIFY_CHUNK: usize = 100;

/// Why a session ended (`sessions.end_reason`). Append-only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndReason {
    Logout,
    Revoked,
    OthersRevoked,
    PasswordChanged,
    Ban,
    AccountDeleted,
    RefreshReuse,
    Limit,
    AuthenticatorRevoked,
    Superseded,
    Repudiated,
    RequestCompleted,
    Upgrade,
    Cli,
    AbsoluteCap,
}

impl EndReason {
    /// The stored form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Logout => "logout",
            Self::Revoked => "revoked",
            Self::OthersRevoked => "others_revoked",
            Self::PasswordChanged => "password_changed",
            Self::Ban => "ban",
            Self::AccountDeleted => "account_deleted",
            Self::RefreshReuse => "refresh_reuse",
            Self::Limit => "limit",
            Self::AuthenticatorRevoked => "authenticator_revoked",
            Self::Superseded => "superseded",
            Self::Repudiated => "repudiated",
            Self::RequestCompleted => "request_completed",
            Self::Upgrade => "upgrade",
            Self::Cli => "cli",
            Self::AbsoluteCap => "absolute_cap",
        }
    }
}

/// Internal notice on `user:{id}`, consumed by the WebSocket loop and never
/// forwarded to the client.
#[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionNotice {
    /// Close the sockets of these sessions.
    SessionRevoked { session_ids: Vec<Uuid> },
    /// Close every socket of the user (ban, account deletion, password
    /// reset), also those whose session row is already gone.
    AllSessionsRevoked,
}

/// The payload on [`SESSION_END_CHANNEL`].
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct EndPayload {
    user_id: Uuid,
    session_ids: Vec<Uuid>,
    all: bool,
}

/// End these sessions of `user_id` in the caller's transaction (revocation
/// order above). Returns the ids actually ended now (live ones of this
/// user); already ended or foreign ids are skipped.
#[tracing::instrument(skip(conn, ctx, sids), fields(user_id = %user_id, count = sids.len(), reason = reason.as_str()))]
pub async fn end_sessions_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    user_id: Uuid,
    sids: &[Uuid],
    reason: EndReason,
) -> AuthResult<Vec<Uuid>> {
    if sids.is_empty() {
        return Ok(Vec::new());
    }
    let ended: Vec<Uuid> = sqlx::query_scalar(
        "UPDATE sessions SET ended_at = NOW(), end_reason = $3
          WHERE user_id = $1 AND id = ANY($2) AND ended_at IS NULL
          RETURNING id",
    )
    .bind(user_id)
    .bind(sids)
    .bind(reason.as_str())
    .fetch_all(&mut *conn)
    .await?;
    after_update(conn, ctx, user_id, &ended, false).await?;
    Ok(ended)
}

/// End every live session of `user_id` except `keep`. With `keep = None`
/// every socket of the user closes, also one whose row is already gone.
#[tracing::instrument(skip(conn, ctx), fields(user_id = %user_id, reason = reason.as_str()))]
pub async fn end_all_sessions_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    user_id: Uuid,
    keep: Option<Uuid>,
    reason: EndReason,
) -> AuthResult<Vec<Uuid>> {
    let ended: Vec<Uuid> = sqlx::query_scalar(
        "UPDATE sessions SET ended_at = NOW(), end_reason = $3
          WHERE user_id = $1 AND ended_at IS NULL AND ($2::uuid IS NULL OR id <> $2)
          RETURNING id",
    )
    .bind(user_id)
    .bind(keep)
    .bind(reason.as_str())
    .fetch_all(&mut *conn)
    .await?;
    after_update(conn, ctx, user_id, &ended, keep.is_none()).await?;
    Ok(ended)
}

/// Steps 2–4 of the revocation order (and, from Task 8, the B.7 rule-2
/// cascade of the ended sessions).
async fn after_update(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    user_id: Uuid,
    ended: &[Uuid],
    all: bool,
) -> AuthResult<()> {
    if ended.is_empty() && !all {
        return Ok(());
    }
    if !ended.is_empty() {
        sqlx::query("DELETE FROM elevated_sessions WHERE session_id = ANY($1)")
            .bind(ended)
            .execute(&mut *conn)
            .await?;
    }
    if !ended.is_empty() {
        denylist_and_drop_elevation(ctx, user_id, ended).await;
    }
    // One notification per chunk: an unbounded payload fails the whole
    // transaction (and with it a ban) once it exceeds 8000 bytes.
    let mut chunks: Vec<&[Uuid]> = ended.chunks(NOTIFY_CHUNK).collect();
    if chunks.is_empty() {
        chunks.push(&[]);
    }
    for (i, chunk) in chunks.into_iter().enumerate() {
        let payload = serde_json::to_string(&EndPayload {
            user_id,
            session_ids: chunk.to_vec(),
            all: all && i == 0,
        })
        .map_err(|e| super::AuthError::Internal(format!("JSON error: {e}")))?;
        sqlx::query("SELECT pg_notify($1, $2)")
            .bind(SESSION_END_CHANNEL)
            .bind(payload)
            .execute(&mut *conn)
            .await?;
    }
    // B.7 rule 2: what the ended sessions issued or hold.
    crate::approvals::cascade::on_sessions_ended(conn, ctx, ended).await?;
    Ok(())
}

/// Denylist `ended` and drop their elevation cache in ONE Valkey round-trip
/// (a pipeline), best effort: this runs inside the revoking transaction,
/// so a hung Valkey costs one command timeout, never two per session. The
/// commit ends the sessions; the denylist only rejects earlier. The
/// elevation cache is a positive cache, so it is dropped in the revoking
/// path too (the listener drops it again after commit, and an ended session
/// is refused before any elevation check).
async fn denylist_and_drop_elevation(ctx: &AuthCtx<'_>, user_id: Uuid, ended: &[Uuid]) {
    let ttl = (ctx.config.jwt_access_expiry + DENYLIST_LEEWAY_SECS).max(1);
    let pipeline = ctx.redis.pipeline();
    for sid in ended {
        // Queued only; nothing is sent until `all()`.
        let queued = pipeline
            .set::<(), _, _>(
                super::session_check::denylist_key(*sid),
                "1",
                Some(Expiration::EX(ttl)),
                None,
                false,
            )
            .await
            .and(
                pipeline
                    .del::<(), _>(crate::admin::shared::elevated_cache_key(*sid))
                    .await,
            );
        if let Err(e) = queued {
            tracing::warn!(error = %e, user_id = %user_id, "Denylist pipeline failed; the session rows decide");
            return;
        }
    }
    if let Err(e) = pipeline.all::<Vec<Value>>().await {
        tracing::warn!(error = %e, user_id = %user_id, count = ended.len(), "Denylist write failed; the session rows decide");
    }
}

/// Republish committed session ends to the user's WebSocket channel and
/// clear their elevation cache (best effort). Reconnects with backoff.
pub fn spawn_session_end_listener(pool: PgPool, redis: Client) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut backoff = Duration::from_millis(250);
        loop {
            match listen(&pool, &redis).await {
                Ok(()) => backoff = Duration::from_millis(250),
                Err(e) => {
                    tracing::warn!(error = %e, "Session end listener failed; reconnecting");
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(10));
                }
            }
        }
    })
}

async fn listen(pool: &PgPool, redis: &Client) -> Result<(), sqlx::Error> {
    // A dedicated one-connection pool with the same options, so the
    // listener never holds a connection of the application's pool.
    let own = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect_with((*pool.connect_options()).clone())
        .await?;
    let mut listener = PgListener::connect_with(&own).await?;
    listener.listen(SESSION_END_CHANNEL).await?;
    loop {
        let n = listener.recv().await?;
        let Ok(p) = serde_json::from_str::<EndPayload>(n.payload()) else {
            tracing::warn!("Session end listener: malformed payload");
            continue;
        };
        let notice = if p.all {
            SessionNotice::AllSessionsRevoked
        } else {
            SessionNotice::SessionRevoked {
                session_ids: p.session_ids.clone(),
            }
        };
        if let Ok(payload) = serde_json::to_string(&notice) {
            if let Err(e) = redis
                .publish::<(), _, _>(crate::ws::events::channels::user_events(p.user_id), payload)
                .await
            {
                // Sockets re-check their row within 60 s (the backstop).
                tracing::warn!(error = %e, user_id = %p.user_id, "Failed to notify sockets of ended sessions");
            }
        }
        for sid in &p.session_ids {
            crate::admin::clear_elevated_cache(redis, *sid).await;
        }
    }
}
