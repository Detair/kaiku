//! Data Governance Error Types

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum GovernanceError {
    #[error("Export job not found")]
    ExportNotFound,

    #[error("An export is already in progress")]
    ExportAlreadyPending,

    #[error("Export archive has expired")]
    ExportExpired,

    #[error("Account deletion already scheduled")]
    DeletionAlreadyScheduled,

    #[error("No pending deletion to cancel")]
    NoDeletionPending,

    #[error("Password verification failed")]
    PasswordInvalid,

    #[error("Cannot delete account while owning guilds: {0}")]
    OwnsGuilds(String),

    #[error("OIDC users must provide account confirmation")]
    OidcPasswordNotSupported,

    #[error("File storage not configured")]
    StorageNotConfigured,

    #[error("Validation error: {0}")]
    Validation(String),

    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for GovernanceError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            Self::ExportNotFound => (StatusCode::NOT_FOUND, "NOT_FOUND", self.to_string()),
            Self::ExportAlreadyPending => (StatusCode::CONFLICT, "CONFLICT", self.to_string()),
            Self::ExportExpired => (StatusCode::GONE, "EXPORT_EXPIRED", self.to_string()),
            Self::DeletionAlreadyScheduled => (StatusCode::CONFLICT, "CONFLICT", self.to_string()),
            Self::NoDeletionPending => (StatusCode::NOT_FOUND, "NOT_FOUND", self.to_string()),
            Self::PasswordInvalid => (StatusCode::UNAUTHORIZED, "UNAUTHORIZED", self.to_string()),
            Self::OwnsGuilds(_) => (StatusCode::CONFLICT, "OWNS_GUILDS", self.to_string()),
            Self::OidcPasswordNotSupported => (
                StatusCode::BAD_REQUEST,
                "VALIDATION_ERROR",
                self.to_string(),
            ),
            Self::StorageNotConfigured => (
                StatusCode::SERVICE_UNAVAILABLE,
                "STORAGE_NOT_CONFIGURED",
                self.to_string(),
            ),
            Self::Validation(_) => (
                StatusCode::BAD_REQUEST,
                "VALIDATION_ERROR",
                self.to_string(),
            ),
            Self::Database(e) => {
                tracing::error!(error = %e, "Governance database error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    "Internal server error".to_string(),
                )
            }
        };

        (status, Json(json!({ "error": code, "message": message }))).into_response()
    }
}
