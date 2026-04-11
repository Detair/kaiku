//! Discovery error types.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error("Guild discovery is not enabled on this server")]
    Disabled,
    #[error("Guild not found or not discoverable")]
    NotFound,
    #[error("Forbidden: {0}")]
    Forbidden(String),
    #[error("Validation error: {0}")]
    Validation(String),
    #[error("Limit exceeded: {0}")]
    LimitExceeded(String),
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for DiscoveryError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            Self::Disabled => (
                StatusCode::NOT_FOUND,
                "DISCOVERY_DISABLED",
                "Guild discovery is not enabled on this server".to_string(),
            ),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "GUILD_NOT_FOUND",
                "Guild not found or not discoverable".to_string(),
            ),
            Self::Forbidden(msg) => (StatusCode::FORBIDDEN, "FORBIDDEN", msg.clone()),
            Self::Validation(msg) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR", msg.clone()),
            Self::LimitExceeded(msg) => (StatusCode::FORBIDDEN, "LIMIT_EXCEEDED", msg.clone()),
            Self::Database(err) => {
                tracing::error!(%err, "Discovery endpoint database error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    "Database error".to_string(),
                )
            }
        };
        (
            status,
            Json(serde_json::json!({ "error": code, "message": message })),
        )
            .into_response()
    }
}
