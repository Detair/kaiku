//! Message Reactions API
//!
//! Handlers for adding, removing, and listing message reactions.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::AuthUser;
use crate::db;
use crate::ws::{broadcast_to_channel, ServerEvent};

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct AddReactionRequest {
    pub emoji: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ReactionResponse {
    pub emoji: String,
    pub count: i64,
    pub me: bool,
}

#[derive(Debug, FromRow)]
struct ReactionRow {
    emoji: String,
    count: i64,
    user_reacted: bool,
}

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug, thiserror::Error)]
pub enum ReactionsError {
    #[error("Message not found")]
    MessageNotFound,
    #[error("Channel not found")]
    ChannelNotFound,
    #[error("Invalid emoji")]
    InvalidEmoji,
    #[error("Forbidden")]
    Forbidden,
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for ReactionsError {
    fn into_response(self) -> axum::response::Response {
        let (status, code, message) = match &self {
            Self::MessageNotFound => (
                StatusCode::NOT_FOUND,
                "MESSAGE_NOT_FOUND",
                "Message not found",
            ),
            Self::ChannelNotFound => (
                StatusCode::NOT_FOUND,
                "CHANNEL_NOT_FOUND",
                "Channel not found",
            ),
            Self::InvalidEmoji => (StatusCode::BAD_REQUEST, "INVALID_EMOJI", "Invalid emoji"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "FORBIDDEN", "Forbidden"),
            Self::Database(err) => {
                tracing::error!("Database error: {}", err);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    "Database error",
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

/// Add a reaction to a message.
/// PUT `/api/channels/:channel_id/messages/:message_id/reactions`
#[utoipa::path(
    put,
    path = "/api/channels/{channel_id}/messages/{message_id}/reactions",
    tag = "reactions",
    params(
        ("channel_id" = Uuid, Path, description = "Channel ID"),
        ("message_id" = Uuid, Path, description = "Message ID"),
    ),
    request_body = AddReactionRequest,
    responses(
        (status = 201, description = "Reaction added", body = ReactionResponse),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn add_reaction(
    State(state): State<AppState>,
    Path((channel_id, message_id)): Path<(Uuid, Uuid)>,
    auth_user: AuthUser,
    Json(req): Json<AddReactionRequest>,
) -> Result<impl IntoResponse, ReactionsError> {
    // Validate emoji length (max 64 chars for custom emoji IDs)
    if req.emoji.is_empty() || req.emoji.len() > 64 {
        return Err(ReactionsError::InvalidEmoji);
    }

    // Check channel exists
    let _ = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(ReactionsError::ChannelNotFound)?;

    // Check if user has VIEW_CHANNEL permission
    crate::permissions::require_channel_access(&state.db, auth_user.id, channel_id)
        .await
        .map_err(|_| ReactionsError::Forbidden)?;

    // Check message exists and belongs to channel
    let message = db::find_message_by_id(&state.db, message_id)
        .await?
        .ok_or(ReactionsError::MessageNotFound)?;

    if message.channel_id != channel_id {
        return Err(ReactionsError::MessageNotFound);
    }

    // Insert reaction + apply reaction-role effects atomically.
    let mut tx = state.db.begin().await?;

    let result = sqlx::query(
        r"
        INSERT INTO message_reactions (message_id, user_id, emoji)
        VALUES ($1, $2, $3)
        ON CONFLICT (message_id, user_id, emoji) DO NOTHING
        ",
    )
    .bind(message_id)
    .bind(auth_user.id)
    .bind(&req.emoji)
    .execute(&mut *tx)
    .await?;

    // Reaction-role hook (only for guild channels, only when newly inserted).
    let mut hook_outcome = None;
    if result.rows_affected() > 0 {
        let channel = db::find_channel_by_id(&state.db, channel_id)
            .await?
            .ok_or(ReactionsError::ChannelNotFound)?;
        if let Some(guild_id) = channel.guild_id {
            hook_outcome = crate::guild::reaction_roles::apply_on_reaction_add(
                &mut tx,
                guild_id,
                message_id,
                auth_user.id,
                &req.emoji,
            )
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "reaction-role hook (add) failed");
                ReactionsError::Database(sqlx::Error::Protocol("reaction-role hook".into()))
            })?
            .map(|o| (guild_id, o));
        }
    }

    // Get updated count inside the same transaction.
    let count: (i64,) = sqlx::query_as(
        r"
        SELECT COUNT(*) FROM message_reactions
        WHERE message_id = $1 AND emoji = $2
        ",
    )
    .bind(message_id)
    .bind(&req.emoji)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    // Broadcast after commit.
    if result.rows_affected() > 0 {
        if let Err(e) = broadcast_to_channel(
            &state.redis,
            channel_id,
            &ServerEvent::ReactionAdd {
                channel_id,
                message_id,
                user_id: auth_user.id,
                emoji: req.emoji.clone(),
            },
        )
        .await
        {
            tracing::warn!("Failed to broadcast reaction_add event: {}", e);
        }
    }

    if let Some((guild_id, outcome)) = hook_outcome {
        // Roles changed → members/admins update live.
        if let Err(e) = crate::ws::broadcast_to_guild(
            &state.redis,
            guild_id,
            &ServerEvent::MemberRolesUpdated {
                guild_id,
                user_id: auth_user.id,
                role_ids: outcome.role_ids,
            },
        )
        .await
        {
            tracing::warn!("Failed to broadcast member_roles_updated: {}", e);
        }
        // Cleared sibling reactions (unique swap) → tell clients to drop them.
        for cleared in outcome.cleared_emojis {
            if let Err(e) = broadcast_to_channel(
                &state.redis,
                channel_id,
                &ServerEvent::ReactionRemove {
                    channel_id,
                    message_id,
                    user_id: auth_user.id,
                    emoji: cleared,
                },
            )
            .await
            {
                tracing::warn!("Failed to broadcast cleared reaction: {}", e);
            }
        }
    }

    let status = if result.rows_affected() > 0 {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };

    Ok((
        status,
        Json(ReactionResponse {
            emoji: req.emoji,
            count: count.0,
            me: true,
        }),
    ))
}

/// Remove a reaction from a message.
/// DELETE `/api/channels/:channel_id/messages/:message_id/reactions/:emoji`
#[utoipa::path(
    delete,
    path = "/api/channels/{channel_id}/messages/{message_id}/reactions/{emoji}",
    tag = "reactions",
    params(
        ("channel_id" = Uuid, Path, description = "Channel ID"),
        ("message_id" = Uuid, Path, description = "Message ID"),
        ("emoji" = String, Path, description = "Emoji"),
    ),
    responses(
        (status = 204, description = "Reaction removed"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn remove_reaction(
    State(state): State<AppState>,
    Path((channel_id, message_id, emoji)): Path<(Uuid, Uuid, String)>,
    auth_user: AuthUser,
) -> Result<impl IntoResponse, ReactionsError> {
    // Check channel exists
    let _ = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(ReactionsError::ChannelNotFound)?;

    // Check if user has VIEW_CHANNEL permission
    crate::permissions::require_channel_access(&state.db, auth_user.id, channel_id)
        .await
        .map_err(|_| ReactionsError::Forbidden)?;

    // Check message exists and belongs to channel
    let message = db::find_message_by_id(&state.db, message_id)
        .await?
        .ok_or(ReactionsError::MessageNotFound)?;

    if message.channel_id != channel_id {
        return Err(ReactionsError::MessageNotFound);
    }

    let mut tx = state.db.begin().await?;
    let result = sqlx::query(
        r"
        DELETE FROM message_reactions
        WHERE message_id = $1 AND user_id = $2 AND emoji = $3
        ",
    )
    .bind(message_id)
    .bind(auth_user.id)
    .bind(&emoji)
    .execute(&mut *tx)
    .await?;

    let mut hook_outcome = None;
    if result.rows_affected() > 0 {
        if let Some(guild_id) = db::find_channel_by_id(&state.db, channel_id)
            .await?
            .and_then(|c| c.guild_id)
        {
            hook_outcome = crate::guild::reaction_roles::apply_on_reaction_remove(
                &mut tx,
                guild_id,
                message_id,
                auth_user.id,
                &emoji,
            )
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "reaction-role hook (remove) failed");
                ReactionsError::Database(sqlx::Error::Protocol("reaction-role hook".into()))
            })?
            .map(|o| (guild_id, o));
        }
    }

    tx.commit().await?;

    // Only broadcast if a row was actually deleted
    if result.rows_affected() > 0 {
        if let Err(e) = broadcast_to_channel(
            &state.redis,
            channel_id,
            &ServerEvent::ReactionRemove {
                channel_id,
                message_id,
                user_id: auth_user.id,
                emoji: emoji.clone(),
            },
        )
        .await
        {
            tracing::warn!("Failed to broadcast reaction_remove event: {}", e);
        }
    }

    if let Some((guild_id, outcome)) = hook_outcome {
        if let Err(e) = crate::ws::broadcast_to_guild(
            &state.redis,
            guild_id,
            &ServerEvent::MemberRolesUpdated {
                guild_id,
                user_id: auth_user.id,
                role_ids: outcome.role_ids,
            },
        )
        .await
        {
            tracing::warn!("Failed to broadcast member_roles_updated: {}", e);
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

/// Get reactions for a message.
/// GET `/api/channels/:channel_id/messages/:message_id/reactions`
#[utoipa::path(
    get,
    path = "/api/channels/{channel_id}/messages/{message_id}/reactions",
    tag = "reactions",
    params(
        ("channel_id" = Uuid, Path, description = "Channel ID"),
        ("message_id" = Uuid, Path, description = "Message ID"),
    ),
    responses(
        (status = 200, description = "List of reactions"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn get_reactions(
    State(state): State<AppState>,
    Path((channel_id, message_id)): Path<(Uuid, Uuid)>,
    auth_user: AuthUser,
) -> Result<impl IntoResponse, ReactionsError> {
    // Check channel exists
    let _ = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(ReactionsError::ChannelNotFound)?;

    // Check if user has VIEW_CHANNEL permission
    crate::permissions::require_channel_access(&state.db, auth_user.id, channel_id)
        .await
        .map_err(|_| ReactionsError::Forbidden)?;

    // Check message exists and belongs to channel
    let message = db::find_message_by_id(&state.db, message_id)
        .await?
        .ok_or(ReactionsError::MessageNotFound)?;

    if message.channel_id != channel_id {
        return Err(ReactionsError::MessageNotFound);
    }

    let reactions = sqlx::query_as::<_, ReactionRow>(
        r"
        SELECT
            emoji,
            COUNT(*) as count,
            BOOL_OR(user_id = $2) as user_reacted
        FROM message_reactions
        WHERE message_id = $1
        GROUP BY emoji
        ORDER BY MIN(created_at)
        ",
    )
    .bind(message_id)
    .bind(auth_user.id)
    .fetch_all(&state.db)
    .await?;

    let response: Vec<ReactionResponse> = reactions
        .into_iter()
        .map(|r| ReactionResponse {
            emoji: r.emoji,
            count: r.count,
            me: r.user_reacted,
        })
        .collect();

    Ok(Json(response))
}
