//! Request and response types for global search.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Query parameters for the global search endpoint.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct GlobalSearchQuery {
    pub q: String,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
    pub date_from: Option<DateTime<Utc>>,
    pub date_to: Option<DateTime<Utc>>,
    pub author_id: Option<Uuid>,
    /// Filter results to a specific channel.
    pub channel_id: Option<Uuid>,
    pub has: Option<String>,
    pub sort: Option<String>,
}

const fn default_limit() -> i64 {
    25
}

/// Author information in a search result.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GlobalSearchAuthor {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
}

/// Source context (guild or DM) for a search result.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GlobalSearchSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub guild_id: Option<Uuid>,
    pub guild_name: Option<String>,
}

/// A single search result message.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GlobalSearchResult {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub channel_name: String,
    pub author: GlobalSearchAuthor,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub headline: String,
    pub rank: f32,
    pub source: GlobalSearchSource,
}

/// Paginated search response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GlobalSearchResponse {
    pub results: Vec<GlobalSearchResult>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}
