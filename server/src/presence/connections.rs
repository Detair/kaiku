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
//! Every published status change carries a per-user version
//! (`presence:ver:{user_id}`, incremented in the publishing script): sockets
//! drop updates older than what they already sent, so the connect snapshot
//! and live updates cannot overwrite each other out of order.
//!
//! Sockets that end without a disconnect (crashed server, a failed disconnect
//! command) are covered by the sweeper ([`sweep_expired`]): the global set
//! `presence:due` holds each user's latest expiry, and an atomic expire step
//! announces `offline` once the last socket has expired.
//!
//! While a user is invisible (`presence:hidden:{user_id}`, mirroring
//! `users.invisible`), connect, disconnect and expiry announce nothing.
//!
//! **Stored vs. shown status.** `users.status` holds the status the user chose
//! (`online`, `away`, `busy`; `offline` once the last socket closed) and
//! `users.invisible` marks "appear offline". What other people see comes from
//! [`effective_status`], which also needs to know whether the user has a live
//! socket ([`live_users`]).

use std::collections::{HashMap, HashSet};
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

/// Global sorted set: user id → latest socket expiry (ms). The sweeper reads
/// users whose latest expiry has passed.
pub const DUE_KEY: &str = "presence:due";

/// Valkey key of a user's presence version (incremented on every published
/// status change).
#[must_use]
pub fn version_key(user_id: Uuid) -> String {
    format!("presence:ver:{user_id}")
}

/// Valkey key that mirrors `users.invisible`: while it exists, lifecycle
/// changes (connect, disconnect, expiry) announce nothing.
#[must_use]
pub fn hidden_key(user_id: Uuid) -> String {
    format!("presence:hidden:{user_id}")
}

/// Keys every lifecycle script gets, in this order: the user's socket set,
/// the global due set, the user's presence version, the user's hidden flag.
fn lifecycle_keys(user_id: Uuid) -> Vec<String> {
    vec![
        sockets_key(user_id),
        DUE_KEY.to_string(),
        version_key(user_id),
        hidden_key(user_id),
    ]
}

/// Lua helper shared by the scripts: publish ARGV-provided event JSON on a
/// channel as a versioned envelope, unless the user is hidden (KEYS[4]).
/// `INCR` and `PUBLISH` run in the same script, so version order is publish
/// order.
macro_rules! lua_announce {
    () => {
        r#"
local function announce(payload, channel)
  if payload == '' or redis.call('EXISTS', KEYS[4]) == 1 then
    return
  end
  local v = redis.call('INCR', KEYS[3])
  redis.call('PUBLISH', channel, '{"v":' .. v .. ',"event":' .. payload .. '}')
end
"#
    };
}

/// Register (or refresh) a socket. Prunes expired members first, so a socket
/// whose own entry had expired counts as new again, and raises the user's
/// entry in the due set.
///
/// ARGV = now ms, expiry ms, socket id, TTL ms, event payload (empty: publish
/// nothing), publish channel, user id, "1" to mark the user hidden first.
/// Returns 1 on 0 → 1.
const SOCKET_ONLINE_SCRIPT: &str = concat!(
    lua_announce!(),
    r"
if ARGV[8] == '1' then
  redis.call('SET', KEYS[4], '1')
end
redis.call('ZREMRANGEBYSCORE', KEYS[1], '-inf', ARGV[1])
local before = redis.call('ZCARD', KEYS[1])
redis.call('ZADD', KEYS[1], ARGV[2], ARGV[3])
redis.call('PEXPIRE', KEYS[1], ARGV[4])
redis.call('ZADD', KEYS[2], 'GT', ARGV[2], ARGV[7])
if before == 0 then
  announce(ARGV[5], ARGV[6])
  return 1
end
return 0
"
);

/// Remove a socket, then prune expired members. Reports 1 → 0 when this call
/// emptied the set by removing anything (its own socket or expired ones), so
/// an `offline` is never lost and never sent twice.
///
/// ARGV = now ms, socket id, event payload, publish channel, user id.
const SOCKET_OFFLINE_SCRIPT: &str = concat!(
    lua_announce!(),
    r"
local removed = redis.call('ZREM', KEYS[1], ARGV[2])
local pruned = redis.call('ZREMRANGEBYSCORE', KEYS[1], '-inf', ARGV[1])
local after = redis.call('ZCARD', KEYS[1])
if after == 0 then
  redis.call('ZREM', KEYS[2], ARGV[5])
end
if removed + pruned > 0 and after == 0 then
  announce(ARGV[3], ARGV[4])
  return 1
end
return 0
"
);

