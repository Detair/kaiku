# DM Voice Calls Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add voice calling to DM and group DM conversations, reusing existing SFU infrastructure.

**Architecture:** Call signaling via Redis Streams (event-sourced), API endpoints for call lifecycle, WebSocket events for real-time updates. DM channel ID = voice room ID. Frontend state machine manages call UI states.

**Tech Stack:** Rust/Axum (backend), Redis Streams (state), Solid.js (frontend), existing SFU (media)

---

## Task 1: Backend - Call Types and State Machine

**Files:**
- Create: `server/src/voice/call.rs`
- Modify: `server/src/voice/mod.rs`

**Step 1: Create call types module**

```rust
// server/src/voice/call.rs
//! DM Voice Call State Management
//!
//! Event-sourced call state using Redis Streams for multi-node coordination.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

/// Call event types for Redis Stream
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CallEventType {
    Started { initiator: Uuid },
    Joined { user_id: Uuid },
    Left { user_id: Uuid },
    Declined { user_id: Uuid },
    Ended { reason: EndReason },
}

/// Reason for call ending
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    Cancelled,    // Initiator hung up before anyone joined
    AllDeclined,  // All recipients declined
    NoAnswer,     // Timeout (90s)
    LastLeft,     // Last participant left
}

/// Derived call state from event stream
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CallState {
    Ringing {
        started_by: Uuid,
        started_at: DateTime<Utc>,
        declined_by: HashSet<Uuid>,
        target_users: HashSet<Uuid>,
    },
    Active {
        started_at: DateTime<Utc>,
        participants: HashSet<Uuid>,
    },
    Ended {
        reason: EndReason,
        duration_secs: Option<u32>,
        ended_at: DateTime<Utc>,
    },
}

/// A single event in the call stream
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallEvent {
    pub event_id: String,
    pub timestamp: DateTime<Utc>,
    pub event: CallEventType,
}

impl CallState {
    /// Create initial ringing state
    pub fn new_ringing(initiator: Uuid, target_users: HashSet<Uuid>) -> Self {
        CallState::Ringing {
            started_by: initiator,
            started_at: Utc::now(),
            declined_by: HashSet::new(),
            target_users,
        }
    }

    /// Apply an event to derive new state
    pub fn apply(self, event: &CallEventType) -> Result<Self, CallStateError> {
        match (self, event) {
            // Ringing -> Active when someone joins
            (
                CallState::Ringing {
                    started_at,
                    started_by,
                    ..
                },
                CallEventType::Joined { user_id },
            ) => {
                let mut participants = HashSet::new();
                participants.insert(started_by);
                participants.insert(*user_id);
                Ok(CallState::Active {
                    started_at,
                    participants,
                })
            }

            // Ringing -> Ringing with decline recorded
            (
                CallState::Ringing {
                    started_by,
                    started_at,
                    mut declined_by,
                    target_users,
                },
                CallEventType::Declined { user_id },
            ) => {
                declined_by.insert(*user_id);
                // Check if all targets declined
                if declined_by.len() >= target_users.len() {
                    Ok(CallState::Ended {
                        reason: EndReason::AllDeclined,
                        duration_secs: None,
                        ended_at: Utc::now(),
                    })
                } else {
                    Ok(CallState::Ringing {
                        started_by,
                        started_at,
                        declined_by,
                        target_users,
                    })
                }
            }

            // Ringing -> Ended when initiator cancels
            (CallState::Ringing { .. }, CallEventType::Ended { reason }) => {
                Ok(CallState::Ended {
                    reason: *reason,
                    duration_secs: None,
                    ended_at: Utc::now(),
                })
            }

            // Active -> Active with new participant
            (
                CallState::Active {
                    started_at,
                    mut participants,
                },
                CallEventType::Joined { user_id },
            ) => {
                participants.insert(*user_id);
                Ok(CallState::Active {
                    started_at,
                    participants,
                })
            }

            // Active -> Active or Ended when someone leaves
            (
                CallState::Active {
                    started_at,
                    mut participants,
                },
                CallEventType::Left { user_id },
            ) => {
                participants.remove(user_id);
                if participants.is_empty() {
                    let duration = Utc::now()
                        .signed_duration_since(started_at)
                        .num_seconds() as u32;
                    Ok(CallState::Ended {
                        reason: EndReason::LastLeft,
                        duration_secs: Some(duration),
                        ended_at: Utc::now(),
                    })
                } else {
                    Ok(CallState::Active {
                        started_at,
                        participants,
                    })
                }
            }

            // Active -> Ended
            (
                CallState::Active { started_at, .. },
                CallEventType::Ended { reason },
            ) => {
                let duration = Utc::now()
                    .signed_duration_since(started_at)
                    .num_seconds() as u32;
                Ok(CallState::Ended {
                    reason: *reason,
                    duration_secs: Some(duration),
                    ended_at: Utc::now(),
                })
            }

            // Ended state is terminal
            (CallState::Ended { .. }, _) => Err(CallStateError::CallAlreadyEnded),

            // Invalid transitions
            (state, event) => Err(CallStateError::InvalidTransition {
                state: format!("{:?}", state),
                event: format!("{:?}", event),
            }),
        }
    }

    /// Check if call is still active (not ended)
    pub fn is_active(&self) -> bool {
        !matches!(self, CallState::Ended { .. })
    }

    /// Get participants if call is active
    pub fn participants(&self) -> Option<&HashSet<Uuid>> {
        match self {
            CallState::Active { participants, .. } => Some(participants),
            _ => None,
        }
    }
}

/// Errors for call state transitions
#[derive(Debug, thiserror::Error)]
pub enum CallStateError {
    #[error("Call has already ended")]
    CallAlreadyEnded,
    #[error("Invalid state transition: {state} + {event}")]
    InvalidTransition { state: String, event: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ringing_to_active_on_join() {
        let mut targets = HashSet::new();
        targets.insert(Uuid::new_v4());
        let initiator = Uuid::new_v4();
        let joiner = Uuid::new_v4();

        let state = CallState::new_ringing(initiator, targets);
        let new_state = state.apply(&CallEventType::Joined { user_id: joiner }).unwrap();

        assert!(matches!(new_state, CallState::Active { .. }));
        if let CallState::Active { participants, .. } = new_state {
            assert!(participants.contains(&initiator));
            assert!(participants.contains(&joiner));
        }
    }

    #[test]
    fn test_all_declined_ends_call() {
        let target = Uuid::new_v4();
        let mut targets = HashSet::new();
        targets.insert(target);
        let initiator = Uuid::new_v4();

        let state = CallState::new_ringing(initiator, targets);
        let new_state = state.apply(&CallEventType::Declined { user_id: target }).unwrap();

        assert!(matches!(new_state, CallState::Ended { reason: EndReason::AllDeclined, .. }));
    }

    #[test]
    fn test_last_participant_leaves_ends_call() {
        let mut participants = HashSet::new();
        let user = Uuid::new_v4();
        participants.insert(user);

        let state = CallState::Active {
            started_at: Utc::now(),
            participants,
        };
        let new_state = state.apply(&CallEventType::Left { user_id: user }).unwrap();

        assert!(matches!(new_state, CallState::Ended { reason: EndReason::LastLeft, .. }));
    }

    #[test]
    fn test_ended_state_is_terminal() {
        let state = CallState::Ended {
            reason: EndReason::Cancelled,
            duration_secs: None,
            ended_at: Utc::now(),
        };
        let result = state.apply(&CallEventType::Joined { user_id: Uuid::new_v4() });

        assert!(matches!(result, Err(CallStateError::CallAlreadyEnded)));
    }
}
```

