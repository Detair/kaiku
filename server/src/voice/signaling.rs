//! WebRTC Signaling Messages
//!
//! Note: Voice signaling is primarily handled through WebSocket events
//! (ClientEvent/ServerEvent in ws/mod.rs). These types are kept for
//! documentation and potential alternative transport use.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Participant info for room state messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct ParticipantInfo {
    /// User ID.
    pub user_id: Uuid,
    /// Whether the user is muted.
    pub muted: bool,
}

/// Signaling message types.
///
/// These messages are exchanged between client and server for WebRTC
/// session establishment and voice channel coordination.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(dead_code)]
pub enum SignalingMessage {
    // Client -> Server messages
    /// Join voice channel
    Join { channel_id: Uuid },
    /// Leave voice channel
    Leave { channel_id: Uuid },
    /// SDP Answer (response to server offer)
    Answer { channel_id: Uuid, sdp: String },
    /// ICE Candidate from client
    IceCandidate { channel_id: Uuid, candidate: String },
    /// Mute self
    Mute { channel_id: Uuid },
    /// Unmute self
    Unmute { channel_id: Uuid },

    // Server -> Client messages
    /// SDP Offer (server creates offer, client responds with answer)
    Offer { channel_id: Uuid, sdp: String },
    /// ICE Candidate from server
    ServerIceCandidate { channel_id: Uuid, candidate: String },
    /// User joined notification
    UserJoined { channel_id: Uuid, user_id: Uuid },
    /// User left notification
    UserLeft { channel_id: Uuid, user_id: Uuid },
    /// User muted notification
    UserMuted { channel_id: Uuid, user_id: Uuid },
    /// User unmuted notification
    UserUnmuted { channel_id: Uuid, user_id: Uuid },
    /// User speaking indicator
    Speaking { channel_id: Uuid, user_id: Uuid, speaking: bool },
    /// Current room state (sent on join)
    RoomState {
        channel_id: Uuid,
        participants: Vec<ParticipantInfo>,
    },
    /// Error response
    Error { code: String, message: String },
}
