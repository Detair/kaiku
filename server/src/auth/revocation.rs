//! Access-token revocation denylist.
//!
//! Access tokens are stateless and short-lived (~15 min), so logging out or
//! revoking a session does not, by itself, stop an already-issued access token
//! from being accepted until it expires. To close that window we keep a small
//! Redis denylist of revoked **session ids** (the `sid` claim carried by every
//! access token, equal to the session row id). On logout / session revoke we
//! add the session id with a TTL equal to the access-token lifetime — after
//! that the token is expired anyway, so the entry can be dropped.
//!
//! The check **fails closed**: if Valkey cannot answer (within the command
//! timeout) the caller rejects the request as temporarily unavailable instead
//! of accepting a token that may have been revoked.

use fred::prelude::*;

use crate::db::{LogThrottle, ValkeyUnavailable};

/// Redis key for a revoked session id.
fn revoked_key(sid: &str) -> String {
    format!("canis:revoked_sid:{sid}")
}

/// Add a session id to the revocation denylist for `ttl_secs` seconds.
///
/// `ttl_secs` should be the access-token lifetime; once it elapses any access
/// token bearing this `sid` is already expired.
pub async fn revoke_session(
    redis: &Client,
    sid: &str,
    ttl_secs: i64,
) -> Result<(), fred::error::Error> {
    let ttl = ttl_secs.max(1);
    redis
        .set::<(), _, _>(
            revoked_key(sid),
            "1",
            Some(Expiration::EX(ttl)),
            None,
            false,
        )
        .await
}

/// Returns `Ok(true)` if the session id has been revoked.
///
/// A Valkey error or timeout is returned as [`ValkeyUnavailable`]; callers
/// fail closed (503 / WebSocket close 1013). The warning is throttled so an
/// outage does not log once per request.
pub async fn is_session_revoked(redis: &Client, sid: &str) -> Result<bool, ValkeyUnavailable> {
    static WARN: LogThrottle = LogThrottle::new();

    match redis.exists::<i64, _>(revoked_key(sid)).await {
        Ok(count) => Ok(count > 0),
        Err(e) => {
            if WARN.allow(std::time::Duration::from_mins(1)) {
                tracing::warn!(
                    error = %e,
                    "Token revocation denylist unavailable; rejecting authenticated requests"
                );
            }
            Err(ValkeyUnavailable(e))
        }
    }
}

/// Internal `user:{id}` message telling that user's sockets which sessions
/// ended. Sockets consume it and never forward it to clients.
#[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionNotice {
    /// These sessions ended; a socket of one of them closes with 1008.
    SessionRevoked { session_ids: Vec<uuid::Uuid> },
}

/// End `session_ids` of `user_id` immediately.
///
/// Denylists each id for the access-token lifetime (so every access token of
/// those sessions is refused) and tells the user's open sockets, which close
/// with 1008 when their own session is listed. Unlike the denylist *check*,
/// this fails closed: a Valkey error is returned so callers can abort the
/// surrounding transaction instead of reporting a revocation that did not
/// take effect.
pub async fn end_sessions(
    state: &crate::api::AppState,
    user_id: uuid::Uuid,
    session_ids: &[uuid::Uuid],
) -> Result<(), super::AuthError> {
    if session_ids.is_empty() {
        return Ok(());
    }
    for sid in session_ids {
        revoke_session(
            &state.redis,
            &sid.to_string(),
            state.config.jwt_access_expiry,
        )
        .await
        .map_err(|e| {
            tracing::error!(error = %e, user_id = %user_id, "Failed to denylist ended session");
            super::AuthError::Internal("Failed to end session".to_string())
        })?;
    }

    let notice = SessionNotice::SessionRevoked {
        session_ids: session_ids.to_vec(),
    };
    let payload = serde_json::to_string(&notice)
        .map_err(|e| super::AuthError::Internal(format!("JSON error: {e}")))?;
    state
        .redis
        .publish::<(), _, _>(crate::ws::events::channels::user_events(user_id), payload)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, user_id = %user_id, "Failed to notify sockets of ended sessions");
            super::AuthError::Internal("Failed to end session".to_string())
        })?;
    Ok(())
}
