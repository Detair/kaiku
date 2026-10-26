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
//! Every published status change carries a per-user version, and the last
//! published state (`presence:state:{user_id}`: status and version) is
//! written in the same script that publishes. Snapshots read only that
//! state, so a snapshot is always a published state with its own version;
//! sockets drop anything older than what they already sent, so snapshot and
//! live updates cannot overwrite each other out of order.
//!
//! Sockets that end without a disconnect (crashed server, a failed disconnect
//! command) are covered by the sweeper ([`sweep_expired`]): the global set
//! `presence:due` holds each user's latest expiry, and an atomic expire step
//! announces `offline` once the last socket has expired.
//!
//! Status changes and recovery go through one atomic reconcile step
//! ([`reconcile`]): it records the status sequence (`users.presence_seq`) in
//! `presence:seq:{user_id}`, ignores anything older than what was applied,
//! and publishes only what differs from the last status shown
//! (`presence:state:{user_id}`). Reordered, repeated or retried commands
//! from several devices therefore announce each change once, in database
//! order. A status change that is not reconciled at once stays marked dirty
//! in the database (`users.presence_dirty_since`) and the sweeper on any
//! server reconciles it ([`sweep_dirty`]).
//!
//! **Stored vs. shown status.** `users.status` holds the status the user chose
//! (`online`, `away`, `busy`; `offline` once the last socket closed). What
//! other people see comes from [`effective_status`], which also needs to know
//! whether the user has a live socket ([`live_users`]).

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
/// presence: the `users.status` value. Expects the users table aliased as
/// `u`. Turn it into what others see with [`effective_status`].
macro_rules! stored_presence_sql {
    () => {
        "u.status::text"
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

/// Valkey hash with a user's last **published** presence.
///
/// Fields: `status` (`online`, `away`, `busy`, `offline`; missing =
/// `offline`) and `v`, its version (incremented with every status publish);
/// `activity` / `custom` (the last published `rich_presence_update` /
/// `custom_status_update` event JSON) with their own versions `av` / `cv`.
/// Written only by the publishing scripts, in the same step as the publish,
/// so a snapshot read from it is always a state observers were (or will be)
/// sent, with its own version.
#[must_use]
pub fn state_key(user_id: Uuid) -> String {
    format!("presence:state:{user_id}")
}

/// Valkey key holding the newest status sequence (`users.presence_seq`) whose
/// state has been applied. A status change with an older sequence (two
/// devices, reordered) changes nothing.
#[must_use]
pub fn seq_key(user_id: Uuid) -> String {
    format!("presence:seq:{user_id}")
}

/// Keys every presence script gets, in this order: the user's socket set, the
/// global due set, the user's published state, the user's status sequence.
fn lifecycle_keys(user_id: Uuid) -> Vec<String> {
    vec![
        sockets_key(user_id),
        DUE_KEY.to_string(),
        state_key(user_id),
        seq_key(user_id),
    ]
}

/// Lua helpers shared by the scripts. Every script gets the user id as
/// ARGV[1] and the current time (ms) as ARGV[2].
///
/// - `applied_seq()`: the newest status sequence applied (KEYS[4]), -1 if none.
/// - `shown()`: the last status published (KEYS[3] `status`), `offline` if none.
/// - `publish(status)`: bump the version (KEYS[3] `v`), record the status as published and publish
///   a `presence_update` as a versioned envelope, all in the same script (version order is publish
///   order).
/// - `show(status)`: `publish` unless observers already see `status`.
/// - `live()`: the user has a live socket.
macro_rules! lua_presence {
    () => {
        r#"
local channel = 'presence:' .. ARGV[1]
local function applied_seq()
  return tonumber(redis.call('GET', KEYS[4]) or '-1')
end
local function shown()
  return redis.call('HGET', KEYS[3], 'status') or 'offline'
end
local function publish(status)
  local v = redis.call('HINCRBY', KEYS[3], 'v', 1)
  redis.call('HSET', KEYS[3], 'status', status)
  local event = '{"type":"presence_update","user_id":"' .. ARGV[1] .. '","status":"' .. status .. '"}'
  redis.call('PUBLISH', channel, '{"v":' .. v .. ',"event":' .. event .. '}')
end
local function show(status)
  if status ~= '' and shown() ~= status then
    publish(status)
  end
end
local function live()
  return redis.call('ZCOUNT', KEYS[1], '(' .. ARGV[2], '+inf') > 0
end
"#
    };
}

/// Register (or refresh) a socket. Prunes expired members first, so a socket
/// whose own entry had expired counts as new again, and raises the user's
/// entry in the due set.
///
/// ARGV = user id, now ms, expiry ms, socket id, key TTL ms, status to show
/// on 0 → 1 (empty: nothing), status sequence that status was read at (empty:
/// unknown).
///
/// The status is shown only if no newer status change has been applied: a
/// stale read never publishes (the caller reconciles instead). Returns 1 on
/// 0 → 1, plus 2 if the read was stale.
const SOCKET_ONLINE_SCRIPT: &str = concat!(
    lua_presence!(),
    r"
local stale = false
if ARGV[7] ~= '' then
  if applied_seq() > tonumber(ARGV[7]) then
    stale = true
  else
    redis.call('SET', KEYS[4], ARGV[7])
  end
end
redis.call('ZREMRANGEBYSCORE', KEYS[1], '-inf', ARGV[2])
local before = redis.call('ZCARD', KEYS[1])
redis.call('ZADD', KEYS[1], ARGV[3], ARGV[4])
redis.call('PEXPIRE', KEYS[1], ARGV[5])
redis.call('ZADD', KEYS[2], 'GT', ARGV[3], ARGV[1])
local result = 0
if stale then
  result = 2
end
if before == 0 then
  if not stale then
    show(ARGV[6])
  end
  return result + 1
end
return result
"
);

/// Remove a socket, then prune expired members. Reports 1 → 0 when the set is
/// empty afterwards and this call removed anything (its own socket, expired
/// ones) or the user's due entry: the due entry marks "online, `offline` not
/// announced yet" and survives the key's own TTL. So an `offline` is never
/// lost and never sent twice.
///
/// ARGV = user id, now ms, socket id, "1" to announce.
const SOCKET_OFFLINE_SCRIPT: &str = concat!(
    lua_presence!(),
    r"
local removed = redis.call('ZREM', KEYS[1], ARGV[3])
local pruned = redis.call('ZREMRANGEBYSCORE', KEYS[1], '-inf', ARGV[2])
local after = redis.call('ZCARD', KEYS[1])
if after > 0 then
  return 0
end
local was_due = redis.call('ZREM', KEYS[2], ARGV[1])
if removed + pruned + was_due > 0 then
  if ARGV[4] == '1' then
    show('offline')
  end
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
/// ARGV = user id, now ms.
const EXPIRE_SCRIPT: &str = concat!(
    lua_presence!(),
    r"
local pruned = redis.call('ZREMRANGEBYSCORE', KEYS[1], '-inf', ARGV[2])
local after = redis.call('ZCARD', KEYS[1])
if after == 0 then
  local was_due = redis.call('ZREM', KEYS[2], ARGV[1])
  if pruned + was_due > 0 then
    show('offline')
    return 1
  end
  return 0
end
local top = redis.call('ZRANGE', KEYS[1], -1, -1, 'WITHSCORES')
redis.call('ZADD', KEYS[2], top[2], ARGV[1])
return 0
"
);

/// Bring what observers see in line with the user's status as of a status
/// sequence read from (or just written to) the database.
///
/// If a newer sequence has been applied, nothing changes and that sequence is
/// returned: the newer change reconciles itself. Otherwise the sequence is
/// recorded and observers are shown `offline` without a live socket, else the
/// status; a `presence_update` goes out only if that differs from what they
/// last saw.
///
/// ARGV = user id, now ms, status sequence, status.
/// Returns `{1, 1 if published}` or `{0, newer applied sequence}`.
const RECONCILE_SCRIPT: &str = concat!(
    lua_presence!(),
    r"
local applied = applied_seq()
if applied > tonumber(ARGV[3]) then
  return {0, applied}
end
redis.call('SET', KEYS[4], ARGV[3])
local target = ARGV[4]
if not live() then
  target = 'offline'
end
if shown() ~= target then
  publish(target)
  return {1, 1}
end
return {1, 0}
"
);

/// Replace the applied status sequence with `ARGV[4]` only if it still holds
/// exactly `ARGV[3]` (a sequence the database does not know, e.g. after a
/// restore). Compare-and-set, so a concurrent newer change is never undone.
///
/// ARGV = user id, now ms, expected sequence, new sequence.
const REPLACE_FUTURE_SEQ_SCRIPT: &str = r"
if redis.call('GET', KEYS[4]) == ARGV[3] then
  redis.call('SET', KEYS[4], ARGV[4])
  return 1
end
return 0
";

/// Live socket count per user. KEYS = socket sets.
const LIVE_COUNTS_SCRIPT: &str = r"
local out = {}
for i, key in ipairs(KEYS) do
  out[i] = redis.call('ZCOUNT', key, '(' .. ARGV[1], '+inf')
end
return out
";

/// Published state per user. KEYS = state hashes. Returns status (empty if
/// never published), version, activity event (empty if none), its version,
/// custom status event (empty if none), its version, repeating.
const PUBLISHED_STATES_SCRIPT: &str = r"
local out = {}
for _, key in ipairs(KEYS) do
  local state = redis.call('HMGET', key, 'status', 'v', 'activity', 'av', 'custom', 'cv')
  for i = 1, 6 do
    local value = state[i]
    if not value then
      if i % 2 == 0 then value = '0' else value = '' end
    end
    out[#out + 1] = value
  end
end
return out
";

/// Publish a rich-presence event (activity or custom status) as a versioned
/// envelope and record it as published, in one step. With ARGV[5] = '1' only
/// if it differs from what was last published (reconcile).
///
/// ARGV = user id, now ms, kind (`activity` | `custom`), event JSON, only-if-
/// different flag. Returns 1 if published.
const PUBLISH_RICH_SCRIPT: &str = r#"
local field = ARGV[3]
if ARGV[5] == '1' and redis.call('HGET', KEYS[3], field) == ARGV[4] then
  return 0
end
local version_field = 'av'
if field == 'custom' then
  version_field = 'cv'
end
local v = redis.call('HINCRBY', KEYS[3], version_field, 1)
redis.call('HSET', KEYS[3], field, ARGV[4])
redis.call('PUBLISH', 'presence:' .. ARGV[1],
  '{"rv":' .. v .. ',"kind":"' .. field .. '","event":' .. ARGV[4] .. '}')
return 1
"#;

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

/// Register or refresh `socket_id`; `true` when it is the user's first live
/// socket (0 → 1). Publishes nothing.
pub async fn socket_online(
    redis: &Client,
    user_id: Uuid,
    socket_id: Uuid,
    now_ms: i64,
) -> Result<bool, PresenceError> {
    socket_online_detailed(redis, user_id, socket_id, now_ms, None)
        .await
        .map(|outcome| outcome.first)
}

/// What registering a socket found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OnlineOutcome {
    /// First live socket (0 → 1).
    pub first: bool,
    /// A newer status change has been applied than the one read: nothing was
    /// shown, and the caller reconciles from a fresh database read.
    pub stale: bool,
}

/// Register or refresh `socket_id`.
///
/// `status` is the status read from the database with its status sequence.
/// On the 0 → 1 transition it is shown (versioned, unless observers already
/// see it) in the same atomic step, unless a newer status change has been
/// applied meanwhile.
pub async fn socket_online_detailed(
    redis: &Client,
    user_id: Uuid,
    socket_id: Uuid,
    now_ms: i64,
    status: Option<(&str, i64)>,
) -> Result<OnlineOutcome, PresenceError> {
    let ttl = ttl_ms();
    let result: i64 = redis
        .eval(
            SOCKET_ONLINE_SCRIPT,
            lifecycle_keys(user_id),
            vec![
                user_id.to_string(),
                now_ms.to_string(),
                now_ms.saturating_add(ttl).to_string(),
                socket_id.to_string(),
                key_ttl_ms().to_string(),
                status.map(|(s, _)| s.to_string()).unwrap_or_default(),
                status.map(|(_, seq)| seq.to_string()).unwrap_or_default(),
            ],
        )
        .await?;
    Ok(OnlineOutcome {
        first: result & 1 == 1,
        stale: result & 2 == 2,
    })
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
/// `presence:{user_id}` in the same atomic step when the transition happens.
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
    let last: i64 = redis
        .eval(
            SOCKET_OFFLINE_SCRIPT,
            lifecycle_keys(user_id),
            vec![
                user_id.to_string(),
                now_ms.to_string(),
                socket_id.to_string(),
                if announce { "1" } else { "0" }.to_string(),
            ],
        )
        .await?;
    Ok(last == 1)
}

/// Sweeper step for one user (see [`sweep_users`]); `true` when the user's
/// last socket expired (1 → 0, `offline` announced).
pub async fn expire_user(
    redis: &Client,
    user_id: Uuid,
    now_ms: i64,
) -> Result<bool, PresenceError> {
    let last: i64 = redis
        .eval(
            EXPIRE_SCRIPT,
            lifecycle_keys(user_id),
            vec![user_id.to_string(), now_ms.to_string()],
        )
        .await?;
    Ok(last == 1)
}

/// What [`reconcile`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reconciled {
    /// Applied; `published` tells whether a status was published.
    Applied {
        /// A `presence_update` was published.
        published: bool,
    },
    /// A newer status sequence has been applied; nothing changed.
    Ahead {
        /// The applied sequence found.
        applied: i64,
    },
}

/// Make observers see `status` as of status sequence `seq`.
///
/// Atomic with recording the sequence (see [`RECONCILE_SCRIPT`]): status
/// changes, late announcements and stale registrations all go through here.
pub async fn reconcile(
    redis: &Client,
    user_id: Uuid,
    seq: i64,
    status: &str,
    now_ms: i64,
) -> Result<Reconciled, PresenceError> {
    let (applied, detail): (i64, i64) = redis
        .eval(
            RECONCILE_SCRIPT,
            lifecycle_keys(user_id),
            vec![
                user_id.to_string(),
                now_ms.to_string(),
                seq.to_string(),
                status.to_string(),
            ],
        )
        .await?;
    if applied == 1 {
        Ok(Reconciled::Applied {
            published: detail == 1,
        })
    } else {
        Ok(Reconciled::Ahead { applied: detail })
    }
}

/// Replace an applied status sequence that is ahead of the database (unknown
/// to it, e.g. after a restore) with `seq`, but only if it still holds exactly
/// `expected`; `true` if replaced.
pub async fn replace_future_seq(
    redis: &Client,
    user_id: Uuid,
    expected: i64,
    seq: i64,
) -> Result<bool, PresenceError> {
    let replaced: i64 = redis
        .eval(
            REPLACE_FUTURE_SEQ_SCRIPT,
            lifecycle_keys(user_id),
            vec![
                user_id.to_string(),
                String::new(),
                expected.to_string(),
                seq.to_string(),
            ],
        )
        .await?;
    Ok(replaced == 1)
}

/// Publish a rich-presence event (activity or custom status) as a versioned
/// envelope and record it as published (`presence:state`), in one step.
pub async fn publish_rich(
    redis: &Client,
    user_id: Uuid,
    event: &crate::ws::ServerEvent,
) -> Result<(), PresenceError> {
    publish_rich_inner(redis, user_id, event, false)
        .await
        .map(|_| ())
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

/// A user's last published presence (see [`state_key`]).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Published {
    /// Status observers were sent (`offline` if never published).
    pub status: String,
    /// Its version (0 if never published).
    pub version: u64,
    /// Last published `rich_presence_update` event JSON, if any.
    pub activity: Option<String>,
    /// Its version.
    pub activity_version: u64,
    /// Last published `custom_status_update` event JSON, if any.
    pub custom: Option<String>,
    /// Its version.
    pub custom_version: u64,
}

/// The last published presence of `user_ids`, read in one atomic step.
pub async fn published_states(
    redis: &Client,
    user_ids: &[Uuid],
) -> Result<HashMap<Uuid, Published>, PresenceError> {
    if user_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let keys: Vec<String> = user_ids.iter().map(|id| state_key(*id)).collect();
    let values: Vec<String> = redis
        .eval(PUBLISHED_STATES_SCRIPT, keys, Vec::<String>::new())
        .await?;
    let text = |v: Option<&String>| v.filter(|v| !v.is_empty()).cloned();
    let number = |v: Option<&String>| v.and_then(|v| v.parse().ok()).unwrap_or(0);
    Ok(user_ids
        .iter()
        .zip(values.chunks(6))
        .map(|(id, row)| {
            (
                *id,
                Published {
                    status: text(row.first()).unwrap_or_else(|| "offline".to_string()),
                    version: number(row.get(1)),
                    activity: text(row.get(2)),
                    activity_version: number(row.get(3)),
                    custom: text(row.get(4)),
                    custom_version: number(row.get(5)),
                },
            )
        })
        .collect())
}

/// Which rich-presence field an event belongs to.
const fn rich_kind(event: &crate::ws::ServerEvent) -> Option<&'static str> {
    match event {
        crate::ws::ServerEvent::RichPresenceUpdate { .. } => Some("activity"),
        crate::ws::ServerEvent::CustomStatusUpdate { .. } => Some("custom"),
        _ => None,
    }
}

async fn publish_rich_inner(
    redis: &Client,
    user_id: Uuid,
    event: &crate::ws::ServerEvent,
    only_if_different: bool,
) -> Result<bool, PresenceError> {
    let (Some(kind), Ok(payload)) = (rich_kind(event), serde_json::to_string(event)) else {
        return Ok(false);
    };
    let published: i64 = redis
        .eval(
            PUBLISH_RICH_SCRIPT,
            lifecycle_keys(user_id),
            vec![
                user_id.to_string(),
                String::new(),
                kind.to_string(),
                payload,
                if only_if_different { "1" } else { "0" }.to_string(),
            ],
        )
        .await?;
    Ok(published == 1)
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

/// Reconcile a user's published presence from a fresh database read and,
/// on success, clear the user's dirty mark (`users.presence_dirty_since`) up
/// to the status sequence reconciled.
///
/// If a newer status sequence than the database's has been applied: when the
/// database has caught up, a newer change applied it and reconciles itself;
/// otherwise the sequence is unknown to the database (e.g. after a restore)
/// and is replaced compare-and-set on the exact value found (a concurrent
/// newer change is never undone), then the reconcile is repeated.
///
/// `Err(())` (database or Valkey unreachable) means "still owed": the user
/// stays dirty (or the caller retries).
pub async fn reconcile_from_db(db: &sqlx::PgPool, redis: &Client, user_id: Uuid) -> Result<(), ()> {
    for _ in 0..3 {
        let (stored, seq) = stored_status(db, user_id).await?;
        let shown = effective_status(&stored, Some(true));
        let reconciled = reconcile(redis, user_id, seq, shown, now_ms())
            .await
            .map_err(|e| log_unavailable(&e))?;
        let applied = match reconciled {
            Reconciled::Applied { .. } => {
                clear_dirty(db, user_id, seq).await;
                return reconcile_rich_from_db(db, redis, user_id).await;
            }
            Reconciled::Ahead { applied } => applied,
        };
        let (_, db_seq) = stored_status(db, user_id).await?;
        if db_seq >= applied {
            return Ok(());
        }
        replace_future_seq(redis, user_id, applied, db_seq)
            .await
            .map_err(|e| log_unavailable(&e))?;
    }
    Ok(())
}

/// Publish the user's stored activity and custom status where they differ
/// from what was last published (heals a lost or reordered rich update).
async fn reconcile_rich_from_db(
    db: &sqlx::PgPool,
    redis: &Client,
    user_id: Uuid,
) -> Result<(), ()> {
    let (activity, custom_status): (Option<serde_json::Value>, Option<serde_json::Value>) =
        sqlx::query_as("SELECT activity, custom_status FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(db)
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, %user_id, "Failed to read rich presence");
            })?;
    let activity = crate::ws::ServerEvent::RichPresenceUpdate {
        user_id,
        activity: activity.and_then(|a| serde_json::from_value(a).ok()),
    };
    let custom = crate::ws::ServerEvent::CustomStatusUpdate {
        user_id,
        custom_status: custom_status.and_then(|c| serde_json::from_value(c).ok()),
    };
    for event in [activity, custom] {
        publish_rich_inner(redis, user_id, &event, true)
            .await
            .map_err(|e| log_unavailable(&e))?;
    }
    Ok(())
}

