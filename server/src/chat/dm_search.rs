//! DM Message Search Handler
//!
//! Full-text search for messages within a user's DM channels.

use std::time::Instant;

use axum::extract::{Query, State};
use axum::Json;
use uuid::Uuid;

use super::types::{DmSearchAuthor, DmSearchQuery, DmSearchResponse, DmSearchResult};
use super::ChatError;
use crate::api::AppState;
use crate::auth::AuthUser;
use crate::chat::dm;
use crate::db;

// ============================================================================
// Handler
// ============================================================================

/// Search messages within a user's DM channels.
/// GET `/api/dm/search?q=...`
#[utoipa::path(
    get,
    path = "/api/dm/search",
    tag = "search",
    responses(
        (status = 200, body = DmSearchResponse),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state))]
pub async fn search_dm_messages(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<DmSearchQuery>,
) -> Result<Json<DmSearchResponse>, ChatError> {
    // Validate query
    let search_term = query.q.trim();
    if search_term.is_empty() {
        return Err(ChatError::InvalidQuery(
            "Search query cannot be empty".to_string(),
        ));
    }
    if search_term.len() < 2 {
        return Err(ChatError::InvalidQuery(
            "Search query must be at least 2 characters".to_string(),
        ));
    }
    if search_term.len() > 1000 {
        return Err(ChatError::InvalidQuery(
            "Search query must not exceed 1000 characters".to_string(),
        ));
    }

    // Validate date range
    if let (Some(from), Some(to)) = (query.date_from, query.date_to) {
        if from > to {
            return Err(ChatError::InvalidQuery(
                "date_from must be before date_to".to_string(),
            ));
        }
    }

    // Validate has filter
    if let Some(ref has) = query.has {
        if has != "link" && has != "file" {
            return Err(ChatError::InvalidQuery(
                "has must be \"link\" or \"file\"".to_string(),
            ));
        }
    }

    // Validate sort param
    let sort = match query.sort.as_deref() {
        None | Some("relevance") => db::SearchSort::Relevance,
        Some("date") => db::SearchSort::Date,
        Some(_) => {
            return Err(ChatError::InvalidQuery(
                "sort must be \"relevance\" or \"date\"".to_string(),
            ));
        }
    };

    // Get all DM channels for this user
    let dm_channels = dm::list_user_dms(&state.db, auth.id).await?;

    let mut dm_channel_ids: Vec<Uuid> = dm_channels.iter().map(|c| c.id).collect();

    // If channel_id filter is provided, restrict to that channel (or empty if not accessible)
    if let Some(filter_channel_id) = query.channel_id {
        if dm_channel_ids.contains(&filter_channel_id) {
            dm_channel_ids = vec![filter_channel_id];
        } else {
            dm_channel_ids.clear();
        }
    }

    // If no DM channels, return empty results
    if dm_channel_ids.is_empty() {
        return Ok(Json(DmSearchResponse {
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

    // Get total count
    let total =
        db::count_search_messages_filtered(&state.db, &dm_channel_ids, search_term, &filters)
            .await?;

    // Search messages
    let start = Instant::now();
    let messages = db::search_messages_filtered(
        &state.db,
        &dm_channel_ids,
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
    let user_ids: Vec<Uuid> = messages.iter().filter_map(|m| m.user_id).collect();
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
    let results: Vec<DmSearchResult> = messages
        .into_iter()
        .map(|msg| {
            let author = msg
                .user_id
                .and_then(|uid| user_map.get(&uid))
                .map(|u| DmSearchAuthor {
                    id: u.id,
                    username: u.username.clone(),
                    display_name: u.display_name.clone(),
                    avatar_url: u.avatar_url.clone(),
                })
                .unwrap_or_else(|| DmSearchAuthor {
                    id: msg.user_id.unwrap_or(Uuid::nil()),
                    username: "deleted".to_string(),
                    display_name: "Deleted User".to_string(),
                    avatar_url: None,
                });

            let channel_name = channel_map
                .get(&msg.channel_id)
                .cloned()
                .unwrap_or_else(|| "unknown".to_string());

            DmSearchResult {
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

    Ok(Json(DmSearchResponse {
        results,
        total,
        limit,
        offset,
    }))
}
