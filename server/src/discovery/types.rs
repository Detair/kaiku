//! Guild Discovery Type Definitions

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Shared tag validation regex: letters, numbers, and hyphens only.
/// Used by both guild settings (tag creation) and discovery (tag filtering).
pub static TAG_REGEX: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"^[a-zA-Z0-9-]+$").expect("valid tag regex"));

/// Sort order for discovery browsing.
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum DiscoverSort {
    /// Sort by member count (most popular first).
    Members,
    /// Sort by creation date (newest first).
    #[default]
    Newest,
}

/// Query parameters for browsing discoverable guilds.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct DiscoverQuery {
    /// Full-text search query (searches guild name and description).
    pub q: Option<String>,
    /// Filter by tags (comma-separated, array overlap).
    #[serde(default, deserialize_with = "deserialize_tags")]
    pub tags: Option<Vec<String>>,
    /// Sort order: "members" (popular) or "newest" (default).
    #[serde(default)]
    pub sort: DiscoverSort,
    /// Number of results per page (1-50, default 20).
    pub limit: Option<i64>,
    /// Offset for pagination (default 0).
    pub offset: Option<i64>,
}

fn deserialize_tags<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(deserializer)?;
    Ok(opt.map(|s| {
        s.split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect()
    }))
}

/// A guild visible in the discovery listing.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DiscoverableGuild {
    pub id: Uuid,
    pub name: String,
    pub icon_url: Option<String>,
    pub banner_url: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub member_count: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Paginated response for guild discovery.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DiscoverResponse {
    pub guilds: Vec<DiscoverableGuild>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

/// Response after joining a discoverable guild.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct JoinDiscoverableResponse {
    pub guild_id: Uuid,
    pub guild_name: String,
    pub already_member: bool,
}