/// The user's stored status and status sequence.
async fn stored_status(db: &sqlx::PgPool, user_id: Uuid) -> Result<(String, i64), ()> {
    sqlx::query_as("SELECT status::text, presence_seq FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(db)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, %user_id, "Failed to read presence");
        })
}

/// Clear the dirty mark if no newer status change happened since `seq`.
pub async fn clear_dirty(db: &sqlx::PgPool, user_id: Uuid, seq: i64) {
    if let Err(e) = sqlx::query(
        "UPDATE users SET presence_dirty_since = NULL WHERE id = $1 AND presence_seq <= $2",
    )
    .bind(user_id)
    .bind(seq)
    .execute(db)
    .await
    {
        tracing::warn!(error = %e, %user_id, "Failed to clear presence dirty mark");
    }
}

/// How long a status change may stay unreconciled before the sweeper takes
/// over (the changing socket normally reconciles at once).
pub const DIRTY_GRACE: Duration = Duration::from_secs(5);

/// Reconcile users whose status change was not (yet) published.
///
/// Covers users marked dirty by `set_status` and still dirty after
/// [`DIRTY_GRACE`] (Valkey failed, the server died, the socket closed). Runs
/// on every server, so the retry does not depend on the socket that made the
/// change. Returns the users reconciled.
pub async fn sweep_dirty(db: &sqlx::PgPool, redis: &Client, limit: i64) -> Vec<Uuid> {
    let grace = i64::try_from(DIRTY_GRACE.as_millis()).unwrap_or(0);
    let dirty: Vec<(Uuid,)> = match sqlx::query_as(
        "SELECT id FROM users
         WHERE presence_dirty_since IS NOT NULL
           AND presence_dirty_since < NOW() - make_interval(secs => $1::double precision / 1000)
         ORDER BY presence_dirty_since
         LIMIT $2",
    )
    .bind(grace)
    .bind(limit)
    .fetch_all(db)
    .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(error = %e, "Presence sweep: dirty query failed");
            return Vec::new();
        }
    };
    let mut done = Vec::new();
    for (user_id,) in dirty {
        if reconcile_from_db(db, redis, user_id).await.is_ok() {
            done.push(user_id);
        }
    }
    done
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
    let mut users = Vec::with_capacity(due.len());
    for id in due {
        match Uuid::parse_str(&id) {
            Ok(user_id) => users.push(user_id),
            Err(_) => {
                let _: i64 = redis.zrem(DUE_KEY, id).await?;
            }
        }
    }
    sweep_users(db, redis, &users, now_ms).await
}

