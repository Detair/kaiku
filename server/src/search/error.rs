//! Error types for global search operations.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

/// Errors that can occur during global message search.
#[derive(Debug)]
pub enum GlobalSearchError {
    InvalidQuery(String),
    Forbidden,
    Database(sqlx::Error),
}

impl IntoResponse for GlobalSearchError {
    fn into_response(self) -> Response {
        let (status, body) = match &self {
            Self::InvalidQuery(msg) => (
                StatusCode::BAD_REQUEST,
                serde_json::json!({"error": "INVALID_QUERY", "message": msg}),
            ),
            Self::Forbidden => (
                StatusCode::FORBIDDEN,
                serde_json::json!({"error": "FORBIDDEN", "message": "You do not have access to this channel"}),
            ),
            Self::Database(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"error": "INTERNAL_ERROR", "message": "Database error"}),
            ),
        };
        (status, Json(body)).into_response()
    }
}

impl From<sqlx::Error> for GlobalSearchError {
    fn from(err: sqlx::Error) -> Self {
        tracing::error!(error = %err, "Global search database error");
        Self::Database(err)
    }
}

impl From<crate::chat::ChatError> for GlobalSearchError {
    fn from(err: crate::chat::ChatError) -> Self {
        match err {
            crate::chat::ChatError::Database(e) => Self::from(e),
            other => {
                tracing::error!(error = %other, "Global search chat error");
                Self::Database(sqlx::Error::Protocol(other.to_string()))
            }
        }
    }
}
