//! Guild error types.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::permissions::PermissionError;

#[derive(Debug, thiserror::Error)]
pub enum GuildError {
    #[error("Guild not found")]
    NotFound,

    #[error("Access denied")]
    Forbidden,

    #[error("{0}")]
    ForbiddenMsg(String),

    #[error("{0}")]
    Permission(PermissionError),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Limit exceeded: {0}")]
    LimitExceeded(String),

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl IntoResponse for GuildError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "GUILD_NOT_FOUND",
                "Guild not found".to_string(),
            ),
            Self::Forbidden => (
                StatusCode::FORBIDDEN,
                "FORBIDDEN",
                "Access denied".to_string(),
            ),
            Self::ForbiddenMsg(msg) => (StatusCode::FORBIDDEN, "FORBIDDEN", msg.clone()),
            Self::Permission(e) => (StatusCode::FORBIDDEN, "PERMISSION_DENIED", e.to_string()),
            Self::Validation(msg) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR", msg.clone()),
            Self::LimitExceeded(msg) => (StatusCode::FORBIDDEN, "LIMIT_EXCEEDED", msg.clone()),
            Self::Database(err) => {
                tracing::error!(%err, "Guild endpoint database error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    "Database error".to_string(),
                )
            }
            Self::Internal(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                msg.clone(),
            ),
        };
        (
            status,
            Json(serde_json::json!({ "error": code, "message": message })),
        )
            .into_response()
    }
}
