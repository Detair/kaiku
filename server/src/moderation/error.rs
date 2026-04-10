//! Moderation module error types.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ModerationError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Report not found")]
    NotFound,

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Rate limited: too many reports")]
    RateLimited,

    #[error("Duplicate report: you already have an active report for this target")]
    Duplicate,
}

impl IntoResponse for ModerationError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            Self::Database(err) => {
                tracing::error!("Database error: {}", err);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    "Database error".to_string(),
                )
            }
            Self::NotFound => (StatusCode::NOT_FOUND, "REPORT_NOT_FOUND", self.to_string()),
            Self::Validation(msg) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR", msg.clone()),
            Self::RateLimited => (
                StatusCode::TOO_MANY_REQUESTS,
                "RATE_LIMITED",
                self.to_string(),
            ),
            Self::Duplicate => (StatusCode::CONFLICT, "DUPLICATE_REPORT", self.to_string()),
        };

        (
            status,
            Json(serde_json::json!({ "error": code, "message": message })),
        )
            .into_response()
    }
}