**Step 2: Add module to voice/mod.rs**

Add to `server/src/voice/mod.rs`:
```rust
pub mod call;
```

**Step 3: Run tests**

```bash
cargo test -p vc-server call::tests -- --nocapture
```
Expected: All 4 tests pass

**Step 4: Commit**

```bash
git add server/src/voice/call.rs server/src/voice/mod.rs
git commit -m "feat(voice): Add call state types and state machine"
```

---

## Task 2: Backend - Redis Call Service

**Files:**
- Create: `server/src/voice/call_service.rs`
- Modify: `server/src/voice/mod.rs`

**Step 1: Create Redis-backed call service**

```rust
// server/src/voice/call_service.rs
//! Redis Streams-backed call service for DM voice calls.

use crate::voice::call::{CallEvent, CallEventType, CallState, EndReason};
use chrono::Utc;
use fred::prelude::*;
use std::collections::HashSet;
use std::sync::Arc;
use uuid::Uuid;

/// Call service for managing DM voice call state
pub struct CallService {
    redis: Arc<RedisClient>,
}

impl CallService {
    pub fn new(redis: Arc<RedisClient>) -> Self {
        Self { redis }
    }

    /// Get Redis stream key for a channel's call events
    fn stream_key(channel_id: Uuid) -> String {
        format!("call_events:{}", channel_id)
    }

    /// Get current call state by replaying events from stream
    pub async fn get_call_state(&self, channel_id: Uuid) -> Result<Option<CallState>, CallError> {
        let key = Self::stream_key(channel_id);

        // Read all events from stream
        let events: Vec<(String, Vec<(String, String)>)> = self
            .redis
            .xrange(&key, "-", "+", None)
            .await
            .map_err(|e| CallError::Redis(e.to_string()))?;

        if events.is_empty() {
            return Ok(None);
        }

        // Parse and replay events to derive state
        let mut state: Option<CallState> = None;

        for (event_id, fields) in events {
            // Parse event from Redis hash fields
            let event_json = fields
                .iter()
                .find(|(k, _)| k == "data")
                .map(|(_, v)| v.clone())
                .ok_or_else(|| CallError::InvalidEvent("Missing data field".into()))?;

            let event_type: CallEventType = serde_json::from_str(&event_json)
                .map_err(|e| CallError::InvalidEvent(e.to_string()))?;

            let event = CallEvent {
                event_id,
                timestamp: Utc::now(), // Could parse from event_id
                event: event_type.clone(),
            };

            state = Some(match state {
                None => {
                    // First event must be Started
                    if let CallEventType::Started { initiator } = event_type {
                        // Get target users from fields
                        let targets_json = fields
                            .iter()
                            .find(|(k, _)| k == "targets")
                            .map(|(_, v)| v.clone())
                            .unwrap_or_else(|| "[]".to_string());
                        let targets: HashSet<Uuid> = serde_json::from_str(&targets_json)
                            .unwrap_or_default();
                        CallState::new_ringing(initiator, targets)
                    } else {
                        return Err(CallError::InvalidEvent("First event must be Started".into()));
                    }
                }
                Some(current) => {
                    current.apply(&event.event)
                        .map_err(|e| CallError::StateTransition(e.to_string()))?
                }
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
        if let Some(existing) = self.get_call_state(channel_id).await? {
            return Err(CallError::CallAlreadyExists);
        }

        let key = Self::stream_key(channel_id);
        let event = CallEventType::Started { initiator };
        let event_json = serde_json::to_string(&event)
            .map_err(|e| CallError::Serialization(e.to_string()))?;
        let targets_json = serde_json::to_string(&target_users)
            .map_err(|e| CallError::Serialization(e.to_string()))?;

        // Add event to stream with 90s TTL
        let _: String = self
            .redis
            .xadd(
                &key,
                false,
                None,
                "*",
                vec![("data", event_json.as_str()), ("targets", targets_json.as_str())],
            )
            .await
            .map_err(|e| CallError::Redis(e.to_string()))?;

        // Set TTL for auto-cleanup (90s ring timeout)
        let _: () = self
            .redis
            .expire(&key, 90)
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
        let event_json = serde_json::to_string(&event)
            .map_err(|e| CallError::Serialization(e.to_string()))?;

        let _: String = self
            .redis
            .xadd(&key, false, None, "*", vec![("data", event_json.as_str())])
            .await
            .map_err(|e| CallError::Redis(e.to_string()))?;

        // Remove TTL once call is active (cleanup on leave instead)
        let _: () = self
            .redis
            .persist(&key)
            .await
            .map_err(|e| CallError::Redis(e.to_string()))?;

        state
            .apply(&event)
            .map_err(|e| CallError::StateTransition(e.to_string()))
    }

    /// Record a user declining the call
    pub async fn decline_call(&self, channel_id: Uuid, user_id: Uuid) -> Result<CallState, CallError> {
        let state = self
            .get_call_state(channel_id)
            .await?
            .ok_or(CallError::CallNotFound)?;

        let key = Self::stream_key(channel_id);
        let event = CallEventType::Declined { user_id };
        let event_json = serde_json::to_string(&event)
            .map_err(|e| CallError::Serialization(e.to_string()))?;

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
    pub async fn leave_call(&self, channel_id: Uuid, user_id: Uuid) -> Result<CallState, CallError> {
        let state = self
            .get_call_state(channel_id)
            .await?
            .ok_or(CallError::CallNotFound)?;

        let key = Self::stream_key(channel_id);
        let event = CallEventType::Left { user_id };
        let event_json = serde_json::to_string(&event)
            .map_err(|e| CallError::Serialization(e.to_string()))?;

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
    pub async fn end_call(&self, channel_id: Uuid, reason: EndReason) -> Result<CallState, CallError> {
        let state = self
            .get_call_state(channel_id)
            .await?
            .ok_or(CallError::CallNotFound)?;

        let key = Self::stream_key(channel_id);
        let event = CallEventType::Ended { reason };
        let event_json = serde_json::to_string(&event)
            .map_err(|e| CallError::Serialization(e.to_string()))?;

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
    async fn cleanup_call(&self, channel_id: Uuid) -> Result<(), CallError> {
        let key = Self::stream_key(channel_id);
        // Keep stream for a short time for late-joiners to see "ended" state
        let _: () = self
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
```

