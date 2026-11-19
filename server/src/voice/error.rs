//! Voice Service Errors

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
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

    /// Screen share `stream_id` already exists in the room.
    #[error("Screen share stream already exists")]
    DuplicateStreamId,

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

    /// Rate limited.
    #[error("Rate limited: {0}")]
    RateLimited(&'static str),

    /// Internal error.
    #[error("Internal error: {0}")]
    Internal(String),

    /// Database error.
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    /// A message for a socket could not be queued (closed or stalled client).
    #[error("Outbound send failed: {0}")]
    Outbound(#[from] crate::ws::handlers::OutboundSendError),

    /// The user may not share their screen in this channel.
    #[error("Not allowed to share the screen in this channel")]
    ScreenShareForbidden,

    /// A screen share limit (per user or per channel) is reached.
    #[error("{0}")]
    ScreenShareLimit(&'static str),

    /// A video track arrived without an announced source and was dropped.
    #[error("Video track without an announced source was dropped")]
    VideoSourceUnannounced,

    /// The join was refused (deliberately without the reason).
    #[error("Cannot join this call")]
    JoinRefused,
}

impl VoiceError {
    /// Machine-readable code (UPPER_SNAKE), used in HTTP bodies and in the
    /// `code` of the WebSocket `voice_error` event.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::RoomNotFound(_) => "ROOM_NOT_FOUND",
            Self::ParticipantNotFound(_) => "PARTICIPANT_NOT_FOUND",
            Self::WebRtc(_) => "WEBRTC_ERROR",
            Self::Signaling(_) => "SIGNALING_ERROR",
            Self::DuplicateStreamId => "DUPLICATE_STREAM_ID",
            Self::IceConnectionFailed => "ICE_FAILED",
            Self::ChannelFull { .. } => "CHANNEL_FULL",
            Self::Unauthorized => "UNAUTHORIZED",
            Self::ChannelNotFound(_) => "CHANNEL_NOT_FOUND",
            Self::AlreadyJoined => "ALREADY_JOINED",
            Self::NotInChannel => "NOT_IN_CHANNEL",
            Self::RateLimited(_) => "RATE_LIMITED",
            Self::Internal(_) | Self::Outbound(_) => "INTERNAL_ERROR",
            Self::Database(_) => "DATABASE_ERROR",
            Self::ScreenShareForbidden => "SCREEN_SHARE_FORBIDDEN",
            Self::ScreenShareLimit(_) => "SCREEN_SHARE_LIMIT",
            Self::VideoSourceUnannounced => "VIDEO_SOURCE_UNANNOUNCED",
            Self::JoinRefused => "JOIN_REFUSED",
        }
    }

    /// Human-readable text for the client. Internal details (database,
    /// WebRTC stack) stay in the server log.
    #[must_use]
    pub fn client_message(&self) -> String {
        match self {
            Self::WebRtc(_) => "WebRTC operation failed".to_string(),
            Self::Internal(_) | Self::Outbound(_) => "Internal server error".to_string(),
            Self::Database(_) => "Database error".to_string(),
            _ => self.to_string(),
        }
    }
}

impl IntoResponse for VoiceError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::RoomNotFound(_) | Self::ParticipantNotFound(_) | Self::ChannelNotFound(_) => {
                StatusCode::NOT_FOUND
            }
            Self::WebRtc(_) | Self::Internal(_) | Self::Outbound(_) | Self::Database(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
            Self::Signaling(_) | Self::NotInChannel | Self::VideoSourceUnannounced => {
                StatusCode::BAD_REQUEST
            }
            Self::DuplicateStreamId
            | Self::ChannelFull { .. }
            | Self::AlreadyJoined
            | Self::ScreenShareLimit(_) => StatusCode::CONFLICT,
            Self::IceConnectionFailed => StatusCode::SERVICE_UNAVAILABLE,
            Self::Unauthorized | Self::ScreenShareForbidden | Self::JoinRefused => {
                StatusCode::FORBIDDEN
            }
            Self::RateLimited(_) => StatusCode::TOO_MANY_REQUESTS,
        };
        if let Self::Database(err) = &self {
            tracing::error!(%err, "Voice endpoint database error");
        }

        let body = Json(serde_json::json!({
            "error": self.code(),
            "message": self.client_message(),
        }));

        (status, body).into_response()
    }
}

impl From<webrtc::Error> for VoiceError {
    fn from(err: webrtc::Error) -> Self {
        Self::WebRtc(err.to_string())
    }
}
