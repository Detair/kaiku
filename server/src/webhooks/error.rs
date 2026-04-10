//! Webhooks module error types.

use axum::http::StatusCode;
use thiserror::Error;

/// Webhook errors.
#[derive(Error, Debug)]
pub enum WebhookError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Application not found")]
    ApplicationNotFound,
    #[error("Webhook not found")]
    NotFound,
    #[error("Forbidden: you don't own this application")]
    Forbidden,
    #[error("Validation: {0}")]
    Validation(String),
    #[error("Maximum webhooks reached (5 per application)")]
    MaxWebhooksReached,
}

impl From<WebhookError> for (StatusCode, String) {
    fn from(err: WebhookError) -> Self {
        match err {
            WebhookError::Database(e) => {
                tracing::error!("Database error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
            WebhookError::ApplicationNotFound => (StatusCode::NOT_FOUND, err.to_string()),
            WebhookError::NotFound => (StatusCode::NOT_FOUND, err.to_string()),
            WebhookError::Forbidden => (StatusCode::FORBIDDEN, err.to_string()),
            WebhookError::Validation(msg) => (StatusCode::BAD_REQUEST, msg),
            WebhookError::MaxWebhooksReached => (StatusCode::CONFLICT, err.to_string()),
        }
    }
}