**Step 2: Add module to voice/mod.rs**

Add to `server/src/voice/mod.rs`:
```rust
pub mod call_service;
```

**Step 3: Build to check compilation**

```bash
cargo build -p vc-server
```
Expected: Compiles successfully

**Step 4: Commit**

```bash
git add server/src/voice/call_service.rs server/src/voice/mod.rs
git commit -m "feat(voice): Add Redis Streams-backed call service"
```

---

## Task 3: Backend - Call API Handlers

**Files:**
- Create: `server/src/voice/call_handlers.rs`
- Modify: `server/src/voice/mod.rs`
- Modify: `server/src/api/mod.rs`

**Step 1: Create API handlers**

```rust
// server/src/voice/call_handlers.rs
//! HTTP handlers for DM voice call API endpoints.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

use crate::{
    api::AppState,
    auth::AuthUser,
    db::ChannelType,
    voice::call::{CallState, EndReason},
    voice::call_service::{CallError, CallService},
};

/// Response for call state
#[derive(Debug, Serialize)]
pub struct CallStateResponse {
    pub channel_id: Uuid,
    #[serde(flatten)]
    pub state: CallState,
}

/// Start call request (empty body - targets derived from DM participants)
#[derive(Debug, Deserialize)]
pub struct StartCallRequest {}

/// Call API error response
#[derive(Debug, Serialize)]
pub struct CallApiError {
    pub error: String,
    pub code: String,
}

impl IntoResponse for CallError {
    fn into_response(self) -> axum::response::Response {
        let (status, code) = match &self {
            CallError::CallNotFound => (StatusCode::NOT_FOUND, "call_not_found"),
            CallError::CallAlreadyExists => (StatusCode::CONFLICT, "call_already_exists"),
            CallError::Redis(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
            CallError::InvalidEvent(_) => (StatusCode::BAD_REQUEST, "invalid_event"),
            CallError::StateTransition(_) => (StatusCode::CONFLICT, "invalid_transition"),
            CallError::Serialization(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        };

        let body = Json(CallApiError {
            error: self.to_string(),
            code: code.to_string(),
        });

        (status, body).into_response()
    }
}

/// Verify user is a DM participant
async fn verify_dm_participant(
    state: &AppState,
    channel_id: Uuid,
    user_id: Uuid,
) -> Result<Vec<Uuid>, CallError> {
    // Get channel and verify it's a DM
    let channel = sqlx::query!(
        "SELECT channel_type FROM channels WHERE id = $1",
        channel_id
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|e| CallError::Redis(e.to_string()))?
    .ok_or(CallError::CallNotFound)?;

    if channel.channel_type != "dm" {
        return Err(CallError::CallNotFound);
    }

    // Get all participants
    let participants: Vec<Uuid> = sqlx::query_scalar!(
        "SELECT user_id FROM dm_participants WHERE channel_id = $1",
        channel_id
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| CallError::Redis(e.to_string()))?;

    // Verify user is a participant
    if !participants.contains(&user_id) {
        return Err(CallError::CallNotFound);
    }

    Ok(participants)
}

/// GET /api/dm/:id/call - Get current call state
pub async fn get_call(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<Json<Option<CallStateResponse>>, CallError> {
    // Verify membership
    verify_dm_participant(&state, channel_id, auth.id).await?;

    let call_service = CallService::new(state.redis.clone());
    let call_state = call_service.get_call_state(channel_id).await?;

    Ok(Json(call_state.map(|state| CallStateResponse {
        channel_id,
        state,
    })))
}

/// POST /api/dm/:id/call/start - Start a new call
pub async fn start_call(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<(StatusCode, Json<CallStateResponse>), CallError> {
    // Verify membership and get other participants
    let participants = verify_dm_participant(&state, channel_id, auth.id).await?;
    let target_users: HashSet<Uuid> = participants
        .into_iter()
        .filter(|&id| id != auth.id)
        .collect();

    if target_users.is_empty() {
        return Err(CallError::InvalidEvent("No other participants in DM".into()));
    }

    let call_service = CallService::new(state.redis.clone());
    let call_state = call_service
        .start_call(channel_id, auth.id, target_users)
        .await?;

    // TODO: Broadcast CallStarted WebSocket event to participants

    Ok((
        StatusCode::CREATED,
        Json(CallStateResponse {
            channel_id,
            state: call_state,
        }),
    ))
}

/// POST /api/dm/:id/call/join - Join an active call
pub async fn join_call(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<Json<CallStateResponse>, CallError> {
    // Verify membership
    verify_dm_participant(&state, channel_id, auth.id).await?;

    let call_service = CallService::new(state.redis.clone());
    let call_state = call_service.join_call(channel_id, auth.id).await?;

    // TODO: Broadcast ParticipantJoined WebSocket event

    Ok(Json(CallStateResponse {
        channel_id,
        state: call_state,
    }))
}

/// POST /api/dm/:id/call/decline - Decline a call
pub async fn decline_call(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<Json<CallStateResponse>, CallError> {
    // Verify membership
    verify_dm_participant(&state, channel_id, auth.id).await?;

    let call_service = CallService::new(state.redis.clone());
    let call_state = call_service.decline_call(channel_id, auth.id).await?;

    // TODO: Broadcast CallDeclined WebSocket event

    Ok(Json(CallStateResponse {
        channel_id,
        state: call_state,
    }))
}

/// POST /api/dm/:id/call/leave - Leave an active call
pub async fn leave_call(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<Json<CallStateResponse>, CallError> {
    // Verify membership
    verify_dm_participant(&state, channel_id, auth.id).await?;

    let call_service = CallService::new(state.redis.clone());
    let call_state = call_service.leave_call(channel_id, auth.id).await?;

    // TODO: Broadcast ParticipantLeft or CallEnded WebSocket event

    Ok(Json(CallStateResponse {
        channel_id,
        state: call_state,
    }))
}

/// Build the call router
pub fn call_router() -> axum::Router<AppState> {
    use axum::routing::{get, post};

    axum::Router::new()
        .route("/:id/call", get(get_call))
        .route("/:id/call/start", post(start_call))
        .route("/:id/call/join", post(join_call))
        .route("/:id/call/decline", post(decline_call))
        .route("/:id/call/leave", post(leave_call))
}
```

