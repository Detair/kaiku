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
//! `users.invisible` together with the `users.presence_seq` it reflects),
//! connect, disconnect and expiry announce nothing. Writes of the flag carry
//! the sequence and an older write never replaces a newer one, so reordered
//! or failed writes cannot leave it stale; every connect re-applies it from
//! the database.
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

/// Valkey key that mirrors `users.invisible` as `"{presence_seq}:{0|1}"`:
/// while it says hidden, lifecycle changes (connect, disconnect, expiry)
/// announce nothing.
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

/// Lua helpers shared by the scripts.
///
/// - `hidden()`: the hidden flag (KEYS[4], value `"{seq}:{0|1}"`) says invisible.
/// - `set_hidden(seq, bit)`: store the flag unless it already reflects a newer status sequence;
///   returns whether it was stored.
/// - `publish(payload, channel)`: publish as a versioned envelope (`INCR` and `PUBLISH` in the same
///   script, so version order is publish order).
/// - `announce(payload, channel)`: `publish` unless the payload is empty or the user is hidden.
macro_rules! lua_announce {
    () => {
        r#"
local function hidden()
  local flag = redis.call('GET', KEYS[4])
  return flag ~= false and string.sub(flag, -1) == '1'
end
local function set_hidden(seq, bit)
  local flag = redis.call('GET', KEYS[4])
  if flag then
    local stored = tonumber(string.match(flag, '^(%d+):'))
    if stored and stored > tonumber(seq) then
      return false
    end
  end
  redis.call('SET', KEYS[4], seq .. ':' .. bit)
  return true
end
local function publish(payload, channel)
  local v = redis.call('INCR', KEYS[3])
  redis.call('PUBLISH', channel, '{"v":' .. v .. ',"event":' .. payload .. '}')
end
local function announce(payload, channel)
  if payload == '' or hidden() then
    return
  end
  publish(payload, channel)
end
"#
    };
}

