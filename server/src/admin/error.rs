//! Admin module error types.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use thiserror::Error;

use crate::permissions::PermissionError;

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
