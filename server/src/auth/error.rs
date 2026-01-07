//! Authentication Error Types

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use thiserror::Error;

/// Authentication error types.
#[derive(Debug, Error)]
pub enum AuthError {
    /// Invalid credentials (wrong username/password).
    #[error("Invalid credentials")]
    InvalidCredentials,

    /// User not found.
    #[error("User not found")]
    UserNotFound,

    /// User already exists (registration).
    #[error("Username or email already taken")]
    UserAlreadyExists,

    /// Invalid or expired token.
    #[error("Invalid or expired token")]
    InvalidToken,

    /// Token has expired.
    #[error("Token expired")]
    TokenExpired,

    /// Missing Authorization header.
    #[error("Missing authorization header")]
    MissingAuthHeader,

    /// Invalid authorization header format.
    #[error("Invalid authorization header format")]
    InvalidAuthHeader,

    /// MFA required but not provided.
    #[error("MFA verification required")]
    MfaRequired,

    /// Invalid MFA code.
    #[error("Invalid MFA code")]
    InvalidMfaCode,

    /// Validation error.
    #[error("Validation failed: {0}")]
    Validation(String),

    /// Password hashing error.
    #[error("Password processing failed")]
    PasswordHash,

    /// Database error.
    #[error("Database error")]
    Database(#[from] sqlx::Error),

    /// JWT error.
    #[error("Token error")]
    Jwt(#[from] jsonwebtoken::errors::Error),

    /// Internal server error.
    #[error("Internal server error")]
    Internal(String),
}

/// Error response body for JSON responses.
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    /// Human-readable error message.
    pub error: String,
    /// Machine-readable error code.
    pub code: String,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            AuthError::InvalidCredentials => (StatusCode::UNAUTHORIZED, "INVALID_CREDENTIALS"),
            AuthError::UserNotFound => (StatusCode::NOT_FOUND, "USER_NOT_FOUND"),
            AuthError::UserAlreadyExists => (StatusCode::CONFLICT, "USER_EXISTS"),
            AuthError::InvalidToken => (StatusCode::UNAUTHORIZED, "INVALID_TOKEN"),
            AuthError::TokenExpired => (StatusCode::UNAUTHORIZED, "TOKEN_EXPIRED"),
            AuthError::MissingAuthHeader => (StatusCode::UNAUTHORIZED, "MISSING_AUTH"),
            AuthError::InvalidAuthHeader => (StatusCode::UNAUTHORIZED, "INVALID_AUTH_HEADER"),
            AuthError::MfaRequired => (StatusCode::FORBIDDEN, "MFA_REQUIRED"),
            AuthError::InvalidMfaCode => (StatusCode::UNAUTHORIZED, "INVALID_MFA"),
            AuthError::Validation(_) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR"),
            AuthError::PasswordHash => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
            AuthError::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
            AuthError::Jwt(_) => (StatusCode::UNAUTHORIZED, "TOKEN_ERROR"),
            AuthError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
        };

        let body = Json(ErrorResponse {
            error: self.to_string(),
            code: code.to_string(),
        });

        (status, body).into_response()
    }
}

/// Result type for auth operations.
pub type AuthResult<T> = Result<T, AuthError>;
