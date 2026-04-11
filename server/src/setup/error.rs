//! Error types for setup operations.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use thiserror::Error;

/// Errors that can occur during server setup.
#[derive(Debug, Error)]
pub enum SetupError {
    #[error("Server setup has already been completed")]
    SetupAlreadyComplete,

    #[error("Only system administrators can complete setup")]
    Unauthorized,

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Database error")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for SetupError {
    fn into_response(self) -> Response {
        // Log database errors before converting to response
        if let Self::Database(ref err) = self {
            tracing::error!(
                error = %err,
                error_debug = ?err,
                "Setup endpoint returned database error"
            );
        }

        let (status, code) = match &self {
            Self::SetupAlreadyComplete => (StatusCode::FORBIDDEN, "SETUP_ALREADY_COMPLETE"),
            Self::Unauthorized => (StatusCode::FORBIDDEN, "FORBIDDEN"),
            Self::Validation(_) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR"),
            Self::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
        };

        let message = self.to_string();

        (
            status,
            Json(serde_json::json!({ "error": code, "message": message })),
        )
            .into_response()
    }
}
