//! Redis Streams-backed call service for DM voice calls.

use crate::voice::call::{CallEventType, CallState, EndReason};
use fred::prelude::*;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// Call service for managing DM voice call state
pub struct CallService {
    redis: RedisClient,
}

impl CallService {
    pub fn new(redis: RedisClient) -> Self {
        Self { redis }
    }

    /// Get Redis stream key for a channel's call events
    fn stream_key(channel_id: Uuid) -> String {
        format!("call_events:{}", channel_id)
    }

    /// Get current call state by replaying events from stream
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

            let event_type: CallEventType = serde_json::from_str(data)
                .map_err(|e| CallError::InvalidEvent(e.to_string()))?;

            state = Some(match state {
                None => {
                    // First event must be Started
                    if let CallEventType::Started { initiator } = event_type {
                        // Get target users from fields
                        let targets_json = fields_map.get("targets").cloned().unwrap_or_default();
                        let targets: HashSet<Uuid> =
                            serde_json::from_str(&targets_json).unwrap_or_default();
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

        // Set TTL for auto-cleanup (120s ring timeout)
        let _: bool = self
            .redis
            .expire(&key, 120)
            .await
            .map_err(|e| CallError::Redis(e.to_string()))?;

        Ok(CallState::new_ringing(initiator, target_users))
    }

    /// Record a user joining the call
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
            .xadd(
                &key,
                false,
                None,
                "*",
                vec![("data", event_json.as_str())],
            )
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
            .xadd(
                &key,
                false,
                None,
                "*",
                vec![("data", event_json.as_str())],
            )
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
            .xadd(
                &key,
                false,
                None,
                "*",
                vec![("data", event_json.as_str())],
            )
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
            .xadd(
                &key,
                false,
                None,
                "*",
                vec![("data", event_json.as_str())],
            )
            .await
            .map_err(|e| CallError::Redis(e.to_string()))?;

        let new_state = state
            .apply(&event)
            .map_err(|e| CallError::StateTransition(e.to_string()))?;

        self.cleanup_call(channel_id).await?;

        Ok(new_state)
    }

    /// Clean up call stream after call ends
    async fn cleanup_call(&self, channel_id: Uuid) -> Result<(), CallError> {
        let key = Self::stream_key(channel_id);
        // Keep stream for a short time for late-joiners to see "ended" state
        let _: bool = self
            .redis
            .expire(&key, 5)
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