/// Sweeper step for one user: prune expired sockets; if that empties the set,
/// announce the 1 → 0 transition and drop the user from the due set,
/// otherwise set the due score to the latest remaining expiry.
///
/// ARGV = now ms, event payload, publish channel, user id.
const EXPIRE_SCRIPT: &str = concat!(
    lua_announce!(),
    r"
local pruned = redis.call('ZREMRANGEBYSCORE', KEYS[1], '-inf', ARGV[1])
local after = redis.call('ZCARD', KEYS[1])
if after == 0 then
  redis.call('ZREM', KEYS[2], ARGV[4])
  if pruned > 0 then
    announce(ARGV[2], ARGV[3])
    return 1
  end
  return 0
end
local top = redis.call('ZRANGE', KEYS[1], -1, -1, 'WITHSCORES')
redis.call('ZADD', KEYS[2], top[2], ARGV[4])
return 0
"
);

/// Status change chosen by the user: set or clear the hidden flag, then
/// publish (versioned) unless the payload is empty. The hidden flag does not
/// suppress this publish: entering invisible announces `offline` once.
///
/// ARGV = "1" hidden / "0" visible, event payload, publish channel.
const SET_STATUS_SCRIPT: &str = r#"
if ARGV[1] == '1' then
  redis.call('SET', KEYS[4], '1')
else
  redis.call('DEL', KEYS[4])
end
if ARGV[2] ~= '' then
  local v = redis.call('INCR', KEYS[3])
  redis.call('PUBLISH', ARGV[3], '{"v":' .. v .. ',"event":' .. ARGV[2] .. '}')
end
return 1
"#;

/// Late announcement (a socket that could not announce at registration): only
/// while the user has a live socket and is not hidden.
///
/// ARGV = now ms, event payload, publish channel.
const ANNOUNCE_IF_LIVE_SCRIPT: &str = concat!(
    lua_announce!(),
    r"
if redis.call('ZCOUNT', KEYS[1], '(' .. ARGV[1], '+inf') == 0 then
  return 0
end
announce(ARGV[2], ARGV[3])
return 1
"
);