/// The sweep for given users only (see [`sweep_expired`]).
///
/// Each runs the atomic expire step; a user whose last socket expired gets
/// `offline` and a stored `online` reset to `offline`.
/// Returns those users.
pub async fn sweep_users(
    db: &sqlx::PgPool,
    redis: &Client,
    users: &[Uuid],
    now_ms: i64,
) -> Result<Vec<Uuid>, PresenceError> {
    let mut offline = Vec::new();
    for user_id in users {
        if expire_user(redis, *user_id, now_ms).await? {
            offline.push(*user_id);
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

/// How often every connected user's published presence is reconciled
/// against the database and their live sockets ([`reconcile_connected`]).
pub const RECONCILE_INTERVAL: Duration = Duration::from_mins(1);

/// Self-healing: reconcile the published presence of every connected user.
///
/// Covers every user with registered sockets (the due set, at most `limit`),
/// from a fresh database read and their live sockets. Publishes (versioned)
/// only where the published status differs, so any status that went stale for
/// whatever reason (lost publish, missed dirty mark, a race) is corrected
/// within one [`RECONCILE_INTERVAL`]. Returns how many users were reconciled.
pub async fn reconcile_connected(
    db: &sqlx::PgPool,
    redis: &Client,
    limit: i64,
) -> Result<usize, PresenceError> {
    let population: i64 = redis.zcard(DUE_KEY).await?;
    let cursor: Option<i64> = redis.get(RECONCILE_CURSOR_KEY).await?;
    let (ranges, next) = reconcile_window(population, cursor.unwrap_or(0), limit);
    let mut users = Vec::new();
    for (start, stop) in ranges {
        let ids: Vec<String> = redis
            .zrange(DUE_KEY, start, stop, None, false, None, false)
            .await?;
        users.extend(ids.iter().filter_map(|id| Uuid::parse_str(id).ok()));
    }
    let _: () = redis
        .set(RECONCILE_CURSOR_KEY, next, None, None, false)
        .await?;
    Ok(reconcile_users(db, redis, &users).await)
}

/// Rank offset in the due set where the next periodic reconcile continues.
pub const RECONCILE_CURSOR_KEY: &str = "presence:reconcile_cursor";

/// Which ranks of the due set one periodic reconcile covers.
///
/// For a due set of `population` users, starting at `cursor`, at most `limit`
/// users; returns the rank ranges (inclusive) and the next cursor. Everyone
/// when the population fits; otherwise a rotating window, so every connected
/// user is reconciled at least once every `ceil(population / limit)` runs.
#[must_use]
pub fn reconcile_window(population: i64, cursor: i64, limit: i64) -> (Vec<(i64, i64)>, i64) {
    if population <= 0 || limit <= 0 {
        return (Vec::new(), 0);
    }
    if population <= limit {
        return (vec![(0, population - 1)], 0);
    }
    let start = cursor.rem_euclid(population);
    let end = start + limit - 1;
    if end < population {
        (vec![(start, end)], (end + 1) % population)
    } else {
        let wrapped = end - population;
        (vec![(start, population - 1), (0, wrapped)], wrapped + 1)
    }
}

/// Reconcile the given users from a fresh database read (see
/// [`reconcile_from_db`]); returns how many succeeded.
pub async fn reconcile_users(db: &sqlx::PgPool, redis: &Client, users: &[Uuid]) -> usize {
    let mut done = 0;
    for user_id in users {
        if reconcile_from_db(db, redis, *user_id).await.is_ok() {
            done += 1;
        }
    }
    done
}

/// Spawn the presence sweeper.
///
/// Runs [`sweep_expired`] and [`sweep_dirty`] every [`SWEEP_INTERVAL`] and
/// [`reconcile_connected`] every [`RECONCILE_INTERVAL`] (first run after a
/// random offset, so servers do not run it in step).
pub fn spawn_presence_sweep(db: sqlx::PgPool, redis: Client) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut sweep = tokio::time::interval(SWEEP_INTERVAL);
        sweep.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let offset =
            Duration::from_millis(u64::try_from(Uuid::new_v4().as_u128() % 10_000).unwrap_or(0));
        let mut reconcile = tokio::time::interval_at(
            tokio::time::Instant::now() + RECONCILE_INTERVAL + offset,
            RECONCILE_INTERVAL,
        );
        reconcile.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = sweep.tick() => {
                    if let Err(e) = sweep_expired(&db, &redis, now_ms(), SWEEP_BATCH).await {
                        log_unavailable(&e);
                    }
                    sweep_dirty(&db, &redis, SWEEP_BATCH).await;
                }
                _ = reconcile.tick() => {
                    if let Err(e) = reconcile_connected(&db, &redis, RECONCILE_BATCH).await {
                        log_unavailable(&e);
                    }
                }
            }
        }
    })
}

