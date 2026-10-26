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

    /// Resource already exists.
    #[error("{0}")]
    Conflict(String),

    /// Validation error.
    #[error("Validation failed: {0}")]
    Validation(String),

    /// A requested server (guild) does not exist (C23).
    #[error("unknown server")]
    UnknownServer,

    /// Invalid request answered with the uppercase `VALIDATION_ERROR` code
    /// (C23 account creation; other admin validation keeps `validation`).
    #[error("{0}")]
    InvalidRequest(String),

    /// Database error.
    #[error("Database error")]
    Database(#[from] sqlx::Error),

    /// Permission error.
    #[error("Permission denied: {0}")]
    Permission(#[from] PermissionError),

    /// Re-authentication failed or was refused (C24); answered with the
    /// auth error codes (`INVALID_CREDENTIALS`, `MFA_CODE_REQUIRED`, ...).
    #[error(transparent)]
    Reauth(#[from] crate::auth::AuthError),

    /// Internal server error.
    #[error("Internal error: {0}")]
    Internal(String),
}

impl IntoResponse for AdminError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Self::Reauth(e) => return e.into_response(),
            Self::NotAdmin => (
                StatusCode::FORBIDDEN,
                serde_json::json!({"error": "NOT_ADMIN", "message": "System admin privileges required"}),
            ),
            Self::ElevationRequired => (
                StatusCode::FORBIDDEN,
                serde_json::json!({"error": "ELEVATION_REQUIRED", "message": "This action requires an elevated session"}),
            ),
            Self::MfaRequired => (
                StatusCode::BAD_REQUEST,
                serde_json::json!({"error": "MFA_REQUIRED", "message": "MFA must be enabled to elevate session"}),
            ),
            Self::InvalidMfaCode => (
                StatusCode::UNAUTHORIZED,
                serde_json::json!({"error": "INVALID_MFA_CODE", "message": "Invalid MFA code"}),
            ),
            Self::NotFound(what) => (
                StatusCode::NOT_FOUND,
                serde_json::json!({"error": "NOT_FOUND", "message": format!("{} not found", what)}),
            ),
            Self::Conflict(msg) => (
                StatusCode::CONFLICT,
                serde_json::json!({"error": "CONFLICT", "message": msg}),
            ),
            Self::Validation(msg) => (
                StatusCode::BAD_REQUEST,
                serde_json::json!({"error": "VALIDATION_ERROR", "message": msg}),
            ),
            Self::UnknownServer => (
                StatusCode::BAD_REQUEST,
                serde_json::json!({"error": "VALIDATION_ERROR", "message": "unknown server"}),
            ),
            Self::InvalidRequest(msg) => (
                StatusCode::BAD_REQUEST,
                serde_json::json!({"error": "VALIDATION_ERROR", "message": msg}),
            ),
            Self::Database(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"error": "INTERNAL_ERROR", "message": "Database error"}),
            ),
            Self::Permission(e) => (
                StatusCode::FORBIDDEN,
                serde_json::json!({"error": "PERMISSION_DENIED", "message": e.to_string()}),
            ),
            Self::Internal(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"error": "INTERNAL_ERROR", "message": msg}),
            ),
        };
        (status, Json(body)).into_response()
    }
}
