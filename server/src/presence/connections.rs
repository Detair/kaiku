//! Per-device presence: which sockets of a user are live.
//!
//! Each user has a Valkey sorted set `presence:sockets:{user_id}`: one member
//! per open WebSocket (the socket id), scored with the socket's expiry in
//! Unix milliseconds. A socket registers on connect, refreshes its expiry
//! ([`SOCKET_TTL`]) on every heartbeat pong and removes itself on disconnect.
//! A socket of a crashed server is never removed, but it stops counting once
//! its expiry has passed, and the next update of the set prunes it.
//!
//! Registering and removing are single Lua scripts, so the set update and the
//! "first live socket" / "last live socket" check are one atomic step: two
//! devices connecting or disconnecting at the same time never both report the
//! transition, and never both miss it. The scripts can also `PUBLISH` the
//! presence event in that same step, so a disconnect of one device and a
//! connect of another reach subscribers in the order they happened.
//!
//! **Stored vs. shown status.** `users.status` holds the status the user chose
//! (`online`, `away`, `busy`; `offline` once the last socket closed) and
//! `users.invisible` marks "appear offline". What other people see comes from
//! [`effective_status`], which also needs to know whether the user has a live
//! socket ([`live_users`]).

use std::collections::HashSet;
use std::time::Duration;

use fred::interfaces::LuaInterface;
use fred::prelude::*;
use uuid::Uuid;

/// How long a socket counts as live after its last heartbeat. Three server
/// heartbeat intervals (30 s), so one late pong never flips presence.
pub const SOCKET_TTL: Duration = Duration::from_secs(90);

/// At most this many users are presence peers of one user (friends first,
/// then members of the user's servers).
pub const PRESENCE_PEER_LIMIT: i64 = 500;

/// SQL expression (a string literal, for `concat!`) for a user's stored
/// presence: the `users.status` value, or `invisible` when the user chose to
/// appear offline. Expects the users table aliased as `u`. Turn it into what
/// others see with [`effective_status`].
macro_rules! stored_presence_sql {
    () => {
        "CASE WHEN u.invisible THEN 'invisible' ELSE u.status::text END"
    };
}
pub(crate) use stored_presence_sql;

