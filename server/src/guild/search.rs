//! Guild Message Search Handler
//!
//! Full-text search for messages within a guild using `PostgreSQL`.tsvector.

use std::time::Instant;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::AuthUser;
use crate::db;

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug)]
pub enum SearchError {
    GuildNotFound,
    NotMember,
    InvalidQuery(String),
    Database(sqlx::Error),
}

impl IntoResponse for SearchError {
    fn into_response(self) -> Response {
        let (status, body) = match &self {
            Self::GuildNotFound => (
                StatusCode::NOT_FOUND,
                serde_json::json!({"error": "NOT_FOUND", "message": "Guild not found"}),
            ),
            Self::NotMember => (
                StatusCode::FORBIDDEN,
                serde_json::json!({"error": "FORBIDDEN", "message": "Not a member of this guild"}),
            ),
            Self::InvalidQuery(msg) => (
                StatusCode::BAD_REQUEST,
                serde_json::json!({"error": "INVALID_QUERY", "message": msg}),
            ),
            Self::Database(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"error": "INTERNAL_ERROR", "message": "Database error"}),
            ),
        };
        (status, Json(body)).into_response()
    }
}

impl From<sqlx::Error> for SearchError {
    fn from(err: sqlx::Error) -> Self {
        tracing::error!(error = %err, "Search database error");
        Self::Database(err)
    }
}

// ============================================================================
// Request/Response Types
// ============================================================================

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SearchQuery {
    /// Search query string (supports websearch syntax: AND, OR, quotes)
    pub q: String,
    /// Maximum results to return (default 25, max 100)
    #[serde(default = "default_limit")]
    pub limit: i64,
    /// Offset for pagination (default 0)
    #[serde(default)]
    pub offset: i64,
    /// Filter: only messages after this date (ISO 8601)
    pub date_from: Option<DateTime<Utc>>,
    /// Filter: only messages before this date (ISO 8601)
    pub date_to: Option<DateTime<Utc>>,
    /// Filter: only messages in this channel
    pub channel_id: Option<Uuid>,
    /// Filter: only messages by this author
    pub author_id: Option<Uuid>,
    /// Filter: "link" or "file"
    pub has: Option<String>,
    /// Sort order: "relevance" (default) or "date"
    pub sort: Option<String>,
}

const fn default_limit() -> i64 {
    25
}

/// Author info for search results
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SearchAuthor {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
}

/// Search result item
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SearchResult {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub channel_name: String,
    pub author: SearchAuthor,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub headline: String,
    pub rank: f32,
}

/// Search response with results and pagination
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

// ============================================================================
// Handler
// ============================================================================

