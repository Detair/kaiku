//! Refresh rotation with reuse detection (spec B.4, RFC 9700 §4.14.2, DC2).
//!
//! The refresh JWT carries the session id (`sid`) and a generation (`gen`),
//! authenticated by the server's Ed25519 key. Under the session's row lock
//! (B.7 rule 1: refresh locks only that row):
//!
//! - `gen` = the row's and the token is the stored one → rotate in place (`gen + 1`, new `jti`,
//!   sliding expiry capped at the absolute cap);
//! - `gen` = the row's − 1, presented ≤ 10 s after that rotation, and no retry in the last 24 h →
//!   the **identical** successor again (a lost response), re-signed from the stored `jti`/`iat`;
//! - any other earlier `gen` → reuse: the session ends (`refresh_reuse`) with a notice → 401
//!   `SESSION_REUSED`;
//! - anything unrecognised → 401 `INVALID_TOKEN` and nothing changes.
//!
//! Legacy tokens (no `gen`) matching the stored hash are rotated once into
//! the new format. Inactivity or the absolute cap → 401 `SESSION_EXPIRED`
//! (the cap also ends the row).
//!
//! Everything under the lock runs on the transaction's own connection, and
//! the denylist of a `sid` token is read before the lock (its verdict is
//! applied after the ban check), so no second pool connection and no Valkey
//! round-trip is awaited while the row is held.

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use axum::Json;
use axum_extra::extract::CookieJar;
use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

use super::assurance::Assurance;
use super::ctx::AuthCtx;
use super::error::{AuthError, AuthResult};
use super::revocation::{end_sessions_tx, EndReason};
use super::session_issue::{IssuedSession, SessionClient};
use super::types::{AuthResponse, RefreshRequest};
use crate::api::AppState;
use crate::security_events::{record, Actor, EventType, NewSecurityEvent};

/// The predecessor may be presented again this long after its rotation.
pub const RETRY_WINDOW_SECS: i64 = 10;
/// At most one lost-response retry per session in this period.
pub const RETRY_BUDGET_HOURS: i64 = 24;

/// What a refresh did.
#[derive(Debug)]
pub enum RefreshOutcome {
    /// A new successor.
    Rotated(IssuedSession),
    /// The identical successor again (lost response).
    Retried(IssuedSession),
}

impl RefreshOutcome {
    /// The issued tokens either way.
    #[must_use]
    pub fn into_session(self) -> IssuedSession {
        match self {
            Self::Rotated(s) | Self::Retried(s) => s,
        }
    }
}

#[derive(sqlx::FromRow)]
struct Row {
    id: Uuid,
    user_id: Uuid,
    token_hash: String,
    assurance: String,
    expires_at: DateTime<Utc>,
    absolute_expires_at: DateTime<Utc>,
    ended_at: Option<DateTime<Utc>>,
    refresh_generation: i64,
    refresh_jti: Option<Uuid>,
    refresh_issued_at: Option<DateTime<Utc>>,
    refresh_retry_at: Option<DateTime<Utc>>,
}

const ROW_COLUMNS: &str = "id, user_id, token_hash, assurance, expires_at, absolute_expires_at,
    ended_at, refresh_generation, refresh_jti, refresh_issued_at, refresh_retry_at";