/// How many connected users one periodic reconcile handles at most.
pub const RECONCILE_BATCH: i64 = 1000;

/// A rich-presence update as published on `presence:{user_id}`.
///
/// The event plus its kind (`activity` | `custom`) and that kind's version.
/// Internal: sockets forward `event` only if `rv` is newer than what they
/// sent.
#[derive(Debug, serde::Deserialize)]
pub struct RichEnvelope {
    /// Version of this kind (`av` / `cv` in `presence:state:{user_id}`).
    pub rv: u64,
    /// `activity` or `custom`.
    pub kind: String,
    /// The event to forward.
    pub event: crate::ws::ServerEvent,
}

/// A presence update as published on `presence:{user_id}`.
///
/// The event plus the user's presence version. Internal: sockets unwrap it
/// and forward `event` only if `v` is newer than what they already sent for
/// that user.
#[derive(Debug, serde::Deserialize)]
pub struct PresenceEnvelope {
    /// Presence version (`v` in `presence:state:{user_id}`).
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
        /// The counterpart whose relation to the recipient changed (friend,
        /// block, server membership). Without it (own server membership,
        /// server deleted) everything is reloaded.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        user_id: Option<Uuid>,
    },
    /// A socket's own fence (on its `user:{id}` channel): once the socket has
    /// processed it, every notice published before it has been applied.
    PresenceFence {
        /// Matches the waiting socket.
        id: Uuid,
    },
}

