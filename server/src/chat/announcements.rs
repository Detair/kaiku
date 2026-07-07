//! Announcement channels: follow a source announcement channel so its posts
//! crosspost into a channel in the follower's guild.
//!
//! Publishing in an announcement channel is gated by `SEND_ANNOUNCEMENTS` (see
//! the message-create handler). On publish, a fan-out copies the message into
//! every follower's target channel. Crossposts are flagged (`is_crosspost`) so
//! they are never re-crossposted (loop guard).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::AuthUser;
use crate::db::{self, ChannelType};
use crate::permissions::{require_guild_permission, GuildPermissions, PermissionError};

#[derive(Debug, Error)]
pub enum AnnouncementError {
    #[error("Channel not found")]
    ChannelNotFound,
    #[error("Source is not an announcement channel")]
    NotAnnouncement,
    #[error("Already following this channel")]
    AlreadyFollowing,
    #[error("Follow not found")]
    FollowNotFound,
    #[error("Cannot view the source channel")]
    SourceForbidden,
    #[error("{0}")]
    Permission(#[from] PermissionError),
    #[error("Database error")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for AnnouncementError {
    fn into_response(self) -> Response {
        if let Self::Database(e) = &self {
            tracing::error!(error = %e, "Announcement database operation failed");
        }
        let (status, code, msg) = match &self {
            Self::ChannelNotFound => (StatusCode::NOT_FOUND, "CHANNEL_NOT_FOUND", self.to_string()),
            Self::NotAnnouncement => (
                StatusCode::BAD_REQUEST,
                "NOT_ANNOUNCEMENT",
                self.to_string(),
            ),
            Self::AlreadyFollowing => (StatusCode::CONFLICT, "ALREADY_FOLLOWING", self.to_string()),
            Self::FollowNotFound => (StatusCode::NOT_FOUND, "FOLLOW_NOT_FOUND", self.to_string()),
            Self::SourceForbidden => (StatusCode::FORBIDDEN, "SOURCE_FORBIDDEN", self.to_string()),
            Self::Permission(_) => (StatusCode::FORBIDDEN, "PERMISSION_DENIED", self.to_string()),
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

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct FollowRequest {
    pub source_channel_id: Uuid,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct FollowResponse {
    pub id: Uuid,
    pub source_channel_id: Uuid,
    pub target_channel_id: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct FollowerInfo {
    pub id: Uuid,
    pub target_channel_id: Uuid,
}

async fn channel_guild(state: &AppState, channel_id: Uuid) -> Result<Uuid, AnnouncementError> {
    db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(AnnouncementError::ChannelNotFound)?
        .guild_id
        .ok_or(AnnouncementError::ChannelNotFound)
}

/// `POST /api/channels/{target}/follow` — follow a source announcement channel.
#[tracing::instrument(skip(state, body))]
pub async fn follow_channel(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(target_channel_id): Path<Uuid>,
    Json(body): Json<FollowRequest>,
) -> Result<(StatusCode, Json<FollowResponse>), AnnouncementError> {
    // The source must be an announcement channel and the actor must be able to
    // view it.
    let source = db::find_channel_by_id(&state.db, body.source_channel_id)
        .await?
        .ok_or(AnnouncementError::ChannelNotFound)?;
    if source.channel_type != ChannelType::Announcement {
        return Err(AnnouncementError::NotAnnouncement);
    }
    if crate::permissions::require_channel_access(&state.db, auth.id, body.source_channel_id)
        .await
        .is_err()
    {
        return Err(AnnouncementError::SourceForbidden);
    }

    // The actor must manage channels in the FOLLOWER (target) guild.
    let target_guild = channel_guild(&state, target_channel_id).await?;
    require_guild_permission(
        &state.db,
        target_guild,
        auth.id,
        GuildPermissions::MANAGE_CHANNELS,
    )
    .await?;

    let row: Option<(Uuid, DateTime<Utc>)> = sqlx::query_as(
        "INSERT INTO announcement_follows (source_channel_id, target_channel_id, created_by)
         VALUES ($1, $2, $3)
         ON CONFLICT (source_channel_id, target_channel_id) DO NOTHING
         RETURNING id, created_at",
    )
    .bind(body.source_channel_id)
    .bind(target_channel_id)
    .bind(auth.id)
    .fetch_optional(&state.db)
    .await?;

    let (id, created_at) = row.ok_or(AnnouncementError::AlreadyFollowing)?;
    Ok((
        StatusCode::CREATED,
        Json(FollowResponse {
            id,
            source_channel_id: body.source_channel_id,
            target_channel_id,
            created_at,
        }),
    ))
}

/// `DELETE /api/channels/{target}/follow/{id}` — stop following.
#[tracing::instrument(skip(state))]
pub async fn unfollow_channel(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((target_channel_id, follow_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, AnnouncementError> {
    let target_guild = channel_guild(&state, target_channel_id).await?;
    require_guild_permission(
        &state.db,
        target_guild,
        auth.id,
        GuildPermissions::MANAGE_CHANNELS,
    )
    .await?;

    let affected =
        sqlx::query("DELETE FROM announcement_follows WHERE id = $1 AND target_channel_id = $2")
            .bind(follow_id)
            .bind(target_channel_id)
            .execute(&state.db)
            .await?
            .rows_affected();
    if affected == 0 {
        return Err(AnnouncementError::FollowNotFound);
    }
    Ok(Json(
        serde_json::json!({ "unfollowed": true, "id": follow_id }),
    ))
}

/// `GET /api/channels/{source}/followers` — list followers of a source channel.
#[tracing::instrument(skip(state))]
pub async fn list_followers(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(source_channel_id): Path<Uuid>,
) -> Result<Json<Vec<FollowerInfo>>, AnnouncementError> {
    // Must manage channels in the source's guild to see its followers.
    let source_guild = channel_guild(&state, source_channel_id).await?;
    require_guild_permission(
        &state.db,
        source_guild,
        auth.id,
        GuildPermissions::MANAGE_CHANNELS,
    )
    .await?;

    let rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT id, target_channel_id FROM announcement_follows WHERE source_channel_id = $1",
    )
    .bind(source_channel_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, target_channel_id)| FollowerInfo {
                id,
                target_channel_id,
            })
            .collect(),
    ))
}

/// Fan out an announcement to all follower target channels.
///
/// Spawned after a publish so slow/many followers can't block the publisher.
/// Each crosspost is flagged `is_crosspost = true` so it is never re-crossposted.
pub fn spawn_crosspost(state: AppState, source_channel_id: Uuid, content: String) {
    tokio::spawn(async move {
        let follows: Vec<(Uuid,)> = match sqlx::query_as(
            "SELECT target_channel_id FROM announcement_follows WHERE source_channel_id = $1",
        )
        .bind(source_channel_id)
        .fetch_all(&state.db)
        .await
        {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!(error = %e, "crosspost: failed to load follows");
                return;
            }
        };

        for (target,) in follows {
            // Insert a crosspost copy attributed to no author (system-forwarded).
            let inserted: Result<(Uuid,), _> = sqlx::query_as(
                "INSERT INTO messages (channel_id, content, encrypted, is_crosspost, message_type)
                 VALUES ($1, $2, false, true, 'user') RETURNING id",
            )
            .bind(target)
            .bind(&content)
            .fetch_one(&state.db)
            .await;
            let Ok((message_id,)) = inserted else {
                tracing::warn!("crosspost: insert failed for target {target}");
                continue;
            };
            // Broadcast so the target channel sees it live (arrives as MessageNew).
            let payload = serde_json::json!({
                "id": message_id,
                "channel_id": target,
                "content": content,
                "is_crosspost": true,
            });
            if let Err(e) = crate::ws::broadcast_to_channel(
                &state.redis,
                target,
                &crate::ws::ServerEvent::MessageNew {
                    channel_id: target,
                    message: payload,
                },
            )
            .await
            {
                tracing::warn!(error = %e, "crosspost: broadcast failed");
            }
        }
    });
}
