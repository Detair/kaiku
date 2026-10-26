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