/// Live socket count and presence version per user. KEYS = socket set,
/// version key, alternating.
const LIVE_SNAPSHOT_SCRIPT: &str = r"
local out = {}
for i = 1, #KEYS, 2 do
  out[#out + 1] = redis.call('ZCOUNT', KEYS[i], '(' .. ARGV[1], '+inf')
  out[#out + 1] = tonumber(redis.call('GET', KEYS[i + 1]) or '0')
end
return out
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
    socket_online_announce(redis, user_id, socket_id, now_ms, None, false).await
}

/// [`socket_online`] that also announces the 0 → 1 transition.
///
/// On the transition it publishes `announce` (versioned) on
/// `presence:{user_id}` in the same atomic step, unless the user is hidden.
/// `mark_hidden` sets the hidden flag first (the database says the user is
/// invisible).
pub async fn socket_online_announce(
    redis: &Client,
    user_id: Uuid,
    socket_id: Uuid,
    now_ms: i64,
    announce: Option<&str>,
    mark_hidden: bool,
) -> Result<bool, PresenceError> {
    let ttl = ttl_ms();
    let first: i64 = redis
        .eval(
            SOCKET_ONLINE_SCRIPT,
            lifecycle_keys(user_id),
            vec![
                now_ms.to_string(),
                now_ms.saturating_add(ttl).to_string(),
                socket_id.to_string(),
                ttl.to_string(),
                announce.unwrap_or_default().to_string(),
                presence_channel(user_id),
                user_id.to_string(),
                if mark_hidden { "1" } else { "" }.to_string(),
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
    socket_offline_inner(redis, user_id, socket_id, now_ms, false).await
}

/// [`socket_offline`] that also publishes `offline` (versioned) on
/// `presence:{user_id}` in the same atomic step when the transition happens,
/// unless the user is hidden.
pub async fn socket_offline_announce(
    redis: &Client,
    user_id: Uuid,
    socket_id: Uuid,
    now_ms: i64,
) -> Result<bool, PresenceError> {
    socket_offline_inner(redis, user_id, socket_id, now_ms, true).await
}

async fn socket_offline_inner(
    redis: &Client,
    user_id: Uuid,
    socket_id: Uuid,
    now_ms: i64,
    announce: bool,
) -> Result<bool, PresenceError> {
    let payload = if announce {
        status_event_json(user_id, "offline")
    } else {
        String::new()
    };
    let last: i64 = redis
        .eval(
            SOCKET_OFFLINE_SCRIPT,
            lifecycle_keys(user_id),
            vec![
                now_ms.to_string(),
                socket_id.to_string(),
                payload,
                presence_channel(user_id),
                user_id.to_string(),
            ],
        )
        .await?;
    Ok(last == 1)
}

/// Sweeper step for one user (see [`sweep_expired`]); `true` when the user's
/// last socket expired (1 → 0, `offline` announced unless hidden).
pub async fn expire_user(
    redis: &Client,
    user_id: Uuid,
    now_ms: i64,
) -> Result<bool, PresenceError> {
    let last: i64 = redis
        .eval(
            EXPIRE_SCRIPT,
            lifecycle_keys(user_id),
            vec![
                now_ms.to_string(),
                status_event_json(user_id, "offline"),
                presence_channel(user_id),
                user_id.to_string(),
            ],
        )
        .await?;
    Ok(last == 1)
}

/// Publish a status the user chose (versioned) and set the hidden flag:
/// `hidden = true` for invisible. `status = None` publishes nothing.
pub async fn publish_chosen_status(
    redis: &Client,
    user_id: Uuid,
    status: Option<&str>,
    hidden: bool,
) -> Result<(), PresenceError> {
    let payload = status.map(|s| status_event_json(user_id, s));
    let _: i64 = redis
        .eval(
            SET_STATUS_SCRIPT,
            lifecycle_keys(user_id),
            vec![
                if hidden { "1" } else { "0" }.to_string(),
                payload.unwrap_or_default(),
                presence_channel(user_id),
            ],
        )
        .await?;
    Ok(())
}

/// Publish `status` (versioned) only if the user has a live socket and is not
/// hidden; `true` if published.
pub async fn announce_if_live(
    redis: &Client,
    user_id: Uuid,
    status: &str,
    now_ms: i64,
) -> Result<bool, PresenceError> {
    let done: i64 = redis
        .eval(
            ANNOUNCE_IF_LIVE_SCRIPT,
            lifecycle_keys(user_id),
            vec![
                now_ms.to_string(),
                status_event_json(user_id, status),
                presence_channel(user_id),
            ],
        )
        .await?;
    Ok(done == 1)
}

/// The users among `user_ids` that have at least one live socket. One Valkey
/// round trip, whatever the number of users.
pub async fn live_users(
    redis: &Client,
    user_ids: &[Uuid],
    now_ms: i64,
) -> Result<HashSet<Uuid>, PresenceError> {
    Ok(live_snapshot(redis, user_ids, now_ms).await?.live)
}

/// Live users and presence versions, read in one atomic Valkey step.
#[derive(Debug, Default)]
pub struct LiveSnapshot {
    /// Users with at least one live socket.
    pub live: HashSet<Uuid>,
    /// Presence version per user (0 = never announced).
    pub versions: HashMap<Uuid, u64>,
}

/// [`LiveSnapshot`] of `user_ids` (one round trip).
pub async fn live_snapshot(
    redis: &Client,
    user_ids: &[Uuid],
    now_ms: i64,
) -> Result<LiveSnapshot, PresenceError> {
    if user_ids.is_empty() {
        return Ok(LiveSnapshot::default());
    }
    let keys: Vec<String> = user_ids
        .iter()
        .flat_map(|id| [sockets_key(*id), version_key(*id)])
        .collect();
    let values: Vec<i64> = redis
        .eval(LIVE_SNAPSHOT_SCRIPT, keys, vec![now_ms.to_string()])
        .await?;
    let mut snapshot = LiveSnapshot::default();
    for (id, pair) in user_ids.iter().zip(values.chunks(2)) {
        if pair.first().copied().unwrap_or(0) > 0 {
            snapshot.live.insert(*id);
        }
        let version = pair.get(1).copied().unwrap_or(0);
        snapshot
            .versions
            .insert(*id, u64::try_from(version).unwrap_or(0));
    }
    Ok(snapshot)
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

/// How many due users one sweep handles at most.
pub const SWEEP_BATCH: i64 = 200;

/// How often the sweeper runs.
pub const SWEEP_INTERVAL: Duration = Duration::from_secs(20);

/// Announce `offline` for users whose last socket expired without a disconnect.
///
/// Covers crashed servers and failed disconnect commands. Handles at most
/// `limit` users whose latest expiry is before `now_ms`, each in one atomic
/// step that cannot race a concurrent connect, then resets a stored `online`
/// to `offline` for those that went offline. Returns those users.
pub async fn sweep_expired(
    db: &sqlx::PgPool,
    redis: &Client,
    now_ms: i64,
    limit: i64,
) -> Result<Vec<Uuid>, PresenceError> {
    let due: Vec<String> = redis
        // Expiries are positive Unix milliseconds.
        .zrangebyscore(DUE_KEY, 0_i64, now_ms, false, Some((0, limit)))
        .await?;
    let mut offline = Vec::new();
    for id in due {
        let Ok(user_id) = Uuid::parse_str(&id) else {
            let _: i64 = redis.zrem(DUE_KEY, id).await?;
            continue;
        };
        if expire_user(redis, user_id, now_ms).await? {
            offline.push(user_id);
        }
    }
    if !offline.is_empty() {
        if let Err(e) = sqlx::query(
            "UPDATE users SET status = 'offline'::user_status WHERE id = ANY($1) AND status = 'online'",
        )
        .bind(&offline)
        .execute(db)
        .await
        {
            tracing::warn!(error = %e, "Presence sweep: status reset failed");
        }
    }
    Ok(offline)
}

/// Spawn the presence sweeper ([`sweep_expired`] every [`SWEEP_INTERVAL`]).
pub fn spawn_presence_sweep(db: sqlx::PgPool, redis: Client) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(SWEEP_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            if let Err(e) = sweep_expired(&db, &redis, now_ms(), SWEEP_BATCH).await {
                log_unavailable(&e);
            }
        }
    })
}

/// A presence update as published on `presence:{user_id}`.
///
/// The event plus the user's presence version. Internal: sockets unwrap it
/// and forward `event` only if `v` is newer than what they already sent for
/// that user.
#[derive(Debug, serde::Deserialize)]
pub struct PresenceEnvelope {
    /// Presence version (`presence:ver:{user_id}`).
    pub v: u64,
    /// The event to forward.
    pub event: crate::ws::ServerEvent,
}

/// Internal notice on `user:{id}` / `guild:{id}`: the presence audience changed.
///
/// Sent when friendship, blocking or server membership changes. Sockets
/// recompute their presence peers; the notice is never forwarded.
#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PresenceNotice {
    /// Recompute presence peers, block sets and server subscriptions.
    PresenceAudienceChanged,
}

/// Tell the sockets of `users` (and of every member of `guild`) to recompute
/// their presence peers. Best effort: failures are logged (throttled).
pub async fn notify_audience_changed(redis: &Client, users: &[Uuid], guild: Option<Uuid>) {
    let Ok(payload) = serde_json::to_string(&PresenceNotice::PresenceAudienceChanged) else {
        return;
    };
    let channels = users
        .iter()
        .map(|id| crate::ws::channels::user_events(*id))
        .chain(guild.map(crate::ws::channels::guild_events));
    for channel in channels {
        let sent: Result<(), fred::error::Error> = redis.publish(&channel, payload.as_str()).await;
        if let Err(e) = sent {
            log_unavailable(&PresenceError(e));
        }
    }
}

/// JSON of a `presence_update` event.
#[must_use]
pub fn status_event_json(user_id: Uuid, status: &str) -> String {
    serde_json::to_string(&crate::ws::ServerEvent::PresenceUpdate {
        user_id,
        status: status.to_string(),
    })
    .unwrap_or_default()
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
