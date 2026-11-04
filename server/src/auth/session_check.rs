//! The authoritative session check (spec B.4 rev 5).
//!
//! Every authenticated request, every WebSocket handshake and re-check, and
//! the `?token=` download path decide from the committed `sessions` row,
//! read by primary key. The Valkey denylist may only speed up a rejection: a
//! hit rejects at once; a miss or an unreachable denylist is skipped and
//! never makes a session valid. There is **no positive cache** of live
//! sessions (a later cache needs synchronous invalidation in the revoking
//! commit path and a security review).

use chrono::{DateTime, Utc};
use fred::prelude::*;
use sqlx::PgPool;
use uuid::Uuid;

use super::assurance::Assurance;
use super::client_class::ClientClass;
use super::strong::AuthenticatorRef;
use crate::db::{LogThrottle, User};

/// A live session with its user and ban state (one query, DC6).
#[derive(Debug, Clone)]
pub struct RequestSession {
    pub session_id: Uuid,
    pub user: User,
    pub banned: bool,
    pub assurance: Assurance,
    pub auth_methods: Vec<String>,
    pub auth_time: DateTime<Utc>,
    pub client_class: ClientClass,
    pub authenticator: Option<AuthenticatorRef>,
    pub created_at: DateTime<Utc>,
    pub absolute_expires_at: DateTime<Utc>,
}

/// The `WHERE` clause that makes a session live (alias `s`; the one source
/// of truth, also for the lock-time re-checks).
#[macro_export]
macro_rules! live_clause {
    () => {
        "s.ended_at IS NULL AND s.expires_at > clock_timestamp() AND s.absolute_expires_at > clock_timestamp()"
    };
}

#[derive(sqlx::FromRow)]
struct SessionCols {
    s_id: Uuid,
    s_assurance: String,
    s_auth_methods: Vec<String>,
    s_auth_time: DateTime<Utc>,
    s_client_class: String,
    s_authenticator_type: Option<String>,
    s_authenticator_id: Option<Uuid>,
    s_created_at: DateTime<Utc>,
    s_absolute_expires_at: DateTime<Utc>,
    s_banned: bool,
}

/// Load a live session, its user and the user's ban state by session id,
/// in one query. `None` if the row is missing, ended or expired.
pub async fn load_live(pool: &PgPool, sid: Uuid) -> sqlx::Result<Option<RequestSession>> {
    #[derive(sqlx::FromRow)]
    struct Row {
        #[sqlx(flatten)]
        session: SessionCols,
        #[sqlx(flatten)]
        user: User,
    }
    let row = sqlx::query_as::<_, Row>(concat!(
        "SELECT u.*,
                s.id AS s_id, s.assurance AS s_assurance, s.auth_methods AS s_auth_methods,
                s.auth_time AS s_auth_time, s.client_class AS s_client_class,
                s.authenticator_type AS s_authenticator_type,
                s.authenticator_id AS s_authenticator_id, s.created_at AS s_created_at,
                s.absolute_expires_at AS s_absolute_expires_at,
                EXISTS (SELECT 1 FROM global_bans gb
                         WHERE gb.user_id = u.id
                           AND (gb.expires_at IS NULL OR gb.expires_at > NOW())) AS s_banned
           FROM sessions s
           JOIN users u ON u.id = s.user_id
          WHERE s.id = $1 AND ",
        live_clause!()
    ))
    .bind(sid)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| {
        let s = r.session;
        RequestSession {
            session_id: s.s_id,
            user: r.user,
            banned: s.s_banned,
            assurance: Assurance::parse(&s.s_assurance),
            auth_methods: s.s_auth_methods,
            auth_time: s.s_auth_time,
            client_class: ClientClass::parse(&s.s_client_class).unwrap_or(ClientClass::Web),
            authenticator: AuthenticatorRef::from_columns(
                s.s_authenticator_type.as_deref(),
                s.s_authenticator_id,
            ),
            created_at: s.s_created_at,
            absolute_expires_at: s.s_absolute_expires_at,
        }
    }))
}

/// Whether the session's committed row is live (the socket re-check).
pub async fn is_live(pool: &PgPool, sid: Uuid) -> sqlx::Result<bool> {
    sqlx::query_scalar::<_, bool>(concat!(
        "SELECT EXISTS (SELECT 1 FROM sessions s WHERE s.id = $1 AND ",
        live_clause!(),
        ")"
    ))
    .bind(sid)
    .fetch_one(pool)
    .await
}

/// Whether the session's committed row is live and belongs to `user_id`
/// (the WebSocket handshake and re-check).
pub async fn is_live_for_user(pool: &PgPool, sid: Uuid, user_id: Uuid) -> sqlx::Result<bool> {
    sqlx::query_scalar::<_, bool>(concat!(
        "SELECT EXISTS (SELECT 1 FROM sessions s WHERE s.id = $1 AND s.user_id = $2 AND ",
        live_clause!(),
        ")"
    ))
    .bind(sid)
    .bind(user_id)
    .fetch_one(pool)
    .await
}

/// Result of the denylist lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denylist {
    /// The session was ended: reject at once.
    Hit,
    /// Not listed: decide from the row.
    Miss,
    /// Valkey did not answer: skip it and decide from the row.
    Unavailable,
}

/// The denylist key of a session (shared with `revocation`).
#[must_use]
pub fn denylist_key(sid: Uuid) -> String {
    format!("canis:revoked_sid:{sid}")
}

/// Look the session up in the Valkey denylist (bounded by the client's
/// command timeout). Never makes a session valid.
pub async fn denylist(redis: &fred::clients::Client, sid: Uuid) -> Denylist {
    static WARN: LogThrottle = LogThrottle::new();
    match redis.exists::<i64, _>(denylist_key(sid)).await {
        Ok(n) if n > 0 => Denylist::Hit,
        Ok(_) => Denylist::Miss,
        Err(e) => {
            if WARN.allow(std::time::Duration::from_mins(1)) {
                tracing::warn!(
                    error = %e,
                    "Session denylist unavailable; deciding from the session rows"
                );
            }
            Denylist::Unavailable
        }
    }
}

/// Why a token's session was refused.
#[derive(Debug)]
pub enum Refusal {
    /// No `sid`, unknown or ended session, other user, system account:
    /// 401 `INVALID_TOKEN`.
    Invalid,
    /// The account is banned: 403 `ACCOUNT_BANNED`.
    Banned,
    /// The database could not be read.
    Db(sqlx::Error),
}

/// The full check for an access token's claims: denylist first (a hit
/// rejects), then the committed row. The token's `sub` must be the row's
/// user.
pub async fn check_claims(
    pool: &PgPool,
    redis: &fred::clients::Client,
    sub: &str,
    sid: Option<&str>,
) -> Result<RequestSession, Refusal> {
    let sid: Uuid = sid.and_then(|s| s.parse().ok()).ok_or(Refusal::Invalid)?;
    let user_id: Uuid = sub.parse().map_err(|_| Refusal::Invalid)?;
    if denylist(redis, sid).await == Denylist::Hit {
        return Err(Refusal::Invalid);
    }
    let session = load_live(pool, sid)
        .await
        .map_err(Refusal::Db)?
        .ok_or(Refusal::Invalid)?;
    if session.user.id != user_id || session.user.is_system {
        return Err(Refusal::Invalid);
    }
    if session.banned {
        return Err(Refusal::Banned);
    }
    Ok(session)
}