/// Register (or refresh) a socket. Prunes expired members first, so a socket
/// whose own entry had expired counts as new again, and raises the user's
/// entry in the due set.
///
/// ARGV = now ms, expiry ms, socket id, key TTL ms, event payload (empty:
/// publish nothing), publish channel, user id, status sequence and hidden bit
/// read from the database (empty: unknown, keep the flag). Returns 1 on 0 → 1.
const SOCKET_ONLINE_SCRIPT: &str = concat!(
    lua_announce!(),
    r"
if ARGV[8] ~= '' then
  set_hidden(ARGV[8], ARGV[9])
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

/// Remove a socket, then prune expired members. Reports 1 → 0 when the set is
/// empty afterwards and this call removed anything (its own socket, expired
/// ones) or the user's due entry: the due entry marks "online, `offline` not
/// announced yet" and survives the key's own TTL. So an `offline` is never
/// lost and never sent twice.
///
/// ARGV = now ms, socket id, event payload, publish channel, user id.
const SOCKET_OFFLINE_SCRIPT: &str = concat!(
    lua_announce!(),
    r"
local removed = redis.call('ZREM', KEYS[1], ARGV[2])
local pruned = redis.call('ZREMRANGEBYSCORE', KEYS[1], '-inf', ARGV[1])
local after = redis.call('ZCARD', KEYS[1])
if after > 0 then
  return 0
end
local was_due = redis.call('ZREM', KEYS[2], ARGV[5])
if removed + pruned + was_due > 0 then
  announce(ARGV[3], ARGV[4])
  return 1
end
return 0
"
);

/// Sweeper step for one user: prune expired sockets; if the set is empty
/// (also when the key itself has expired), announce the 1 → 0 transition if
/// the user was still due, and drop the due entry; otherwise set the due score
/// to the latest remaining expiry.
///
/// ARGV = now ms, event payload, publish channel, user id.
const EXPIRE_SCRIPT: &str = concat!(
    lua_announce!(),
    r"
local pruned = redis.call('ZREMRANGEBYSCORE', KEYS[1], '-inf', ARGV[1])
local after = redis.call('ZCARD', KEYS[1])
if after == 0 then
  local was_due = redis.call('ZREM', KEYS[2], ARGV[4])
  if pruned + was_due > 0 then
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

/// Status change chosen by the user: store the hidden flag for this status
/// sequence, then publish (versioned) unless the payload is empty. A write
/// older than the stored flag changes and publishes nothing (a newer status
/// change already did). The hidden flag does not suppress this publish:
/// entering invisible announces `offline` once.
///
/// ARGV = status sequence, hidden bit, event payload, publish channel.
/// Returns 1 if applied.
const SET_STATUS_SCRIPT: &str = concat!(
    lua_announce!(),
    r"
if not set_hidden(ARGV[1], ARGV[2]) then
  return 0
end
if ARGV[3] ~= '' then
  publish(ARGV[3], ARGV[4])
end
return 1
"
);

/// Late announcement (a socket that could not announce at registration, or a
/// status change whose publish failed): store the hidden flag for the status
/// sequence read from the database, then publish. Invisible: the `offline`
/// payload is published regardless (the one intentional announcement).
/// Visible: only while the user has a live socket. Nothing if a newer status
/// change already wrote the flag.
///
/// ARGV = now ms, status sequence, hidden bit, event payload, publish channel.
const SYNC_ANNOUNCE_SCRIPT: &str = concat!(
    lua_announce!(),
    r"
if not set_hidden(ARGV[2], ARGV[3]) then
  return 0
end
if ARGV[3] == '1' then
  publish(ARGV[4], ARGV[5])
  return 1
end
if redis.call('ZCOUNT', KEYS[1], '(' .. ARGV[1], '+inf') == 0 then
  return 0
end
announce(ARGV[4], ARGV[5])
return 1
"
);

/// Live socket count, presence version and hidden bit per user. KEYS =
/// socket set, version key, hidden flag, repeating.
const LIVE_SNAPSHOT_SCRIPT: &str = r"
local out = {}
for i = 1, #KEYS, 3 do
  out[#out + 1] = redis.call('ZCOUNT', KEYS[i], '(' .. ARGV[1], '+inf')
  out[#out + 1] = tonumber(redis.call('GET', KEYS[i + 1]) or '0')
  local flag = redis.call('GET', KEYS[i + 2])
  out[#out + 1] = (flag and string.sub(flag, -1) == '1') and 1 or 0
end
return out
";

fn ttl_ms() -> i64 {
    i64::try_from(SOCKET_TTL.as_millis()).unwrap_or(i64::MAX)
}

/// TTL of the socket-set key: the members' TTL plus room for three sweeps, so
/// the key normally outlives its last member. Correctness does not depend on
/// it (the due entry marks a pending `offline`); it keeps pruning cheap.
fn key_ttl_ms() -> i64 {
    let headroom = i64::try_from(SWEEP_INTERVAL.as_millis()).unwrap_or(0) * 3;
    ttl_ms().saturating_add(headroom)
}

/// A user's chosen visibility as of a status sequence (`users.presence_seq`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Visibility {
    /// `users.presence_seq` the value was read or written at.
    pub seq: i64,
    /// `users.invisible`.
    pub hidden: bool,
}

impl Visibility {
    const fn bit(self) -> &'static str {
        if self.hidden {
            "1"
        } else {
            "0"
        }
    }
}

/// Register or refresh `socket_id`; `true` when it is the user's first live
/// socket (0 → 1). Publishes nothing.
pub async fn socket_online(
    redis: &Client,
    user_id: Uuid,
    socket_id: Uuid,
    now_ms: i64,
) -> Result<bool, PresenceError> {
    socket_online_announce(redis, user_id, socket_id, now_ms, None, None).await
}

/// [`socket_online`] that also announces the 0 → 1 transition.
///
/// On the transition it publishes `announce` (versioned) on
/// `presence:{user_id}` in the same atomic step, unless the user is hidden.
/// `visibility` (read from the database) updates the hidden flag first,
/// unless the flag reflects a newer status change.
pub async fn socket_online_announce(
    redis: &Client,
    user_id: Uuid,
    socket_id: Uuid,
    now_ms: i64,
    announce: Option<&str>,
    visibility: Option<Visibility>,
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
                key_ttl_ms().to_string(),
                announce.unwrap_or_default().to_string(),
                presence_channel(user_id),
                user_id.to_string(),
                visibility.map(|v| v.seq.to_string()).unwrap_or_default(),
                visibility
                    .map(Visibility::bit)
                    .unwrap_or_default()
                    .to_string(),
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

/// Publish a status the user chose (versioned) and store the hidden flag.
///
/// The flag is stored for `visibility`; `status = None` publishes nothing.
/// Returns `false` if a newer status change already wrote the flag (nothing
/// changed or published).
pub async fn publish_chosen_status(
    redis: &Client,
    user_id: Uuid,
    status: Option<&str>,
    visibility: Visibility,
) -> Result<bool, PresenceError> {
    let payload = status.map(|s| status_event_json(user_id, s));
    let applied: i64 = redis
        .eval(
            SET_STATUS_SCRIPT,
            lifecycle_keys(user_id),
            vec![
                visibility.seq.to_string(),
                visibility.bit().to_string(),
                payload.unwrap_or_default(),
                presence_channel(user_id),
            ],
        )
        .await?;
    Ok(applied == 1)
}

/// Late announcement: repair the hidden flag, then publish `status`.
///
/// The flag is stored for `visibility` unless a newer status change wrote it.
/// An invisible user's `offline` is published regardless of live sockets; a
/// visible status only while the user has a live socket. Returns whether
/// something was published.
pub async fn sync_and_announce(
    redis: &Client,
    user_id: Uuid,
    visibility: Visibility,
    status: &str,
    now_ms: i64,
) -> Result<bool, PresenceError> {
    let done: i64 = redis
        .eval(
            SYNC_ANNOUNCE_SCRIPT,
            lifecycle_keys(user_id),
            vec![
                now_ms.to_string(),
                visibility.seq.to_string(),
                visibility.bit().to_string(),
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
    /// Users whose hidden flag says invisible.
    pub hidden: HashSet<Uuid>,
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
        .flat_map(|id| [sockets_key(*id), version_key(*id), hidden_key(*id)])
        .collect();
    let values: Vec<i64> = redis
        .eval(LIVE_SNAPSHOT_SCRIPT, keys, vec![now_ms.to_string()])
        .await?;
    let mut snapshot = LiveSnapshot::default();
    for (id, row) in user_ids.iter().zip(values.chunks(3)) {
        if row.first().copied().unwrap_or(0) > 0 {
            snapshot.live.insert(*id);
        }
        let version = row.get(1).copied().unwrap_or(0);
        snapshot
            .versions
            .insert(*id, u64::try_from(version).unwrap_or(0));
        if row.get(2).copied().unwrap_or(0) == 1 {
            snapshot.hidden.insert(*id);
        }
    }
    Ok(snapshot)
}

/// [`live_snapshot`], failing open: on a Valkey error it logs (throttled)
/// and returns `None`.
pub async fn live_snapshot_or_log(redis: &Client, user_ids: &[Uuid]) -> Option<LiveSnapshot> {
    match live_snapshot(redis, user_ids, now_ms()).await {
        Ok(snapshot) => Some(snapshot),
        Err(e) => {
            log_unavailable(&e);
            None
        }
    }
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
    /// Recompute the audience. With `user_id` (server membership change of
    /// that user, sent on `guild:{id}`), other members only re-check that one
    /// user; without it, or for that user's own sockets, everything is
    /// reloaded.
    PresenceAudienceChanged {
        /// The user whose membership changed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        user_id: Option<Uuid>,
    },
}

/// Tell the sockets of `users` to reload their presence audience. Best
/// effort: failures are logged (throttled).
pub async fn notify_audience_changed(redis: &Client, users: &[Uuid]) {
    let notice = PresenceNotice::PresenceAudienceChanged { user_id: None };
    let channels: Vec<String> = users
        .iter()
        .map(|id| crate::ws::channels::user_events(*id))
        .collect();
    publish_notice(redis, &notice, channels).await;
}

/// A server's membership changed: `changed` joined or left (members re-check
/// only that user; `changed` itself reloads everything), or `None` (server
/// deleted: everyone reloads).
pub async fn notify_guild_audience_changed(redis: &Client, guild_id: Uuid, changed: Option<Uuid>) {
    let notice = PresenceNotice::PresenceAudienceChanged { user_id: changed };
    let mut channels = vec![crate::ws::channels::guild_events(guild_id)];
    if let Some(user) = changed {
        // The user may not follow the server channel (yet).
        channels.push(crate::ws::channels::user_events(user));
    }
    publish_notice(redis, &notice, channels).await;
}

async fn publish_notice(redis: &Client, notice: &PresenceNotice, channels: Vec<String>) {
    let Ok(payload) = serde_json::to_string(notice) else {
        return;
    };
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