/// Search messages within a guild.
/// GET `/api/guilds/:guild_id/search?q=...`
#[utoipa::path(
    get,
    path = "/api/guilds/{id}/search",
    tag = "search",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 200, description = "Search results")),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn search_messages(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, SearchError> {
    // Validate query
    let search_term = query.q.trim();
    if search_term.is_empty() {
        return Err(SearchError::InvalidQuery(
            "Search query cannot be empty".to_string(),
        ));
    }
    if search_term.len() < 2 {
        return Err(SearchError::InvalidQuery(
            "Search query must be at least 2 characters".to_string(),
        ));
    }
    if search_term.len() > 1000 {
        return Err(SearchError::InvalidQuery(
            "Search query must not exceed 1000 characters".to_string(),
        ));
    }

    // Validate date range
    if let (Some(from), Some(to)) = (query.date_from, query.date_to) {
        if from > to {
            return Err(SearchError::InvalidQuery(
                "date_from must be before date_to".to_string(),
            ));
        }
    }

    // Validate has filter
    if let Some(ref has) = query.has {
        if has != "link" && has != "file" {
            return Err(SearchError::InvalidQuery(
                "has must be \"link\" or \"file\"".to_string(),
            ));
        }
    }

    // Validate sort param
    let sort = match query.sort.as_deref() {
        None | Some("relevance") => db::SearchSort::Relevance,
        Some("date") => db::SearchSort::Date,
        Some(_) => {
            return Err(SearchError::InvalidQuery(
                "sort must be \"relevance\" or \"date\"".to_string(),
            ));
        }
    };

    // Check guild exists
    let guild_exists: (bool,) = sqlx::query_as("SELECT EXISTS(SELECT 1 FROM guilds WHERE id = $1)")
        .bind(guild_id)
        .fetch_one(&state.db)
        .await?;
    if !guild_exists.0 {
        return Err(SearchError::GuildNotFound);
    }

    // Get all channel IDs in this guild and filter by VIEW_CHANNEL permission
    let guild_channels = db::get_guild_channels(&state.db, guild_id).await?;
    let all_channel_ids: Vec<Uuid> = guild_channels.iter().map(|c| c.id).collect();
    let mut accessible_channel_ids = crate::permissions::filter_accessible_channels(
        &state.db,
        guild_id,
        auth.id,
        &all_channel_ids,
    )
    .await
    .map_err(|e| match e {
        crate::permissions::PermissionError::NotGuildMember => SearchError::NotMember,
        _ => SearchError::NotMember,
    })?;

    // If channel_id filter is provided, restrict to that channel (or empty if not accessible)
    if let Some(filter_channel_id) = query.channel_id {
        if accessible_channel_ids.contains(&filter_channel_id) {
            accessible_channel_ids = vec![filter_channel_id];
        } else {
            accessible_channel_ids.clear();
        }
    }

    // If no channels, return empty results
    if accessible_channel_ids.is_empty() {
        return Ok(Json(SearchResponse {
            results: vec![],
            total: 0,
            limit: query.limit,
            offset: query.offset,
        }));
    }

    // Build search filters
    let filters = db::SearchFilters {
        date_from: query.date_from,
        date_to: query.date_to,
        author_id: query.author_id,
        has_link: query.has.as_deref() == Some("link"),
        has_file: query.has.as_deref() == Some("file"),
        sort,
    };

    // Clamp limit
    let limit = query.limit.clamp(1, 100);
    let offset = query.offset.max(0);

    // Get total count (filtered by accessible channels)
    let total = db::count_search_messages_filtered(
        &state.db,
        &accessible_channel_ids,
        search_term,
        &filters,
    )
    .await?;

    // Search messages (filtered by accessible channels)
    let start = Instant::now();
    let messages = db::search_messages_filtered(
        &state.db,
        &accessible_channel_ids,
        search_term,
        &filters,
        limit,
        offset,
    )
    .await?;
    let elapsed = start.elapsed();
    tracing::info!(
        user_id = %auth.id,
        query_length = search_term.len(),
        result_count = messages.len(),
        duration_ms = elapsed.as_millis(),
        "search_query"
    );

    // Get user IDs and channel IDs for bulk lookup
    let user_ids: Vec<Uuid> = messages.iter().map(|m| m.user_id).collect();
    let channel_ids: Vec<Uuid> = messages.iter().map(|m| m.channel_id).collect();

    // Bulk fetch users
    let users = db::find_users_by_ids(&state.db, &user_ids).await?;
    let user_map: std::collections::HashMap<Uuid, db::User> =
        users.into_iter().map(|u| (u.id, u)).collect();

    // Bulk fetch channel names
    let channels: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id, name FROM channels WHERE id = ANY($1)")
            .bind(&channel_ids)
            .fetch_all(&state.db)
            .await?;
    let channel_map: std::collections::HashMap<Uuid, String> = channels.into_iter().collect();

    // Build results
    let results: Vec<SearchResult> = messages
        .into_iter()
        .map(|msg| {
            let author = user_map
                .get(&msg.user_id)
                .map(|u| SearchAuthor {
                    id: u.id,
                    username: u.username.clone(),
                    display_name: u.display_name.clone(),
                    avatar_url: u.avatar_url.clone(),
                })
                .unwrap_or_else(|| SearchAuthor {
                    id: msg.user_id,
                    username: "deleted".to_string(),
                    display_name: "Deleted User".to_string(),
                    avatar_url: None,
                });

            let channel_name = channel_map
                .get(&msg.channel_id)
                .cloned()
                .unwrap_or_else(|| "unknown".to_string());

            SearchResult {
                id: msg.id,
                channel_id: msg.channel_id,
                channel_name,
                author,
                content: msg.content,
                created_at: msg.created_at,
                headline: msg.headline,
                rank: msg.rank,
            }
        })
        .collect();

    Ok(Json(SearchResponse {
        results,
        total,
        limit,
        offset,
    }))
}
