//! Valkey Streams-backed call service for DM voice calls.
//!
//! # Storage and concurrency
//!
//! A call lives in the stream `call_events:{channel_id}`. Its first entry
//! (`Started`) carries a random `call_id` (the call generation), the target
//! users and the start time; later entries are the call events.
//!
//! Every change is a compare-and-set in one Lua script: it applies only if the
//! stream still has the `call_id` **and** the last entry id that the caller
//! read. Otherwise the caller re-reads and decides again. So concurrent
//! decline/join/leave/timeout calls are serialized, a stale actor (an old ring
//! timer, a late hook) can never touch a newer call on the same channel, and
//! exactly one actor ends a call.
//!
//! A call that ends is deleted in the same atomic step, so a new call can start
//! right away (starting is "create if the key does not exist", also atomic).
//! The stream additionally carries a TTL while ringing as a backstop in case
//! the ring timer is lost (process restart).

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use fred::prelude::*;
use uuid::Uuid;

use crate::api::AppState;
use crate::voice::call::{CallEventType, CallState, EndReason};
use crate::ws::{broadcast_to_users, ServerEvent};

/// Ring timeout: a call nobody answers ends after this many seconds.
pub const RING_TIMEOUT_SECS: u64 = 90;
/// Extra lifetime of a ringing stream beyond the ring timeout. The ring timer
/// normally ends the call (with a `call_ended` event); the TTL only cleans up
/// if the timer was lost.
const RING_TTL_GRACE_SECS: u64 = 30;
/// Attempts for one compare-and-set transition before giving up.
const MAX_CAS_ATTEMPTS: usize = 8;

/// Reason a call ended (name used by the call lifecycle API).
pub type CallEndReason = EndReason;

/// Start a call only if the channel has none. Returns 1 if started.
///
/// KEYS[1] = stream; ARGV = `call_id`, Started JSON, targets JSON, start ms, TTL.
const START_SCRIPT: &str = r"
if redis.call('EXISTS', KEYS[1]) == 1 then return 0 end
redis.call('XADD', KEYS[1], '*', 'call_id', ARGV[1], 'data', ARGV[2], 'targets', ARGV[3], 'started_ms', ARGV[4])
redis.call('EXPIRE', KEYS[1], ARGV[5])
return 1
";

/// Compare-and-set on a call stream. Returns 1 if applied, 0 if the call
/// changed (different generation or a newer last entry) or is gone.
///
/// KEYS[1] = stream; ARGV = `call_id`, last entry id, op, event JSON.
/// op: `append`, `append_persist` (append and drop the ringing TTL) or
/// `delete` (the call ended).
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
  return 1
end
redis.call('XADD', KEYS[1], '*', 'data', ARGV[4])
if ARGV[3] == 'append_persist' then redis.call('PERSIST', KEYS[1]) end
return 1
";

