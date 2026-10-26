//! Admin module types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

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

/// `POST /api/admin/elevate` body (C24): the caller confirms the password,
/// plus a current TOTP code when MFA is on.
#[derive(Deserialize, Validate, utoipa::ToSchema)]
pub struct ElevateRequest {
    /// The account password.
    #[validate(length(min = 1))]
    pub password: String,
    /// Current TOTP code; required when the account has MFA on.
    #[validate(length(max = 16))]
    pub mfa_code: Option<String>,
    /// Why the admin elevates (stored in the audit log).
    #[validate(length(max = 255))]
    pub reason: Option<String>,
}

impl std::fmt::Debug for ElevateRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElevateRequest")
            .field("password", &"[REDACTED]")
            .field("mfa_code", &self.mfa_code.as_ref().map(|_| "[REDACTED]"))
            .field("reason", &self.reason)
            .finish()
    }
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

/// Request to create a local account with a temporary password.
///
/// The new user must replace the temporary password at first sign-in.
#[derive(Deserialize, Validate, utoipa::ToSchema)]
pub struct CreateUserRequest {
    /// Username (3-32 lowercase alphanumeric + underscore).
    #[validate(
        length(min = 3, max = 32),
        regex(path = "crate::auth::types::USERNAME_REGEX")
    )]
    pub username: String,
    /// Display name (optional, defaults to the username).
    #[validate(length(max = 64))]
    pub display_name: Option<String>,
    /// Temporary password handed to the user (8-128 characters).
    #[validate(length(min = 8, max = 128))]
    pub temporary_password: String,
    /// Servers (guilds) the new account joins as an active member. Optional,
    /// at most 10; duplicates are ignored and every id must exist.
    #[serde(default)]
    #[validate(length(max = 10))]
    pub guild_ids: Vec<Uuid>,
}

impl std::fmt::Debug for CreateUserRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreateUserRequest")
            .field("username", &self.username)
            .field("display_name", &self.display_name)
            .field("temporary_password", &"[REDACTED]")
            .field("guild_ids", &self.guild_ids)
            .finish()
    }
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
