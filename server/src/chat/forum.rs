//! Forum channels: posts (thread starters) and per-channel tags.
//!
//! A forum post's root message lives in `messages` (`parent_id` NULL); replies
//! reuse the existing thread-reply endpoint. Creating a post is one transaction
//! (root message + `forum_posts` row + tag links). Moderation (pin/lock/delete)
//! needs `MANAGE_POSTS` or post authorship.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;
use validator::Validate;

use crate::api::AppState;
use crate::auth::AuthUser;
use crate::db::{self, ChannelType};
use crate::permissions::{require_guild_permission, GuildPermissions, PermissionError};

// ============================================================================
// Errors
// ============================================================================

#[derive(Debug, Error)]
pub enum ForumError {
    #[error("Channel is not a forum channel")]
    NotForum,
    #[error("Channel not found")]
    ChannelNotFound,
    #[error("Post not found")]
    PostNotFound,
    #[error("Tag not found")]
    TagNotFound,
    #[error("Not a member of this guild")]
    NotMember,
    #[error("{0}")]
    Permission(#[from] PermissionError),
    #[error("Validation failed: {0}")]
    Validation(String),
    #[error("Database error")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for ForumError {
    fn into_response(self) -> Response {
        if let Self::Database(e) = &self {
            tracing::error!(error = %e, "Forum database operation failed");
        }
        let (status, code, msg) = match &self {
            Self::NotForum => (StatusCode::BAD_REQUEST, "NOT_FORUM", self.to_string()),
            Self::ChannelNotFound => (StatusCode::NOT_FOUND, "CHANNEL_NOT_FOUND", self.to_string()),
            Self::PostNotFound => (StatusCode::NOT_FOUND, "POST_NOT_FOUND", self.to_string()),
            Self::TagNotFound => (StatusCode::NOT_FOUND, "TAG_NOT_FOUND", self.to_string()),
            Self::NotMember => (StatusCode::FORBIDDEN, "NOT_MEMBER", self.to_string()),
            Self::Permission(_) => (StatusCode::FORBIDDEN, "PERMISSION_DENIED", self.to_string()),
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
            Json(serde_json::json!({ "error": code, "message": msg })),
        )
            .into_response()
    }
}

fn map_perm(e: PermissionError) -> ForumError {
    match e {
        PermissionError::NotGuildMember => ForumError::NotMember,
        other => ForumError::Permission(other),
    }
}

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct CreatePostRequest {
    #[validate(length(min = 1, max = 128))]
    pub title: String,
    #[validate(length(min = 1, max = 8000))]
    pub content: String,
    #[serde(default)]
    pub tag_ids: Vec<Uuid>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdatePostRequest {
    pub pinned: Option<bool>,
    pub locked: Option<bool>,
}

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct CreateTagRequest {
    #[validate(length(min = 1, max = 32))]
    pub name: String,
    #[validate(length(max = 64))]
    pub emoji: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ForumPostResponse {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub root_message_id: Uuid,
    pub title: String,
    pub author_id: Option<Uuid>,
    pub pinned: bool,
    pub locked: bool,
    pub reply_count: i32,
    pub tag_ids: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
    pub last_activity_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ForumTagResponse {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub name: String,
    pub emoji: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListPostsQuery {
    pub tag: Option<Uuid>,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

const fn default_limit() -> i64 {
    50
}

// ============================================================================
// Helpers
// ============================================================================

/// Fetch a channel, assert it is a forum, and return its `guild_id`.
async fn forum_guild_id(state: &AppState, channel_id: Uuid) -> Result<Uuid, ForumError> {
    let channel = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(ForumError::ChannelNotFound)?;
    if channel.channel_type != ChannelType::Forum {
        return Err(ForumError::NotForum);
    }
    channel.guild_id.ok_or(ForumError::ChannelNotFound)
}

async fn tag_ids_for_post(state: &AppState, post_id: Uuid) -> Result<Vec<Uuid>, ForumError> {
    let rows: Vec<(Uuid,)> =
        sqlx::query_as("SELECT tag_id FROM forum_post_tags WHERE post_id = $1")
            .bind(post_id)
            .fetch_all(&state.db)
            .await?;
    Ok(rows.into_iter().map(|(t,)| t).collect())
}

// ============================================================================
// Post handlers
// ============================================================================

/// `POST /api/channels/{id}/posts` — create a forum post.
#[tracing::instrument(skip(state, body))]
pub async fn create_post(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
    Json(body): Json<CreatePostRequest>,
) -> Result<(StatusCode, Json<ForumPostResponse>), ForumError> {
    body.validate()
        .map_err(|e| ForumError::Validation(crate::validation::format_validation_errors(&e)))?;

    let guild_id = forum_guild_id(&state, channel_id).await?;
    require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        GuildPermissions::SEND_MESSAGES,
    )
    .await
    .map_err(map_perm)?;

    let mut tx = state.db.begin().await?;

    // Root message opens the thread (parent_id NULL).
    let root: (Uuid, DateTime<Utc>) = sqlx::query_as(
        "INSERT INTO messages (channel_id, user_id, content, encrypted)
         VALUES ($1, $2, $3, false) RETURNING id, created_at",
    )
    .bind(channel_id)
    .bind(auth.id)
    .bind(&body.content)
    .fetch_one(&mut *tx)
    .await?;

    let post: (Uuid, DateTime<Utc>, DateTime<Utc>) = sqlx::query_as(
        "INSERT INTO forum_posts (channel_id, root_message_id, title, author_id)
         VALUES ($1, $2, $3, $4)
         RETURNING id, created_at, last_activity_at",
    )
    .bind(channel_id)
    .bind(root.0)
    .bind(&body.title)
    .bind(auth.id)
    .fetch_one(&mut *tx)
    .await?;

    // Link tags that belong to this channel (silently ignore foreign tag ids).
    for tag_id in &body.tag_ids {
        sqlx::query(
            "INSERT INTO forum_post_tags (post_id, tag_id)
             SELECT $1, $2 WHERE EXISTS (
                 SELECT 1 FROM forum_tags WHERE id = $2 AND channel_id = $3
             )
             ON CONFLICT DO NOTHING",
        )
        .bind(post.0)
        .bind(tag_id)
        .bind(channel_id)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    let tag_ids = tag_ids_for_post(&state, post.0).await?;
    let response = ForumPostResponse {
        id: post.0,
        channel_id,
        root_message_id: root.0,
        title: body.title,
        author_id: Some(auth.id),
        pinned: false,
        locked: false,
        reply_count: 0,
        tag_ids,
        created_at: post.1,
        last_activity_at: post.2,
    };

    // Live update for the card list.
    if let Err(e) = crate::ws::broadcast_to_channel(
        &state.redis,
        channel_id,
        &crate::ws::ServerEvent::ForumPostCreated {
            channel_id,
            post: serde_json::to_value(&response).unwrap_or_default(),
        },
    )
    .await
    {
        tracing::warn!("Failed to broadcast forum_post_created: {e}");
    }

    Ok((StatusCode::CREATED, Json(response)))
}

/// `GET /api/channels/{id}/posts` — list posts (recent activity, optional tag).
#[tracing::instrument(skip(state))]
pub async fn list_posts(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
    Query(q): Query<ListPostsQuery>,
) -> Result<Json<Vec<ForumPostResponse>>, ForumError> {
    let guild_id = forum_guild_id(&state, channel_id).await?;
    require_guild_permission(&state.db, guild_id, auth.id, GuildPermissions::empty())
        .await
        .map_err(map_perm)?;

    let limit = q.limit.clamp(1, 100);
    let rows: Vec<(
        Uuid,
        Uuid,
        String,
        Option<Uuid>,
        bool,
        bool,
        DateTime<Utc>,
        DateTime<Utc>,
        Option<i32>,
    )> = sqlx::query_as(
        "SELECT p.id, p.root_message_id, p.title, p.author_id, p.pinned, p.locked,
                p.created_at, p.last_activity_at, m.thread_reply_count
         FROM forum_posts p
         JOIN messages m ON m.id = p.root_message_id
         WHERE p.channel_id = $1
           AND ($2::uuid IS NULL OR EXISTS (
                 SELECT 1 FROM forum_post_tags pt WHERE pt.post_id = p.id AND pt.tag_id = $2))
         ORDER BY p.pinned DESC, p.last_activity_at DESC
         LIMIT $3",
    )
    .bind(channel_id)
    .bind(q.tag)
    .bind(limit)
    .fetch_all(&state.db)
    .await?;

    let mut posts = Vec::with_capacity(rows.len());
    for r in rows {
        let tag_ids = tag_ids_for_post(&state, r.0).await?;
        posts.push(ForumPostResponse {
            id: r.0,
            channel_id,
            root_message_id: r.1,
            title: r.2,
            author_id: r.3,
            pinned: r.4,
            locked: r.5,
            reply_count: r.8.unwrap_or(0),
            tag_ids,
            created_at: r.6,
            last_activity_at: r.7,
        });
    }
    Ok(Json(posts))
}

/// `PATCH /api/forum/posts/{post_id}` — pin/lock (moderation or author).
#[tracing::instrument(skip(state, body))]
pub async fn update_post(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(post_id): Path<Uuid>,
    Json(body): Json<UpdatePostRequest>,
) -> Result<Json<serde_json::Value>, ForumError> {
    let (channel_id, author_id): (Uuid, Option<Uuid>) =
        sqlx::query_as("SELECT channel_id, author_id FROM forum_posts WHERE id = $1")
            .bind(post_id)
            .fetch_optional(&state.db)
            .await?
            .ok_or(ForumError::PostNotFound)?;

    let guild_id = forum_guild_id(&state, channel_id).await?;
    // Pin/lock is a moderation action → MANAGE_POSTS (author may lock their own).
    let ctx = require_guild_permission(&state.db, guild_id, auth.id, GuildPermissions::empty())
        .await
        .map_err(map_perm)?;
    let is_author = author_id == Some(auth.id);
    let can_manage = ctx.is_owner || ctx.computed_permissions.has(GuildPermissions::MANAGE_POSTS);
    // Pinning is always moderation; locking may be done by the author too.
    if body.pinned.is_some() && !can_manage {
        return Err(ForumError::Permission(PermissionError::MissingPermission(
            GuildPermissions::MANAGE_POSTS,
        )));
    }
    if body.locked.is_some() && !can_manage && !is_author {
        return Err(ForumError::Permission(PermissionError::MissingPermission(
            GuildPermissions::MANAGE_POSTS,
        )));
    }

    sqlx::query(
        "UPDATE forum_posts SET
            pinned = COALESCE($2, pinned),
            locked = COALESCE($3, locked)
         WHERE id = $1",
    )
    .bind(post_id)
    .bind(body.pinned)
    .bind(body.locked)
    .execute(&state.db)
    .await?;

    Ok(Json(serde_json::json!({ "updated": true, "id": post_id })))
}

/// `DELETE /api/forum/posts/{post_id}` — delete (moderation or author).
#[tracing::instrument(skip(state))]
pub async fn delete_post(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(post_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ForumError> {
    let (channel_id, author_id, root_message_id): (Uuid, Option<Uuid>, Uuid) = sqlx::query_as(
        "SELECT channel_id, author_id, root_message_id FROM forum_posts WHERE id = $1",
    )
    .bind(post_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ForumError::PostNotFound)?;

    let guild_id = forum_guild_id(&state, channel_id).await?;
    let ctx = require_guild_permission(&state.db, guild_id, auth.id, GuildPermissions::empty())
        .await
        .map_err(map_perm)?;
    let can = ctx.is_owner
        || ctx.computed_permissions.has(GuildPermissions::MANAGE_POSTS)
        || author_id == Some(auth.id);
    if !can {
        return Err(ForumError::Permission(PermissionError::MissingPermission(
            GuildPermissions::MANAGE_POSTS,
        )));
    }

    // Deleting the root message cascades the forum_posts row + tags + replies.
    sqlx::query("DELETE FROM messages WHERE id = $1")
        .bind(root_message_id)
        .execute(&state.db)
        .await?;

    Ok(Json(serde_json::json!({ "deleted": true, "id": post_id })))
}

// ============================================================================
// Tag handlers
// ============================================================================

/// `GET /api/channels/{id}/tags` — list a forum channel's tags.
#[tracing::instrument(skip(state))]
pub async fn list_tags(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<Json<Vec<ForumTagResponse>>, ForumError> {
    let guild_id = forum_guild_id(&state, channel_id).await?;
    require_guild_permission(&state.db, guild_id, auth.id, GuildPermissions::empty())
        .await
        .map_err(map_perm)?;

    let rows: Vec<(Uuid, String, Option<String>)> = sqlx::query_as(
        "SELECT id, name, emoji FROM forum_tags WHERE channel_id = $1 ORDER BY name",
    )
    .bind(channel_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, name, emoji)| ForumTagResponse {
                id,
                channel_id,
                name,
                emoji,
            })
            .collect(),
    ))
}

/// `POST /api/channels/{id}/tags` — create a tag (`MANAGE_POSTS`).
#[tracing::instrument(skip(state, body))]
pub async fn create_tag(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
    Json(body): Json<CreateTagRequest>,
) -> Result<(StatusCode, Json<ForumTagResponse>), ForumError> {
    body.validate()
        .map_err(|e| ForumError::Validation(crate::validation::format_validation_errors(&e)))?;
    let guild_id = forum_guild_id(&state, channel_id).await?;
    require_guild_permission(&state.db, guild_id, auth.id, GuildPermissions::MANAGE_POSTS)
        .await
        .map_err(map_perm)?;

    let row: (Uuid,) = sqlx::query_as(
        "INSERT INTO forum_tags (channel_id, name, emoji) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(channel_id)
    .bind(&body.name)
    .bind(&body.emoji)
    .fetch_one(&state.db)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db) if db.is_unique_violation() => {
            ForumError::Validation("A tag with that name already exists".to_string())
        }
        other => ForumError::Database(other),
    })?;

    Ok((
        StatusCode::CREATED,
        Json(ForumTagResponse {
            id: row.0,
            channel_id,
            name: body.name,
            emoji: body.emoji,
        }),
    ))
}

/// `DELETE /api/channels/{id}/tags/{tag_id}` — delete a tag (`MANAGE_POSTS`).
#[tracing::instrument(skip(state))]
pub async fn delete_tag(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((channel_id, tag_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ForumError> {
    let guild_id = forum_guild_id(&state, channel_id).await?;
    require_guild_permission(&state.db, guild_id, auth.id, GuildPermissions::MANAGE_POSTS)
        .await
        .map_err(map_perm)?;

    let affected = sqlx::query("DELETE FROM forum_tags WHERE id = $1 AND channel_id = $2")
        .bind(tag_id)
        .bind(channel_id)
        .execute(&state.db)
        .await?
        .rows_affected();
    if affected == 0 {
        return Err(ForumError::TagNotFound);
    }
    Ok(Json(serde_json::json!({ "deleted": true, "id": tag_id })))
}
