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
//! The check is intentionally **fail-open**: if Redis is unavailable we accept
//! the token rather than lock everyone out. This is no worse than the previous
//! behaviour (no revocation at all) and preserves availability.

use fred::prelude::*;

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

/// Returns `true` if the session id has been revoked.
///
/// Fails open (returns `false`) on Redis errors so an outage cannot lock out
/// legitimate users; a warning is logged so the condition is observable.
pub async fn is_session_revoked(redis: &Client, sid: &str) -> bool {
    match redis.exists::<i64, _>(revoked_key(sid)).await {
        Ok(count) => count > 0,
        Err(e) => {
            tracing::warn!(error = %e, "Failed to check token revocation denylist; failing open");
            false
        }
    }
}
