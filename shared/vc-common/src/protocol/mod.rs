//! WebSocket Protocol Messages
//!
//! Shared message types for real-time communication.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::{Message, UserProfile, UserStatus};

/// Client-to-server WebSocket events.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientEvent {
    /// Ping for keepalive
    Ping,

    /// Subscribe to channel events
    Subscribe {
        /// Channel to subscribe to.
        channel_id: Uuid
    },

    /// Unsubscribe from channel events
    Unsubscribe {
        /// Channel to unsubscribe from.
        channel_id: Uuid
    },

    /// Send typing indicator
    Typing {
        /// Channel user is typing in.
        channel_id: Uuid
    },

    /// Stop typing indicator
    StopTyping {
        /// Channel user stopped typing in.
        channel_id: Uuid
    },

    /// Voice: Join channel
    VoiceJoin {
        /// Voice channel to join.
        channel_id: Uuid
    },

    /// Voice: Leave channel
    VoiceLeave {
        /// Voice channel to leave.
        channel_id: Uuid
    },

    /// Voice: SDP Offer
    VoiceOffer {
        /// Voice channel.
        channel_id: Uuid,
        /// SDP offer.
        sdp: String
    },

    /// Voice: SDP Answer
    VoiceAnswer {
        /// Voice channel.
        channel_id: Uuid,
        /// SDP answer.
        sdp: String
    },

    /// Voice: ICE Candidate
    VoiceIce {
        /// Voice channel.
        channel_id: Uuid,
        /// ICE candidate.
        candidate: String
    },

    /// Voice: Mute self
    VoiceMute {
        /// Voice channel.
        channel_id: Uuid
    },

    /// Voice: Unmute self
    VoiceUnmute {
        /// Voice channel.
        channel_id: Uuid
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
        user: UserProfile
    },

    /// New message
    MessageCreate {
        /// New message.
        message: Message
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
        user: UserProfile
    },

    /// User stopped typing
    TypingStop {
        /// Channel user stopped typing in.
        channel_id: Uuid,
        /// User who stopped typing.
        user_id: Uuid
    },

    /// User presence changed
    PresenceUpdate {
        /// User whose presence changed.
        user_id: Uuid,
        /// New status.
        status: UserStatus
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
        user_id: Uuid
    },

    /// Voice: SDP Offer from another user
    VoiceOffer {
        /// Voice channel.
        channel_id: Uuid,
        /// User sending offer.
        user_id: Uuid,
        /// SDP offer.
        sdp: String,
    },

    /// Voice: SDP Answer from another user
    VoiceAnswer {
        /// Voice channel.
        channel_id: Uuid,
        /// User sending answer.
        user_id: Uuid,
        /// SDP answer.
        sdp: String,
    },

    /// Voice: ICE Candidate from another user
    VoiceIce {
        /// Voice channel.
        channel_id: Uuid,
        /// User sending candidate.
        user_id: Uuid,
        /// ICE candidate.
        candidate: String,
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
        message: String
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
