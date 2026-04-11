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
    /// "incoming" or "outgoing" for pending requests, None otherwise
    pub direction: Option<String>,
}

/// Request to send a friend request
#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct SendFriendRequestBody {
    /// Username or user ID of the person to add
    #[validate(length(min = 1))]
    pub username: String,
}
