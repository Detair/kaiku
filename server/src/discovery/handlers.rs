//! Guild Discovery Handlers

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use sqlx::QueryBuilder;
use uuid::Uuid;

use super::types::{
    DiscoverQuery, DiscoverResponse, DiscoverSort, DiscoverableGuild, JoinDiscoverableResponse,
    TAG_REGEX,
};
use crate::api::AppState;
use crate::auth::AuthUser;

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error("Guild discovery is not enabled on this server")]
    Disabled,
    #[error("Guild not found or not discoverable")]
    NotFound,
    #[error("Validation error: {0}")]
    Validation(String),
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for DiscoveryError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            Self::Disabled => (
                StatusCode::NOT_FOUND,
                "DISCOVERY_DISABLED",
                "Guild discovery is not enabled on this server".to_string(),
            ),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "GUILD_NOT_FOUND",
                "Guild not found or not discoverable".to_string(),
            ),
            Self::Validation(msg) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR", msg.clone()),
            Self::Database(err) => {
                tracing::error!(%err, "Discovery endpoint database error");
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

// ============================================================================
// Handlers
// ============================================================================

/// Browse discoverable guilds with optional search, tag filter, and sorting.
#[utoipa::path(
    get,
    path = "/api/discover/guilds",
    tag = "discovery",
    params(DiscoverQuery),
    responses(
        (status = 200, description = "List of discoverable guilds", body = DiscoverResponse),
        (status = 400, description = "Validation error (invalid search query or tags)"),
        (status = 404, description = "Discovery disabled"),
    ),
)]
#[tracing::instrument(skip(state))]
pub async fn browse_guilds(
    State(state): State<AppState>,
    Query(query): Query<DiscoverQuery>,
) -> Result<Json<DiscoverResponse>, DiscoveryError> {
    if !state.config.enable_guild_discovery {
        return Err(DiscoveryError::Disabled);
    }

    // Validate search query length (Issue #6)
    if let Some(ref q) = query.q {
        if q.len() > 200 {
            return Err(DiscoveryError::Validation(
                "Search query too long (max 200 characters)".to_string(),
            ));
        }
    }

    // Validate tag filter content and count (same rules as tag creation)
    if let Some(ref tags) = query.tags {
        if tags.len() > 10 {
            return Err(DiscoveryError::Validation(
                "Maximum 10 tags for filtering".to_string(),
            ));
        }
        for tag in tags {
            if tag.len() < 2 || tag.len() > 32 {
                return Err(DiscoveryError::Validation(
                    "Each filter tag must be 2-32 characters".to_string(),
                ));
            }
            if !TAG_REGEX.is_match(tag) {
                return Err(DiscoveryError::Validation(
                    "Tags may only contain letters, numbers, and hyphens".to_string(),
                ));
            }
        }
    }

    let limit = query.limit.unwrap_or(20).clamp(1, 50);
    let offset = query.offset.unwrap_or(0).clamp(0, 10_000);

    // Build the WHERE clause
    let has_search = query.q.as_ref().is_some_and(|q| !q.trim().is_empty());
    let has_tags = query.tags.as_ref().is_some_and(|t| !t.is_empty());

    // Use denormalized member_count column (maintained by trigger) to avoid
    // a LEFT JOIN + GROUP BY that would scan the entire guild_members table.
    // COUNT(*) OVER() provides the total count atomically with the page data.
    let mut builder: QueryBuilder<sqlx::Postgres> = QueryBuilder::new(
        r"SELECT g.id, g.name, g.icon_url, g.banner_url, g.description, g.tags, g.created_at,
                 g.member_count::bigint,
                 COUNT(*) OVER() as total_count
          FROM guilds g
          WHERE g.discoverable = true AND g.suspended_at IS NULL",
    );

    if has_search {
        builder.push(" AND g.search_vector @@ websearch_to_tsquery('english', ");
        builder.push_bind(query.q.as_ref().unwrap().trim().to_string());
        builder.push(")");
    }

    if has_tags {
        let lower_tags: Vec<String> = query.tags.as_ref().unwrap().iter().map(|t| t.to_lowercase()).collect();
        builder.push(" AND g.tags && ");
        builder.push_bind(lower_tags);
    }

    // Sort
    match query.sort {
        DiscoverSort::Members => {
            builder.push(" ORDER BY g.member_count DESC, g.created_at DESC");
        }
        DiscoverSort::Newest => {
            builder.push(" ORDER BY g.created_at DESC");
        }
    }

    builder.push(" LIMIT ");
    builder.push_bind(limit);
    builder.push(" OFFSET ");
    builder.push_bind(offset);

    let rows: Vec<(
        Uuid,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Vec<String>,
        chrono::DateTime<chrono::Utc>,
        i64,
        i64,
    )> = builder.build_query_as().fetch_all(&state.db).await?;

    // Extract total from the first row's window function.
    // When offset exceeds total rows, the query returns 0 rows and OVER() can't report the count,
    // so we fall back to a separate count query.
    let total = if let Some(first) = rows.first() {
        first.8
    } else if offset > 0 {
        // Past-the-end page: re-run the WHERE clause with COUNT(*) only
        let mut count_builder: QueryBuilder<sqlx::Postgres> = QueryBuilder::new(
            "SELECT COUNT(*) FROM guilds g WHERE g.discoverable = true AND g.suspended_at IS NULL",
        );
        if has_search {
            count_builder.push(" AND g.search_vector @@ websearch_to_tsquery('english', ");
            count_builder.push_bind(query.q.as_ref().unwrap().trim().to_string());
            count_builder.push(")");
        }
        if has_tags {
            let lower_tags: Vec<String> = query.tags.as_ref().unwrap().iter().map(|t| t.to_lowercase()).collect();
            count_builder.push(" AND g.tags && ");
            count_builder.push_bind(lower_tags);
        }
        let (count,): (i64,) = count_builder.build_query_as().fetch_one(&state.db).await?;
        count
    } else {
        0
    };

    let guilds = rows
        .into_iter()
        .map(
            |(id, name, icon_url, banner_url, description, tags, created_at, member_count, _)| {
                DiscoverableGuild {
                    id,
                    name,
                    icon_url,
                    banner_url,
                    description,
                    tags,
                    member_count,
                    created_at,
                }
            },
        )
        .collect();

    Ok(Json(DiscoverResponse {
        guilds,
        total,
        limit,
        offset,
    }))
}

