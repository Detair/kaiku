//! Admin module types.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
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
}

impl IntoResponse for AdminError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Self::NotAdmin => (StatusCode::FORBIDDEN, serde_json::json!({"error": "not_admin", "message": "System admin privileges required"})),
            Self::ElevationRequired => (StatusCode::FORBIDDEN, serde_json::json!({"error": "elevation_required", "message": "This action requires an elevated session"})),
            Self::MfaRequired => (StatusCode::BAD_REQUEST, serde_json::json!({"error": "mfa_required", "message": "MFA must be enabled to elevate session"})),
            Self::InvalidMfaCode => (StatusCode::UNAUTHORIZED, serde_json::json!({"error": "invalid_mfa_code", "message": "Invalid MFA code"})),
            Self::NotFound(what) => (StatusCode::NOT_FOUND, serde_json::json!({"error": "not_found", "message": format!("{} not found", what)})),
            Self::Validation(msg) => (StatusCode::BAD_REQUEST, serde_json::json!({"error": "validation", "message": msg})),
            Self::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": "database", "message": "Database error"})),
            Self::Permission(e) => (StatusCode::FORBIDDEN, serde_json::json!({"error": "permission", "message": e.to_string()})),
        };
        (status, Json(body)).into_response()
    }
}


// Request types
#[derive(Debug, Deserialize)]
pub struct ElevateRequest {
    pub mfa_code: String,
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ElevateResponse {
    pub elevated: bool,
    pub expires_at: DateTime<Utc>,
    pub session_id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct GlobalBanRequest {
    pub reason: String,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct SuspendGuildRequest {
    pub reason: String,
}

#[derive(Debug, Deserialize)]
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
