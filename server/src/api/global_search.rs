//! Global Message Search Handler
//!
//! Full-text search across all guilds and DMs the authenticated user has access to.

use std::time::Instant;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::AuthUser;
use crate::chat::dm;
use crate::{db, permissions};

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug)]
pub enum GlobalSearchError {
    InvalidQuery(String),
    Database(sqlx::Error),
}

impl IntoResponse for GlobalSearchError {
    fn into_response(self) -> Response {
        let (status, body) = match &self {
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

impl From<sqlx::Error> for GlobalSearchError {
    fn from(err: sqlx::Error) -> Self {
        tracing::error!(error = %err, "Global search database error");
        Self::Database(err)
    }
}

// ============================================================================
// Request/Response Types
// ============================================================================

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
    pub has: Option<String>,
    pub sort: Option<String>,
}

const fn default_limit() -> i64 {
    25
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GlobalSearchAuthor {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GlobalSearchSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub guild_id: Option<Uuid>,
    pub guild_name: Option<String>,
}

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

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GlobalSearchResponse {
    pub results: Vec<GlobalSearchResult>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

// ============================================================================
// Handler
// ============================================================================

/// Search messages across all guilds and DMs.
/// GET `/api/search?q=...`
#[utoipa::path(
    get,
    path = "/api/search",
    tag = "search",
    responses(
        (status = 200, description = "Search results"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state))]
pub async fn search_all(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<GlobalSearchQuery>,
) -> Result<Json<GlobalSearchResponse>, GlobalSearchError> {
    // Validate query
    let search_term = query.q.trim();
    if search_term.is_empty() {
        return Err(GlobalSearchError::InvalidQuery(
            "Search query cannot be empty".to_string(),
        ));
    }
    if search_term.len() < 2 {
        return Err(GlobalSearchError::InvalidQuery(
            "Search query must be at least 2 characters".to_string(),
        ));
    }
    if search_term.len() > 1000 {
        return Err(GlobalSearchError::InvalidQuery(
            "Search query must not exceed 1000 characters".to_string(),
        ));
    }

    // Validate date range
    if let (Some(from), Some(to)) = (query.date_from, query.date_to) {
        if from > to {
            return Err(GlobalSearchError::InvalidQuery(
                "date_from must be before date_to".to_string(),
            ));
        }
    }

    // Validate has filter
    if let Some(ref has) = query.has {
        if has != "link" && has != "file" {
            return Err(GlobalSearchError::InvalidQuery(
                "has must be \"link\" or \"file\"".to_string(),
            ));
        }
    }

    // Validate sort param
    let sort = match query.sort.as_deref() {
        None | Some("relevance") => db::SearchSort::Relevance,
        Some("date") => db::SearchSort::Date,
        Some(_) => {
            return Err(GlobalSearchError::InvalidQuery(
                "sort must be \"relevance\" or \"date\"".to_string(),
            ));
        }
    };

    // 1. Get user's guild IDs
    let guild_ids = db::get_user_guild_ids(&state.db, auth.id).await?;

    // 2. Batch-fetch all channels across all guilds (1 query instead of N)
    let mut all_channel_ids: Vec<Uuid> = Vec::new();
    let mut channel_guild_map: std::collections::HashMap<Uuid, Uuid> =
        std::collections::HashMap::new();

    if !guild_ids.is_empty() {
        let guild_channels: Vec<db::Channel> = sqlx::query_as(
            "SELECT id, name, channel_type, category_id, guild_id, topic, icon_url, \
             user_limit, position, max_screen_shares, created_at, updated_at \
             FROM channels WHERE guild_id = ANY($1) ORDER BY position ASC",
        )
        .bind(&guild_ids)
        .fetch_all(&state.db)
        .await?;

        // 2b. Get permission context once per guild (N queries, not N*M)
        let mut guild_perm_map: std::collections::HashMap<
            Uuid,
            Option<permissions::MemberPermissionContext>,
        > = std::collections::HashMap::new();
        for &guild_id in &guild_ids {
            let ctx = permissions::get_member_permission_context(&state.db, guild_id, auth.id)
                .await
                .ok()
                .flatten();
            guild_perm_map.insert(guild_id, ctx);
        }

        // 2c. Filter channels by VIEW_CHANNEL permission
        for channel in &guild_channels {
            let guild_id = match channel.guild_id {
                Some(gid) => gid,
                None => continue,
            };

            let ctx = match guild_perm_map.get(&guild_id) {
                Some(Some(ctx)) => ctx,
                _ => continue,
            };

            // Guild owners have full access
            if ctx.is_owner {
                all_channel_ids.push(channel.id);
                channel_guild_map.insert(channel.id, guild_id);
                continue;
            }

            // Check VIEW_CHANNEL with channel overrides
            let overrides = db::get_channel_overrides(&state.db, channel.id).await?;
            let perms = permissions::compute_guild_permissions(
                auth.id,
                ctx.guild_owner_id,
                ctx.everyone_permissions,
                &ctx.member_roles,
                Some(&overrides),
            );

            if perms.has(permissions::GuildPermissions::VIEW_CHANNEL) {
                all_channel_ids.push(channel.id);
                channel_guild_map.insert(channel.id, guild_id);
            }
        }
    }

    // 3. Get DM channel IDs
    let dm_channels = dm::list_user_dms(&state.db, auth.id).await?;
    let dm_channel_ids: Vec<Uuid> = dm_channels.iter().map(|c| c.id).collect();
    all_channel_ids.extend(&dm_channel_ids);

    // If no channels at all, return empty
    if all_channel_ids.is_empty() {
        return Ok(Json(GlobalSearchResponse {
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

    let limit = query.limit.clamp(1, 100);
    let offset = query.offset.max(0);

    // 4. Search and count
    let total =
        db::count_search_messages_filtered(&state.db, &all_channel_ids, search_term, &filters)
            .await?;

    let start = Instant::now();
    let messages = db::search_messages_filtered(
        &state.db,
        &all_channel_ids,
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

    // 5. Bulk fetch users
    let user_ids: Vec<Uuid> = messages.iter().filter_map(|m| m.user_id).collect();
    let users = db::find_users_by_ids(&state.db, &user_ids).await?;
    let user_map: std::collections::HashMap<Uuid, db::User> =
        users.into_iter().map(|u| (u.id, u)).collect();

    // 6. Bulk fetch channel names
    let msg_channel_ids: Vec<Uuid> = messages.iter().map(|m| m.channel_id).collect();
    let channels: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id, name FROM channels WHERE id = ANY($1)")
            .bind(&msg_channel_ids)
            .fetch_all(&state.db)
            .await?;
    let channel_name_map: std::collections::HashMap<Uuid, String> = channels.into_iter().collect();

    // 7. Bulk fetch guild names for source enrichment
    let guild_name_map: std::collections::HashMap<Uuid, String> = if guild_ids.is_empty() {
        std::collections::HashMap::new()
    } else {
        let rows: Vec<(Uuid, String)> =
            sqlx::query_as("SELECT id, name FROM guilds WHERE id = ANY($1)")
                .bind(&guild_ids)
                .fetch_all(&state.db)
                .await?;
        rows.into_iter().collect()
    };

    // 8. Build results with source info
    let dm_channel_set: std::collections::HashSet<Uuid> = dm_channel_ids.into_iter().collect();

    let results: Vec<GlobalSearchResult> = messages
        .into_iter()
        .map(|msg| {
            let author = msg
                .user_id
                .and_then(|uid| user_map.get(&uid))
                .map(|u| GlobalSearchAuthor {
                    id: u.id,
                    username: u.username.clone(),
                    display_name: u.display_name.clone(),
                    avatar_url: u.avatar_url.clone(),
                })
                .unwrap_or_else(|| GlobalSearchAuthor {
                    id: msg.user_id.unwrap_or(Uuid::nil()),
                    username: "deleted".to_string(),
                    display_name: "Deleted User".to_string(),
                    avatar_url: None,
                });

            let channel_name = channel_name_map
                .get(&msg.channel_id)
                .cloned()
                .unwrap_or_else(|| "unknown".to_string());

            let source = if dm_channel_set.contains(&msg.channel_id) {
                GlobalSearchSource {
                    source_type: "dm".to_string(),
                    guild_id: None,
                    guild_name: None,
                }
            } else {
                let guild_id = channel_guild_map.get(&msg.channel_id).copied();
                let guild_name = guild_id.and_then(|gid| guild_name_map.get(&gid).cloned());
                GlobalSearchSource {
                    source_type: "guild".to_string(),
                    guild_id,
                    guild_name,
                }
            };

            GlobalSearchResult {
                id: msg.id,
                channel_id: msg.channel_id,
                channel_name,
                author,
                content: msg.content,
                created_at: msg.created_at,
                headline: msg.headline,
                rank: msg.rank,
                source,
            }
        })
        .collect();

    Ok(Json(GlobalSearchResponse {
        results,
        total,
        limit,
        offset,
    }))
}