**Step 2: Add module export**

Add to `server/src/voice/mod.rs`:
```rust
pub mod call_handlers;
```

**Step 3: Integrate router into API**

In `server/src/api/mod.rs`, add the call routes. Find where dm_router is nested and add:

```rust
// In the router builder, add call routes under /api/dm
.nest("/api/dm", voice::call_handlers::call_router())
```

Note: May need to merge with existing dm_router or add as separate nest.

**Step 4: Build and verify**

```bash
cargo build -p vc-server
```
Expected: Compiles successfully

**Step 5: Commit**

```bash
git add server/src/voice/call_handlers.rs server/src/voice/mod.rs server/src/api/mod.rs
git commit -m "feat(voice): Add call API handlers"
```

---

## Task 4: Backend - WebSocket Call Events

**Files:**
- Modify: `server/src/ws/events.rs` (or equivalent)
- Modify: `server/src/voice/call_handlers.rs`

**Step 1: Add call events to WebSocket event types**

Find the WebSocket events enum (likely in `server/src/ws/`) and add:

```rust
// Add to ServerEvent enum
CallStarted {
    channel_id: Uuid,
    initiator: Uuid,
    initiator_name: String,
},
CallEnded {
    channel_id: Uuid,
    reason: EndReason,
    duration_secs: Option<u32>,
},
ParticipantJoined {
    channel_id: Uuid,
    user_id: Uuid,
    username: String,
},
ParticipantLeft {
    channel_id: Uuid,
    user_id: Uuid,
},
CallDeclined {
    channel_id: Uuid,
    user_id: Uuid,
},
IncomingCall {
    channel_id: Uuid,
    initiator: Uuid,
    initiator_name: String,
},
```

