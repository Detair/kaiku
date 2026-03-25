//! Guild Management Handlers

use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::QueryBuilder;
use uuid::Uuid;
use validator::Validate;

use super::limits;
use super::types::{
    CreateGuildRequest, Guild, GuildCommandInfo, GuildMember, GuildSettings, GuildWithMemberCount,
    UpdateGuildRequest, UpdateGuildSettingsRequest,
};
use crate::api::AppState;
use crate::auth::AuthUser;
use crate::db::{self, ChannelType};
use crate::discovery::types::TAG_REGEX;
use crate::permissions::{require_guild_permission, GuildPermissions, PermissionError};
use crate::util::format_file_size;
use crate::ws::{broadcast_to_user, ServerEvent};

// ============================================================================
// Response Types
// ============================================================================

/// Channel with unread message count for the current user.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ChannelWithUnread {
    #[serde(flatten)]
    #[schema(inline)]
    pub channel: db::Channel,
    /// Number of unread messages (only for text channels).
    pub unread_count: i64,
    /// User's read cursor for this channel.
    pub last_read_message_id: Option<Uuid>,
    /// Most recent message in this channel.
    pub last_message_id: Option<Uuid>,
}

/// A bot installed in a guild.
#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct InstalledBot {
    pub application_id: Uuid,
    pub bot_user_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub installed_by: Uuid,
    pub installed_at: chrono::DateTime<chrono::Utc>,
}

// ============================================================================
// Request Types
// ============================================================================

/// Position specification for a channel in reorder request.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ChannelPosition {
    pub id: Uuid,
    pub position: i32,
    #[serde(default)]
    pub category_id: Option<Uuid>,
}

/// Request to reorder channels in a guild.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ReorderChannelsRequest {
    pub channels: Vec<ChannelPosition>,
}

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug)]
pub enum GuildError {
    NotFound,
    Forbidden,
    ForbiddenMsg(String),
    Permission(PermissionError),
    Validation(String),
    LimitExceeded(String),
    Database(sqlx::Error),
    Internal(String),
}

impl IntoResponse for GuildError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "GUILD_NOT_FOUND",
                "Guild not found".to_string(),
            ),
            Self::Forbidden => (
                StatusCode::FORBIDDEN,
                "FORBIDDEN",
                "Access denied".to_string(),
            ),
            Self::ForbiddenMsg(msg) => (StatusCode::FORBIDDEN, "FORBIDDEN", msg.clone()),
            Self::Permission(e) => (StatusCode::FORBIDDEN, "PERMISSION_DENIED", e.to_string()),
            Self::Validation(msg) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR", msg.clone()),
            Self::LimitExceeded(msg) => (StatusCode::FORBIDDEN, "LIMIT_EXCEEDED", msg.clone()),
            Self::Database(err) => {
                tracing::error!(%err, "Guild endpoint database error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    "Database error".to_string(),
                )
            }
            Self::Internal(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                msg.clone(),
            ),
        };
        (
            status,
            Json(serde_json::json!({ "error": code, "message": message })),
        )
            .into_response()
    }
}

impl From<sqlx::Error> for GuildError {
    fn from(err: sqlx::Error) -> Self {
        Self::Database(err)
    }
}

// ============================================================================
// Handlers
// ============================================================================

/// Create a new guild
#[utoipa::path(
    post,
    path = "/api/guilds",
    tag = "guilds",
    request_body = CreateGuildRequest,
    responses((status = 200, body = Guild)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn create_guild(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateGuildRequest>,
) -> Result<Json<Guild>, GuildError> {
    // Validate request
    body.validate()
        .map_err(|e| GuildError::Validation(crate::validation::format_validation_errors(&e)))?;

    // Validate tags if provided
    if let Some(ref tags) = body.tags {
        if tags.len() > 5 {
            return Err(GuildError::Validation("Maximum 5 tags allowed".to_string()));
        }
        for tag in tags {
            if tag.len() < 2 || tag.len() > 32 {
                return Err(GuildError::Validation(
                    "Each tag must be 2-32 characters".to_string(),
                ));
            }
            if !TAG_REGEX.is_match(tag) {
                return Err(GuildError::Validation(
                    "Tags may only contain letters, numbers, and hyphens".to_string(),
                ));
            }
        }
    }

    // Validate banner_url if provided
    if let Some(ref url) = body.banner_url {
        if !url.is_empty() {
            if url.len() > 2048 {
                return Err(GuildError::Validation(
                    "Banner URL too long (max 2048 characters)".to_string(),
                ));
            }
            if !url.starts_with("https://") {
                return Err(GuildError::Validation(
                    "Banner URL must use HTTPS".to_string(),
                ));
            }
        }
    }

    let mut tx = state.db.begin().await?;

    // Serialize guild creation per owner to enforce strict user guild limits.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 51))")
        .bind(auth.id)
        .execute(&mut *tx)
        .await?;

    // Check guild creation limit
    let owned_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM guilds WHERE owner_id = $1")
        .bind(auth.id)
        .fetch_one(&mut *tx)
        .await?;
    if owned_count >= state.config.max_guilds_per_user {
        return Err(GuildError::LimitExceeded(format!(
            "Maximum number of guilds reached ({})",
            state.config.max_guilds_per_user
        )));
    }

    // Prepare discovery fields
    let discoverable = body.discoverable.unwrap_or(false);
    let tags: Vec<String> = body
        .tags
        .map(|t| t.into_iter().map(|s| s.to_lowercase()).collect())
        .unwrap_or_default();
    let banner_url: Option<String> =
        body.banner_url
            .and_then(|u| if u.is_empty() { None } else { Some(u) });

    // Insert guild with discovery fields
    let guild_id = Uuid::now_v7();
    let guild = sqlx::query_as::<_, Guild>(
        r"INSERT INTO guilds (id, name, owner_id, description, discoverable, tags, banner_url)
           VALUES ($1, $2, $3, $4, $5, $6, $7)
           RETURNING id, name, owner_id, icon_url, description, threads_enabled, discoverable, tags, banner_url, plan, created_at",
    )
    .bind(guild_id)
    .bind(&body.name)
    .bind(auth.id)
    .bind(&body.description)
    .bind(discoverable)
    .bind(&tags)
    .bind(&banner_url)
    .fetch_one(&mut *tx)
    .await?;

    // Add owner as member
    sqlx::query("INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)")
        .bind(guild_id)
        .bind(auth.id)
        .execute(&mut *tx)
        .await?;

    // Create default @everyone role with sensible default permissions
    sqlx::query(
        r"INSERT INTO guild_roles (guild_id, name, permissions, position, is_default)
           VALUES ($1, 'everyone', $2, 0, true)",
    )
    .bind(guild_id)
    .bind(crate::permissions::GuildPermissions::EVERYONE_DEFAULT.bits() as i64)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Json(guild))
}

