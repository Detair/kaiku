//! Valkey Streams-backed call service for DM voice calls.
//!
//! # Storage
//!
//! A call lives in the stream `call_events:{channel_id}`. Its first entry
//! (`Started`) carries a random `call_id` (the call **generation**), the target
//! users and the start time; later entries are the call events. While a call
//! rings, `call_ringing_by:{initiator}` holds the member
//! `{channel_id}:{call_id}` (used to cancel it when the initiator goes offline).
//!
//! # Concurrency
//!
//! - **Start** is one Lua script: create the stream only if none exists, index it and publish
//!   `incoming_call` (to the targets) and `call_started` (to the initiator's sessions).
//! - **Every other change** is a compare-and-set in one Lua script: it applies only if the stream
//!   still has the `call_id` **and** the last entry id that the caller read. On a mismatch the
//!   caller re-reads and decides again.
//! - **Generation pinning**: an operation pins the `call_id` it first observed (or was given, e.g.
//!   the ring timer). If a retry finds another call, the operation stops as a no-op ("call not
//!   found"): it never acts on a replacement call.
//! - A call that ends is deleted (and un-indexed) in the same script, so a new call can start right
//!   away. The script also **publishes the call's events** (`call_ended`, `call_declined`, ...) to
//!   the members' user channels, so a call's `call_ended` always precedes the next call's
//!   `incoming_call`. All call events carry `call_id`; clients match on it.
//! - Recipients are the DM's current participants, read right before the script runs.
//!
//! Streams carry TTLs as backstops only: ringing = ring timeout + 30 s (the
//! ring timer normally ends the call first), active = 6 h, refreshed on every
//! join/leave. An active call whose voice room is empty and that saw no event
//! for [`ORPHAN_GRACE_SECS`] counts as orphaned (e.g. after a restart) and is
//! ended (`last_left`) when someone starts a new call; this assumes the voice
//! room lives on this node (single-node deployment).
//!
//! The scripts touch two keys (stream and ringing index); Kaiku runs a single
//! Valkey, not a cluster.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use fred::prelude::*;
use sqlx::PgPool;
use uuid::Uuid;

use crate::api::AppState;
use crate::voice::call::{CallEventType, CallState, EndReason};
use crate::ws::events::channels;
use crate::ws::ServerEvent;

/// Ring timeout: a call nobody answers ends after this many seconds.
pub const RING_TIMEOUT_SECS: u64 = 90;
/// Extra lifetime of a ringing stream beyond the ring timeout. The ring timer
/// normally ends the call (with a `call_ended` event); the TTL only cleans up
/// if the timer was lost.
const RING_TTL_GRACE_SECS: u64 = 30;
/// Backstop lifetime of an active call stream, refreshed on every join/leave.
pub const ACTIVE_TTL_SECS: u64 = 6 * 60 * 60;
/// An active call whose voice room is empty counts as orphaned once its last
/// event is this old (protects a call that was just accepted and whose
/// members are still connecting to the room).
pub const ORPHAN_GRACE_SECS: i64 = 30;
/// How long after a socket closes the initiator's open sockets are counted
/// (once) before their ringing calls are cancelled. Covers a closing socket's
/// QUIT still in flight and quick reconnects.
pub const DISCONNECT_GRACE: Duration = Duration::from_secs(3);
/// Attempts for one compare-and-set transition before giving up.
const MAX_CAS_ATTEMPTS: usize = 8;

/// Reason a call ended (name used by the call lifecycle API).
pub type CallEndReason = EndReason;

/// Start a call only if the channel has none. Returns 1 if started.
///
/// KEYS: stream, ringing index of the initiator.
/// ARGV: `call_id`, Started JSON, targets JSON, start ms, TTL, index member,
/// then (user channel, payload) pairs to publish.
const START_SCRIPT: &str = r"
if redis.call('EXISTS', KEYS[1]) == 1 then return 0 end
redis.call('XADD', KEYS[1], '*', 'call_id', ARGV[1], 'data', ARGV[2], 'targets', ARGV[3], 'started_ms', ARGV[4])
redis.call('EXPIRE', KEYS[1], ARGV[5])
redis.call('SADD', KEYS[2], ARGV[6])
redis.call('EXPIRE', KEYS[2], ARGV[5])
for i = 7, #ARGV, 2 do redis.call('PUBLISH', ARGV[i], ARGV[i + 1]) end
return 1
";

