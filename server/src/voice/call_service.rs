//! Redis Streams-backed call service for DM voice calls.

use crate::voice::call::{CallEventType, CallState, EndReason};
use fred::prelude::*;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// Ring timeout - call ends after this many seconds if no one answers
const RING_TIMEOUT_SECS: i64 = 90;
/// Cleanup delay - ended calls stay visible for this many seconds
const CLEANUP_DELAY_SECS: i64 = 5;

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

    /// Get current call state by replaying events from stream
    #[tracing::instrument(skip(self))]
    pub async fn get_call_state(&self, channel_id: Uuid) -> Result<Option<CallState>, CallError> {
        let key = Self::stream_key(channel_id);

        // Read all events from stream using XRANGE
        let events: Vec<(String, HashMap<String, String>)> = self
            .redis
            .xrange_values(&key, "-", "+", None)
            .await
            .map_err(|e| CallError::Redis(e.to_string()))?;

        if events.is_empty() {
            return Ok(None);
        }

        // Parse and replay events to derive state
        let mut state: Option<CallState> = None;

        for entry in events {
            // Entry is (id, fields_map) tuple
            let (_id, fields_map) = entry;

            // Get the data field from the entry
            let data = fields_map
                .get("data")
                .ok_or_else(|| CallError::InvalidEvent("Missing data field".into()))?;

            let event_type: CallEventType =
                serde_json::from_str(data).map_err(|e| CallError::InvalidEvent(e.to_string()))?;

            state = Some(match state {
                None => {
                    // First event must be Started
                    if let CallEventType::Started { initiator } = event_type {
                        // Get target users from fields
                        let targets_json = fields_map.get("targets").cloned().unwrap_or_default();
                        let targets: HashSet<Uuid> = match serde_json::from_str(&targets_json) {
                            Ok(t) => t,
                            Err(e) => {
                                tracing::warn!(
                                    channel_id = %channel_id,
                                    targets_json = %targets_json,
                                    error = %e,
                                    "Failed to parse call targets, using empty set"
                                );
                                HashSet::new()
                            }
                        };
                        CallState::new_ringing(initiator, targets)
                    } else {
                        return Err(CallError::InvalidEvent(
                            "First event must be Started".into(),
                        ));
                    }
                }
                Some(current) => current
                    .apply(&event_type)
                    .map_err(|e| CallError::StateTransition(e.to_string()))?,
            });
        }

        // Filter out ended calls (cleanup should remove them, but just in case)
        Ok(state.filter(|s| s.is_active()))
    }

    /// Start a new call
    ///
    /// # Race Condition (TOCTOU)
    /// There is a time-of-check-to-time-of-use race between checking for an existing
    /// call and creating the new one. This is acceptable for MVP because:
    /// - Concurrent starts will both succeed but one will immediately fail on join
    /// - DM calls are 1:1, making concurrent starts extremely rare
    /// - The failure mode is graceful (user sees "call already exists" error)
    #[tracing::instrument(skip(self))]
    pub async fn start_call(
        &self,
        channel_id: Uuid,
        initiator: Uuid,
        target_users: HashSet<Uuid>,
    ) -> Result<CallState, CallError> {
        // Check if call already exists
        if self.get_call_state(channel_id).await?.is_some() {
            return Err(CallError::CallAlreadyExists);
        }

        let key = Self::stream_key(channel_id);
        let event = CallEventType::Started { initiator };
        let event_json =
            serde_json::to_string(&event).map_err(|e| CallError::Serialization(e.to_string()))?;
        let targets_json = serde_json::to_string(&target_users)
            .map_err(|e| CallError::Serialization(e.to_string()))?;

        // Add event to stream
        let _: String = self
            .redis
            .xadd(
                &key,
                false,
                None,
                "*",
                vec![
                    ("data", event_json.as_str()),
                    ("targets", targets_json.as_str()),
                ],
            )
            .await
            .map_err(|e| CallError::Redis(e.to_string()))?;

        // Set TTL for auto-cleanup (ring timeout)
        let _: bool = self
            .redis
            .expire(&key, RING_TIMEOUT_SECS, None)
            .await
            .map_err(|e| CallError::Redis(e.to_string()))?;

        Ok(CallState::new_ringing(initiator, target_users))
    }

    /// Record a user joining the call
    #[tracing::instrument(skip(self))]
    pub async fn join_call(&self, channel_id: Uuid, user_id: Uuid) -> Result<CallState, CallError> {
        let state = self
            .get_call_state(channel_id)
            .await?
            .ok_or(CallError::CallNotFound)?;

        let key = Self::stream_key(channel_id);
        let event = CallEventType::Joined { user_id };
        let event_json =
            serde_json::to_string(&event).map_err(|e| CallError::Serialization(e.to_string()))?;

        let _: String = self
            .redis
            .xadd(&key, false, None, "*", vec![("data", event_json.as_str())])
            .await
            .map_err(|e| CallError::Redis(e.to_string()))?;

        // Remove TTL once call is active (cleanup on leave instead)
        let _: bool = self
            .redis
            .persist(&key)
            .await
            .map_err(|e| CallError::Redis(e.to_string()))?;

        state
            .apply(&event)
            .map_err(|e| CallError::StateTransition(e.to_string()))
    }

    /// Record a user declining the call
    #[tracing::instrument(skip(self))]
    pub async fn decline_call(
        &self,
        channel_id: Uuid,
        user_id: Uuid,
    ) -> Result<CallState, CallError> {
        let state = self
            .get_call_state(channel_id)
            .await?
            .ok_or(CallError::CallNotFound)?;

        let key = Self::stream_key(channel_id);
        let event = CallEventType::Declined { user_id };
        let event_json =
            serde_json::to_string(&event).map_err(|e| CallError::Serialization(e.to_string()))?;

        let _: String = self
            .redis
            .xadd(&key, false, None, "*", vec![("data", event_json.as_str())])
            .await
            .map_err(|e| CallError::Redis(e.to_string()))?;

        let new_state = state
            .apply(&event)
            .map_err(|e| CallError::StateTransition(e.to_string()))?;

        // Clean up if call ended
        if !new_state.is_active() {
            self.cleanup_call(channel_id).await?;
        }

        Ok(new_state)
    }

    /// Record a user leaving the call
    ///
    /// This handles both:
    /// - Active calls: sends Left event
    /// - Ringing calls (initiator): sends Ended { Cancelled } event
    #[tracing::instrument(skip(self))]
    pub async fn leave_call(
        &self,
        channel_id: Uuid,
        user_id: Uuid,
    ) -> Result<CallState, CallError> {
        let state = self
            .get_call_state(channel_id)
            .await?
            .ok_or(CallError::CallNotFound)?;

        // Determine the appropriate event based on current state
        let event = match &state {
            // If ringing and user is initiator, cancel the call
            CallState::Ringing { started_by, .. } if *started_by == user_id => {
                CallEventType::Ended {
                    reason: EndReason::Cancelled,
                }
            }
            // If ringing but user is not initiator, they should use decline instead
            CallState::Ringing { .. } => {
                return Err(CallError::StateTransition(
                    "Use decline endpoint for recipients".into(),
                ));
            }
            // Active call: normal leave
            CallState::Active { .. } => CallEventType::Left { user_id },
            // Already ended
            CallState::Ended { .. } => {
                return Err(CallError::StateTransition("Call already ended".into()));
            }
        };

        let key = Self::stream_key(channel_id);
        let event_json =
            serde_json::to_string(&event).map_err(|e| CallError::Serialization(e.to_string()))?;

        let _: String = self
            .redis
            .xadd(&key, false, None, "*", vec![("data", event_json.as_str())])
            .await
            .map_err(|e| CallError::Redis(e.to_string()))?;

        let new_state = state
            .apply(&event)
            .map_err(|e| CallError::StateTransition(e.to_string()))?;

        // Clean up if call ended
        if !new_state.is_active() {
            self.cleanup_call(channel_id).await?;
        }

        Ok(new_state)
    }

    /// End a call with a specific reason
    #[tracing::instrument(skip(self))]
    pub async fn end_call(
        &self,
        channel_id: Uuid,
        reason: EndReason,
    ) -> Result<CallState, CallError> {
        let state = self
            .get_call_state(channel_id)
            .await?
            .ok_or(CallError::CallNotFound)?;

        let key = Self::stream_key(channel_id);
        let event = CallEventType::Ended { reason };
        let event_json =
            serde_json::to_string(&event).map_err(|e| CallError::Serialization(e.to_string()))?;

        let _: String = self
            .redis
            .xadd(&key, false, None, "*", vec![("data", event_json.as_str())])
            .await
            .map_err(|e| CallError::Redis(e.to_string()))?;

        let new_state = state
            .apply(&event)
            .map_err(|e| CallError::StateTransition(e.to_string()))?;

        self.cleanup_call(channel_id).await?;

        Ok(new_state)
    }

    /// Clean up call stream after call ends
    #[tracing::instrument(skip(self))]
    async fn cleanup_call(&self, channel_id: Uuid) -> Result<(), CallError> {
        let key = Self::stream_key(channel_id);
        // Keep stream for a short time for late-joiners to see "ended" state
        let _: bool = self
            .redis
            .expire(&key, CLEANUP_DELAY_SECS, None)
            .await
            .map_err(|e| CallError::Redis(e.to_string()))?;
        Ok(())
    }
}

/// Call service errors
#[derive(Debug, thiserror::Error)]
pub enum CallError {
    #[error("Call not found")]
    CallNotFound,
    #[error("Call already exists")]
    CallAlreadyExists,
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
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("CallNotFound"));
    }
}
