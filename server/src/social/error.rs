//! Social module error types.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use thiserror::Error;

/// Error types for social operations
#[derive(Debug, Error)]
pub enum SocialError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("User not found")]
    UserNotFound,

    #[error("Cannot send friend request to yourself")]
    SelfFriendRequest,

    #[error("Friend request already exists")]
    AlreadyExists,

    #[error("You are blocked by this user")]
    Blocked,

    #[error("Friendship not found")]
    FriendshipNotFound,

    #[error("Not authorized to perform this action")]
    Unauthorized,

    #[error("Validation error: {0}")]
    Validation(String),
}

impl IntoResponse for SocialError {
    fn into_response(self) -> Response {
        use serde_json::json;

        let (status, code, message) = match &self {
            Self::Database(err) => {
                tracing::error!("Database error: {}", err);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    "Database error".to_string(),
                )
            }
            Self::UserNotFound => (StatusCode::NOT_FOUND, "USER_NOT_FOUND", self.to_string()),
            Self::SelfFriendRequest => (
                StatusCode::BAD_REQUEST,
                "SELF_FRIEND_REQUEST",
                self.to_string(),
            ),
            Self::AlreadyExists => (StatusCode::CONFLICT, "ALREADY_EXISTS", self.to_string()),
            Self::Blocked => (StatusCode::FORBIDDEN, "BLOCKED", self.to_string()),
            Self::FriendshipNotFound => (
                StatusCode::NOT_FOUND,
                "FRIENDSHIP_NOT_FOUND",
                self.to_string(),
            ),
            Self::Unauthorized => (StatusCode::FORBIDDEN, "UNAUTHORIZED", self.to_string()),
            Self::Validation(msg) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR", msg.clone()),
        };

        (status, Json(json!({ "error": code, "message": message }))).into_response()
    }
}