**Step 2: Update handlers to broadcast events**

Update the TODOs in `call_handlers.rs` to actually broadcast events via Redis pub/sub to the appropriate users.

**Step 3: Build and verify**

```bash
cargo build -p vc-server
```

**Step 4: Commit**

```bash
git add server/src/ws/ server/src/voice/call_handlers.rs
git commit -m "feat(voice): Add WebSocket events for call signaling"
```

---

## Task 5: Frontend - Call Store

**Files:**
- Create: `client/src/stores/call.ts`

**Step 1: Create call state store**

```typescript
// client/src/stores/call.ts
import { createStore } from "solid-js/store";
import { createSignal } from "solid-js";

// Call state types matching backend
export type EndReason = "cancelled" | "all_declined" | "no_answer" | "last_left";

export type CallState =
  | { status: "idle" }
  | { status: "outgoing_ringing"; channelId: string; startedAt: number }
  | { status: "incoming_ringing"; channelId: string; initiator: string; initiatorName: string }
  | { status: "connecting"; channelId: string }
  | { status: "connected"; channelId: string; participants: string[]; startedAt: number }
  | { status: "reconnecting"; channelId: string; countdown: number }
  | { status: "ended"; channelId: string; reason: EndReason; duration?: number };

// Store state
interface CallStoreState {
  currentCall: CallState;
  activeCallsByChannel: Record<string, { initiator: string; initiatorName: string; participants: string[] }>;
}

const [state, setState] = createStore<CallStoreState>({
  currentCall: { status: "idle" },
  activeCallsByChannel: {},
});

// Actions
export function startCall(channelId: string) {
  setState("currentCall", {
    status: "outgoing_ringing",
    channelId,
    startedAt: Date.now(),
  });
}

export function receiveIncomingCall(channelId: string, initiator: string, initiatorName: string) {
  // Only update if we're idle
  if (state.currentCall.status === "idle") {
    setState("currentCall", {
      status: "incoming_ringing",
      channelId,
      initiator,
      initiatorName,
    });
  }
  // Always track in activeCallsByChannel for sidebar indicator
  setState("activeCallsByChannel", channelId, {
    initiator,
    initiatorName,
    participants: [initiator],
  });
}

export function joinCall(channelId: string) {
  setState("currentCall", {
    status: "connecting",
    channelId,
  });
}

export function callConnected(channelId: string, participants: string[]) {
  setState("currentCall", {
    status: "connected",
    channelId,
    participants,
    startedAt: Date.now(),
  });
}

export function participantJoined(channelId: string, userId: string) {
  if (state.currentCall.status === "connected" && state.currentCall.channelId === channelId) {
    setState("currentCall", "participants", (prev) => [...prev, userId]);
  }
  // Update active calls
  setState("activeCallsByChannel", channelId, "participants", (prev) =>
    prev ? [...prev, userId] : [userId]
  );
}

export function participantLeft(channelId: string, userId: string) {
  if (state.currentCall.status === "connected" && state.currentCall.channelId === channelId) {
    setState("currentCall", "participants", (prev) => prev.filter((id) => id !== userId));
  }
  // Update active calls
  setState("activeCallsByChannel", channelId, "participants", (prev) =>
    prev ? prev.filter((id) => id !== userId) : []
  );
}

export function declineCall(channelId: string) {
  if (
    state.currentCall.status === "incoming_ringing" &&
    state.currentCall.channelId === channelId
  ) {
    setState("currentCall", { status: "idle" });
  }
}

export function endCall(channelId: string, reason: EndReason, duration?: number) {
  setState("currentCall", {
    status: "ended",
    channelId,
    reason,
    duration,
  });
  // Remove from active calls
  setState("activeCallsByChannel", channelId, undefined!);

  // Reset to idle after showing ended state briefly
  setTimeout(() => {
    if (state.currentCall.status === "ended") {
      setState("currentCall", { status: "idle" });
    }
  }, 3000);
}

export function callEndedExternally(channelId: string, reason: EndReason, duration?: number) {
  // Remove from active calls
  setState("activeCallsByChannel", channelId, undefined!);

  // Update current call if it's the one that ended
  if (
    state.currentCall.status !== "idle" &&
    "channelId" in state.currentCall &&
    state.currentCall.channelId === channelId
  ) {
    endCall(channelId, reason, duration);
  }
}

// Selectors
export function getCurrentCall() {
  return state.currentCall;
}

export function getActiveCallForChannel(channelId: string) {
  return state.activeCallsByChannel[channelId];
}

export function isInCall() {
  return state.currentCall.status !== "idle" && state.currentCall.status !== "ended";
}

export function isInCallForChannel(channelId: string) {
  return (
    state.currentCall.status !== "idle" &&
    state.currentCall.status !== "ended" &&
    "channelId" in state.currentCall &&
    state.currentCall.channelId === channelId
  );
}

// Export store for reactive access
export { state as callState };
```