/// A Valkey presence command failed. Presence is best effort: callers log
/// (throttled) and carry on.
#[derive(Debug, thiserror::Error)]
#[error("presence store unavailable: {0}")]
pub struct PresenceError(#[from] pub fred::error::Error);

/// Valkey key of a user's socket set.
#[must_use]
pub fn sockets_key(user_id: Uuid) -> String {
    format!("presence:sockets:{user_id}")
}

/// Register (or refresh) a socket. Prunes expired members first, so a socket
/// whose own entry had expired counts as new again.
///
/// KEYS[1] = set; ARGV = now ms, expiry ms, socket id, TTL ms, event payload
/// (empty: publish nothing), publish channel. Returns 1 on 0 → 1.
const SOCKET_ONLINE_SCRIPT: &str = r"
local key = KEYS[1]
redis.call('ZREMRANGEBYSCORE', key, '-inf', ARGV[1])
local before = redis.call('ZCARD', key)
redis.call('ZADD', key, ARGV[2], ARGV[3])
redis.call('PEXPIRE', key, ARGV[4])
if before == 0 then
  if ARGV[5] ~= '' then
    redis.call('PUBLISH', ARGV[6], ARGV[5])
  end
  return 1
end
return 0
";

/// Remove a socket, then prune expired members. Reports 1 → 0 only when this
/// call removed the socket and no live socket remains (a socket removed twice
/// never reports twice).
///
/// KEYS[1] = set; ARGV = now ms, socket id, event payload (empty: publish
/// nothing), publish channel. Returns 1 on 1 → 0.
const SOCKET_OFFLINE_SCRIPT: &str = r"
local key = KEYS[1]
local removed = redis.call('ZREM', key, ARGV[2])
redis.call('ZREMRANGEBYSCORE', key, '-inf', ARGV[1])
local after = redis.call('ZCARD', key)
if removed == 1 and after == 0 then
  if ARGV[3] ~= '' then
    redis.call('PUBLISH', ARGV[4], ARGV[3])
  end
  return 1
end
return 0
";

/// Number of live sockets per key (members with an expiry after ARGV[1]).
const LIVE_COUNTS_SCRIPT: &str = r"
local counts = {}
for i, key in ipairs(KEYS) do
  counts[i] = redis.call('ZCOUNT', key, '(' .. ARGV[1], '+inf')
end
return counts
";

fn ttl_ms() -> i64 {
    i64::try_from(SOCKET_TTL.as_millis()).unwrap_or(i64::MAX)
}

/// Register or refresh `socket_id`; `true` when it is the user's first live
/// socket (0 → 1). Publishes nothing.
pub async fn socket_online(
    redis: &Client,
    user_id: Uuid,
    socket_id: Uuid,
    now_ms: i64,
) -> Result<bool, PresenceError> {
    socket_online_announce(redis, user_id, socket_id, now_ms, None).await
}

/// [`socket_online`] that also publishes `announce` on `presence:{user_id}`
/// in the same atomic step when the transition happens.
pub async fn socket_online_announce(
    redis: &Client,
    user_id: Uuid,
    socket_id: Uuid,
    now_ms: i64,
    announce: Option<&str>,
) -> Result<bool, PresenceError> {
    let ttl = ttl_ms();
    let first: i64 = redis
        .eval(
            SOCKET_ONLINE_SCRIPT,
            vec![sockets_key(user_id)],
            vec![
                now_ms.to_string(),
                now_ms.saturating_add(ttl).to_string(),
                socket_id.to_string(),
                ttl.to_string(),
                announce.unwrap_or_default().to_string(),
                presence_channel(user_id),
            ],
        )
        .await?;
    Ok(first == 1)
}

/// Remove `socket_id`; `true` when no live socket remains (1 → 0).
/// Publishes nothing.
pub async fn socket_offline(
    redis: &Client,
    user_id: Uuid,
    socket_id: Uuid,
    now_ms: i64,
) -> Result<bool, PresenceError> {
    socket_offline_announce(redis, user_id, socket_id, now_ms, None).await
}

/// [`socket_offline`] that also publishes `announce` on `presence:{user_id}`
/// in the same atomic step when the transition happens.
pub async fn socket_offline_announce(
    redis: &Client,
    user_id: Uuid,
    socket_id: Uuid,
    now_ms: i64,
    announce: Option<&str>,
) -> Result<bool, PresenceError> {
    let last: i64 = redis
        .eval(
            SOCKET_OFFLINE_SCRIPT,
            vec![sockets_key(user_id)],
            vec![
                now_ms.to_string(),
                socket_id.to_string(),
                announce.unwrap_or_default().to_string(),
                presence_channel(user_id),
            ],
        )
        .await?;
    Ok(last == 1)
}

/// The users among `user_ids` that have at least one live socket. One Valkey
/// round trip, whatever the number of users.
pub async fn live_users(
    redis: &Client,
    user_ids: &[Uuid],
    now_ms: i64,
) -> Result<HashSet<Uuid>, PresenceError> {
    if user_ids.is_empty() {
        return Ok(HashSet::new());
    }
    let keys: Vec<String> = user_ids.iter().map(|id| sockets_key(*id)).collect();
    let counts: Vec<i64> = redis
        .eval(LIVE_COUNTS_SCRIPT, keys, vec![now_ms.to_string()])
        .await?;
    Ok(user_ids
        .iter()
        .zip(counts)
        .filter(|(_, count)| *count > 0)
        .map(|(id, _)| *id)
        .collect())
}

/// [`live_users`], failing open: on a Valkey error it logs (throttled) and
/// returns `None`, and callers fall back to the stored status.
pub async fn live_users_or_log(redis: &Client, user_ids: &[Uuid]) -> Option<HashSet<Uuid>> {
    match live_users(redis, user_ids, now_ms()).await {
        Ok(live) => Some(live),
        Err(e) => {
            log_unavailable(&e);
            None
        }
    }
}

/// Log a presence store failure at most once a minute (presence fails open).
pub fn log_unavailable(e: &PresenceError) {
    static WARN: crate::db::LogThrottle = crate::db::LogThrottle::new();
    if WARN.allow(Duration::from_mins(1)) {
        tracing::warn!(
            error = %e,
            "Presence store unavailable; continuing without it (logged at most once a minute)"
        );
    }
}

/// Number of live sockets among `scores` (expiry ms) at `now_ms`. A socket
/// whose expiry is `now_ms` or earlier is not live.
#[must_use]
pub fn live_count(scores: &[i64], now_ms: i64) -> usize {
    scores.iter().filter(|expiry| **expiry > now_ms).count()
}

/// What other people see, from the stored presence (`stored_presence_sql!`)
/// and whether the user has a live socket.
///
/// `live = None` means the socket sets could not be read (Valkey down): the
/// stored status is shown as is, except that `invisible` stays hidden.
#[must_use]
pub fn effective_status(stored: &str, live: Option<bool>) -> &str {
    match (stored, live) {
        ("invisible", _) | (_, Some(false)) => "offline",
        // Connected, but the last disconnect of an earlier socket wrote
        // `offline` after this one connected.
        ("offline", Some(true)) => "online",
        (status, _) => status,
    }
}

/// Current time in Unix milliseconds.
#[must_use]
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn presence_channel(user_id: Uuid) -> String {
    crate::ws::channels::user_presence(user_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expired_sockets_are_not_live() {
        let now = 1_000_000;
        // Expired long ago (crashed server), expiring exactly now, still live.
        assert_eq!(live_count(&[now - 90_000, now, now + 1], now), 1);
        assert_eq!(live_count(&[], now), 0);
        assert_eq!(live_count(&[now + 90_000, now + 30_000], now), 2);
    }

    #[test]
    fn effective_status_needs_a_live_socket() {
        assert_eq!(effective_status("online", Some(true)), "online");
        assert_eq!(effective_status("busy", Some(true)), "busy");
        assert_eq!(effective_status("away", Some(true)), "away");
        assert_eq!(effective_status("offline", Some(true)), "online");
        assert_eq!(effective_status("invisible", Some(true)), "offline");
        // Left behind by a crashed server, or a manual status while away.
        assert_eq!(effective_status("online", Some(false)), "offline");
        assert_eq!(effective_status("busy", Some(false)), "offline");
        // Valkey down: stored status, invisible stays hidden.
        assert_eq!(effective_status("busy", None), "busy");
        assert_eq!(effective_status("invisible", None), "offline");
    }
}