/// Rotate `raw` (see the module docs).
#[tracing::instrument(skip_all)]
pub async fn rotate(
    ctx: &AuthCtx<'_>,
    raw: &str,
    client: &SessionClient,
) -> AuthResult<RefreshOutcome> {
    let claims = super::jwt::verify_refresh_signature(raw, &ctx.config.jwt_public_key)?;
    let user_id: Uuid = claims.sub.parse().map_err(|_| AuthError::InvalidToken)?;
    let presented_hash = super::hash_token(raw);
    let sid: Option<Uuid> = match claims.sid.as_deref() {
        Some(s) => Some(s.parse().map_err(|_| AuthError::InvalidToken)?),
        None => None,
    };
    // The denylist (an end that may not be visible yet) is read before the
    // row lock, so no Valkey round-trip runs under it; a hit refuses below,
    // after the ban check. An unreachable denylist is skipped (B.4 rev 5).
    let denied = match sid {
        Some(sid) => {
            super::session_check::denylist(ctx.redis, sid).await
                == super::session_check::Denylist::Hit
        }
        None => false,
    };

    let mut tx = ctx.db.begin().await?;
    let row: Option<Row> = match (sid, claims.gen) {
        (Some(sid), Some(_)) => {
            sqlx::query_as(sqlx::AssertSqlSafe(format!(
                "SELECT {ROW_COLUMNS} FROM sessions WHERE id = $1 FOR UPDATE"
            )))
            .bind(sid)
            .fetch_optional(&mut *tx)
            .await?
        }
        // Legacy token: only its stored hash identifies the session.
        (None, None) => {
            sqlx::query_as(sqlx::AssertSqlSafe(format!(
                "SELECT {ROW_COLUMNS} FROM sessions WHERE token_hash = $1 FOR UPDATE"
            )))
            .bind(&presented_hash)
            .fetch_optional(&mut *tx)
            .await?
        }
        _ => None,
    };
    let Some(row) = row else {
        return Err(AuthError::InvalidToken);
    };
    if row.user_id != user_id {
        return Err(AuthError::InvalidToken);
    }
    // A banned account is told why (its sessions were ended by the ban).
    // Read in this transaction: a second pool connection while this one
    // holds the row could starve the pool under concurrent refreshes.
    let banned: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM global_bans
                         WHERE user_id = $1 AND (expires_at IS NULL OR expires_at > NOW()))",
    )
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;
    if banned {
        return Err(AuthError::AccountBanned);
    }
    // Ended sessions are refused. A legacy token's session id is known only
    // now; its one denylist read runs under the lock (once per token).
    if row.ended_at.is_some()
        || denied
        || (sid.is_none()
            && super::session_check::denylist(ctx.redis, row.id).await
                == super::session_check::Denylist::Hit)
    {
        return Err(AuthError::InvalidToken);
    }

    let now = Utc::now();
    if row.absolute_expires_at <= now {
        end_sessions_tx(&mut tx, ctx, user_id, &[row.id], EndReason::AbsoluteCap).await?;
        tx.commit().await?;
        return Err(AuthError::SessionExpired);
    }
    if row.expires_at <= now {
        // Inactivity: the session lapses (a key device signs in again).
        return Err(AuthError::SessionExpired);
    }
    let assurance = Assurance::parse(&row.assurance);
    let hash_matches =
        crate::approvals::secrets::ct_eq(presented_hash.as_bytes(), row.token_hash.as_bytes());

    match claims.gen {
        // The current token, or a legacy token that matched by hash.
        Some(gen) if gen == row.refresh_generation && hash_matches => {}
        None if hash_matches => {}
        Some(gen) if gen + 1 == row.refresh_generation => {
            let fresh = row
                .refresh_issued_at
                .is_some_and(|t| now - t <= Duration::seconds(RETRY_WINDOW_SECS));
            let budget = row
                .refresh_retry_at
                .is_none_or(|t| now - t >= Duration::hours(RETRY_BUDGET_HOURS));
            if fresh && budget {
                let issued = retry(&mut tx, ctx, &row, user_id, assurance, now).await?;
                tx.commit().await?;
                return Ok(RefreshOutcome::Retried(issued));
            }
            return reuse(tx, ctx, &row, user_id).await;
        }
        Some(gen) if gen < row.refresh_generation => return reuse(tx, ctx, &row, user_id).await,
        _ => return Err(AuthError::InvalidToken),
    }

    // Rotate in place: the `sid` stays, the generation moves on.
    let next_gen = row.refresh_generation + 1;
    let tokens = super::jwt::generate_session_tokens(
        user_id,
        row.id,
        assurance,
        next_gen,
        &ctx.config.jwt_private_key,
        ctx.config.jwt_access_expiry,
        ctx.config.jwt_refresh_expiry,
    )?;
    let expires_at =
        (now + Duration::seconds(ctx.config.jwt_refresh_expiry)).min(row.absolute_expires_at);
    sqlx::query(
        "UPDATE sessions
            SET token_hash = $2, refresh_generation = $3, refresh_jti = $4,
                refresh_issued_at = to_timestamp($5), expires_at = $6, last_used_at = NOW(),
                ip_address = $7::inet, user_agent = $8
          WHERE id = $1",
    )
    .bind(row.id)
    .bind(super::hash_token(&tokens.refresh_token))
    .bind(next_gen)
    .bind(tokens.refresh_jti)
    .bind(tokens.refresh_iat as f64)
    .bind(expires_at)
    .bind(client.ip.map(|ip| ip.to_string()))
    .bind(client.user_agent.as_deref())
    .execute(&mut *tx)
    .await?;
    let setup_complete = setup_complete(&mut tx).await?;
    tx.commit().await?;
    Ok(RefreshOutcome::Rotated(IssuedSession {
        tokens,
        session_id: row.id,
        assurance,
        absolute_expires_at: row.absolute_expires_at,
        setup_complete,
        event_ids: Vec::new(),
    }))
}