/// List guilds for the current user with member counts
#[utoipa::path(
    get,
    path = "/api/guilds",
    tag = "guilds",
    responses((status = 200, body = Vec<GuildWithMemberCount>)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn list_guilds(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<GuildWithMemberCount>>, GuildError> {
    // Query guilds with member count in a single query
    let rows: Vec<(
        Uuid,
        String,
        Uuid,
        Option<String>,
        Option<String>,
        bool,
        bool,
        Vec<String>,
        Option<String>,
        String,
        chrono::DateTime<chrono::Utc>,
        i64,
    )> = sqlx::query_as(
        r"SELECT
            g.id, g.name, g.owner_id, g.icon_url, g.description, g.threads_enabled,
            g.discoverable, g.tags, g.banner_url, g.plan, g.created_at,
            g.member_count::bigint
           FROM guilds g
           INNER JOIN guild_members gm ON g.id = gm.guild_id
           WHERE gm.user_id = $1
           ORDER BY g.created_at",
    )
    .bind(auth.id)
    .fetch_all(&state.db)
    .await?;

    let guilds = rows
        .into_iter()
        .map(
            |(
                id,
                name,
                owner_id,
                icon_url,
                description,
                threads_enabled,
                discoverable,
                tags,
                banner_url,
                plan,
                created_at,
                member_count,
            )| {
                GuildWithMemberCount {
                    guild: Guild {
                        id,
                        name,
                        owner_id,
                        icon_url,
                        description,
                        threads_enabled,
                        discoverable,
                        tags,
                        banner_url,
                        plan,
                        created_at,
                    },
                    member_count,
                }
            },
        )
        .collect();

    Ok(Json(guilds))
}

/// Get guild details
#[utoipa::path(
    get,
    path = "/api/guilds/{id}",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 200, body = Guild)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn get_guild(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<Guild>, GuildError> {
    // Verify membership
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if !is_member {
        return Err(GuildError::Forbidden);
    }

    let guild = sqlx::query_as::<_, Guild>(
        "SELECT id, name, owner_id, icon_url, description, threads_enabled, discoverable, tags, banner_url, plan, created_at FROM guilds WHERE id = $1",
    )
    .bind(guild_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(GuildError::NotFound)?;

    Ok(Json(guild))
}

/// Update guild
#[utoipa::path(
    patch,
    path = "/api/guilds/{id}",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    request_body = UpdateGuildRequest,
    responses((status = 200, body = Guild)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn update_guild(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(body): Json<UpdateGuildRequest>,
) -> Result<Json<Guild>, GuildError> {
    // Validate request
    body.validate()
        .map_err(|e| GuildError::Validation(crate::validation::format_validation_errors(&e)))?;

    // Verify ownership
    let owner_check: Option<(Uuid,)> = sqlx::query_as("SELECT owner_id FROM guilds WHERE id = $1")
        .bind(guild_id)
        .fetch_optional(&state.db)
        .await?;

    let owner_id = owner_check.ok_or(GuildError::NotFound)?.0;

    if owner_id != auth.id {
        return Err(GuildError::Forbidden);
    }

    // Build dynamic update query
    let mut has_changes = false;
    let mut builder = QueryBuilder::new("UPDATE guilds SET ");
    {
        let mut sep = builder.separated(", ");
        if let Some(name) = body.name {
            sep.push("name = ").push_bind_unseparated(name);
            has_changes = true;
        }
        if let Some(desc) = body.description {
            sep.push("description = ").push_bind_unseparated(desc);
            has_changes = true;
        }
        if let Some(icon) = body.icon_url {
            sep.push("icon_url = ").push_bind_unseparated(icon);
            has_changes = true;
        }
    }

    if !has_changes {
        return get_guild(State(state), auth, Path(guild_id)).await;
    }

    builder.push(" WHERE id = ");
    builder.push_bind(guild_id);
    builder
        .push(" RETURNING id, name, owner_id, icon_url, description, threads_enabled, discoverable, tags, banner_url, plan, created_at");

    let updated_guild = builder
        .build_query_as::<Guild>()
        .fetch_one(&state.db)
        .await?;

    Ok(Json(updated_guild))
}

/// Delete guild (owner only)
#[utoipa::path(
    delete,
    path = "/api/guilds/{id}",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 204, description = "Guild deleted")),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn delete_guild(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<StatusCode, GuildError> {
    // Verify ownership
    let owner_check: Option<(Uuid,)> = sqlx::query_as("SELECT owner_id FROM guilds WHERE id = $1")
        .bind(guild_id)
        .fetch_optional(&state.db)
        .await?;

    let owner_id = owner_check.ok_or(GuildError::NotFound)?.0;

    if owner_id != auth.id {
        return Err(GuildError::Forbidden);
    }

    sqlx::query("DELETE FROM guilds WHERE id = $1")
        .bind(guild_id)
        .execute(&state.db)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

/// Initialize `channel_read_state` for all text channels in a guild.
/// Sets `last_read_at` to `NOW()` so pre-existing messages don't appear as unread.
pub(crate) async fn initialize_channel_read_state(
    db: &sqlx::PgPool,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<(), GuildError> {
    sqlx::query(
        r"INSERT INTO channel_read_state (user_id, channel_id, last_read_at, last_read_message_id)
           SELECT $1, c.id, NOW(), (
               SELECT m.id FROM messages m
               WHERE m.channel_id = c.id AND m.deleted_at IS NULL
               ORDER BY m.created_at DESC LIMIT 1
           )
           FROM channels c
           WHERE c.guild_id = $2 AND c.channel_type = 'text'
           ON CONFLICT (user_id, channel_id) DO NOTHING",
    )
    .bind(user_id)
    .bind(guild_id)
    .execute(db)
    .await?;
    Ok(())
}

/// Leave guild
#[utoipa::path(
    post,
    path = "/api/guilds/{id}/leave",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 204, description = "Left guild")),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn leave_guild(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<StatusCode, GuildError> {
    // Verify membership
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if !is_member {
        return Err(GuildError::NotFound);
    }

    // Check if owner (owners can't leave, must transfer ownership first)
    let owner_check: (Uuid,) = sqlx::query_as("SELECT owner_id FROM guilds WHERE id = $1")
        .bind(guild_id)
        .fetch_one(&state.db)
        .await?;

    if owner_check.0 == auth.id {
        return Err(GuildError::Validation(
            "Guild owner must transfer ownership before leaving".to_string(),
        ));
    }

    // Remove membership
    sqlx::query("DELETE FROM guild_members WHERE guild_id = $1 AND user_id = $2")
        .bind(guild_id)
        .bind(auth.id)
        .execute(&state.db)
        .await?;

    // Dispatch MemberLeft to bot ecosystem (non-blocking)
    {
        let db = state.db.clone();
        let redis = state.redis.clone();
        let gid = guild_id;
        let uid = auth.id;
        tokio::spawn(async move {
            crate::ws::bot_events::publish_member_left(&db, &redis, gid, uid).await;
            crate::webhooks::dispatch::dispatch_guild_event(
                &db,
                &redis,
                gid,
                crate::webhooks::events::BotEventType::MemberLeft,
                serde_json::json!({ "guild_id": gid, "user_id": uid }),
            )
            .await;
        });
    }

    Ok(StatusCode::NO_CONTENT)
}

/// List guild members
#[utoipa::path(
    get,
    path = "/api/guilds/{id}/members",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 200, body = Vec<GuildMember>)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn list_members(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<Vec<GuildMember>>, GuildError> {
    // Verify membership
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if !is_member {
        return Err(GuildError::Forbidden);
    }

    let members = sqlx::query_as::<_, GuildMember>(
        r"SELECT
            u.id as user_id,
            u.username,
            u.display_name,
            u.avatar_url,
            gm.nickname,
            gm.joined_at,
            u.status::text as status,
            u.last_seen_at
           FROM guild_members gm
           INNER JOIN users u ON gm.user_id = u.id
           WHERE gm.guild_id = $1
           ORDER BY gm.joined_at",
    )
    .bind(guild_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(members))
}

/// Kick a member from guild (owner only)
#[utoipa::path(
    delete,
    path = "/api/guilds/{id}/members/{user_id}",
    tag = "guilds",
    params(
        ("id" = Uuid, Path, description = "Guild ID"),
        ("user_id" = Uuid, Path, description = "User ID")
    ),
    responses((status = 204, description = "Member kicked")),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn kick_member(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((guild_id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, GuildError> {
    // Verify ownership
    let owner_check: Option<(Uuid,)> = sqlx::query_as("SELECT owner_id FROM guilds WHERE id = $1")
        .bind(guild_id)
        .fetch_optional(&state.db)
        .await?;

    let owner_id = owner_check.ok_or(GuildError::NotFound)?.0;

    if owner_id != auth.id {
        return Err(GuildError::Forbidden);
    }

    // Cannot kick yourself (owner)
    if user_id == auth.id {
        return Err(GuildError::Validation(
            "Cannot kick yourself from the guild".to_string(),
        ));
    }

    // Remove membership
    let result = sqlx::query("DELETE FROM guild_members WHERE guild_id = $1 AND user_id = $2")
        .bind(guild_id)
        .bind(user_id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(GuildError::NotFound);
    }

    // Dispatch MemberLeft to bot ecosystem (non-blocking)
    {
        let db = state.db.clone();
        let redis = state.redis.clone();
        let gid = guild_id;
        let uid = user_id;
        tokio::spawn(async move {
            crate::ws::bot_events::publish_member_left(&db, &redis, gid, uid).await;
            crate::webhooks::dispatch::dispatch_guild_event(
                &db,
                &redis,
                gid,
                crate::webhooks::events::BotEventType::MemberLeft,
                serde_json::json!({ "guild_id": gid, "user_id": uid }),
            )
            .await;
        });
    }

    Ok(StatusCode::NO_CONTENT)
}

/// List guild channels with unread counts
#[utoipa::path(
    get,
    path = "/api/guilds/{id}/channels",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 200, body = Vec<ChannelWithUnread>)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn list_channels(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<Vec<ChannelWithUnread>>, GuildError> {
    // Verify membership before fetching channels
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if !is_member {
        return Err(GuildError::Forbidden);
    }

    let all_channels = db::get_guild_channels(&state.db, guild_id).await?;
    let all_channel_ids: Vec<Uuid> = all_channels.iter().map(|c| c.id).collect();

    // Batch permission check: fetch membership + roles once, batch-fetch overrides
    let accessible_ids = crate::permissions::filter_accessible_channels(
        &state.db,
        guild_id,
        auth.id,
        &all_channel_ids,
    )
    .await
    .map_err(|e| match e {
        crate::permissions::PermissionError::NotGuildMember => GuildError::Forbidden,
        other => GuildError::Permission(other),
    })?;

    let accessible_set: std::collections::HashSet<Uuid> = accessible_ids.into_iter().collect();
    let channels: Vec<db::Channel> = all_channels
        .into_iter()
        .filter(|c| accessible_set.contains(&c.id))
        .collect();

    // Collect text channel IDs for batched unread count query
    let text_channel_ids: Vec<Uuid> = channels
        .iter()
        .filter(|c| c.channel_type == ChannelType::Text)
        .map(|c| c.id)
        .collect();

    // Single CTE query: unread counts, read cursors, and last message IDs in one round trip
    let channel_states: std::collections::HashMap<Uuid, (i64, Option<Uuid>, Option<Uuid>)> =
        if text_channel_ids.is_empty() {
            std::collections::HashMap::new()
        } else {
            sqlx::query_as::<_, (Uuid, i64, Option<Uuid>, Option<Uuid>)>(
                r"
                WITH cursors AS (
                    SELECT channel_id, last_read_message_id,
                           (SELECT created_at FROM messages WHERE id = last_read_message_id) AS cursor_at
                    FROM channel_read_state
                    WHERE user_id = $1 AND channel_id = ANY($2)
                ),
                latest_msgs AS (
                    SELECT DISTINCT ON (channel_id) channel_id, id AS last_message_id
                    FROM messages
                    WHERE channel_id = ANY($2) AND deleted_at IS NULL
                    ORDER BY channel_id, created_at DESC
                )
                SELECT
                    c.id AS channel_id,
                    COUNT(m.id)::bigint AS unread_count,
                    crs.last_read_message_id,
                    lm.last_message_id
                FROM channels c
                LEFT JOIN cursors crs ON crs.channel_id = c.id
                LEFT JOIN latest_msgs lm ON lm.channel_id = c.id
                LEFT JOIN messages m
                    ON m.channel_id = c.id
                    AND m.deleted_at IS NULL
                    AND (crs.cursor_at IS NULL OR m.created_at > crs.cursor_at)
                WHERE c.id = ANY($2)
                GROUP BY c.id, crs.last_read_message_id, lm.last_message_id
                ",
            )
            .bind(auth.id)
            .bind(&text_channel_ids)
            .fetch_all(&state.db)
            .await?
            .into_iter()
            .map(|(channel_id, unread, cursor, last_msg)| (channel_id, (unread, cursor, last_msg)))
            .collect()
        };

    // Build result with unread counts, read cursors, and last message IDs
    let result: Vec<ChannelWithUnread> = channels
        .into_iter()
        .map(|channel| {
            let is_text = channel.channel_type == ChannelType::Text;
            let (unread_count, last_read_message_id, last_message_id) = if is_text {
                channel_states
                    .get(&channel.id)
                    .map(|(u, r, l)| (*u, *r, *l))
                    .unwrap_or((0, None, None))
            } else {
                (0, None, None)
            };
            ChannelWithUnread {
                channel,
                unread_count,
                last_read_message_id,
                last_message_id,
            }
        })
        .collect();

    Ok(Json(result))
}

/// Reorder channels in a guild.
///
/// `POST /api/guilds/:guild_id/channels/reorder`
#[utoipa::path(
    post,
    path = "/api/guilds/{id}/channels/reorder",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    request_body = ReorderChannelsRequest,
    responses((status = 204, description = "Channels reordered")),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state, body))]
pub async fn reorder_channels(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(body): Json<ReorderChannelsRequest>,
) -> Result<StatusCode, GuildError> {
    // Check MANAGE_CHANNELS permission
    let _ctx = require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        GuildPermissions::MANAGE_CHANNELS,
    )
    .await
    .map_err(|e| match e {
        PermissionError::NotGuildMember => GuildError::Forbidden,
        other => GuildError::Permission(other),
    })?;

    if body.channels.is_empty() {
        return Ok(StatusCode::NO_CONTENT);
    }

    // Update positions in transaction
    let mut tx = state.db.begin().await?;

    for ch in &body.channels {
        // Validate category type restriction when moving to a new category
        if let Some(cat_id) = ch.category_id {
            let cat_type: Option<(String,)> =
                sqlx::query_as("SELECT category_type::TEXT FROM channel_categories WHERE id = $1")
                    .bind(cat_id)
                    .fetch_optional(&mut *tx)
                    .await?;

            if let Some((cat_type,)) = cat_type {
                if cat_type != "mixed" {
                    let ch_type: Option<(String,)> =
                        sqlx::query_as("SELECT channel_type::TEXT FROM channels WHERE id = $1")
                            .bind(ch.id)
                            .fetch_optional(&mut *tx)
                            .await?;

                    if let Some((ch_type,)) = ch_type {
                        if cat_type != ch_type {
                            return Err(GuildError::Validation(format!(
                                "Cannot move {ch_type} channel to {cat_type}-only category"
                            )));
                        }
                    }
                }
            }
        }

        sqlx::query(
            r"
            UPDATE channels
            SET position = $3, category_id = $4
            WHERE id = $1 AND guild_id = $2
            ",
        )
        .bind(ch.id)
        .bind(guild_id)
        .bind(ch.position)
        .bind(ch.category_id)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}

/// Install a bot into a guild.
///
/// `POST /api/guilds/:guild_id/bots/:bot_id/add`
#[utoipa::path(
    post,
    path = "/api/guilds/{id}/bots/{bot_id}/add",
    tag = "guilds",
    params(
        ("id" = Uuid, Path, description = "Guild ID"),
        ("bot_id" = Uuid, Path, description = "Bot user ID")
    ),
    responses((status = 204, description = "Bot added")),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn add_bot_to_guild(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((guild_id, bot_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, GuildError> {
    let _ctx =
        require_guild_permission(&state.db, guild_id, auth.id, GuildPermissions::MANAGE_GUILD)
            .await
            .map_err(|e| match e {
                PermissionError::NotGuildMember => GuildError::Forbidden,
                other => GuildError::Permission(other),
            })?;

    // Validate bot exists and is installable (outside lock)
    let bot_exists = sqlx::query!(
        "SELECT id FROM users WHERE id = $1 AND is_bot = true",
        bot_id
    )
    .fetch_optional(&state.db)
    .await?;

    if bot_exists.is_none() {
        return Err(GuildError::NotFound);
    }

    let app = sqlx::query!(
        "SELECT id, owner_id, public FROM bot_applications WHERE bot_user_id = $1",
        bot_id
    )
    .fetch_optional(&state.db)
    .await?;

    let app = match app {
        Some(app) => app,
        None => return Err(GuildError::NotFound),
    };

    if !app.public && app.owner_id != auth.id {
        return Err(GuildError::NotFound);
    }

    let application_id = app.id;

    // Advisory lock seed 63 = bot_install (see db/mod.rs registry)
    let mut tx = state.db.begin().await?;

    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 63))")
        .bind(guild_id)
        .execute(&mut *tx)
        .await?;

    let bot_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM guild_bot_installations WHERE guild_id = $1")
            .bind(guild_id)
            .fetch_one(&mut *tx)
            .await?;

    if bot_count >= state.config.max_bots_per_guild {
        return Err(GuildError::LimitExceeded(format!(
            "Maximum number of bots per guild reached ({})",
            state.config.max_bots_per_guild
        )));
    }

    sqlx::query(
        "INSERT INTO guild_bot_installations (guild_id, application_id, installed_by) VALUES ($1, $2, $3) ON CONFLICT (guild_id, application_id) DO NOTHING",
    )
    .bind(guild_id)
    .bind(application_id)
    .bind(auth.id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}

/// List bots installed in a guild.
///
/// `GET /api/guilds/:guild_id/bots`
#[utoipa::path(
    get,
    path = "/api/guilds/{id}/bots",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 200, body = Vec<InstalledBot>)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn list_guild_bots(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<Vec<InstalledBot>>, GuildError> {
    // Verify membership
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if !is_member {
        return Err(GuildError::Forbidden);
    }

    let bots = sqlx::query_as::<_, InstalledBot>(
        r"SELECT
            gbi.application_id,
            ba.bot_user_id,
            ba.name,
            ba.description,
            gbi.installed_by,
            gbi.installed_at
           FROM guild_bot_installations gbi
           INNER JOIN bot_applications ba ON gbi.application_id = ba.id
           WHERE gbi.guild_id = $1
           ORDER BY gbi.installed_at",
    )
    .bind(guild_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(bots))
}

/// Remove a bot from a guild.
///
/// `DELETE /api/guilds/:guild_id/bots/:bot_id`
#[utoipa::path(
    delete,
    path = "/api/guilds/{id}/bots/{bot_id}",
    tag = "guilds",
    params(
        ("id" = Uuid, Path, description = "Guild ID"),
        ("bot_id" = Uuid, Path, description = "Bot user ID")
    ),
    responses((status = 204, description = "Bot removed")),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn remove_bot_from_guild(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((guild_id, bot_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, GuildError> {
    let _ctx =
        require_guild_permission(&state.db, guild_id, auth.id, GuildPermissions::MANAGE_GUILD)
            .await
            .map_err(|e| match e {
                PermissionError::NotGuildMember => GuildError::Forbidden,
                other => GuildError::Permission(other),
            })?;

    // Look up application_id from bot_user_id
    let app = sqlx::query!(
        "SELECT id FROM bot_applications WHERE bot_user_id = $1",
        bot_id
    )
    .fetch_optional(&state.db)
    .await?;

    let application_id = match app {
        Some(app) => app.id,
        None => return Err(GuildError::NotFound),
    };

    let result = sqlx::query!(
        "DELETE FROM guild_bot_installations WHERE guild_id = $1 AND application_id = $2",
        guild_id,
        application_id
    )
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        return Err(GuildError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT)
}

/// List available slash commands in a guild (from installed bots).
///
/// Returns both guild-scoped and global commands from all installed bots.
/// Guild-scoped commands take precedence over global commands with the same name.
///
/// `GET /api/guilds/:guild_id/commands`
#[utoipa::path(
    get,
    path = "/api/guilds/{id}/commands",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 200, body = Vec<GuildCommandInfo>)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn list_guild_commands(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<Vec<GuildCommandInfo>>, GuildError> {
    // Verify membership
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if !is_member {
        return Err(GuildError::Forbidden);
    }

    // Return all commands from installed bots (no DISTINCT ON).
    let rows: Vec<(String, String, String, Uuid)> = sqlx::query_as(
        r"SELECT sc.name, sc.description, ba.name as bot_name, ba.id as application_id
           FROM slash_commands sc
           INNER JOIN bot_applications ba ON sc.application_id = ba.id
           INNER JOIN guild_bot_installations gbi ON ba.id = gbi.application_id
           WHERE gbi.guild_id = $1 AND (sc.guild_id = $1 OR sc.guild_id IS NULL)
           ORDER BY sc.name, (sc.guild_id IS NULL), sc.created_at",
    )
    .bind(guild_id)
    .fetch_all(&state.db)
    .await?;

    // Compute ambiguity: count how many distinct apps provide each command name.
    let mut name_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for (name, _, _, _) in &rows {
        *name_counts.entry(name.clone()).or_insert(0) += 1;
    }

    let result: Vec<GuildCommandInfo> = rows
        .into_iter()
        .map(|(name, description, bot_name, application_id)| {
            let is_ambiguous = name_counts.get(&name).copied().unwrap_or(0) > 1;
            GuildCommandInfo {
                name,
                description,
                bot_name,
                application_id,
                is_ambiguous,
            }
        })
        .collect();

    Ok(Json(result))
}

/// Mark all text channels in a guild as read.
/// POST /api/guilds/{id}/read-all
#[utoipa::path(
    post,
    path = "/api/guilds/{id}/read-all",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 204, description = "All channels marked as read")),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn mark_all_channels_read(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<StatusCode, GuildError> {
    // Verify guild membership
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if !is_member {
        return Err(GuildError::Forbidden);
    }

    let now = chrono::Utc::now();

    // Batch UPSERT channel_read_state for all text channels in this guild
    // Uses a subquery to get the latest message ID per channel
    let rows: Vec<(Uuid, Option<Uuid>)> = sqlx::query_as(
        r"INSERT INTO channel_read_state (user_id, channel_id, last_read_at, last_read_message_id)
          SELECT $1, c.id, $3, (
              SELECT m.id FROM messages m
              WHERE m.channel_id = c.id AND m.deleted_at IS NULL
              ORDER BY m.created_at DESC LIMIT 1
          )
          FROM channels c
          WHERE c.guild_id = $2 AND c.channel_type = 'text'
          ON CONFLICT (user_id, channel_id)
          DO UPDATE SET last_read_at = EXCLUDED.last_read_at, last_read_message_id = EXCLUDED.last_read_message_id
          RETURNING channel_id, last_read_message_id",
    )
    .bind(auth.id)
    .bind(guild_id)
    .bind(now)
    .fetch_all(&state.db)
    .await?;

    // Broadcast ChannelRead events for each updated channel
    for (channel_id, last_read_message_id) in &rows {
        if let Err(e) = broadcast_to_user(
            &state.redis,
            auth.id,
            &ServerEvent::ChannelRead {
                channel_id: *channel_id,
                last_read_message_id: *last_read_message_id,
            },
        )
        .await
        {
            tracing::warn!(
                user_id = %auth.id,
                channel_id = %channel_id,
                error = %e,
                "Failed to broadcast ChannelRead event"
            );
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

/// Get guild settings.
/// GET /api/guilds/{id}/settings
#[utoipa::path(
    get,
    path = "/api/guilds/{id}/settings",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 200, body = GuildSettings)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn get_guild_settings(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<GuildSettings>, GuildError> {
    // Verify guild membership
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if !is_member {
        return Err(GuildError::Forbidden);
    }

    let settings: (bool, bool, Vec<String>, Option<String>, bool) = sqlx::query_as(
        r"SELECT g.threads_enabled, g.discoverable, g.tags, g.banner_url,
                 (gm.discovery_prompt_dismissed_at IS NOT NULL) AS dismissed
          FROM guilds g
          INNER JOIN guild_members gm ON gm.guild_id = g.id AND gm.user_id = $2
          WHERE g.id = $1",
    )
    .bind(guild_id)
    .bind(auth.id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(GuildError::NotFound)?;

    Ok(Json(GuildSettings {
        threads_enabled: settings.0,
        discoverable: settings.1,
        tags: settings.2,
        banner_url: settings.3,
        discovery_prompt_dismissed: settings.4,
    }))
}

/// Update guild settings (requires `MANAGE_GUILD`).
/// PATCH /api/guilds/{id}/settings
#[utoipa::path(
    patch,
    path = "/api/guilds/{id}/settings",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    request_body = UpdateGuildSettingsRequest,
    responses((status = 200, body = GuildSettings)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn update_guild_settings(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(body): Json<UpdateGuildSettingsRequest>,
) -> Result<Json<GuildSettings>, GuildError> {
    // Check MANAGE_GUILD permission
    require_guild_permission(&state.db, guild_id, auth.id, GuildPermissions::MANAGE_GUILD)
        .await
        .map_err(GuildError::Permission)?;

    // Validate tags if provided
    if let Some(ref tags) = body.tags {
        if tags.len() > 5 {
            return Err(GuildError::Validation("Maximum 5 tags allowed".to_string()));
        }
        for tag in tags {
            if tag.len() < 2 || tag.len() > 32 {
                return Err(GuildError::Validation(
                    "Each tag must be 2-32 characters".to_string(),
                ));
            }
            if !TAG_REGEX.is_match(tag) {
                return Err(GuildError::Validation(
                    "Tags may only contain letters, numbers, and hyphens".to_string(),
                ));
            }
        }
    }

    // Validate banner_url if provided (empty string clears the banner)
    if let Some(ref url) = body.banner_url {
        if !url.is_empty() {
            if url.len() > 2048 {
                return Err(GuildError::Validation(
                    "Banner URL too long (max 2048 characters)".to_string(),
                ));
            }
            if !url.starts_with("https://") {
                return Err(GuildError::Validation(
                    "Banner URL must use HTTPS".to_string(),
                ));
            }
        }
    }

    let mut has_changes = false;
    let mut builder = QueryBuilder::new("UPDATE guilds SET ");
    {
        let mut sep = builder.separated(", ");
        if let Some(threads_enabled) = body.threads_enabled {
            sep.push("threads_enabled = ")
                .push_bind_unseparated(threads_enabled);
            has_changes = true;
        }
        if let Some(discoverable) = body.discoverable {
            sep.push("discoverable = ")
                .push_bind_unseparated(discoverable);
            has_changes = true;
        }
        if let Some(tags) = body.tags {
            let tags: Vec<String> = tags.into_iter().map(|t| t.to_lowercase()).collect();
            sep.push("tags = ").push_bind_unseparated(tags);
            has_changes = true;
        }
        if let Some(banner_url) = body.banner_url {
            // Normalize empty string to NULL (clears the banner)
            let normalized: Option<String> = if banner_url.is_empty() {
                None
            } else {
                Some(banner_url)
            };
            sep.push("banner_url = ").push_bind_unseparated(normalized);
            has_changes = true;
        }
    }

    if !has_changes {
        return get_guild_settings(State(state), auth, Path(guild_id)).await;
    }

    builder
        .push(" WHERE id = ")
        .push_bind(guild_id)
        .push(" RETURNING threads_enabled, discoverable, tags, banner_url");

    let (threads_enabled, discoverable, tags, banner_url) = builder
        .build_query_as::<(bool, bool, Vec<String>, Option<String>)>()
        .fetch_one(&state.db)
        .await?;

    // Fetch per-member discovery prompt dismissal status
    let dismissed: (bool,) = sqlx::query_as(
        "SELECT (discovery_prompt_dismissed_at IS NOT NULL) FROM guild_members WHERE guild_id = $1 AND user_id = $2",
    )
    .bind(guild_id)
    .bind(auth.id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(GuildSettings {
        threads_enabled,
        discoverable,
        tags,
        banner_url,
        discovery_prompt_dismissed: dismissed.0,
    }))
}

/// Dismiss the discovery setup prompt for the current user in a guild.
/// POST /api/guilds/{id}/dismiss-discovery-prompt
#[utoipa::path(
    post,
    path = "/api/guilds/{id}/dismiss-discovery-prompt",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses(
        (status = 204, description = "Prompt dismissed successfully"),
        (status = 403, description = "Not a member of this guild"),
    ),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn dismiss_discovery_prompt(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<StatusCode, GuildError> {
    // Verify guild membership
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if !is_member {
        return Err(GuildError::Forbidden);
    }

    sqlx::query(
        r"UPDATE guild_members
          SET discovery_prompt_dismissed_at = NOW()
          WHERE guild_id = $1 AND user_id = $2
            AND discovery_prompt_dismissed_at IS NULL",
    )
    .bind(guild_id)
    .bind(auth.id)
    .execute(&state.db)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Guild Usage Stats
// ============================================================================

/// A single resource usage metric (current count vs limit).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UsageStat {
    pub current: i64,
    pub limit: i64,
}

/// Guild resource usage statistics.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GuildUsageStats {
    pub guild_id: Uuid,
    pub plan: String,
    pub members: UsageStat,
    pub channels: UsageStat,
    pub roles: UsageStat,
    pub emojis: UsageStat,
    pub bots: UsageStat,
    pub pages: UsageStat,
}

/// Get guild resource usage stats.
/// GET /api/guilds/{id}/usage
#[utoipa::path(
    get,
    path = "/api/guilds/{id}/usage",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 200, body = GuildUsageStats)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn get_guild_usage(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<GuildUsageStats>, GuildError> {
    // Verify membership
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if !is_member {
        return Err(GuildError::Forbidden);
    }

    // Fetch plan
    let (plan,): (String,) = sqlx::query_as("SELECT plan FROM guilds WHERE id = $1")
        .bind(guild_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(GuildError::NotFound)?;

    // Run count queries in parallel
    let (members, channels, roles, emojis, bots, pages, page_limit) = tokio::join!(
        limits::get_member_count(&state.db, guild_id),
        limits::count_guild_channels(&state.db, guild_id),
        limits::count_guild_roles(&state.db, guild_id),
        limits::count_guild_emojis(&state.db, guild_id),
        limits::count_guild_bots(&state.db, guild_id),
        crate::pages::count_pages(&state.db, Some(guild_id)),
        crate::pages::get_effective_page_limit(
            &state.db,
            guild_id,
            state.config.max_pages_per_guild,
        ),
    );

    Ok(Json(GuildUsageStats {
        guild_id,
        plan,
        members: UsageStat {
            current: members?,
            limit: state.config.max_members_per_guild,
        },
        channels: UsageStat {
            current: channels?,
            limit: state.config.max_channels_per_guild,
        },
        roles: UsageStat {
            current: roles?,
            limit: state.config.max_roles_per_guild,
        },
        emojis: UsageStat {
            current: emojis?,
            limit: state.config.max_emojis_per_guild,
        },
        bots: UsageStat {
            current: bots?,
            limit: state.config.max_bots_per_guild,
        },
        pages: UsageStat {
            current: pages?,
            limit: page_limit.unwrap_or(state.config.max_pages_per_guild),
        },
    }))
}

/// Upload guild banner
#[utoipa::path(
    post,
    path = "/api/guilds/{id}/banner",
    tag = "guilds",
    params(("id" = Uuid, Path, description = "Guild ID")),
    request_body(content = String, content_type = "multipart/form-data"),
    responses(
        (status = 200, body = Guild),
        (status = 400, description = "Bad request (invalid file)"),
        (status = 403, description = "Forbidden (requires MANAGE_GUILD)"),
        (status = 413, description = "Payload too large")
    ),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state, multipart))]
pub async fn upload_guild_banner(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<Json<Guild>, GuildError> {
    // Check permission
    let _ctx =
        require_guild_permission(&state.db, guild_id, auth.id, GuildPermissions::MANAGE_GUILD)
            .await
            .map_err(|e| match e {
                PermissionError::NotGuildMember => GuildError::Forbidden,
                other => GuildError::Permission(other),
            })?;

    // Check if S3 is configured
    let s3 = state
        .s3
        .as_ref()
        .ok_or_else(|| GuildError::Internal("File storage not configured".to_string()))?;

    // Get the file from multipart
    let mut file_data = None;
    let mut filename = None;
    let mut content_type = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| GuildError::Internal(format!("Multipart error: {e}")))?
    {
        if field.name() == Some("banner") {
            filename = field.file_name().map(ToString::to_string);
            content_type = field.content_type().map(ToString::to_string);

            let data = field
                .bytes()
                .await
                .map_err(|e| GuildError::Internal(format!("Upload error: {e}")))?;

            file_data = Some(data);
            break;
        }
    }

    let data = file_data.ok_or(GuildError::Validation(
        "No banner file provided".to_string(),
    ))?;

    // Validate file size (using 5MB limit for banners)
    let max_size = 5 * 1024 * 1024;
    if data.len() > max_size {
        return Err(GuildError::Validation(format!(
            "Banner file too large ({}). Maximum size is {}",
            format_file_size(data.len()),
            format_file_size(max_size)
        )));
    }

    // Validate mime type
    let mime = content_type.unwrap_or_else(|| "application/octet-stream".to_string());
    if !mime.starts_with("image/") {
        return Err(GuildError::Validation("File must be an image".to_string()));
    }

    if mime.contains("svg") {
        return Err(GuildError::Validation(
            "SVG files are not allowed for banners".to_string(),
        ));
    }

    // Validate actual content format
    let detected_format = image::guess_format(&data).map_err(|_| {
        GuildError::Validation(
            "Unable to detect image format. File may be corrupted or not a valid image."
                .to_string(),
        )
    })?;

    match detected_format {
        image::ImageFormat::Png
        | image::ImageFormat::Jpeg
        | image::ImageFormat::Gif
        | image::ImageFormat::WebP => {}
        _ => {
            return Err(GuildError::Validation(format!(
                "Unsupported image format: {detected_format:?}. Only PNG, JPEG, GIF, and WebP are allowed."
            )));
        }
    }

    // Generate S3 key: guilds/{guild_id}/banner-{timestamp}_{filename}
    let timestamp = Utc::now().timestamp();
    let safe_filename = filename
        .unwrap_or_else(|| "banner.png".to_string())
        .replace(|c: char| !c.is_alphanumeric() && c != '.', "_");

    let key = format!("guilds/{guild_id}/banner-{timestamp}_{safe_filename}");

    s3.upload(&key, data.to_vec(), &mime)
        .await
        .map_err(|e| GuildError::Internal(format!("S3 upload failed: {e}")))?;

    // Store redirect URL — /api/files/ endpoint generates presigned URLs on-the-fly
    let url = crate::api::files::file_url(&key);

    // Update guild
    let updated_guild = sqlx::query_as::<_, Guild>(
        "UPDATE guilds SET banner_url = $1 WHERE id = $2 RETURNING id, name, owner_id, icon_url, description, threads_enabled, discoverable, tags, banner_url, plan, created_at"
    )
    .bind(&url)
    .bind(guild_id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(updated_guild))
}