/// Join a discoverable guild (requires authentication).
#[utoipa::path(
    post,
    path = "/api/discover/guilds/{id}/join",
    tag = "discovery",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses(
        (status = 200, description = "Joined the guild", body = JoinDiscoverableResponse),
        (status = 401, description = "Authentication required"),
        (status = 404, description = "Guild not found or not discoverable"),
    ),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn join_discoverable(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<JoinDiscoverableResponse>, DiscoveryError> {
    if !state.config.enable_guild_discovery {
        return Err(DiscoveryError::Disabled);
    }

    // Verify guild is discoverable and not suspended
    let guild: Option<(String,)> = sqlx::query_as(
        "SELECT name FROM guilds WHERE id = $1 AND discoverable = true AND suspended_at IS NULL",
    )
    .bind(guild_id)
    .fetch_optional(&state.db)
    .await?;

    let guild_name = guild.ok_or(DiscoveryError::NotFound)?.0;

    // TODO: Check guild_bans table here when the ban system is implemented.
    // Discovery join is frictionless (no invite code), so banned users must be blocked.

    // Atomic insert with ON CONFLICT to avoid TOCTOU race
    let result = sqlx::query(
        "INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(guild_id)
    .bind(auth.id)
    .execute(&state.db)
    .await?;

    // If no rows affected, user was already a member
    if result.rows_affected() == 0 {
        return Ok(Json(JoinDiscoverableResponse {
            guild_id,
            guild_name,
            already_member: true,
        }));
    }

    // Initialize read state for all text channels
    if let Err(err) =
        crate::guild::handlers::initialize_channel_read_state(&state.db, guild_id, auth.id).await
    {
        tracing::error!(
            ?err,
            guild_id = %guild_id,
            user_id = %auth.id,
            "Failed to initialize channel read state after discovery join"
        );
        // Non-fatal: member was already inserted, read state can be retried on channel access
    }

    // Broadcast MemberJoined to bot ecosystem (non-blocking)
    {
        let db = state.db.clone();
        let redis = state.redis.clone();
        let gid = guild_id;
        let uid = auth.id;
        let span = tracing::info_span!("discovery_member_joined_broadcast", guild_id = %gid, user_id = %uid);
        let handle = tokio::spawn(tracing::Instrument::instrument(async move {
            let user_info: Option<(String, String)> =
                match sqlx::query_as("SELECT username, display_name FROM users WHERE id = $1")
                    .bind(uid)
                    .fetch_optional(&db)
                    .await
                {
                    Ok(info) => info,
                    Err(err) => {
                        tracing::error!(
                            user_id = %uid,
                            guild_id = %gid,
                            %err,
                            "Failed to look up user for MemberJoined event"
                        );
                        return;
                    }
                };

            if let Some((username, display_name)) = user_info {
                crate::ws::bot_events::publish_member_joined(
                    &db,
                    &redis,
                    gid,
                    uid,
                    &username,
                    &display_name,
                )
                .await;
                crate::webhooks::dispatch::dispatch_guild_event(
                    &db,
                    &redis,
                    gid,
                    crate::webhooks::events::BotEventType::MemberJoined,
                    serde_json::json!({
                        "guild_id": gid,
                        "user_id": uid,
                        "username": username,
                        "display_name": display_name,
                    }),
                )
                .await;
            } else {
                tracing::warn!(
                    user_id = %uid,
                    guild_id = %gid,
                    "Skipping MemberJoined broadcast: user not found"
                );
            }
        }, span));
        let watcher_gid = guild_id;
        let watcher_uid = auth.id;
        tokio::spawn(async move {
            if let Err(err) = handle.await {
                tracing::error!(
                    guild_id = %watcher_gid,
                    user_id = %watcher_uid,
                    "MemberJoined broadcast task panicked: {err}",
                );
            }
        });
    }

    Ok(Json(JoinDiscoverableResponse {
        guild_id,
        guild_name,
        already_member: false,
    }))
}