/// The relation between `a` and `b` changed (friendship, block).
///
/// Each one's sockets get a notice naming the other, stop forwarding them at
/// once and re-check them. Best effort: failures are logged (throttled).
pub async fn notify_pair_changed(redis: &Client, a: Uuid, b: Uuid) {
    for (recipient, counterpart) in [(a, b), (b, a)] {
        let notice = PresenceNotice::PresenceAudienceChanged {
            user_id: Some(counterpart),
        };
        publish_notice(
            redis,
            &notice,
            vec![crate::ws::channels::user_events(recipient)],
        )
        .await;
    }
}

/// A server's membership changed: `changed` joined or left (members get a
/// notice naming that user; `changed` itself reloads everything), or `None`
/// (server deleted: everyone reloads).
pub async fn notify_guild_audience_changed(redis: &Client, guild_id: Uuid, changed: Option<Uuid>) {
    let notice = PresenceNotice::PresenceAudienceChanged { user_id: changed };
    publish_notice(
        redis,
        &notice,
        vec![crate::ws::channels::guild_events(guild_id)],
    )
    .await;
    if let Some(user) = changed {
        // The user may not follow the server channel (yet): a full reload.
        let own = PresenceNotice::PresenceAudienceChanged { user_id: None };
        publish_notice(redis, &own, vec![crate::ws::channels::user_events(user)]).await;
    }
}

