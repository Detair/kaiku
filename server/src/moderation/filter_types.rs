//! Content Filter Types
//!
//! Database models, request/response types, and error types
//! for the guild content filtering system.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// Database Enums
// ============================================================================

/// Built-in filter categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "filter_category", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum FilterCategory {
    Slurs,
    HateSpeech,
    Spam,
    AbusiveLanguage,
    Custom,
}

impl std::fmt::Display for FilterCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Slurs => write!(f, "slurs"),
            Self::HateSpeech => write!(f, "hate_speech"),
            Self::Spam => write!(f, "spam"),
            Self::AbusiveLanguage => write!(f, "abusive_language"),
            Self::Custom => write!(f, "custom"),
        }
    }
}

/// Action to take when a filter matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "filter_action", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum FilterAction {
    Block,
    Log,
    Warn,
}

impl std::fmt::Display for FilterAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Block => write!(f, "block"),
            Self::Log => write!(f, "log"),
            Self::Warn => write!(f, "warn"),
        }
    }
}

// ============================================================================
// Database Models
// ============================================================================

/// Guild filter configuration row.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct GuildFilterConfig {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub category: FilterCategory,
    pub enabled: bool,
    pub action: FilterAction,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Custom guild filter pattern row.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct GuildFilterPattern {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub pattern: String,
    pub is_regex: bool,
    pub description: Option<String>,
    pub enabled: bool,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Moderation action log entry.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct ModerationAction {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub user_id: Uuid,
    pub channel_id: Uuid,
    pub action: FilterAction,
    pub category: Option<FilterCategory>,
    pub matched_pattern: String,
    pub original_content: String,
    pub custom_pattern_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

// ============================================================================
// Request Types
// ============================================================================

/// Single category config in bulk update request.
#[derive(Debug, Deserialize)]
pub struct FilterConfigEntry {
    pub category: FilterCategory,
    pub enabled: bool,
    pub action: FilterAction,
}

/// Request to update guild filter configs (bulk upsert).
#[derive(Debug, Deserialize)]
pub struct UpdateFilterConfigsRequest {
    pub configs: Vec<FilterConfigEntry>,
}

/// Request to create a custom filter pattern.
#[derive(Debug, Deserialize)]
pub struct CreatePatternRequest {
    pub pattern: String,
    #[serde(default)]
    pub is_regex: bool,
    pub description: Option<String>,
}

/// Request to update a custom filter pattern.
#[derive(Debug, Deserialize)]
pub struct UpdatePatternRequest {
    pub pattern: Option<String>,
    pub is_regex: Option<bool>,
    pub description: Option<String>,
    pub enabled: Option<bool>,
}

/// Request to test content against active filters.
#[derive(Debug, Deserialize)]
pub struct TestFilterRequest {
    pub content: String,
}

/// Pagination query parameters.
#[derive(Debug, Deserialize)]
pub struct PaginationQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    50
}

// ============================================================================
// Response Types
// ============================================================================

/// Response for moderation log listing.
#[derive(Debug, Serialize)]
pub struct PaginatedModerationLog {
    pub items: Vec<ModerationAction>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

/// Result of testing content against filters.
#[derive(Debug, Serialize)]
pub struct TestFilterResponse {
    pub blocked: bool,
    pub matches: Vec<FilterMatchResponse>,
}

/// A single filter match in test results.
#[derive(Debug, Serialize)]
pub struct FilterMatchResponse {
    pub category: FilterCategory,
    pub action: FilterAction,
    pub matched_pattern: String,
}

// ============================================================================
// Internal Types
// ============================================================================

/// Internal result from the filter engine.
#[derive(Debug)]
pub struct FilterResult {
    pub blocked: bool,
    pub matches: Vec<FilterMatch>,
}

/// A single filter match (internal).
#[derive(Debug)]
pub struct FilterMatch {
    pub category: FilterCategory,
    pub action: FilterAction,
    pub matched_pattern: String,
    pub custom_pattern_id: Option<Uuid>,
}

// ============================================================================
// Error Type
// ============================================================================

/// Errors from filter operations.
#[derive(Debug, thiserror::Error)]
pub enum FilterError {
    #[error("Filter configuration not found")]
    NotFound,

    #[error("Forbidden")]
    Forbidden,

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Database error")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for FilterError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            Self::NotFound => (StatusCode::NOT_FOUND, "NOT_FOUND", self.to_string()),
            Self::Forbidden => (
                StatusCode::FORBIDDEN,
                "FORBIDDEN",
                "Access denied".to_string(),
            ),
            Self::Validation(_) => (
                StatusCode::BAD_REQUEST,
                "VALIDATION_ERROR",
                self.to_string(),
            ),
            Self::Database(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "Database error".to_string(),
            ),
        };

        (
            status,
            Json(serde_json::json!({ "error": code, "message": message })),
        )
            .into_response()
    }
}
