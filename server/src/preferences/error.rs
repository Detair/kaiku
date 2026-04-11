//! Error types for preferences operations.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

/// Error types for preferences operations.
#[derive(Debug, thiserror::Error)]
pub enum PreferencesError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Validation error: {0}")]
    Validation(String),
}

impl IntoResponse for PreferencesError {
    fn into_response(self) -> Response {
        use serde_json::json;

        let (status, code, message) = match &self {
            Self::Database(err) => {
                tracing::error!("Database error: {}", err);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    "Database error".to_string(),
                )
            }
            Self::Validation(msg) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR", msg.clone()),
        };

        (status, Json(json!({ "error": code, "message": message }))).into_response()
    }
}