/// Compare-and-set on a call stream. Returns 1 if applied, 0 if the call
/// changed (different generation or a newer last entry) or is gone.
///
/// KEYS: stream, ringing index of the initiator.
/// ARGV: `call_id`, last entry id, op (`append` | `delete`), event JSON,
/// TTL to set (`0` keeps the current one), index member, then (user channel,
/// payload) pairs to publish once applied.
const CAS_SCRIPT: &str = r"
local last = redis.call('XREVRANGE', KEYS[1], '+', '-', 'COUNT', 1)
if #last == 0 or last[1][1] ~= ARGV[2] then return 0 end
local first = redis.call('XRANGE', KEYS[1], '-', '+', 'COUNT', 1)
local gen = ''
local fields = first[1][2]
for i = 1, #fields, 2 do
  if fields[i] == 'call_id' then gen = fields[i + 1] end
end
if gen ~= ARGV[1] then return 0 end
if ARGV[3] == 'delete' then
  redis.call('DEL', KEYS[1])
  redis.call('SREM', KEYS[2], ARGV[6])
else
  redis.call('XADD', KEYS[1], '*', 'data', ARGV[4])
  if tonumber(ARGV[5]) > 0 then redis.call('EXPIRE', KEYS[1], ARGV[5]) end
end
for i = 7, #ARGV, 2 do redis.call('PUBLISH', ARGV[i], ARGV[i + 1]) end
return 1
";

/// Identity of one call (one generation of a channel's call stream).
#[derive(Debug, Clone)]
pub struct CallMeta {
    /// Random id of this call; differs for every call on the channel (empty
    /// for streams written before generations existed).
    pub call_id: String,
    /// Id of the newest stream entry when the call was read.
    pub last_id: String,
    /// Who started the call.
    pub initiator: Uuid,
    /// Users the call rang (DM participants except the initiator).
    pub targets: HashSet<Uuid>,
    /// When the call started (server clock).
    pub started_at: DateTime<Utc>,
}

impl CallMeta {
    /// Everyone the call rang plus the initiator (fallback recipients).
    #[must_use]
    pub fn members(&self) -> Vec<Uuid> {
        let mut members: Vec<Uuid> = self.targets.iter().copied().collect();
        if !self.targets.contains(&self.initiator) {
            members.push(self.initiator);
        }
        members
    }

    /// `call_id` as sent on the wire (nil for a stream without one).
    #[must_use]
    pub fn call_uuid(&self) -> Uuid {
        Uuid::parse_str(&self.call_id).unwrap_or_else(|_| Uuid::nil())
    }

    /// Seconds since the call's last event (stream entry ids start with the
    /// Valkey time in ms).
    fn idle_secs(&self) -> i64 {
        let last_ms = self
            .last_id
            .split('-')
            .next()
            .and_then(|ms| ms.parse::<i64>().ok())
            .unwrap_or(0);
        (Utc::now().timestamp_millis() - last_ms) / 1000
    }
}

/// A call stream as read: `state` is `None` if the stream is unusable (an
/// already ended or corrupt leftover), which a new call may replace.
struct LoadedCall {
    state: Option<CallState>,
    meta: CallMeta,
}

/// Result of a successful transition.
#[derive(Debug, Clone)]
pub struct Transition {
    /// State after the event (`Ended` if the transition ended the call).
    pub state: CallState,
    /// The call the event was applied to.
    pub meta: CallMeta,
}

/// The `call_ended` event for `state`, if the call ended.
fn ended_event(channel_id: Uuid, state: &CallState, meta: &CallMeta) -> Option<ServerEvent> {
    match state {
        CallState::Ended {
            reason,
            duration_secs,
            ..
        } => Some(ServerEvent::CallEnded {
            channel_id,
            call_id: meta.call_uuid(),
            reason: end_reason_name(*reason),
            duration_secs: *duration_secs,
        }),
        _ => None,
    }
}

