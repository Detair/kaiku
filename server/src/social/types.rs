use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use validator::Validate;

/// Friendship status enum
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema,
)]
#[sqlx(type_name = "friendship_status", rename_all = "lowercase")]
pub enum FriendshipStatus {
    Pending,
    Accepted,
    Blocked,
}

/// Friendship record from database
#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct Friendship {
    pub id: Uuid,
    pub requester_id: Uuid,
    pub addressee_id: Uuid,
    pub status: FriendshipStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Friend user information (enriched with user details)
#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct Friend {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub status_message: Option<String>,
    pub is_online: bool,
    pub friendship_id: Uuid,
    pub friendship_status: FriendshipStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Request to send a friend request
#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct SendFriendRequestBody {
    /// Username or user ID of the person to add
    #[validate(length(min = 1))]
    pub username: String,
}

/// Error types for social operations
#[derive(Debug, thiserror::Error)]
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

impl axum::response::IntoResponse for SocialError {
    fn into_response(self) -> axum::response::Response {
        use axum::http::StatusCode;
        use axum::Json;
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