**Step 2: Commit**

```bash
git add client/src/stores/call.ts
git commit -m "feat(call): Add call state store"
```

---

## Task 6: Frontend - Call API Functions

**Files:**
- Modify: `client/src/lib/tauri.ts`

**Step 1: Add call API functions**

Add to `client/src/lib/tauri.ts`:

```typescript
// Call API types
export interface CallStateResponse {
  channel_id: string;
  status: "ringing" | "active" | "ended";
  started_by?: string;
  started_at?: string;
  participants?: string[];
  declined_by?: string[];
  reason?: string;
  duration_secs?: number;
}

// Call Commands
export async function getCallState(channelId: string): Promise<CallStateResponse | null> {
  return httpRequest<CallStateResponse | null>("GET", `/api/dm/${channelId}/call`);
}

export async function startDMCall(channelId: string): Promise<CallStateResponse> {
  return httpRequest<CallStateResponse>("POST", `/api/dm/${channelId}/call/start`);
}

export async function joinDMCall(channelId: string): Promise<CallStateResponse> {
  return httpRequest<CallStateResponse>("POST", `/api/dm/${channelId}/call/join`);
}

export async function declineDMCall(channelId: string): Promise<CallStateResponse> {
  return httpRequest<CallStateResponse>("POST", `/api/dm/${channelId}/call/decline`);
}

export async function leaveDMCall(channelId: string): Promise<CallStateResponse> {
  return httpRequest<CallStateResponse>("POST", `/api/dm/${channelId}/call/leave`);
}
```

