//! Guild Discovery Handlers

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use sqlx::QueryBuilder;
use uuid::Uuid;

use super::types::{DiscoverQuery, DiscoverResponse, DiscoverableGuild, JoinDiscoverableResponse};
use crate::api::AppState;
use crate::auth::AuthUser;
use crate::db;

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug)]
pub enum DiscoveryError {
    Disabled,
    NotFound,
    Validation(String),
    Database(sqlx::Error),
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

impl From<sqlx::Error> for DiscoveryError {
    fn from(err: sqlx::Error) -> Self {
        Self::Database(err)
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

    let limit = query.limit.unwrap_or(20).clamp(1, 50);
    let offset = query.offset.unwrap_or(0).max(0);

    // Build the WHERE clause
    let has_search = query.q.as_ref().is_some_and(|q| !q.trim().is_empty());
    let has_tags = query.tags.as_ref().is_some_and(|t| !t.is_empty());

    // --- Count query ---
    let mut count_builder: QueryBuilder<sqlx::Postgres> = QueryBuilder::new(
        "SELECT COUNT(*) FROM guilds g WHERE g.discoverable = true AND g.suspended_at IS NULL",
    );

    if has_search {
        count_builder.push(" AND g.search_vector @@ websearch_to_tsquery('english', ");
        count_builder.push_bind(query.q.as_ref().unwrap().trim().to_string());
        count_builder.push(")");
    }

    if has_tags {
        count_builder.push(" AND g.tags && ");
        count_builder.push_bind(query.tags.as_ref().unwrap().clone());
    }

    let (total,): (i64,) = count_builder.build_query_as().fetch_one(&state.db).await?;

    if total == 0 {
        return Ok(Json(DiscoverResponse {
            guilds: vec![],
            total: 0,
            limit,
            offset,
        }));
    }

    // --- Data query ---
    let mut builder: QueryBuilder<sqlx::Postgres> = QueryBuilder::new(
        r"SELECT g.id, g.name, g.icon_url, g.banner_url, g.description, g.tags, g.created_at,
                 COUNT(gm.user_id) as member_count
          FROM guilds g
          LEFT JOIN guild_members gm ON g.id = gm.guild_id
          WHERE g.discoverable = true AND g.suspended_at IS NULL",
    );

    if has_search {
        builder.push(" AND g.search_vector @@ websearch_to_tsquery('english', ");
        builder.push_bind(query.q.as_ref().unwrap().trim().to_string());
        builder.push(")");
    }

    if has_tags {
        builder.push(" AND g.tags && ");
        builder.push_bind(query.tags.as_ref().unwrap().clone());
    }

    builder.push(
        " GROUP BY g.id, g.name, g.icon_url, g.banner_url, g.description, g.tags, g.created_at",
    );

    // Sort
    match query.sort.as_str() {
        "members" => builder.push(" ORDER BY member_count DESC, g.created_at DESC"),
        _ => builder.push(" ORDER BY g.created_at DESC"),
    };

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
    )> = builder.build_query_as().fetch_all(&state.db).await?;

    let guilds = rows
        .into_iter()
        .map(
            |(id, name, icon_url, banner_url, description, tags, created_at, member_count)| {
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

    // Check if already a member
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if is_member {
        return Ok(Json(JoinDiscoverableResponse {
            guild_id,
            guild_name,
            already_member: true,
        }));
    }

    // Add as member
    sqlx::query("INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)")
        .bind(guild_id)
        .bind(auth.id)
        .execute(&state.db)
        .await?;

    // Initialize read state for all text channels
    crate::guild::handlers::initialize_channel_read_state(&state.db, guild_id, auth.id)
        .await
        .map_err(|_| {
            DiscoveryError::Database(sqlx::Error::Protocol(
                "Failed to initialize read state".into(),
            ))
        })?;

    // Broadcast MemberJoined to bot ecosystem (non-blocking)
    {
        let db = state.db.clone();
        let redis = state.redis.clone();
        let gid = guild_id;
        let uid = auth.id;
        tokio::spawn(async move {
            // Look up user info for bot event
            let user_info: Option<(String, String)> =
                sqlx::query_as("SELECT username, display_name FROM users WHERE id = $1")
                    .bind(uid)
                    .fetch_optional(&db)
                    .await
                    .ok()
                    .flatten();

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
            }
        });
    }

    Ok(Json(JoinDiscoverableResponse {
        guild_id,
        guild_name,
        already_member: false,
    }))
}
