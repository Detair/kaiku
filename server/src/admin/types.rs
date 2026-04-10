//! Admin module types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Authenticated system admin user.
#[derive(Debug, Clone)]
pub struct SystemAdminUser {
    pub user_id: Uuid,
    pub username: String,
    pub granted_at: DateTime<Utc>,
}

/// Elevated admin session.
#[derive(Debug, Clone)]
pub struct ElevatedAdmin {
    pub user_id: Uuid,
    pub elevated_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub reason: Option<String>,
}

// Request types
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ElevateRequest {
    pub reason: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ElevateResponse {
    pub elevated: bool,
    pub expires_at: DateTime<Utc>,
    pub session_id: Uuid,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct GlobalBanRequest {
    pub reason: String,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SuspendGuildRequest {
    pub reason: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateAnnouncementRequest {
    pub title: String,
    pub content: String,
    #[serde(default = "default_severity")]
    pub severity: String,
    pub starts_at: Option<DateTime<Utc>>,
    pub ends_at: Option<DateTime<Utc>>,
}

fn default_severity() -> String {
    "info".to_string()
}

/// Admin status response for checking current user's admin state.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AdminStatusResponse {
    pub is_admin: bool,
    pub is_elevated: bool,
    pub elevation_expires_at: Option<DateTime<Utc>>,
}

/// Admin statistics response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AdminStatsResponse {
    pub user_count: i64,
    pub guild_count: i64,
    pub banned_count: i64,
}

// ============================================================================
// Bulk Action Types
// ============================================================================

/// Request to ban multiple users at once.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct BulkBanRequest {
    /// List of user IDs to ban.
    pub user_ids: Vec<Uuid>,
    /// Reason for banning.
    pub reason: String,
    /// Optional expiration time for the ban.
    pub expires_at: Option<DateTime<Utc>>,
}

/// Response for bulk ban operation.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct BulkBanResponse {
    /// Number of users successfully banned.
    pub banned_count: usize,
    /// Number of users that were already banned.
    pub already_banned: usize,
    /// User IDs that failed to ban (with reasons).
    pub failed: Vec<BulkActionFailure>,
}

/// Request to suspend multiple guilds at once.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct BulkSuspendRequest {
    /// List of guild IDs to suspend.
    pub guild_ids: Vec<Uuid>,
    /// Reason for suspension.
    pub reason: String,
}

/// Response for bulk suspend operation.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct BulkSuspendResponse {
    /// Number of guilds successfully suspended.
    pub suspended_count: usize,
    /// Number of guilds that were already suspended.
    pub already_suspended: usize,
    /// Guild IDs that failed to suspend (with reasons).
    pub failed: Vec<BulkActionFailure>,
}

/// Details about a failed bulk action item.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct BulkActionFailure {
    /// ID of the item that failed.
    pub id: Uuid,
    /// Reason for the failure.
    pub reason: String,
}