**Step 2: Commit**

```bash
git add client/src/lib/tauri.ts
git commit -m "feat(call): Add call API functions"
```

---

## Task 7: Frontend - Call Banner Component

**Files:**
- Create: `client/src/components/call/CallBanner.tsx`
- Create: `client/src/components/call/index.ts`

**Step 1: Create CallBanner component**

```tsx
// client/src/components/call/CallBanner.tsx
import { Component, Show, createMemo, createEffect, onCleanup } from "solid-js";
import { Phone, PhoneOff, PhoneIncoming } from "lucide-solid";
import { callState, joinCall, declineCall, endCall, isInCallForChannel } from "@/stores/call";
import { joinDMCall, declineDMCall, leaveDMCall } from "@/lib/tauri";

interface CallBannerProps {
  channelId: string;
  channelName: string;
}

export const CallBanner: Component<CallBannerProps> = (props) => {
  const currentCall = createMemo(() => callState.currentCall);
  const isThisChannel = createMemo(() => {
    const call = currentCall();
    return call.status !== "idle" && "channelId" in call && call.channelId === props.channelId;
  });

  // Timer for connected calls
  let timerInterval: ReturnType<typeof setInterval> | undefined;
  const [duration, setDuration] = createSignal(0);

  createEffect(() => {
    const call = currentCall();
    if (call.status === "connected" && isThisChannel()) {
      timerInterval = setInterval(() => {
        setDuration(Math.floor((Date.now() - call.startedAt) / 1000));
      }, 1000);
    } else {
      if (timerInterval) {
        clearInterval(timerInterval);
        timerInterval = undefined;
      }
      setDuration(0);
    }
  });

  onCleanup(() => {
    if (timerInterval) clearInterval(timerInterval);
  });

  const formatDuration = (secs: number) => {
    const mins = Math.floor(secs / 60);
    const s = secs % 60;
    return `${mins}:${s.toString().padStart(2, "0")}`;
  };

  const handleJoin = async () => {
    try {
      joinCall(props.channelId);
      await joinDMCall(props.channelId);
      // Voice connection will be handled by voice store
    } catch (e) {
      console.error("Failed to join call:", e);
    }
  };

  const handleDecline = async () => {
    try {
      await declineDMCall(props.channelId);
      declineCall(props.channelId);
    } catch (e) {
      console.error("Failed to decline call:", e);
    }
  };

  const handleLeave = async () => {
    try {
      await leaveDMCall(props.channelId);
      endCall(props.channelId, "cancelled");
    } catch (e) {
      console.error("Failed to leave call:", e);
    }
  };

  return (
    <Show when={isThisChannel()}>
      <div
        class="flex items-center justify-between px-4 py-2 bg-accent-primary/20 border-b border-accent-primary/30"
        classList={{
          "animate-pulse": currentCall().status === "incoming_ringing" || currentCall().status === "outgoing_ringing",
        }}
      >
        <div class="flex items-center gap-3">
          <Show
            when={currentCall().status === "incoming_ringing"}
            fallback={<Phone class="w-5 h-5 text-accent-primary" />}
          >
            <PhoneIncoming class="w-5 h-5 text-accent-primary" />
          </Show>

          <span class="text-sm font-medium">
            <Show when={currentCall().status === "incoming_ringing"}>
              {(currentCall() as any).initiatorName} is calling...
            </Show>
            <Show when={currentCall().status === "outgoing_ringing"}>
              Calling...
            </Show>
            <Show when={currentCall().status === "connecting"}>
              Connecting...
            </Show>
            <Show when={currentCall().status === "connected"}>
              In call • {formatDuration(duration())}
            </Show>
          </span>
        </div>

        <div class="flex items-center gap-2">
          <Show when={currentCall().status === "incoming_ringing"}>
            <button
              onClick={handleJoin}
              class="px-3 py-1 text-sm font-medium rounded bg-green-600 hover:bg-green-500 text-white"
            >
              Join
            </button>
            <button
              onClick={handleDecline}
              class="px-3 py-1 text-sm font-medium rounded bg-red-600 hover:bg-red-500 text-white"
            >
              Decline
            </button>
          </Show>

          <Show when={currentCall().status === "connected" || currentCall().status === "outgoing_ringing"}>
            <button
              onClick={handleLeave}
              class="p-2 rounded-full bg-red-600 hover:bg-red-500 text-white"
              title="End call"
            >
              <PhoneOff class="w-4 h-4" />
            </button>
          </Show>
        </div>
      </div>
    </Show>
  );
};
```

