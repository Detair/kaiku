//! Shared DTOs and helpers used across admin handler sub-modules.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// Pagination query parameters with optional search.
#[derive(Debug, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
pub struct PaginationParams {
    /// Maximum number of items to return.
    #[serde(default = "default_limit")]
    pub limit: i64,
    /// Number of items to skip.
    #[serde(default)]
    pub offset: i64,
    /// Search query (searches username, `display_name`, email for users; name for guilds).
    pub search: Option<String>,
}

#[allow(clippy::missing_const_for_fn)]
pub(super) fn default_limit() -> i64 {
    50
}

/// Generic paginated response wrapper.
#[derive(Debug, Serialize, ToSchema)]
pub struct PaginatedResponse<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

/// Delete response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DeleteResponse {
    pub deleted: bool,
    pub id: Uuid,
}

/// Escape a string for CSV (handles commas and quotes).
pub(super) fn escape_csv(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