/// Identity of one call (one generation of a channel's call stream).
#[derive(Debug, Clone)]
pub struct CallMeta {
    /// Random id of this call; differs for every call on the channel.
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
    /// Everyone in the call's DM: the initiator and the targets.
    #[must_use]
    pub fn members(&self) -> Vec<Uuid> {
        let mut members: Vec<Uuid> = self.targets.iter().copied().collect();
        if !self.targets.contains(&self.initiator) {
            members.push(self.initiator);
        }
        members
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

/// Call service for managing DM voice call state
pub struct CallService {
    redis: Client,
}

impl CallService {
    pub const fn new(redis: Client) -> Self {
        Self { redis }
    }

    /// Get Redis stream key for a channel's call events
    fn stream_key(channel_id: Uuid) -> String {
        format!("call_events:{channel_id}")
    }

    /// Set of channels on which `user_id` started a call that may be ringing.
    fn ringing_index_key(user_id: Uuid) -> String {
        format!("call_ringing_by:{user_id}")
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
        let mut meta = CallMeta {
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
        meta.initiator = initiator;

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

    /// Get the current (not ended) call state of a channel.
    #[tracing::instrument(skip(self))]
    pub async fn get_call_state(&self, channel_id: Uuid) -> Result<Option<CallState>, CallError> {
        Ok(self.load(channel_id).await?.and_then(|c| c.state))
    }

    /// The `call_id` of the channel's current call, if any.
    pub async fn current_call_id(&self, channel_id: Uuid) -> Result<Option<String>, CallError> {
        Ok(self
            .load(channel_id)
            .await?
            .filter(|c| c.state.is_some())
            .map(|c| c.meta.call_id))
    }

    async fn cas(
        &self,
        channel_id: Uuid,
        meta: &CallMeta,
        op: &str,
        event_json: &str,
    ) -> Result<bool, CallError> {
        let applied: i64 = self
            .redis
            .eval(
                CAS_SCRIPT,
                vec![Self::stream_key(channel_id)],
                vec![
                    meta.call_id.clone(),
                    meta.last_id.clone(),
                    op.to_string(),
                    event_json.to_string(),
                ],
            )
            .await
            .map_err(|e| CallError::Redis(e.to_string()))?;
        Ok(applied == 1)
    }

    /// Start a new call. Fails with [`CallError::CallAlreadyExists`] if the
    /// channel has a call; an ended or corrupt leftover is replaced.
    #[tracing::instrument(skip(self))]
    pub async fn start_call(
        &self,
        channel_id: Uuid,
        initiator: Uuid,
        target_users: HashSet<Uuid>,
        ring_timeout_secs: u64,
    ) -> Result<Transition, CallError> {
        let key = Self::stream_key(channel_id);
        let event_json = serde_json::to_string(&CallEventType::Started { initiator })
            .map_err(|e| CallError::Serialization(e.to_string()))?;
        let targets_json = serde_json::to_string(&target_users)
            .map_err(|e| CallError::Serialization(e.to_string()))?;

        for _ in 0..MAX_CAS_ATTEMPTS {
            let call_id = Uuid::new_v4().to_string();
            let started_at = Utc::now();
            let started: i64 = self
                .redis
                .eval(
                    START_SCRIPT,
                    vec![key.clone()],
                    vec![
                        call_id.clone(),
                        event_json.clone(),
                        targets_json.clone(),
                        started_at.timestamp_millis().to_string(),
                        (ring_timeout_secs + RING_TTL_GRACE_SECS).to_string(),
                    ],
                )
                .await
                .map_err(|e| CallError::Redis(e.to_string()))?;

            if started == 1 {
                self.index_ringing(initiator, channel_id, ring_timeout_secs)
                    .await;
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
                Some(LoadedCall { state: Some(_), .. }) => {
                    return Err(CallError::CallAlreadyExists);
                }
                // Leftover of an ended or broken call: remove it, then retry.
                Some(LoadedCall { state: None, meta }) => {
                    self.cas(channel_id, &meta, "delete", "").await?;
                }
                // Vanished in between: retry.
                None => {}
            }
        }
        Err(CallError::Contention)
    }

    /// Remember that `initiator` has a ringing call on `channel_id` (used to
    /// cancel it when the initiator goes offline). Best effort.
    async fn index_ringing(&self, initiator: Uuid, channel_id: Uuid, ring_timeout_secs: u64) {
        let key = Self::ringing_index_key(initiator);
        let ttl = i64::try_from(ring_timeout_secs + RING_TTL_GRACE_SECS).unwrap_or(i64::MAX);
        let added: Result<i64, _> = self.redis.sadd(&key, channel_id.to_string()).await;
        if let Err(e) = added {
            tracing::warn!(error = %e, %channel_id, "Failed to index ringing call");
            return;
        }
        let expired: Result<bool, _> = self.redis.expire(&key, ttl, None).await;
        if let Err(e) = expired {
            tracing::warn!(error = %e, %channel_id, "Failed to expire ringing call index");
        }
    }

    /// Apply the event chosen by `decide` to the current call, atomically.
    ///
    /// `decide` sees the current state and call; it returns the event to apply
    /// or `None` to leave the call alone (then `Ok(None)` is returned). If the
    /// call changed in between, the call is re-read and `decide` runs again. A
    /// transition that ends the call deletes it.
    pub async fn transition<F>(
        &self,
        channel_id: Uuid,
        mut decide: F,
    ) -> Result<Option<Transition>, CallError>
    where
        F: FnMut(&CallState, &CallMeta) -> Result<Option<CallEventType>, CallError>,
    {
        for _ in 0..MAX_CAS_ATTEMPTS {
            let Some(LoadedCall {
                state: Some(state),
                meta,
            }) = self.load(channel_id).await?
            else {
                return Err(CallError::CallNotFound);
            };
            let Some(event) = decide(&state, &meta)? else {
                return Ok(None);
            };
            let new_state = state
                .apply(&event)
                .map_err(|e| CallError::StateTransition(e.to_string()))?;
            let op = if !new_state.is_active() {
                "delete"
            } else if matches!(event, CallEventType::Joined { .. }) {
                "append_persist"
            } else {
                "append"
            };
            let event_json = serde_json::to_string(&event)
                .map_err(|e| CallError::Serialization(e.to_string()))?;
            if self.cas(channel_id, &meta, op, &event_json).await? {
                return Ok(Some(Transition {
                    state: new_state,
                    meta,
                }));
            }
        }
        Err(CallError::Contention)
    }

    /// Record a user joining the call
    #[tracing::instrument(skip(self))]
    pub async fn join_call(
        &self,
        channel_id: Uuid,
        user_id: Uuid,
    ) -> Result<Transition, CallError> {
        self.transition(channel_id, |_, _| {
            Ok(Some(CallEventType::Joined { user_id }))
        })
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
        self.transition(channel_id, |_, _| {
            Ok(Some(CallEventType::Declined { user_id }))
        })
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
        self.transition(channel_id, |state, _| match state {
            CallState::Ringing { started_by, .. } if *started_by == user_id => {
                Ok(Some(CallEventType::Ended {
                    reason: EndReason::Cancelled,
                }))
            }
            CallState::Ringing { .. } => Err(CallError::StateTransition(
                "Use decline endpoint for recipients".into(),
            )),
            CallState::Active { .. } => Ok(Some(CallEventType::Left { user_id })),
            CallState::Ended { .. } => Err(CallError::StateTransition("Call already ended".into())),
        })
        .await?
        .ok_or(CallError::CallNotFound)
    }

    /// End the current call if `decide` returns a reason for it. Returns the
    /// ended call, or `None` if there was no call or `decide` kept it.
    pub async fn end_call_when<F>(
        &self,
        channel_id: Uuid,
        mut decide: F,
    ) -> Result<Option<Transition>, CallError>
    where
        F: FnMut(&CallState, &CallMeta) -> Option<EndReason>,
    {
        match self
            .transition(channel_id, |state, meta| {
                Ok(decide(state, meta).map(|reason| CallEventType::Ended { reason }))
            })
            .await
        {
            Err(CallError::CallNotFound) => Ok(None),
            other => other,
        }
    }
}

/// Send `event` to every member of the call's DM (user channels, so it
/// reaches users who do not have the DM open).
pub async fn broadcast_to_members(redis: &Client, members: &[Uuid], event: &ServerEvent) {
    let events: Vec<(Uuid, ServerEvent)> = members.iter().map(|&u| (u, event.clone())).collect();
    if let Err(e) = broadcast_to_users(redis, &events).await {
        tracing::warn!(error = %e, "Failed to broadcast call event");
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

/// Tell the call's members that it ended. No-op unless `state` is `Ended`.
pub async fn broadcast_call_ended(
    redis: &Client,
    channel_id: Uuid,
    meta: &CallMeta,
    state: &CallState,
) {
    if let CallState::Ended {
        reason,
        duration_secs,
        ..
    } = state
    {
        broadcast_to_members(
            redis,
            &meta.members(),
            &ServerEvent::CallEnded {
                channel_id,
                reason: end_reason_name(*reason),
                duration_secs: *duration_secs,
            },
        )
        .await;
    }
}

/// End the call chosen by `decide` and announce it. Idempotent: returns
/// `Ok(false)` if there is no call or `decide` keeps it.
pub async fn end_call_when<F>(
    redis: &Client,
    channel_id: Uuid,
    decide: F,
) -> Result<bool, CallError>
where
    F: FnMut(&CallState, &CallMeta) -> Option<EndReason>,
{
    let service = CallService::new(redis.clone());
    match service.end_call_when(channel_id, decide).await? {
        Some(t) => {
            broadcast_call_ended(redis, channel_id, &t.meta, &t.state).await;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// End the channel's current call (whatever its state), delete its state and
/// tell its members. Idempotent: no call is not an error.
pub async fn end_call(
    state: &AppState,
    channel_id: Uuid,
    reason: CallEndReason,
) -> Result<(), CallError> {
    end_call_when(&state.redis, channel_id, |_, _| Some(reason))
        .await
        .map(|_| ())
}

/// Ring timeout of the call `call_id`: ends it as unanswered if that call is
/// still ringing. A newer call on the channel is never touched. Returns
/// whether the call was ended.
pub async fn ring_timeout_fired(
    redis: &Client,
    channel_id: Uuid,
    call_id: &str,
) -> Result<bool, CallError> {
    end_call_when(redis, channel_id, |state, meta| {
        (meta.call_id == call_id && matches!(state, CallState::Ringing { .. }))
            .then_some(EndReason::NoAnswer)
    })
    .await
}

/// Start the ring timer for a just-started call.
pub fn spawn_ring_timeout(redis: Client, channel_id: Uuid, call_id: String, after: Duration) {
    tokio::spawn(async move {
        tokio::time::sleep(after).await;
        if let Err(e) = ring_timeout_fired(&redis, channel_id, &call_id).await {
            tracing::warn!(error = %e, %channel_id, "Failed to end unanswered call");
        }
    });
}

/// The last voice peer of `channel_id` left; end its DM call if it has one.
///
/// The peer connected at `peer_connected_at`. Ends an active call (`last_left`) or a ringing call
/// whose caller left the room (`cancelled`). A ringing call that started after
/// that peer joined is left alone: the peer belonged to an earlier session and
/// the new caller has not joined the room yet.
pub async fn last_voice_peer_left(
    redis: &Client,
    channel_id: Uuid,
    peer_connected_at: DateTime<Utc>,
) -> Result<bool, CallError> {
    end_call_when(redis, channel_id, |state, meta| match state {
        CallState::Active { .. } => Some(EndReason::LastLeft),
        CallState::Ringing { .. } if meta.started_at <= peer_connected_at => {
            Some(EndReason::Cancelled)
        }
        _ => None,
    })
    .await
}

/// Delays before each check whether a user still has an open socket.
const NUMSUB_RECHECK_DELAYS_MS: [u64; 3] = [0, 1_000, 3_000];

/// Number of sockets (on any node) subscribed to `user_id`'s event channel.
async fn open_user_sockets(redis: &Client, user_id: Uuid) -> Result<i64, CallError> {
    let user_channel = crate::ws::events::channels::user_events(user_id);
    let numsub: Vec<Value> = redis
        .pubsub_numsub(user_channel)
        .await
        .map_err(|e| CallError::Redis(e.to_string()))?;
    Ok(numsub.get(1).and_then(Value::as_i64).unwrap_or(0))
}

/// A socket of `user_id` closed. If the user has no other open socket, their
/// ringing calls end as cancelled (nobody is there to talk to).
///
/// Open sockets are counted with `PUBSUB NUMSUB` on the user's event channel,
/// which every socket subscribes to (on any server node). Call this after the
/// closing socket's subscriber has quit; it may wait a few seconds, so run it
/// off the cleanup path.
pub async fn caller_socket_closed(redis: &Client, user_id: Uuid) -> Result<(), CallError> {
    // The closing socket's QUIT is bounded; under load it may still be in
    // flight. Re-check a few times before concluding another socket is open.
    for (attempt, delay_ms) in NUMSUB_RECHECK_DELAYS_MS.iter().enumerate() {
        if *delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(*delay_ms)).await;
        }
        if open_user_sockets(redis, user_id).await? == 0 {
            break;
        }
        if attempt + 1 == NUMSUB_RECHECK_DELAYS_MS.len() {
            return Ok(());
        }
    }

    let index = CallService::ringing_index_key(user_id);
    let channels: Vec<String> = redis
        .smembers(&index)
        .await
        .map_err(|e| CallError::Redis(e.to_string()))?;
    for channel in channels {
        let Ok(channel_id) = Uuid::parse_str(&channel) else {
            continue;
        };
        if let Err(e) = end_call_when(redis, channel_id, |state, _| match state {
            CallState::Ringing { started_by, .. } if *started_by == user_id => {
                Some(EndReason::Cancelled)
            }
            _ => None,
        })
        .await
        {
            tracing::warn!(error = %e, %channel_id, "Failed to cancel call of offline caller");
            continue;
        }
        let _: Result<i64, _> = redis.srem(&index, channel).await;
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
