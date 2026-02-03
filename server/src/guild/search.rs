//! Guild Message Search Handler
//!
//! Full-text search for messages within a guild using `PostgreSQL`.tsvector.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{api::AppState, auth::AuthUser, db};

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

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    /// Search query string (supports websearch syntax: AND, OR, quotes)
    pub q: String,
    /// Maximum results to return (default 25, max 100)
    #[serde(default = "default_limit")]
    pub limit: i64,
    /// Offset for pagination (default 0)
    #[serde(default)]
    pub offset: i64,
}

const fn default_limit() -> i64 {
    25
}

/// Author info for search results
#[derive(Debug, Serialize)]
pub struct SearchAuthor {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
}

/// Search result item
#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub channel_name: String,
    pub author: SearchAuthor,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

/// Search response with results and pagination
#[derive(Debug, Serialize)]
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

    // Check guild exists
    let guild_exists: (bool,) = sqlx::query_as("SELECT EXISTS(SELECT 1 FROM guilds WHERE id = $1)")
        .bind(guild_id)
        .fetch_one(&state.db)
        .await?;
    if !guild_exists.0 {
        return Err(SearchError::GuildNotFound);
    }

    // Check user is a member of the guild
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if !is_member {
        return Err(SearchError::NotMember);
    }

    // Get all channel IDs in this guild and filter by VIEW_CHANNEL permission
    let guild_channels = db::get_guild_channels(&state.db, guild_id).await?;

    // Filter channels by VIEW_CHANNEL permission
    let mut accessible_channel_ids: Vec<Uuid> = Vec::new();
    for channel in guild_channels {
        // Check if user has VIEW_CHANNEL permission for this channel
        if crate::permissions::require_channel_access(&state.db, auth.id, channel.id)
            .await
            .is_ok()
        {
            accessible_channel_ids.push(channel.id);
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

    // Clamp limit
    let limit = query.limit.clamp(1, 100);
    let offset = query.offset.max(0);

    // Get total count (filtered by accessible channels)
    let total =
        db::count_search_messages_in_channels(&state.db, &accessible_channel_ids, search_term)
            .await?;

    // Search messages (filtered by accessible channels)
    let messages = db::search_messages_in_channels(
        &state.db,
        &accessible_channel_ids,
        search_term,
        limit,
        offset,
    )
    .await?;

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