**Step 2: Create index.ts**

```typescript
// client/src/components/call/index.ts
export { CallBanner } from "./CallBanner";
```

**Step 3: Commit**

```bash
git add client/src/components/call/
git commit -m "feat(call): Add CallBanner component"
```

---

## Task 8: Frontend - Call Button in DM Header

**Files:**
- Modify: `client/src/components/home/DMConversation.tsx` (or equivalent DM view)

**Step 1: Add call button to DM header**

Find the DM conversation header component and add a call button:

```tsx
import { Phone } from "lucide-solid";
import { startCall, isInCall } from "@/stores/call";
import { startDMCall } from "@/lib/tauri";

// In the header section, add:
<button
  onClick={async () => {
    startCall(channelId);
    await startDMCall(channelId);
    // Then join voice
  }}
  disabled={isInCall()}
  class="p-2 rounded hover:bg-surface-layer2 disabled:opacity-50"
  title={isInCall() ? "Already in a call" : "Start call"}
>
  <Phone class="w-5 h-5" />
</button>
```

**Step 2: Add CallBanner to conversation view**

```tsx
import { CallBanner } from "@/components/call";

// At the top of the chat area:
<CallBanner channelId={channelId} channelName={channelName} />
```

**Step 3: Commit**

```bash
git add client/src/components/home/
git commit -m "feat(call): Add call button and banner to DM conversation"
```

---

## Task 9: Frontend - WebSocket Event Handlers

**Files:**
- Modify: `client/src/stores/websocket.ts`

**Step 1: Add call event handlers**

In the WebSocket store, add handlers for call events:

```typescript
import {
  receiveIncomingCall,
  callConnected,
  participantJoined,
  participantLeft,
  callEndedExternally,
} from "@/stores/call";

// In the message handler switch/if chain, add:
case "call_started":
case "incoming_call":
  receiveIncomingCall(
    event.channel_id,
    event.initiator,
    event.initiator_name
  );
  break;

case "call_ended":
  callEndedExternally(event.channel_id, event.reason, event.duration_secs);
  break;

case "participant_joined":
  participantJoined(event.channel_id, event.user_id);
  break;

case "participant_left":
  participantLeft(event.channel_id, event.user_id);
  break;

case "call_declined":
  // Handle UI update for caller
  break;
```

**Step 2: Commit**

```bash
git add client/src/stores/websocket.ts
git commit -m "feat(call): Add WebSocket event handlers for calls"
```

---

## Task 10: Integration Testing

**Step 1: Manual testing checklist**

1. Start the server: `cargo run -p vc-server`
2. Start the client: `cd client && npm run dev`
3. Open two browser windows logged in as different users
4. Create a DM between the two users
5. Test call flow:
   - User A clicks call button
   - User B sees incoming call banner
   - User B clicks Join
   - Both users connected
   - User A clicks hang up
   - Call ends for both

**Step 2: Verify all features**

- [ ] Call button appears in DM header
- [ ] Call button disabled when already in call
- [ ] Incoming call shows pulsing banner
- [ ] Join/Decline buttons work
- [ ] Call timer shows during call
- [ ] Hang up ends call
- [ ] Call ended message appears

**Step 3: Build verification**

```bash
cargo build -p vc-server --release
cd client && npm run build
```

**Step 4: Final commit**

```bash
git add -A
git commit -m "feat(call): Complete DM voice calls implementation"
```

---

## Summary

| Task | Component | Files |
|------|-----------|-------|
| 1 | Call state types | `server/src/voice/call.rs` |
| 2 | Redis call service | `server/src/voice/call_service.rs` |
| 3 | API handlers | `server/src/voice/call_handlers.rs` |
| 4 | WebSocket events | `server/src/ws/events.rs` |
| 5 | Frontend store | `client/src/stores/call.ts` |
| 6 | API functions | `client/src/lib/tauri.ts` |
| 7 | CallBanner | `client/src/components/call/` |
| 8 | DM integration | `client/src/components/home/` |
| 9 | WS handlers | `client/src/stores/websocket.ts` |
| 10 | Integration test | Manual verification |
