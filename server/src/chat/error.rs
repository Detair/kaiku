//! Chat error types.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::permissions::PermissionError;

#[derive(Debug, thiserror::Error)]
pub enum ChatError {
    // -- Channel errors --
    #[error("Channel not found")]
    ChannelNotFound,

    #[error("Access denied")]
    Forbidden,

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Limit exceeded: {0}")]
    LimitExceeded(String),

    // -- Message errors --
    #[error("Message not found")]
    MessageNotFound,

    #[error("Cannot send messages to this user")]
    Blocked,

    #[error("Your message was blocked by the server's content filter.")]
    ContentFiltered,

    // -- Upload errors --
    #[error("File uploads are not configured")]
    StorageNotConfigured,

    #[error("File not found")]
    FileNotFound,

    #[error("File too large (max: {max_size} bytes)")]
    FileTooLarge { max_size: usize },

    #[error("Invalid file type: {mime_type}")]
    InvalidMimeType { mime_type: String },

    #[error("No file provided")]
    NoFile,

    #[error("Invalid filename")]
    InvalidFilename,

    #[error("Storage error: {0}")]
    Storage(String),

    // -- Override errors --
    #[error("Role not found")]
    RoleNotFound,

    #[error("Not a member of this guild")]
    NotMember,

    #[error("{0}")]
    Permission(#[from] PermissionError),

    // -- DM search errors --
    #[error("Invalid query: {0}")]
    InvalidQuery(String),

    // -- Database (shared) --
    #[error("Database error")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for ChatError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            // Channel
            Self::ChannelNotFound => (StatusCode::NOT_FOUND, "CHANNEL_NOT_FOUND", self.to_string()),
            Self::Forbidden => (StatusCode::FORBIDDEN, "FORBIDDEN", self.to_string()),
            Self::Validation(msg) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR", msg.clone()),
            Self::LimitExceeded(msg) => (StatusCode::FORBIDDEN, "LIMIT_EXCEEDED", msg.clone()),

            // Message
            Self::MessageNotFound => (StatusCode::NOT_FOUND, "MESSAGE_NOT_FOUND", self.to_string()),
            Self::Blocked => (StatusCode::FORBIDDEN, "BLOCKED", self.to_string()),
            Self::ContentFiltered => (StatusCode::FORBIDDEN, "CONTENT_FILTERED", self.to_string()),

            // Upload
            Self::StorageNotConfigured => (
                StatusCode::SERVICE_UNAVAILABLE,
                "STORAGE_NOT_CONFIGURED",
                self.to_string(),
            ),
            Self::FileNotFound => (StatusCode::NOT_FOUND, "FILE_NOT_FOUND", self.to_string()),
            Self::FileTooLarge { .. } => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "FILE_TOO_LARGE",
                self.to_string(),
            ),
            Self::InvalidMimeType { .. } => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "INVALID_MIME_TYPE",
                self.to_string(),
            ),
            Self::NoFile => (StatusCode::BAD_REQUEST, "NO_FILE", self.to_string()),
            Self::InvalidFilename => (
                StatusCode::BAD_REQUEST,
                "INVALID_FILENAME",
                self.to_string(),
            ),
            Self::Storage(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "STORAGE_ERROR",
                "Storage operation failed".to_string(),
            ),

            // Override
            Self::RoleNotFound => (StatusCode::NOT_FOUND, "ROLE_NOT_FOUND", self.to_string()),
            Self::NotMember => (StatusCode::FORBIDDEN, "NOT_MEMBER", self.to_string()),
            Self::Permission(e) => (StatusCode::FORBIDDEN, "PERMISSION_DENIED", e.to_string()),

            // DM Search
            Self::InvalidQuery(msg) => (StatusCode::BAD_REQUEST, "INVALID_QUERY", msg.clone()),

            // Database
            Self::Database(err) => {
                tracing::error!(%err, "Chat endpoint database error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    "Database error".to_string(),
                )
            }
        };
        (
            status,
            Json(serde_json::json!({ "error": code, "message": message })),
        )
            .into_response()
    }
}
