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

    #[error("Workspace name is required")]
    NameRequired,

    #[error("Workspace name exceeds maximum length")]
    NameTooLong,

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
                "workspace_not_found",
                "Workspace not found",
            ),
            Self::EntryNotFound => (
                StatusCode::NOT_FOUND,
                "entry_not_found",
                "Workspace entry not found",
            ),
            Self::WorkspaceLimitExceeded => (
                StatusCode::BAD_REQUEST,
                "workspace_limit_exceeded",
                "Maximum workspaces limit reached",
            ),
            Self::EntryLimitExceeded => (
                StatusCode::BAD_REQUEST,
                "entry_limit_exceeded",
                "Maximum entries per workspace limit reached",
            ),
            Self::NameRequired => (
                StatusCode::BAD_REQUEST,
                "name_required",
                "Workspace name is required",
            ),
            Self::NameTooLong => (
                StatusCode::BAD_REQUEST,
                "name_too_long",
                "Workspace name exceeds maximum length (100 characters)",
            ),
            Self::ChannelNotFound => (
                StatusCode::NOT_FOUND,
                "channel_not_found",
                "Channel not found or you don't have access",
            ),
            Self::DuplicateEntry => (
                StatusCode::CONFLICT,
                "duplicate_entry",
                "Channel already in this workspace",
            ),
            Self::InvalidEntries => (
                StatusCode::BAD_REQUEST,
                "invalid_entries",
                "Reorder contains invalid entry IDs",
            ),
            Self::InvalidWorkspaces => (
                StatusCode::BAD_REQUEST,
                "invalid_workspaces",
                "Reorder contains invalid workspace IDs",
            ),
            Self::Database(err) => {
                tracing::error!("Database error in workspaces: {}", err);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "database_error",
                    "Database error",
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
