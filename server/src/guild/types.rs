//! Guild Type Definitions

use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use validator::Validate;

// ============================================================================
// Guild Entity
// ============================================================================

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Guild {
    pub id: Uuid,
    pub name: String,
    pub owner_id: Uuid,
    pub icon_url: Option<String>,
    pub description: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// ============================================================================
// Request Types
// ============================================================================

#[derive(Debug, Deserialize, Validate)]
pub struct CreateGuildRequest {
    #[validate(length(min = 2, max = 100, message = "Name must be 2-100 characters"))]
    pub name: String,
    #[validate(length(max = 1000, message = "Description must be at most 1000 characters"))]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateGuildRequest {
    #[validate(length(min = 2, max = 100, message = "Name must be 2-100 characters"))]
    pub name: Option<String>,
    #[validate(length(max = 1000, message = "Description must be at most 1000 characters"))]
    pub description: Option<String>,
    pub icon_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct JoinGuildRequest {
    pub invite_code: String,
}

// ============================================================================
// Response Types
// ============================================================================

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct GuildMember {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub nickname: Option<String>,
    pub joined_at: chrono::DateTime<chrono::Utc>,
    pub status: String,
    pub last_seen_at: Option<chrono::DateTime<chrono::Utc>>,
}

// ============================================================================
// Invite Types
// ============================================================================

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct GuildInvite {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub code: String,
    pub created_by: Uuid,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub use_count: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateInviteRequest {
    /// Expiry duration: "30m", "1h", "1d", "7d", or "never"
    pub expires_in: String,
}

#[derive(Debug, Serialize)]
pub struct InviteResponse {
    pub id: Uuid,
    pub code: String,
    pub guild_id: Uuid,
    pub guild_name: String,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub use_count: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// ============================================================================
// Role Types
// ============================================================================

/// Request to create a guild role.
#[derive(Debug, Deserialize, Validate)]
pub struct CreateRoleRequest {
    #[validate(length(min = 1, max = 64, message = "Role name must be 1-64 characters"))]
    pub name: String,
    pub color: Option<String>,
    pub permissions: Option<u64>,
}

/// Request to update a guild role.
#[derive(Debug, Deserialize)]
pub struct UpdateRoleRequest {
    pub name: Option<String>,
    pub color: Option<String>,
    pub permissions: Option<u64>,
    pub position: Option<i32>,
}

/// Guild role response.
#[derive(Debug, Serialize)]
pub struct RoleResponse {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub name: String,
    pub color: Option<String>,
    pub permissions: u64,
    pub position: i32,
    pub is_default: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
