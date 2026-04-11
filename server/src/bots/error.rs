//! Error types for bot operations.

use axum::http::StatusCode;
use thiserror::Error;

/// Errors that can occur during bot operations.
#[derive(Error, Debug)]
pub enum BotError {
    /// Database error.
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    /// Application not found.
    #[error("Application not found")]
    NotFound,
    /// User does not own this application.
    #[error("Forbidden: you don't own this application")]
    Forbidden,
    /// Bot user already created for this application.
    #[error("Bot user already exists for this application")]
    BotAlreadyCreated,
    /// Invalid application name.
    #[error("Application name must be between 2 and 100 characters")]
    InvalidName,
}

impl From<BotError> for (StatusCode, String) {
    fn from(err: BotError) -> Self {
        match err {
            BotError::Database(e) => {
                tracing::error!("Database error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
            BotError::NotFound => (StatusCode::NOT_FOUND, err.to_string()),
            BotError::Forbidden => (StatusCode::FORBIDDEN, err.to_string()),
            BotError::BotAlreadyCreated => (StatusCode::CONFLICT, err.to_string()),
            BotError::InvalidName => (StatusCode::BAD_REQUEST, err.to_string()),
        }
    }
}
