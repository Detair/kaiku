//! DM Voice Call State Management
//!
//! Event-sourced call state using Redis Streams for multi-node coordination.
//!
//! This module provides:
//! - Call event types for state reconstruction
//! - Call state machine with transitions
//! - Call capabilities for future extensibility (video, screen share)

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Capabilities for a voice call
///
/// This struct allows future extensibility for video calls and screen sharing
/// while maintaining backwards compatibility with existing audio-only calls.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CallCapabilities {
    /// Audio capability (always true for voice calls)
    pub audio: bool,
    /// Video capability (future: video calls)
    pub video: bool,
    /// Screen share capability (future: screen sharing)
    pub screenshare: bool,
}

impl CallCapabilities {
    /// Create capabilities for an audio-only call (default)
    #[must_use]
    pub const fn audio_only() -> Self {
        Self {
            audio: true,
            video: false,
            screenshare: false,
        }
    }

    /// Create capabilities with video enabled
    #[must_use]
    pub const fn with_video() -> Self {
        Self {
            audio: true,
            video: true,
            screenshare: false,
        }
    }

    /// Create capabilities with screen share enabled
    #[must_use]
    pub const fn with_screenshare() -> Self {
        Self {
            audio: true,
            video: false,
            screenshare: true,
        }
    }

    /// Create capabilities with all features enabled
    #[must_use]
    pub const fn all() -> Self {
        Self {
            audio: true,
            video: true,
            screenshare: true,
        }
    }
}

impl Default for CallCapabilities {
    /// Default to audio-only call (the common case for voice calls)
    fn default() -> Self {
        Self::audio_only()
    }
}

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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    Cancelled,   // Initiator hung up before anyone joined
    AllDeclined, // All recipients declined
    NoAnswer,    // Timeout (90s)
    LastLeft,    // Last participant left
}