/// Publish a fence for one socket of `user_id` (see
/// [`PresenceNotice::PresenceFence`]).
pub async fn publish_fence(redis: &Client, user_id: Uuid, id: Uuid) -> Result<(), PresenceError> {
    let payload = serde_json::to_string(&PresenceNotice::PresenceFence { id }).unwrap_or_default();
    let sent: Result<(), fred::error::Error> = redis
        .publish(crate::ws::channels::user_events(user_id), payload)
        .await;
    Ok(sent?)
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

/// What other people see, from the stored status (`stored_presence_sql!`)
/// and whether the user has a live socket.
///
/// `live = None` means the socket sets could not be read (Valkey down): the
/// stored status is shown as is.
#[must_use]
pub fn effective_status(stored: &str, live: Option<bool>) -> &str {
    match (stored, live) {
        (_, Some(false)) => "offline",
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
    fn reconcile_window_covers_everyone_within_ceil_n_over_bound() {
        // Population fits: everyone, every run.
        assert_eq!(reconcile_window(5, 3, 10), (vec![(0, 4)], 0));
        // Bound smaller than the population: every rank within ceil(n/bound)
        // runs, from any starting cursor.
        for (population, limit) in [(7_i64, 3_i64), (10, 4), (9, 3), (1000, 7)] {
            let runs = (population + limit - 1) / limit;
            for start in [0, 1, population - 1] {
                let mut seen = std::collections::HashSet::new();
                let mut cursor = start;
                for _ in 0..runs {
                    let (ranges, next) = reconcile_window(population, cursor, limit);
                    let covered: i64 = ranges.iter().map(|(a, b)| b - a + 1).sum();
                    assert_eq!(covered, limit, "{population}/{limit}");
                    for (a, b) in ranges {
                        seen.extend(a..=b);
                    }
                    cursor = next;
                }
                assert_eq!(
                    seen.len() as i64,
                    population,
                    "{population}/{limit} from {start}"
                );
            }
        }
        assert_eq!(reconcile_window(0, 0, 10), (Vec::new(), 0));
    }

    #[test]
    fn effective_status_needs_a_live_socket() {
        assert_eq!(effective_status("online", Some(true)), "online");
        assert_eq!(effective_status("busy", Some(true)), "busy");
        assert_eq!(effective_status("away", Some(true)), "away");
        assert_eq!(effective_status("offline", Some(true)), "online");
        // Left behind by a crashed server, or a manual status while away.
        assert_eq!(effective_status("online", Some(false)), "offline");
        assert_eq!(effective_status("busy", Some(false)), "offline");
        // Valkey down: stored status.
        assert_eq!(effective_status("busy", None), "busy");
    }
}
