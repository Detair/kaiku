//! Guild Type Definitions

use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use validator::Validate;

// ============================================================================
// Guild Entity
// ============================================================================

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct Guild {
    pub id: Uuid,
    pub name: String,
    pub owner_id: Uuid,
    pub icon_url: Option<String>,
    pub description: Option<String>,
    pub threads_enabled: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Guild with member count for list responses.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct GuildWithMemberCount {
    #[serde(flatten)]
    #[schema(inline)]
    pub guild: Guild,
    /// Total number of members in the guild.
    pub member_count: i64,
}

// ============================================================================
// Request Types
// ============================================================================

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct CreateGuildRequest {
    #[validate(length(min = 2, max = 100, message = "Name must be 2-100 characters"))]
    pub name: String,
    #[validate(length(max = 1000, message = "Description must be at most 1000 characters"))]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct UpdateGuildRequest {
    #[validate(length(min = 2, max = 100, message = "Name must be 2-100 characters"))]
    pub name: Option<String>,
    #[validate(length(max = 1000, message = "Description must be at most 1000 characters"))]
    pub description: Option<String>,
    pub icon_url: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct JoinGuildRequest {
    pub invite_code: String,
}

// ============================================================================
// Response Types
// ============================================================================

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
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

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct GuildInvite {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub code: String,
    pub created_by: Uuid,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub use_count: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateInviteRequest {
    /// Expiry duration: "30m", "1h", "1d", "7d", or "never"
    pub expires_in: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
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
#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct CreateRoleRequest {
    #[validate(length(min = 1, max = 64, message = "Role name must be 1-64 characters"))]
    pub name: String,
    pub color: Option<String>,
    pub permissions: Option<u64>,
}

/// Request to update a guild role.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateRoleRequest {
    pub name: Option<String>,
    pub color: Option<String>,
    pub permissions: Option<u64>,
    pub position: Option<i32>,
}

/// Guild role response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
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

// ============================================================================
// Emoji Types
// ============================================================================

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, utoipa::ToSchema)]
pub struct GuildEmoji {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub name: String,
    pub image_url: String,
    pub animated: bool,
    pub uploaded_by: Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct CreateEmojiRequest {
    #[validate(length(min = 2, max = 32, message = "Name must be 2-32 characters"))]
    pub name: String,
}

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct UpdateEmojiRequest {
    #[validate(length(min = 2, max = 32, message = "Name must be 2-32 characters"))]
    pub name: String,
}

// ============================================================================
// Guild Settings Types
// ============================================================================

/// Guild settings response (subset of guild-level configuration).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GuildSettings {
    pub threads_enabled: bool,
}

/// Request to update guild settings.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateGuildSettingsRequest {
    pub threads_enabled: Option<bool>,
}

// ============================================================================
// Command Types
// ============================================================================

/// Available slash command in a guild (from installed bots).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GuildCommandInfo {
    pub name: String,
    pub description: String,
    pub bot_name: String,
    pub application_id: Uuid,
    pub is_ambiguous: bool,
}
