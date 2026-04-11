//! Pages error types.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::permissions::PermissionError;

#[derive(Debug, thiserror::Error)]
pub enum PagesError {
    #[error("Page not found")]
    NotFound,

    #[error("Revision not found")]
    RevisionNotFound,

    #[error("Category not found")]
    CategoryNotFound,

    #[error("Access denied")]
    Forbidden,

    #[error("System admin required")]
    AdminRequired,

    #[error("{0}")]
    Permission(PermissionError),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Slug conflict: {0}")]
    SlugConflict(String),

    #[error("Limit exceeded: {0}")]
    LimitExceeded(String),

    #[error("Category conflict: {0}")]
    CategoryConflict(String),

    #[error("Invalid reorder request: {0}")]
    InvalidReorder(String),

    #[error("Not a platform page")]
    NotPlatformPage,

    #[error("Not a guild page")]
    NotGuildPage,

    #[error("Revision data is corrupted: {0}")]
    CorruptRevision(String),

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl IntoResponse for PagesError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            Self::NotFound => (StatusCode::NOT_FOUND, "PAGE_NOT_FOUND", self.to_string()),
            Self::RevisionNotFound => (
                StatusCode::NOT_FOUND,
                "REVISION_NOT_FOUND",
                self.to_string(),
            ),
            Self::CategoryNotFound => (
                StatusCode::NOT_FOUND,
                "CATEGORY_NOT_FOUND",
                self.to_string(),
            ),
            Self::Forbidden => (StatusCode::FORBIDDEN, "FORBIDDEN", self.to_string()),
            Self::AdminRequired => (StatusCode::FORBIDDEN, "FORBIDDEN", self.to_string()),
            Self::Permission(e) => (StatusCode::FORBIDDEN, "PERMISSION_DENIED", e.to_string()),
            Self::Validation(msg) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR", msg.clone()),
            Self::SlugConflict(msg) => (StatusCode::CONFLICT, "SLUG_CONFLICT", msg.clone()),
            Self::LimitExceeded(msg) => (StatusCode::BAD_REQUEST, "LIMIT_EXCEEDED", msg.clone()),
            Self::CategoryConflict(msg) => (StatusCode::CONFLICT, "CATEGORY_CONFLICT", msg.clone()),
            Self::InvalidReorder(msg) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR", msg.clone()),
            Self::NotPlatformPage => (
                StatusCode::BAD_REQUEST,
                "NOT_PLATFORM_PAGE",
                self.to_string(),
            ),
            Self::NotGuildPage => (StatusCode::BAD_REQUEST, "NOT_GUILD_PAGE", self.to_string()),
            Self::CorruptRevision(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                msg.clone(),
            ),
            Self::Database(err) => {
                tracing::error!(%err, "Pages endpoint database error");
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
