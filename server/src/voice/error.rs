//! Voice Service Errors

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use thiserror::Error;
use uuid::Uuid;

/// Errors that can occur during voice operations.
#[derive(Debug, Error)]
pub enum VoiceError {
    /// Room not found.
    #[error("Room not found: {0}")]
    RoomNotFound(Uuid),

    /// Participant not found.
    #[error("Participant not found: {0}")]
    ParticipantNotFound(Uuid),

    /// WebRTC error.
    #[error("WebRTC error: {0}")]
    WebRtc(String),

    /// Signaling error.
    #[error("Signaling error: {0}")]
    Signaling(String),

    /// ICE connection failed.
    #[error("ICE connection failed")]
    IceConnectionFailed,

    /// Voice channel is full.
    #[error("Voice channel is full (max: {max_participants})")]
    ChannelFull {
        /// Maximum allowed participants.
        max_participants: usize,
    },

    /// User not authorized to join channel.
    #[error("Not authorized to join this voice channel")]
    Unauthorized,

    /// Channel not found.
    #[error("Channel not found: {0}")]
    ChannelNotFound(Uuid),

    /// Already in voice channel.
    #[error("Already in voice channel")]
    AlreadyJoined,

    /// Not in voice channel.
    #[error("Not in voice channel")]
    NotInChannel,

    /// Internal error.
    #[error("Internal error: {0}")]
    Internal(String),
}

impl IntoResponse for VoiceError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            Self::RoomNotFound(_) => (StatusCode::NOT_FOUND, "ROOM_NOT_FOUND", self.to_string()),
            Self::ParticipantNotFound(_) => {
                (StatusCode::NOT_FOUND, "PARTICIPANT_NOT_FOUND", self.to_string())
            }
            Self::WebRtc(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "WEBRTC_ERROR",
                "WebRTC operation failed".to_string(),
            ),
            Self::Signaling(_) => (
                StatusCode::BAD_REQUEST,
                "SIGNALING_ERROR",
                self.to_string(),
            ),
            Self::IceConnectionFailed => (
                StatusCode::SERVICE_UNAVAILABLE,
                "ICE_FAILED",
                self.to_string(),
            ),
            Self::ChannelFull { .. } => (StatusCode::CONFLICT, "CHANNEL_FULL", self.to_string()),
            Self::Unauthorized => (StatusCode::FORBIDDEN, "UNAUTHORIZED", self.to_string()),
            Self::ChannelNotFound(_) => {
                (StatusCode::NOT_FOUND, "CHANNEL_NOT_FOUND", self.to_string())
            }
            Self::AlreadyJoined => (StatusCode::CONFLICT, "ALREADY_JOINED", self.to_string()),
            Self::NotInChannel => (StatusCode::BAD_REQUEST, "NOT_IN_CHANNEL", self.to_string()),
            Self::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "Internal server error".to_string(),
            ),
        };

        let body = Json(serde_json::json!({
            "error": message,
            "code": code,
        }));

        (status, body).into_response()
    }
}

impl From<webrtc::Error> for VoiceError {
    fn from(err: webrtc::Error) -> Self {
        Self::WebRtc(err.to_string())
    }
}