/// Derived call state from event stream
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
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
        Self::Ringing {
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
                Self::Ringing {
                    started_at,
                    started_by,
                    ..
                },
                CallEventType::Joined { user_id },
            ) => {
                let mut participants = HashSet::new();
                participants.insert(started_by);
                participants.insert(*user_id);
                Ok(Self::Active {
                    started_at,
                    participants,
                })
            }

            // Ringing -> Ringing with decline recorded
            (
                Self::Ringing {
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
                    Ok(Self::Ended {
                        reason: EndReason::AllDeclined,
                        duration_secs: None,
                        ended_at: Utc::now(),
                    })
                } else {
                    Ok(Self::Ringing {
                        started_by,
                        started_at,
                        declined_by,
                        target_users,
                    })
                }
            }

            // Ringing -> Ended when initiator cancels
            (Self::Ringing { .. }, CallEventType::Ended { reason }) => Ok(Self::Ended {
                reason: *reason,
                duration_secs: None,
                ended_at: Utc::now(),
            }),

            // Active -> Active with new participant
            (
                Self::Active {
                    started_at,
                    mut participants,
                },
                CallEventType::Joined { user_id },
            ) => {
                participants.insert(*user_id);
                Ok(Self::Active {
                    started_at,
                    participants,
                })
            }

            // Active -> Active or Ended when someone leaves
            (
                Self::Active {
                    started_at,
                    mut participants,
                },
                CallEventType::Left { user_id },
            ) => {
                participants.remove(user_id);
                if participants.is_empty() {
                    let duration =
                        Utc::now().signed_duration_since(started_at).num_seconds() as u32;
                    Ok(Self::Ended {
                        reason: EndReason::LastLeft,
                        duration_secs: Some(duration),
                        ended_at: Utc::now(),
                    })
                } else {
                    Ok(Self::Active {
                        started_at,
                        participants,
                    })
                }
            }

            // Active -> Ended
            (Self::Active { started_at, .. }, CallEventType::Ended { reason }) => {
                let duration = Utc::now().signed_duration_since(started_at).num_seconds() as u32;
                Ok(Self::Ended {
                    reason: *reason,
                    duration_secs: Some(duration),
                    ended_at: Utc::now(),
                })
            }

            // Ended state is terminal
            (Self::Ended { .. }, _) => Err(CallStateError::CallAlreadyEnded),

            // Invalid transitions
            (state, event) => Err(CallStateError::InvalidTransition {
                state: format!("{state:?}"),
                event: format!("{event:?}"),
            }),
        }
    }

    /// Check if call is still active (not ended)
    pub const fn is_active(&self) -> bool {
        !matches!(self, Self::Ended { .. })
    }

    /// Get participants if call is active
    pub const fn participants(&self) -> Option<&HashSet<Uuid>> {
        match self {
            Self::Active { participants, .. } => Some(participants),
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

    // ============================================================
    // CallCapabilities tests
    // ============================================================

    #[test]
    fn test_call_capabilities_audio_only() {
        let caps = CallCapabilities::audio_only();
        assert!(caps.audio);
        assert!(!caps.video);
        assert!(!caps.screenshare);
    }

    #[test]
    fn test_call_capabilities_with_video() {
        let caps = CallCapabilities::with_video();
        assert!(caps.audio);
        assert!(caps.video);
        assert!(!caps.screenshare);
    }

    #[test]
    fn test_call_capabilities_with_screenshare() {
        let caps = CallCapabilities::with_screenshare();
        assert!(caps.audio);
        assert!(!caps.video);
        assert!(caps.screenshare);
    }

    #[test]
    fn test_call_capabilities_all() {
        let caps = CallCapabilities::all();
        assert!(caps.audio);
        assert!(caps.video);
        assert!(caps.screenshare);
    }

    #[test]
    fn test_call_capabilities_default() {
        // Default should be audio-only (semantic for voice calls)
        let caps = CallCapabilities::default();
        assert!(caps.audio);
        assert!(!caps.video);
        assert!(!caps.screenshare);
        // Default should equal audio_only()
        assert_eq!(caps, CallCapabilities::audio_only());
    }

    #[test]
    fn test_call_capabilities_serialization() {
        let caps = CallCapabilities::audio_only();
        let json = serde_json::to_string(&caps).unwrap();
        let deserialized: CallCapabilities = serde_json::from_str(&json).unwrap();
        assert_eq!(caps, deserialized);
    }

    // ============================================================
    // Ringing state tests
    // ============================================================

    #[test]
    fn test_ringing_to_active_on_join() {
        let mut targets = HashSet::new();
        targets.insert(Uuid::new_v4());
        let initiator = Uuid::new_v4();
        let joiner = Uuid::new_v4();

        let state = CallState::new_ringing(initiator, targets);
        let new_state = state
            .apply(&CallEventType::Joined { user_id: joiner })
            .unwrap();

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
        let new_state = state
            .apply(&CallEventType::Declined { user_id: target })
            .unwrap();

        assert!(matches!(
            new_state,
            CallState::Ended {
                reason: EndReason::AllDeclined,
                ..
            }
        ));
    }

    #[test]
    fn test_partial_decline_stays_ringing() {
        let target1 = Uuid::new_v4();
        let target2 = Uuid::new_v4();
        let mut targets = HashSet::new();
        targets.insert(target1);
        targets.insert(target2);
        let initiator = Uuid::new_v4();

        let state = CallState::new_ringing(initiator, targets);
        let new_state = state
            .apply(&CallEventType::Declined { user_id: target1 })
            .unwrap();

        // Should still be ringing because target2 hasn't declined
        assert!(matches!(new_state, CallState::Ringing { .. }));
        if let CallState::Ringing { declined_by, .. } = new_state {
            assert!(declined_by.contains(&target1));
            assert!(!declined_by.contains(&target2));
        }
    }

    #[test]
    fn test_ringing_cancelled_by_initiator() {
        let mut targets = HashSet::new();
        targets.insert(Uuid::new_v4());
        let initiator = Uuid::new_v4();

        let state = CallState::new_ringing(initiator, targets);
        let new_state = state
            .apply(&CallEventType::Ended {
                reason: EndReason::Cancelled,
            })
            .unwrap();

        assert!(matches!(
            new_state,
            CallState::Ended {
                reason: EndReason::Cancelled,
                duration_secs: None,
                ..
            }
        ));
    }

    #[test]
    fn test_ringing_no_answer_timeout() {
        let mut targets = HashSet::new();
        targets.insert(Uuid::new_v4());
        let initiator = Uuid::new_v4();

        let state = CallState::new_ringing(initiator, targets);
        let new_state = state
            .apply(&CallEventType::Ended {
                reason: EndReason::NoAnswer,
            })
            .unwrap();

        assert!(matches!(
            new_state,
            CallState::Ended {
                reason: EndReason::NoAnswer,
                ..
            }
        ));
    }

    #[test]
    fn test_ringing_invalid_left_event() {
        let mut targets = HashSet::new();
        targets.insert(Uuid::new_v4());
        let initiator = Uuid::new_v4();

        let state = CallState::new_ringing(initiator, targets);
        let result = state.apply(&CallEventType::Left {
            user_id: Uuid::new_v4(),
        });

        assert!(matches!(
            result,
            Err(CallStateError::InvalidTransition { .. })
        ));
    }

    #[test]
    fn test_ringing_invalid_started_event() {
        let mut targets = HashSet::new();
        targets.insert(Uuid::new_v4());
        let initiator = Uuid::new_v4();

        let state = CallState::new_ringing(initiator, targets);
        let result = state.apply(&CallEventType::Started {
            initiator: Uuid::new_v4(),
        });

        assert!(matches!(
            result,
            Err(CallStateError::InvalidTransition { .. })
        ));
    }

    // ============================================================
    // Active state tests
    // ============================================================

    #[test]
    fn test_active_add_participant() {
        let user1 = Uuid::new_v4();
        let user2 = Uuid::new_v4();
        let new_user = Uuid::new_v4();
        let mut participants = HashSet::new();
        participants.insert(user1);
        participants.insert(user2);

        let state = CallState::Active {
            started_at: Utc::now(),
            participants,
        };
        let new_state = state
            .apply(&CallEventType::Joined { user_id: new_user })
            .unwrap();

        if let CallState::Active { participants, .. } = new_state {
            assert_eq!(participants.len(), 3);
            assert!(participants.contains(&new_user));
        } else {
            panic!("Expected Active state");
        }
    }

    #[test]
    fn test_active_participant_leaves_not_last() {
        let user1 = Uuid::new_v4();
        let user2 = Uuid::new_v4();
        let mut participants = HashSet::new();
        participants.insert(user1);
        participants.insert(user2);

        let state = CallState::Active {
            started_at: Utc::now(),
            participants,
        };
        let new_state = state
            .apply(&CallEventType::Left { user_id: user1 })
            .unwrap();

        assert!(matches!(new_state, CallState::Active { .. }));
        if let CallState::Active { participants, .. } = new_state {
            assert_eq!(participants.len(), 1);
            assert!(participants.contains(&user2));
            assert!(!participants.contains(&user1));
        }
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

        assert!(matches!(
            new_state,
            CallState::Ended {
                reason: EndReason::LastLeft,
                ..
            }
        ));
    }

    #[test]
    fn test_active_ended_manually() {
        let mut participants = HashSet::new();
        participants.insert(Uuid::new_v4());
        participants.insert(Uuid::new_v4());

        let state = CallState::Active {
            started_at: Utc::now(),
            participants,
        };
        let new_state = state
            .apply(&CallEventType::Ended {
                reason: EndReason::LastLeft,
            })
            .unwrap();

        assert!(matches!(
            new_state,
            CallState::Ended {
                reason: EndReason::LastLeft,
                duration_secs: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn test_active_invalid_decline_event() {
        let mut participants = HashSet::new();
        participants.insert(Uuid::new_v4());

        let state = CallState::Active {
            started_at: Utc::now(),
            participants,
        };
        let result = state.apply(&CallEventType::Declined {
            user_id: Uuid::new_v4(),
        });

        assert!(matches!(
            result,
            Err(CallStateError::InvalidTransition { .. })
        ));
    }

    #[test]
    fn test_active_invalid_started_event() {
        let mut participants = HashSet::new();
        participants.insert(Uuid::new_v4());

        let state = CallState::Active {
            started_at: Utc::now(),
            participants,
        };
        let result = state.apply(&CallEventType::Started {
            initiator: Uuid::new_v4(),
        });

        assert!(matches!(
            result,
            Err(CallStateError::InvalidTransition { .. })
        ));
    }

    // ============================================================
    // Ended state tests
    // ============================================================

    #[test]
    fn test_ended_state_is_terminal() {
        let state = CallState::Ended {
            reason: EndReason::Cancelled,
            duration_secs: None,
            ended_at: Utc::now(),
        };
        let result = state.apply(&CallEventType::Joined {
            user_id: Uuid::new_v4(),
        });

        assert!(matches!(result, Err(CallStateError::CallAlreadyEnded)));
    }

    #[test]
    fn test_ended_state_terminal_for_left() {
        let state = CallState::Ended {
            reason: EndReason::LastLeft,
            duration_secs: Some(120),
            ended_at: Utc::now(),
        };
        let result = state.apply(&CallEventType::Left {
            user_id: Uuid::new_v4(),
        });

        assert!(matches!(result, Err(CallStateError::CallAlreadyEnded)));
    }

    #[test]
    fn test_ended_state_terminal_for_declined() {
        let state = CallState::Ended {
            reason: EndReason::NoAnswer,
            duration_secs: None,
            ended_at: Utc::now(),
        };
        let result = state.apply(&CallEventType::Declined {
            user_id: Uuid::new_v4(),
        });

        assert!(matches!(result, Err(CallStateError::CallAlreadyEnded)));
    }

    // ============================================================
    // Helper method tests
    // ============================================================

    #[test]
    fn test_is_active_for_ringing() {
        let state = CallState::new_ringing(Uuid::new_v4(), HashSet::new());
        assert!(state.is_active());
    }

    #[test]
    fn test_is_active_for_active() {
        let mut participants = HashSet::new();
        participants.insert(Uuid::new_v4());
        let state = CallState::Active {
            started_at: Utc::now(),
            participants,
        };
        assert!(state.is_active());
    }

    #[test]
    fn test_is_active_for_ended() {
        let state = CallState::Ended {
            reason: EndReason::Cancelled,
            duration_secs: None,
            ended_at: Utc::now(),
        };
        assert!(!state.is_active());
    }

    #[test]
    fn test_participants_for_active_state() {
        let user = Uuid::new_v4();
        let mut participants = HashSet::new();
        participants.insert(user);
        let state = CallState::Active {
            started_at: Utc::now(),
            participants,
        };
        let result = state.participants();
        assert!(result.is_some());
        assert!(result.unwrap().contains(&user));
    }

    #[test]
    fn test_participants_for_ringing_state() {
        let state = CallState::new_ringing(Uuid::new_v4(), HashSet::new());
        assert!(state.participants().is_none());
    }

    #[test]
    fn test_participants_for_ended_state() {
        let state = CallState::Ended {
            reason: EndReason::Cancelled,
            duration_secs: None,
            ended_at: Utc::now(),
        };
        assert!(state.participants().is_none());
    }

    // ============================================================
    // Serialization tests
    // ============================================================

    #[test]
    fn test_call_event_type_serialization() {
        let event = CallEventType::Started {
            initiator: Uuid::new_v4(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("started"));

        let event = CallEventType::Ended {
            reason: EndReason::AllDeclined,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("all_declined"));
    }

    #[test]
    fn test_end_reason_serialization() {
        let reasons = [
            (EndReason::Cancelled, "cancelled"),
            (EndReason::AllDeclined, "all_declined"),
            (EndReason::NoAnswer, "no_answer"),
            (EndReason::LastLeft, "last_left"),
        ];

        for (reason, expected) in reasons {
            let json = serde_json::to_string(&reason).unwrap();
            assert!(json.contains(expected), "Expected {expected} in {json}");
        }
    }

    #[test]
    fn test_call_state_serialization() {
        let state = CallState::new_ringing(Uuid::new_v4(), HashSet::new());
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("ringing"));

        let mut participants = HashSet::new();
        participants.insert(Uuid::new_v4());
        let state = CallState::Active {
            started_at: Utc::now(),
            participants,
        };
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("active"));

        let state = CallState::Ended {
            reason: EndReason::Cancelled,
            duration_secs: Some(60),
            ended_at: Utc::now(),
        };
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("ended"));
    }
}
