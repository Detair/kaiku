//! Workspace Error Types

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("Workspace not found")]
    NotFound,

    #[error("Workspace entry not found")]
    EntryNotFound,

    #[error("Maximum workspaces limit reached")]
    WorkspaceLimitExceeded,

    #[error("Maximum entries per workspace limit reached")]
    EntryLimitExceeded,

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Channel not found or no access")]
    ChannelNotFound,

    #[error("Channel already in workspace")]
    DuplicateEntry,

    #[error("Invalid entry IDs in reorder request")]
    InvalidEntries,

    #[error("Invalid workspace IDs in reorder request")]
    InvalidWorkspaces,

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for WorkspaceError {
    fn into_response(self) -> axum::response::Response {
        let (status, code, message) = match &self {
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "WORKSPACE_NOT_FOUND",
                "Workspace not found".to_string(),
            ),
            Self::EntryNotFound => (
                StatusCode::NOT_FOUND,
                "ENTRY_NOT_FOUND",
                "Workspace entry not found".to_string(),
            ),
            Self::WorkspaceLimitExceeded => (
                StatusCode::FORBIDDEN,
                "LIMIT_EXCEEDED",
                "Maximum workspaces limit reached".to_string(),
            ),
            Self::EntryLimitExceeded => (
                StatusCode::FORBIDDEN,
                "LIMIT_EXCEEDED",
                "Maximum entries per workspace limit reached".to_string(),
            ),
            Self::Validation(msg) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR", msg.clone()),
            Self::ChannelNotFound => (
                StatusCode::NOT_FOUND,
                "CHANNEL_NOT_FOUND",
                "Channel not found or you don't have access".to_string(),
            ),
            Self::DuplicateEntry => (
                StatusCode::CONFLICT,
                "DUPLICATE_ENTRY",
                "Channel already in this workspace".to_string(),
            ),
            Self::InvalidEntries => (
                StatusCode::BAD_REQUEST,
                "VALIDATION_ERROR",
                "Reorder contains invalid entry IDs".to_string(),
            ),
            Self::InvalidWorkspaces => (
                StatusCode::BAD_REQUEST,
                "VALIDATION_ERROR",
                "Reorder contains invalid workspace IDs".to_string(),
            ),
            Self::Database(err) => {
                tracing::error!(%err, "Workspaces endpoint database error");
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
