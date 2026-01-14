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
    Cancelled,   // Initiator hung up before anyone joined
    AllDeclined, // All recipients declined
    NoAnswer,    // Timeout (90s)
    LastLeft,    // Last participant left
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
            (CallState::Ringing { .. }, CallEventType::Ended { reason }) => Ok(CallState::Ended {
                reason: *reason,
                duration_secs: None,
                ended_at: Utc::now(),
            }),

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
            (CallState::Active { started_at, .. }, CallEventType::Ended { reason }) => {
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
    fn test_last_participant_leaves_ends_call() {
        let mut participants = HashSet::new();
        let user = Uuid::new_v4();
        participants.insert(user);

        let state = CallState::Active {
            started_at: Utc::now(),
            participants,
        };
        let new_state = state
            .apply(&CallEventType::Left { user_id: user })
            .unwrap();

        assert!(matches!(
            new_state,
            CallState::Ended {
                reason: EndReason::LastLeft,
                ..
            }
        ));
    }

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
}
