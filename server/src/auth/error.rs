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

    /// Resource not found.
    #[error("Not found: {0}")]
    NotFound(String),

    /// User already exists (registration).
    #[error("Username or email already taken")]
    UserAlreadyExists,

    /// Email already taken by another user.
    #[error("Email already in use by another account")]
    EmailTaken,

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
    /// Machine-readable error code.
    pub error: String,
    /// Human-readable error message.
    pub message: String,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            Self::InvalidCredentials => (StatusCode::UNAUTHORIZED, "INVALID_CREDENTIALS"),
            Self::UserNotFound => (StatusCode::NOT_FOUND, "USER_NOT_FOUND"),
            Self::NotFound(_) => (StatusCode::NOT_FOUND, "NOT_FOUND"),
            Self::UserAlreadyExists => (StatusCode::CONFLICT, "USER_EXISTS"),
            Self::EmailTaken => (StatusCode::CONFLICT, "EMAIL_TAKEN"),
            Self::InvalidToken => (StatusCode::UNAUTHORIZED, "INVALID_TOKEN"),
            Self::TokenExpired => (StatusCode::UNAUTHORIZED, "TOKEN_EXPIRED"),
            Self::MissingAuthHeader => (StatusCode::UNAUTHORIZED, "MISSING_AUTH"),
            Self::InvalidAuthHeader => (StatusCode::UNAUTHORIZED, "INVALID_AUTH_HEADER"),
            Self::MfaRequired => (StatusCode::FORBIDDEN, "MFA_REQUIRED"),
            Self::InvalidMfaCode => (StatusCode::UNAUTHORIZED, "INVALID_MFA"),
            Self::Validation(_) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR"),
            Self::PasswordHash => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
            Self::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
            Self::Jwt(_) => (StatusCode::UNAUTHORIZED, "TOKEN_ERROR"),
            Self::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
        };

        let body = Json(ErrorResponse {
            error: code.to_string(),
            message: self.to_string(),
        });

        (status, body).into_response()
    }
}

/// Result type for auth operations.
pub type AuthResult<T> = Result<T, AuthError>;
