//! WebSocket Protocol Messages
//!
//! Shared message types for real-time communication.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::{Message, UserProfile, UserStatus};

/// Which `PeerConnection` an ICE candidate belongs to.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PcType {
    Publisher,
    Subscriber,
}

impl std::fmt::Display for PcType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Publisher => write!(f, "publisher"),
            Self::Subscriber => write!(f, "subscriber"),
        }
    }
}

/// Client-to-server WebSocket events.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientEvent {
    /// Ping for keepalive
    Ping,

    /// Subscribe to channel events
    Subscribe {
        /// Channel to subscribe to.
        channel_id: Uuid,
    },

    /// Unsubscribe from channel events
    Unsubscribe {
        /// Channel to unsubscribe from.
        channel_id: Uuid,
    },

    /// Send typing indicator
    Typing {
        /// Channel user is typing in.
        channel_id: Uuid,
    },

    /// Stop typing indicator
    StopTyping {
        /// Channel user stopped typing in.
        channel_id: Uuid,
    },

    /// Voice: Join channel
    VoiceJoin {
        /// Voice channel to join.
        channel_id: Uuid,
    },

    /// Voice: Leave channel
    VoiceLeave {
        /// Voice channel to leave.
        channel_id: Uuid,
    },

    /// Voice: SDP offer for publisher `PeerConnection` (mic, screen, webcam tracks)
    VoicePublisherOffer {
        /// Voice channel.
        channel_id: Uuid,
        /// SDP offer.
        sdp: String,
    },

    /// Voice: SDP answer for subscriber `PeerConnection` (receiving other users' tracks)
    VoiceSubscriberAnswer {
        /// Voice channel.
        channel_id: Uuid,
        /// SDP answer.
        sdp: String,
    },

    /// Voice: ICE Candidate
    VoiceIceCandidate {
        /// Voice channel.
        channel_id: Uuid,
        /// ICE candidate.
        candidate: String,
        /// Which `PeerConnection` this candidate belongs to.
        pc_type: PcType,
    },

    /// Voice: Mute self
    VoiceMute {
        /// Voice channel.
        channel_id: Uuid,
    },

    /// Voice: Unmute self
    VoiceUnmute {
        /// Voice channel.
        channel_id: Uuid,
    },
}

/// Server-to-client WebSocket events.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    /// Pong response
    Pong,

    /// Connection ready with user info
    Ready {
        /// Authenticated user profile.
        user: UserProfile,
    },

    /// New message
    MessageCreate {
        /// New message.
        message: Message,
    },

    /// Message updated
    MessageUpdate {
        /// Channel containing message.
        channel_id: Uuid,
        /// Updated message ID.
        message_id: Uuid,
        /// New content.
        content: String,
    },

    /// Message deleted
    MessageDelete {
        /// Channel containing message.
        channel_id: Uuid,
        /// Deleted message ID.
        message_id: Uuid,
    },

    /// User typing
    TypingStart {
        /// Channel user is typing in.
        channel_id: Uuid,
        /// User who is typing.
        user: UserProfile,
    },

    /// User stopped typing
    TypingStop {
        /// Channel user stopped typing in.
        channel_id: Uuid,
        /// User who stopped typing.
        user_id: Uuid,
    },

    /// User presence changed
    PresenceUpdate {
        /// User whose presence changed.
        user_id: Uuid,
        /// New status.
        status: UserStatus,
    },

    /// Voice: User joined channel
    VoiceUserJoined {
        /// Voice channel.
        channel_id: Uuid,
        /// User who joined.
        user: UserProfile,
    },

    /// Voice: User left channel
    VoiceUserLeft {
        /// Voice channel.
        channel_id: Uuid,
        /// User who left.
        user_id: Uuid,
    },

    /// Voice: SDP answer to client's publisher offer
    VoicePublisherAnswer {
        /// Voice channel.
        channel_id: Uuid,
        /// SDP answer.
        sdp: String,
    },

    /// Voice: SDP offer for subscriber `PeerConnection`
    VoiceSubscriberOffer {
        /// Voice channel.
        channel_id: Uuid,
        /// SDP offer.
        sdp: String,
    },

    /// Voice: ICE Candidate from server
    VoiceIceCandidate {
        /// Voice channel.
        channel_id: Uuid,
        /// ICE candidate.
        candidate: String,
        /// Which `PeerConnection` this candidate belongs to.
        pc_type: PcType,
    },

    /// Voice: User speaking indicator
    VoiceSpeaking {
        /// Voice channel.
        channel_id: Uuid,
        /// User speaking.
        user_id: Uuid,
        /// Whether user is speaking.
        speaking: bool,
    },

    /// Error
    Error {
        /// Error code.
        code: String,
        /// Error message.
        message: String,
    },
}

/// WebSocket message wrapper with optional request ID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsMessage<T> {
    /// Optional request ID for request-response correlation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The actual event
    #[serde(flatten)]
    pub event: T,
}
