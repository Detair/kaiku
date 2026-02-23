//! Admin module types.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::permissions::PermissionError;

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

/// Admin API error type.
#[derive(Debug, Error)]
pub enum AdminError {
    /// User is not a system admin.
    #[error("System admin privileges required")]
    NotAdmin,

    /// Action requires elevated admin session.
    #[error("This action requires an elevated session")]
    ElevationRequired,

    /// MFA must be enabled to elevate session.
    #[error("MFA must be enabled to elevate session")]
    MfaRequired,

    /// Invalid MFA code provided.
    #[error("Invalid MFA code")]
    InvalidMfaCode,

    /// Resource not found.
    #[error("{0} not found")]
    NotFound(String),

    /// Validation error.
    #[error("Validation failed: {0}")]
    Validation(String),

    /// Database error.
    #[error("Database error")]
    Database(#[from] sqlx::Error),

    /// Permission error.
    #[error("Permission denied: {0}")]
    Permission(#[from] PermissionError),

    /// Internal server error.
    #[error("Internal error: {0}")]
    Internal(String),
}

impl IntoResponse for AdminError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Self::NotAdmin => (
                StatusCode::FORBIDDEN,
                serde_json::json!({"error": "not_admin", "message": "System admin privileges required"}),
            ),
            Self::ElevationRequired => (
                StatusCode::FORBIDDEN,
                serde_json::json!({"error": "elevation_required", "message": "This action requires an elevated session"}),
            ),
            Self::MfaRequired => (
                StatusCode::BAD_REQUEST,
                serde_json::json!({"error": "mfa_required", "message": "MFA must be enabled to elevate session"}),
            ),
            Self::InvalidMfaCode => (
                StatusCode::UNAUTHORIZED,
                serde_json::json!({"error": "invalid_mfa_code", "message": "Invalid MFA code"}),
            ),
            Self::NotFound(what) => (
                StatusCode::NOT_FOUND,
                serde_json::json!({"error": "not_found", "message": format!("{} not found", what)}),
            ),
            Self::Validation(msg) => (
                StatusCode::BAD_REQUEST,
                serde_json::json!({"error": "validation", "message": msg}),
            ),
            Self::Database(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"error": "database", "message": "Database error"}),
            ),
            Self::Permission(e) => (
                StatusCode::FORBIDDEN,
                serde_json::json!({"error": "permission", "message": e.to_string()}),
            ),
            Self::Internal(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"error": "internal", "message": msg}),
            ),
        };
        (status, Json(body)).into_response()
    }
}

// Request types
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ElevateRequest {
    pub mfa_code: String,
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