/// Wire name of an end reason (`all_declined`, `no_answer`, ...).
#[must_use]
pub fn end_reason_name(reason: EndReason) -> String {
    serde_json::to_value(reason)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Call service for managing DM voice call state
#[derive(Clone)]
pub struct CallService {
    redis: Client,
    db: PgPool,
}

impl CallService {
    pub const fn new(redis: Client, db: PgPool) -> Self {
        Self { redis, db }
    }

    /// Get Redis stream key for a channel's call events
    fn stream_key(channel_id: Uuid) -> String {
        format!("call_events:{channel_id}")
    }

    /// Set of `{channel_id}:{call_id}` calls that `user_id` started and that
    /// may be ringing.
    fn ringing_index_key(user_id: Uuid) -> String {
        format!("call_ringing_by:{user_id}")
    }

    fn index_member(channel_id: Uuid, call_id: &str) -> String {
        format!("{channel_id}:{call_id}")
    }

    /// Read and replay a channel's call stream.
    async fn load(&self, channel_id: Uuid) -> Result<Option<LoadedCall>, CallError> {
        let key = Self::stream_key(channel_id);
        let events: Vec<(String, HashMap<String, String>)> = self
            .redis
            .xrange_values(&key, "-", "+", None)
            .await
            .map_err(|e| CallError::Redis(e.to_string()))?;

        let Some(((_, first), (last_id, _))) = events.first().zip(events.last()) else {
            return Ok(None);
        };

        let started_at = first
            .get("started_ms")
            .and_then(|ms| ms.parse::<i64>().ok())
            .and_then(|ms| Utc.timestamp_millis_opt(ms).single())
            .unwrap_or_else(Utc::now);
        let targets: HashSet<Uuid> = first
            .get("targets")
            .and_then(|t| serde_json::from_str(t).ok())
            .unwrap_or_default();
        let initiator = first
            .get("data")
            .and_then(|d| serde_json::from_str::<CallEventType>(d).ok())
            .and_then(|e| match e {
                CallEventType::Started { initiator } => Some(initiator),
                _ => None,
            });
        let meta = CallMeta {
            call_id: first.get("call_id").cloned().unwrap_or_default(),
            last_id: last_id.clone(),
            initiator: initiator.unwrap_or_else(Uuid::nil),
            targets: targets.clone(),
            started_at,
        };

        let Some(initiator) = initiator else {
            tracing::warn!(%channel_id, "Call stream does not start with Started; treating as stale");
            return Ok(Some(LoadedCall { state: None, meta }));
        };

        let mut state = CallState::Ringing {
            started_by: initiator,
            started_at,
            declined_by: HashSet::new(),
            target_users: targets,
        };
        for (id, fields) in events.iter().skip(1) {
            let event = fields
                .get("data")
                .and_then(|d| serde_json::from_str::<CallEventType>(d).ok());
            let next = event
                .ok_or(())
                .and_then(|e| state.apply(&e).map_err(|_| ()));
            let Ok(next) = next else {
                tracing::warn!(%channel_id, entry = %id, "Invalid call stream entry; treating as stale");
                return Ok(Some(LoadedCall { state: None, meta }));
            };
            state = next;
        }

        Ok(Some(LoadedCall {
            state: state.is_active().then_some(state),
            meta,
        }))
    }

    /// The channel's current (not ended) call and its identity.
    pub async fn get_call(
        &self,
        channel_id: Uuid,
    ) -> Result<Option<(CallState, CallMeta)>, CallError> {
        Ok(self
            .load(channel_id)
            .await?
            .and_then(|c| c.state.map(|s| (s, c.meta))))
    }

    /// Get the current (not ended) call state of a channel.
    #[tracing::instrument(skip(self))]
    pub async fn get_call_state(&self, channel_id: Uuid) -> Result<Option<CallState>, CallError> {
        Ok(self.get_call(channel_id).await?.map(|(s, _)| s))
    }

    /// The `call_id` of the channel's current call, if any.
    pub async fn current_call_id(&self, channel_id: Uuid) -> Result<Option<String>, CallError> {
        Ok(self.get_call(channel_id).await?.map(|(_, m)| m.call_id))
    }

    /// Who gets the call's events: the DM's current participants (the call's
    /// own members if they cannot be read).
    async fn recipients(&self, channel_id: Uuid, meta: &CallMeta) -> Vec<Uuid> {
        match crate::voice::queries::list_dm_participant_ids(&self.db, channel_id).await {
            Ok(ids) if !ids.is_empty() => ids,
            Ok(_) => meta.members(),
            Err(e) => {
                tracing::warn!(error = %e, %channel_id, "Cannot read DM participants; using call members");
                meta.members()
            }
        }
    }

    /// Script arguments publishing every event to every recipient (in event
    /// order per recipient).
    fn publish_args(recipients: &[Uuid], events: &[ServerEvent]) -> Result<Vec<String>, CallError> {
        let mut args = Vec::with_capacity(recipients.len() * events.len() * 2);
        for event in events {
            let payload = serde_json::to_string(event)
                .map_err(|e| CallError::Serialization(e.to_string()))?;
            for &user in recipients {
                args.push(channels::user_events(user));
                args.push(payload.clone());
            }
        }
        Ok(args)
    }

    async fn cas(
        &self,
        channel_id: Uuid,
        meta: &CallMeta,
        op: &str,
        event_json: &str,
        ttl_secs: u64,
        publish: Vec<String>,
    ) -> Result<bool, CallError> {
        let mut args = vec![
            meta.call_id.clone(),
            meta.last_id.clone(),
            op.to_string(),
            event_json.to_string(),
            ttl_secs.to_string(),
            Self::index_member(channel_id, &meta.call_id),
        ];
        args.extend(publish);
        let applied: i64 = self
            .redis
            .eval(
                CAS_SCRIPT,
                vec![
                    Self::stream_key(channel_id),
                    Self::ringing_index_key(meta.initiator),
                ],
                args,
            )
            .await
            .map_err(|e| CallError::Redis(e.to_string()))?;
        Ok(applied == 1)
    }

    /// Start a new call, ring the targets (`incoming_call`) and start its ring
    /// timer.
    ///
    /// Fails with [`CallError::CallAlreadyExists`] if the channel has a call.
    /// An ended or corrupt leftover is replaced; an orphaned active call (voice
    /// room empty, `room_occupied == false`, idle for [`ORPHAN_GRACE_SECS`]) is
    /// ended as `last_left` first.
    #[tracing::instrument(skip(self, initiator_name))]
    pub async fn start_call(
        &self,
        channel_id: Uuid,
        initiator: Uuid,
        initiator_name: String,
        target_users: HashSet<Uuid>,
        ring_timeout_secs: u64,
        room_occupied: bool,
    ) -> Result<Transition, CallError> {
        let key = Self::stream_key(channel_id);
        let index = Self::ringing_index_key(initiator);
        let event_json = serde_json::to_string(&CallEventType::Started { initiator })
            .map_err(|e| CallError::Serialization(e.to_string()))?;
        let targets_json = serde_json::to_string(&target_users)
            .map_err(|e| CallError::Serialization(e.to_string()))?;
        let targets: Vec<Uuid> = target_users.iter().copied().collect();

        for _ in 0..MAX_CAS_ATTEMPTS {
            let call_uuid = Uuid::new_v4();
            let call_id = call_uuid.to_string();
            let started_at = Utc::now();
            let incoming = ServerEvent::IncomingCall {
                channel_id,
                call_id: call_uuid,
                initiator,
                initiator_name: initiator_name.clone(),
                // Default capabilities: audio only for now
                capabilities: vec!["audio".to_string()],
            };
            let mut args = vec![
                call_id.clone(),
                event_json.clone(),
                targets_json.clone(),
                started_at.timestamp_millis().to_string(),
                (ring_timeout_secs + RING_TTL_GRACE_SECS).to_string(),
                Self::index_member(channel_id, &call_id),
            ];
            args.extend(Self::publish_args(&targets, &[incoming])?);
            // The initiator's own sessions learn the new call (and its id).
            let started_event = ServerEvent::CallStarted {
                channel_id,
                call_id: call_uuid,
                initiator,
                initiator_name: initiator_name.clone(),
                capabilities: vec!["audio".to_string()],
            };
            args.extend(Self::publish_args(&[initiator], &[started_event])?);
            let started: i64 = self
                .redis
                .eval(START_SCRIPT, vec![key.clone(), index.clone()], args)
                .await
                .map_err(|e| CallError::Redis(e.to_string()))?;

            if started == 1 {
                spawn_ring_timeout(
                    self.clone(),
                    channel_id,
                    call_id.clone(),
                    Duration::from_secs(ring_timeout_secs),
                );
                let state = CallState::Ringing {
                    started_by: initiator,
                    started_at,
                    declined_by: HashSet::new(),
                    target_users: target_users.clone(),
                };
                let meta = CallMeta {
                    call_id,
                    last_id: String::new(),
                    initiator,
                    targets: target_users,
                    started_at,
                };
                return Ok(Transition { state, meta });
            }

            match self.load(channel_id).await? {
                Some(LoadedCall {
                    state: Some(CallState::Active { .. }),
                    meta,
                }) if !room_occupied && meta.idle_secs() >= ORPHAN_GRACE_SECS => {
                    tracing::info!(%channel_id, call_id = %meta.call_id, "Ending orphaned active call");
                    self.end_call_when(channel_id, Some(&meta.call_id), |state, _| {
                        matches!(state, CallState::Active { .. }).then_some(EndReason::LastLeft)
                    })
                    .await?;
                }
                Some(LoadedCall { state: Some(_), .. }) => {
                    return Err(CallError::CallAlreadyExists);
                }
                // Leftover of an ended or broken call: remove it, then retry.
                Some(LoadedCall { state: None, meta }) => {
                    self.cas(channel_id, &meta, "delete", "", 0, Vec::new())
                        .await?;
                }
                // Vanished in between: retry.
                None => {}
            }
        }
        Err(CallError::Contention)
    }

    /// Apply the event chosen by `decide` to the current call, atomically,
    /// and publish the events from `announce`.
    ///
    /// The operation is pinned to `pin` (or else to the first call it reads):
    /// if the channel then holds another call, it returns
    /// [`CallError::CallNotFound`] without touching it. `decide` sees the
    /// current state and call and returns the event, or `None` to leave the
    /// call alone (`Ok(None)`). If the call changed in between, it is re-read
    /// and `decide` runs again. A transition that ends the call deletes it.
    /// `before_cas` runs right before each compare-and-set (a test seam;
    /// production passes a no-op).
    pub async fn transition_hooked<D, A, H, Fut>(
        &self,
        channel_id: Uuid,
        pin: Option<&str>,
        mut decide: D,
        mut announce: A,
        mut before_cas: H,
    ) -> Result<Option<Transition>, CallError>
    where
        D: FnMut(&CallState, &CallMeta) -> Result<Option<CallEventType>, CallError>,
        A: FnMut(&CallEventType, &CallState, &CallMeta) -> Vec<ServerEvent>,
        H: FnMut() -> Fut,
        Fut: Future<Output = ()>,
    {
        let mut pinned = pin.map(str::to_string);
        for _ in 0..MAX_CAS_ATTEMPTS {
            let Some((state, meta)) = self.get_call(channel_id).await? else {
                return Err(CallError::CallNotFound);
            };
            match &pinned {
                Some(call_id) if *call_id != meta.call_id => return Err(CallError::CallNotFound),
                Some(_) => {}
                None => pinned = Some(meta.call_id.clone()),
            }
            let Some(event) = decide(&state, &meta)? else {
                return Ok(None);
            };
            let new_state = state
                .apply(&event)
                .map_err(|e| CallError::StateTransition(e.to_string()))?;
            let (op, ttl) = match &new_state {
                CallState::Ended { .. } => ("delete", 0),
                CallState::Active { .. } => ("append", ACTIVE_TTL_SECS),
                CallState::Ringing { .. } => ("append", 0),
            };
            let event_json = serde_json::to_string(&event)
                .map_err(|e| CallError::Serialization(e.to_string()))?;
            let mut events = announce(&event, &new_state, &meta);
            events.extend(ended_event(channel_id, &new_state, &meta));
            let recipients = self.recipients(channel_id, &meta).await;
            let publish = Self::publish_args(&recipients, &events)?;
            before_cas().await;
            if self
                .cas(channel_id, &meta, op, &event_json, ttl, publish)
                .await?
            {
                return Ok(Some(Transition {
                    state: new_state,
                    meta,
                }));
            }
        }
        Err(CallError::Contention)
    }

    /// [`Self::transition_hooked`] without a hook.
    pub async fn transition<D, A>(
        &self,
        channel_id: Uuid,
        pin: Option<&str>,
        decide: D,
        announce: A,
    ) -> Result<Option<Transition>, CallError>
    where
        D: FnMut(&CallState, &CallMeta) -> Result<Option<CallEventType>, CallError>,
        A: FnMut(&CallEventType, &CallState, &CallMeta) -> Vec<ServerEvent>,
    {
        self.transition_hooked(channel_id, pin, decide, announce, || async {})
            .await
    }

    /// Record a user joining the call (`call_participant_joined`).
    #[tracing::instrument(skip(self, username))]
    pub async fn join_call(
        &self,
        channel_id: Uuid,
        user_id: Uuid,
        username: String,
    ) -> Result<Transition, CallError> {
        self.transition(
            channel_id,
            None,
            |_, _| Ok(Some(CallEventType::Joined { user_id })),
            |_, _, meta| {
                vec![ServerEvent::CallParticipantJoined {
                    channel_id,
                    call_id: meta.call_uuid(),
                    user_id,
                    username: username.clone(),
                }]
            },
        )
        .await?
        .ok_or(CallError::CallNotFound)
    }

    /// Record a user declining the call (ends it once every target declined).
    #[tracing::instrument(skip(self))]
    pub async fn decline_call(
        &self,
        channel_id: Uuid,
        user_id: Uuid,
    ) -> Result<Transition, CallError> {
        self.decline_call_hooked(channel_id, user_id, || async {})
            .await
    }

    /// [`Self::decline_call`] with a hook right before the compare-and-set
    /// (test seam).
    pub async fn decline_call_hooked<H, Fut>(
        &self,
        channel_id: Uuid,
        user_id: Uuid,
        before_cas: H,
    ) -> Result<Transition, CallError>
    where
        H: FnMut() -> Fut,
        Fut: Future<Output = ()>,
    {
        self.transition_hooked(
            channel_id,
            None,
            |_, _| Ok(Some(CallEventType::Declined { user_id })),
            |_, _, meta| {
                vec![ServerEvent::CallDeclined {
                    channel_id,
                    call_id: meta.call_uuid(),
                    user_id,
                }]
            },
            before_cas,
        )
        .await?
        .ok_or(CallError::CallNotFound)
    }

    /// Record a user leaving the call
    ///
    /// This handles both:
    /// - Active calls: sends Left event
    /// - Ringing calls (initiator): ends the call as cancelled
    #[tracing::instrument(skip(self))]
    pub async fn leave_call(
        &self,
        channel_id: Uuid,
        user_id: Uuid,
    ) -> Result<Transition, CallError> {
        self.transition(
            channel_id,
            None,
            |state, _| match state {
                CallState::Ringing { started_by, .. } if *started_by == user_id => {
                    Ok(Some(CallEventType::Ended {
                        reason: EndReason::Cancelled,
                    }))
                }
                CallState::Ringing { .. } => Err(CallError::StateTransition(
                    "Use decline endpoint for recipients".into(),
                )),
                CallState::Active { .. } => Ok(Some(CallEventType::Left { user_id })),
                CallState::Ended { .. } => {
                    Err(CallError::StateTransition("Call already ended".into()))
                }
            },
            |_, _, meta| {
                vec![ServerEvent::CallParticipantLeft {
                    channel_id,
                    call_id: meta.call_uuid(),
                    user_id,
                }]
            },
        )
        .await?
        .ok_or(CallError::CallNotFound)
    }

    /// End the channel's call (pinned to `pin`, or else to the first call
    /// read) if `decide` returns a reason for it. Publishes `call_ended`.
    /// Returns the ended call, or `None` if there was no (such) call or
    /// `decide` kept it.
    pub async fn end_call_when<F>(
        &self,
        channel_id: Uuid,
        pin: Option<&str>,
        mut decide: F,
    ) -> Result<Option<Transition>, CallError>
    where
        F: FnMut(&CallState, &CallMeta) -> Option<EndReason>,
    {
        match self
            .transition(
                channel_id,
                pin,
                |state, meta| Ok(decide(state, meta).map(|reason| CallEventType::Ended { reason })),
                |_, _, _| Vec::new(),
            )
            .await
        {
            Err(CallError::CallNotFound) => Ok(None),
            other => other,
        }
    }
}

/// End the channel's current call (whatever its state), delete its state and
/// tell the DM's participants. Idempotent: no call is not an error.
pub async fn end_call(
    state: &AppState,
    channel_id: Uuid,
    reason: CallEndReason,
) -> Result<(), CallError> {
    CallService::new(state.redis.clone(), state.db.clone())
        .end_call_when(channel_id, None, |_, _| Some(reason))
        .await
        .map(|_| ())
}

/// Ring timeout of the call `call_id`: ends it as unanswered if that call is
/// still ringing. A newer call on the channel is never touched. Returns
/// whether the call was ended.
pub async fn ring_timeout_fired(
    calls: &CallService,
    channel_id: Uuid,
    call_id: &str,
) -> Result<bool, CallError> {
    Ok(calls
        .end_call_when(channel_id, Some(call_id), |state, _| {
            matches!(state, CallState::Ringing { .. }).then_some(EndReason::NoAnswer)
        })
        .await?
        .is_some())
}

/// Start the ring timer of call `call_id`.
fn spawn_ring_timeout(calls: CallService, channel_id: Uuid, call_id: String, after: Duration) {
    tokio::spawn(async move {
        tokio::time::sleep(after).await;
        if let Err(e) = ring_timeout_fired(&calls, channel_id, &call_id).await {
            tracing::warn!(error = %e, %channel_id, "Failed to end unanswered call");
        }
    });
}

/// The last voice peer of `channel_id` left; end the channel's DM call if
/// that peer belonged to it.
///
/// The peer belonged to `user_id` and connected at `peer_connected_at`. A
/// peer that connected before the call started belongs to an earlier call and
/// never ends this one. Otherwise an active call ends (`last_left`), and a
/// ringing call ends (`cancelled`) only if `user_id` started it.
pub async fn last_voice_peer_left(
    calls: &CallService,
    channel_id: Uuid,
    user_id: Uuid,
    peer_connected_at: DateTime<Utc>,
) -> Result<bool, CallError> {
    Ok(calls
        .end_call_when(channel_id, None, |state, meta| {
            if peer_connected_at < meta.started_at {
                return None;
            }
            match state {
                CallState::Active { .. } => Some(EndReason::LastLeft),
                CallState::Ringing { started_by, .. } if *started_by == user_id => {
                    Some(EndReason::Cancelled)
                }
                _ => None,
            }
        })
        .await?
        .is_some())
}

/// Whether `user_id` may have a ringing call (cheap pre-check before
/// [`caller_socket_closed`]).
pub async fn may_have_ringing_calls(redis: &Client, user_id: Uuid) -> bool {
    let exists: Result<i64, _> = redis.exists(CallService::ringing_index_key(user_id)).await;
    // On error, check anyway.
    exists.map_or(true, |n| n > 0)
}

/// Number of sockets (on any node) subscribed to `user_id`'s event channel.
async fn open_user_sockets(redis: &Client, user_id: Uuid) -> Result<i64, CallError> {
    let numsub: Vec<Value> = redis
        .pubsub_numsub(channels::user_events(user_id))
        .await
        .map_err(|e| CallError::Redis(e.to_string()))?;
    Ok(numsub.get(1).and_then(Value::as_i64).unwrap_or(0))
}

/// A socket of `user_id` closed. After [`DISCONNECT_GRACE`], if the user has
/// no open socket, the ringing calls they started end as `cancelled`.
///
/// Open sockets are counted (once, after the grace) with `PUBSUB NUMSUB` on
/// the user's event channel, which every socket subscribes to on any node.
/// Only the generations recorded in the user's ringing index are cancelled,
/// never a newer call. If a closing socket's subscriber connection stays
/// alive (its QUIT never completes), the user still counts as online and the
/// call ends by ring timeout instead.
pub async fn caller_socket_closed(calls: &CallService, user_id: Uuid) -> Result<(), CallError> {
    tokio::time::sleep(DISCONNECT_GRACE).await;
    if open_user_sockets(&calls.redis, user_id).await? > 0 {
        return Ok(());
    }

    let index = CallService::ringing_index_key(user_id);
    let members: Vec<String> = calls
        .redis
        .smembers(&index)
        .await
        .map_err(|e| CallError::Redis(e.to_string()))?;
    for member in members {
        let parsed = member
            .split_once(':')
            .and_then(|(c, g)| Uuid::parse_str(c).ok().map(|c| (c, g.to_string())));
        let Some((channel_id, call_id)) = parsed else {
            let _: Result<i64, _> = calls.redis.srem(&index, member).await;
            continue;
        };
        let ended = calls
            .end_call_when(channel_id, Some(&call_id), |state, _| match state {
                CallState::Ringing { started_by, .. } if *started_by == user_id => {
                    Some(EndReason::Cancelled)
                }
                _ => None,
            })
            .await;
        match ended {
            // The script removed the index entry.
            Ok(Some(_)) => {}
            // Not ended: drop the entry only if that generation is gone.
            Ok(None) => {
                if calls.current_call_id(channel_id).await.ok().flatten() != Some(call_id) {
                    let _: Result<i64, _> = calls.redis.srem(&index, member).await;
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, %channel_id, "Failed to cancel call of offline caller");
            }
        }
    }
    Ok(())
}

/// Call service errors
#[derive(Debug, thiserror::Error)]
pub enum CallError {
    #[error("Call not found")]
    CallNotFound,
    #[error("Call already exists")]
    CallAlreadyExists,
    #[error("Call changed concurrently; try again")]
    Contention,
    #[error("Redis error: {0}")]
    Redis(String),
    #[error("Invalid event: {0}")]
    InvalidEvent(String),
    #[error("State transition error: {0}")]
    StateTransition(String),
    #[error("Serialization error: {0}")]
    Serialization(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_key_format() {
        let channel_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let key = CallService::stream_key(channel_id);
        assert_eq!(key, "call_events:550e8400-e29b-41d4-a716-446655440000");
    }

    #[test]
    fn test_stream_key_different_uuids() {
        let uuid1 = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let uuid2 = Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap();

        let key1 = CallService::stream_key(uuid1);
        let key2 = CallService::stream_key(uuid2);

        assert_ne!(key1, key2);
        assert!(key1.starts_with("call_events:"));
        assert!(key2.starts_with("call_events:"));
    }

    #[test]
    fn test_error_display_call_not_found() {
        let err = CallError::CallNotFound;
        assert_eq!(err.to_string(), "Call not found");
    }

    #[test]
    fn test_error_display_call_already_exists() {
        let err = CallError::CallAlreadyExists;
        assert_eq!(err.to_string(), "Call already exists");
    }

    #[test]
    fn test_error_display_redis() {
        let err = CallError::Redis("connection failed".to_string());
        assert_eq!(err.to_string(), "Redis error: connection failed");
    }

    #[test]
    fn test_error_display_invalid_event() {
        let err = CallError::InvalidEvent("missing field".to_string());
        assert_eq!(err.to_string(), "Invalid event: missing field");
    }

    #[test]
    fn test_error_display_state_transition() {
        let err = CallError::StateTransition("invalid transition".to_string());
        assert_eq!(
            err.to_string(),
            "State transition error: invalid transition"
        );
    }

    #[test]
    fn test_error_display_serialization() {
        let err = CallError::Serialization("JSON error".to_string());
        assert_eq!(err.to_string(), "Serialization error: JSON error");
    }

    #[test]
    fn test_error_debug_trait() {
        let err = CallError::CallNotFound;
        // Debug trait should be implemented
        let debug_str = format!("{err:?}");
        assert!(debug_str.contains("CallNotFound"));
    }
}