/// The identical successor again: re-sign the stored `jti`/`iat` of the
/// current generation, spend the 24 h retry budget.
async fn retry(
    conn: &mut sqlx::PgConnection,
    ctx: &AuthCtx<'_>,
    row: &Row,
    user_id: Uuid,
    assurance: Assurance,
    now: DateTime<Utc>,
) -> AuthResult<IssuedSession> {
    let (Some(jti), Some(issued_at)) = (row.refresh_jti, row.refresh_issued_at) else {
        return Err(AuthError::InvalidToken);
    };
    let iat = issued_at.timestamp();
    let refresh_token = super::jwt::encode_refresh(
        user_id,
        row.id,
        row.refresh_generation,
        jti,
        iat,
        iat + ctx.config.jwt_refresh_expiry,
        &ctx.config.jwt_private_key,
    )?;
    if !crate::approvals::secrets::ct_eq(
        super::hash_token(&refresh_token).as_bytes(),
        row.token_hash.as_bytes(),
    ) {
        // Only if the refresh lifetime was reconfigured in between.
        tracing::warn!(session_id = %row.id, "Refresh retry could not reproduce the successor");
        return Err(AuthError::InvalidToken);
    }
    sqlx::query("UPDATE sessions SET refresh_retry_at = $2, last_used_at = NOW() WHERE id = $1")
        .bind(row.id)
        .bind(now)
        .execute(&mut *conn)
        .await?;
    let access_token = super::jwt::encode_access(
        user_id,
        row.id,
        assurance,
        now.timestamp(),
        ctx.config.jwt_access_expiry,
        &ctx.config.jwt_private_key,
    )?;
    Ok(IssuedSession {
        tokens: super::jwt::TokenPair {
            access_token,
            refresh_token,
            access_expires_in: ctx.config.jwt_access_expiry,
            refresh_token_id: row.id,
            refresh_jti: jti,
            refresh_iat: iat,
        },
        session_id: row.id,
        assurance,
        absolute_expires_at: row.absolute_expires_at,
        setup_complete: setup_complete(conn).await?,
        event_ids: Vec::new(),
    })
}

/// Reuse: end the session with a notice, commit, 401 `SESSION_REUSED`.
async fn reuse(
    mut tx: sqlx::Transaction<'_, sqlx::Postgres>,
    ctx: &AuthCtx<'_>,
    row: &Row,
    user_id: Uuid,
) -> AuthResult<RefreshOutcome> {
    end_sessions_tx(&mut tx, ctx, user_id, &[row.id], EndReason::RefreshReuse).await?;
    let mut ev = NewSecurityEvent::new(
        user_id,
        EventType::SessionEndedReuse,
        Actor::System,
        serde_json::json!({ "session_id": row.id }),
    );
    ev.session_id = Some(row.id);
    record(&mut tx, ev).await?;
    tx.commit().await?;
    tracing::warn!(user_id = %user_id, session_id = %row.id, "Refresh token reuse: session ended");
    // The notice goes out with the minute sweep (DC11) or right away when
    // the handler has the app state (see `refresh_token`).
    Err(AuthError::SessionReused)
}

async fn setup_complete(conn: &mut sqlx::PgConnection) -> AuthResult<bool> {
    Ok(sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT value FROM server_config WHERE key = 'setup_complete'",
    )
    .fetch_optional(&mut *conn)
    .await?
    .and_then(|v| v.as_bool())
    .unwrap_or(false))
}

/// `POST /auth/refresh`: rotate the refresh token (body for native
/// clients, the `HttpOnly` cookie for browsers).
#[utoipa::path(
    post,
    path = "/auth/refresh",
    tag = "auth",
    request_body(content = RefreshRequest, description = "Required for native clients; browser clients use the HttpOnly cookie instead"),
    responses(
        (status = 200, description = "Token refreshed (or the identical successor for a lost response)", body = AuthResponse),
        (status = 401, description = "INVALID_TOKEN, SESSION_REUSED or SESSION_EXPIRED"),
    ),
    security(()),
)]
#[tracing::instrument(skip(state, jar, headers, body))]
pub async fn refresh_token(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    body: Option<Json<RefreshRequest>>,
) -> AuthResult<(CookieJar, Json<AuthResponse>)> {
    let (raw, from_legacy_cookie) = super::helpers::extract_refresh_token_with_source(
        body.map(|b| b.0.refresh_token),
        &jar,
        &state.config,
    )?;
    let client = SessionClient::from_request(&state, &headers, addr);
    let outcome = match rotate(&AuthCtx::from(&state), &raw, &client).await {
        Ok(o) => o,
        Err(e) => {
            crate::observability::metrics::record_token_refresh(false);
            if matches!(e, AuthError::SessionReused) {
                // Deliver the reuse notice now instead of with the sweep.
                let state = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = crate::security_events::notices::deliver_pending(&state).await {
                        tracing::warn!(error = %e, "reuse notice delivery failed; the sweep retries");
                    }
                });
            }
            return Err(e);
        }
    };
    crate::observability::metrics::record_token_refresh(true);
    let issued = outcome.into_session();
    let password_change_required = sqlx::query_scalar::<_, bool>(
        "SELECT must_change_password FROM users WHERE id = (SELECT user_id FROM sessions WHERE id = $1)",
    )
    .bind(issued.session_id)
    .fetch_optional(&state.db)
    .await?
    .unwrap_or(false);
    let include_refresh_token = super::helpers::should_return_refresh_token(&headers);
    let mut jar = jar.add(issued.refresh_cookie(&state.config));
    if from_legacy_cookie {
        jar = jar.add(super::cookies::build_clear_legacy_cookie(&state.config));
    }
    Ok((
        jar,
        Json(issued.into_response(password_change_required, include_refresh_token)),
    ))
}
