//! Channel Pins API
//!
//! Handlers for listing, adding, and removing pinned messages in channels.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

use super::messages;
use super::types::MessageResponse;
use crate::api::AppState;
use crate::auth::AuthUser;
use crate::db;
use crate::permissions::GuildPermissions;
use crate::ws::{broadcast_to_channel, ServerEvent};

// ============================================================================
// Constants
// ============================================================================

/// Maximum number of pinned messages per channel.
const MAX_PINS_PER_CHANNEL: i64 = 50;

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, FromRow)]
struct PinRow {
    #[allow(dead_code)]
    id: Uuid,
    message_id: Uuid,
    pinned_by: Uuid,
    pinned_at: DateTime<Utc>,
}

/// Response for a single pinned message with pin metadata.
#[derive(Debug, Serialize)]
pub struct PinnedMessageResponse {
    /// The full message object.
    pub message: MessageResponse,
    /// User who pinned the message.
    pub pinned_by: Uuid,
    /// When the message was pinned.
    pub pinned_at: DateTime<Utc>,
}

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug, thiserror::Error)]
pub enum ChannelPinsError {
    #[error("Message not found")]
    MessageNotFound,
    #[error("Channel not found")]
    ChannelNotFound,
    #[error("Not a guild channel")]
    NotGuildChannel,
    #[error("Pin limit reached (max {MAX_PINS_PER_CHANNEL} per channel)")]
    PinLimitReached,
    #[error("Forbidden")]
    Forbidden,
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for ChannelPinsError {
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
            Self::NotGuildChannel => (
                StatusCode::BAD_REQUEST,
                "NOT_GUILD_CHANNEL",
                "Pins are only supported in guild channels",
            ),
            Self::PinLimitReached => (
                StatusCode::CONFLICT,
                "PIN_LIMIT_REACHED",
                "This channel has reached the maximum of 50 pins",
            ),
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

/// List pinned messages in a channel.
/// GET `/api/channels/:channel_id/pins`
pub async fn list_channel_pins(
    State(state): State<AppState>,
    Path(channel_id): Path<Uuid>,
    auth_user: AuthUser,
) -> Result<impl IntoResponse, ChannelPinsError> {
    // Check channel exists
    let _ = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(ChannelPinsError::ChannelNotFound)?;

    // Check VIEW_CHANNEL permission
    crate::permissions::require_channel_access(&state.db, auth_user.id, channel_id)
        .await
        .map_err(|_| ChannelPinsError::Forbidden)?;

    // Fetch pin rows ordered by most recent first
    let pin_rows = sqlx::query_as::<_, PinRow>(
        "SELECT id, message_id, pinned_by, pinned_at FROM channel_pins WHERE channel_id = $1 ORDER BY pinned_at DESC",
    )
    .bind(channel_id)
    .fetch_all(&state.db)
    .await?;

    if pin_rows.is_empty() {
        return Ok(Json(Vec::<PinnedMessageResponse>::new()));
    }

    let message_ids: Vec<Uuid> = pin_rows.iter().map(|p| p.message_id).collect();

    // Fetch all pinned messages
    let messages = sqlx::query_as::<_, db::Message>(
        "SELECT * FROM messages WHERE id = ANY($1) AND deleted_at IS NULL",
    )
    .bind(&message_ids)
    .fetch_all(&state.db)
    .await?;

    // Build full message responses with author info, attachments, reactions
    let message_responses = messages::build_message_responses(&state.db, auth_user.id, messages)
        .await
        .map_err(|_| ChannelPinsError::Database(sqlx::Error::RowNotFound))?;

    // Index by message ID for fast lookup
    let msg_map: std::collections::HashMap<Uuid, MessageResponse> =
        message_responses.into_iter().map(|m| (m.id, m)).collect();

    // Build responses in pin order, skipping any deleted messages
    let responses: Vec<PinnedMessageResponse> = pin_rows
        .into_iter()
        .filter_map(|pin| {
            msg_map.get(&pin.message_id).map(|_| {
                // We need to remove from map but it's borrowed; collect first
                PinnedMessageResponse {
                    // Safe: we just checked it exists
                    message: msg_map.get(&pin.message_id).unwrap().clone(),
                    pinned_by: pin.pinned_by,
                    pinned_at: pin.pinned_at,
                }
            })
        })
        .collect();

    Ok(Json(responses))
}

/// Pin a message in a channel.
/// PUT `/api/channels/:channel_id/messages/:message_id/pin`
pub async fn pin_message(
    State(state): State<AppState>,
    Path((channel_id, message_id)): Path<(Uuid, Uuid)>,
    auth_user: AuthUser,
) -> Result<impl IntoResponse, ChannelPinsError> {
    // Check channel exists and get guild_id
    let channel = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(ChannelPinsError::ChannelNotFound)?;

    let guild_id = channel.guild_id.ok_or(ChannelPinsError::NotGuildChannel)?;

    // Check PIN_MESSAGES permission
    crate::permissions::require_guild_permission(
        &state.db,
        guild_id,
        auth_user.id,
        GuildPermissions::PIN_MESSAGES,
    )
    .await
    .map_err(|_| ChannelPinsError::Forbidden)?;

    // Verify message exists, belongs to this channel, and is not deleted
    let message = db::find_message_by_id(&state.db, message_id)
        .await?
        .ok_or(ChannelPinsError::MessageNotFound)?;

    if message.channel_id != channel_id {
        return Err(ChannelPinsError::MessageNotFound);
    }

    // Check pin count limit
    let pin_count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM channel_pins WHERE channel_id = $1")
            .bind(channel_id)
            .fetch_one(&state.db)
            .await?;

    if pin_count.0 >= MAX_PINS_PER_CHANNEL {
        return Err(ChannelPinsError::PinLimitReached);
    }

    // Insert pin (idempotent with ON CONFLICT DO NOTHING)
    let result = sqlx::query(
        r"
        INSERT INTO channel_pins (channel_id, message_id, pinned_by)
        VALUES ($1, $2, $3)
        ON CONFLICT (channel_id, message_id) DO NOTHING
        ",
    )
    .bind(channel_id)
    .bind(message_id)
    .bind(auth_user.id)
    .execute(&state.db)
    .await?;

    // Only broadcast and create system message if this was a new pin
    if result.rows_affected() > 0 {
        let pinned_at = Utc::now();

        // Insert system message and fetch it back for broadcasting
        let sys_msg = sqlx::query_as::<_, db::Message>(
            r"
            INSERT INTO messages (channel_id, user_id, content, message_type)
            VALUES ($1, $2, $3, 'system')
            RETURNING *
            ",
        )
        .bind(channel_id)
        .bind(auth_user.id)
        .bind("pinned a message to this channel.")
        .fetch_one(&state.db)
        .await?;

        // Broadcast system message as MessageNew
        let sys_responses =
            messages::build_message_responses(&state.db, auth_user.id, vec![sys_msg])
                .await
                .unwrap_or_default();
        if let Some(sys_response) = sys_responses.into_iter().next() {
            let message_json = serde_json::to_value(&sys_response).unwrap_or_default();
            if let Err(e) = broadcast_to_channel(
                &state.redis,
                channel_id,
                &ServerEvent::MessageNew {
                    channel_id,
                    message: message_json,
                },
            )
            .await
            {
                tracing::warn!(
                    channel_id = %channel_id,
                    error = %e,
                    "Failed to broadcast system message for pin"
                );
            }
        }

        // Broadcast pin event
        if let Err(e) = broadcast_to_channel(
            &state.redis,
            channel_id,
            &ServerEvent::ChannelPinAdded {
                channel_id,
                message_id,
                pinned_by: auth_user.id,
                pinned_at: pinned_at.to_rfc3339(),
            },
        )
        .await
        {
            tracing::warn!(
                channel_id = %channel_id,
                message_id = %message_id,
                error = %e,
                "Failed to broadcast channel_pin_added event"
            );
        }
    }

    Ok(StatusCode::OK)
}

/// Unpin a message from a channel.
/// DELETE `/api/channels/:channel_id/messages/:message_id/pin`
pub async fn unpin_message(
    State(state): State<AppState>,
    Path((channel_id, message_id)): Path<(Uuid, Uuid)>,
    auth_user: AuthUser,
) -> Result<impl IntoResponse, ChannelPinsError> {
    // Check channel exists and get guild_id
    let channel = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(ChannelPinsError::ChannelNotFound)?;

    let guild_id = channel.guild_id.ok_or(ChannelPinsError::NotGuildChannel)?;

    // Check PIN_MESSAGES permission
    crate::permissions::require_guild_permission(
        &state.db,
        guild_id,
        auth_user.id,
        GuildPermissions::PIN_MESSAGES,
    )
    .await
    .map_err(|_| ChannelPinsError::Forbidden)?;

    // Delete the pin
    let result = sqlx::query("DELETE FROM channel_pins WHERE channel_id = $1 AND message_id = $2")
        .bind(channel_id)
        .bind(message_id)
        .execute(&state.db)
        .await?;

    // Only broadcast if a pin was actually removed
    if result.rows_affected() == 0 {
        return Ok(StatusCode::NO_CONTENT);
    }

    // Broadcast unpin event
    if let Err(e) = broadcast_to_channel(
        &state.redis,
        channel_id,
        &ServerEvent::ChannelPinRemoved {
            channel_id,
            message_id,
        },
    )
    .await
    {
        tracing::warn!(
            channel_id = %channel_id,
            message_id = %message_id,
            error = %e,
            "Failed to broadcast channel_pin_removed event"
        );
    }

    Ok(StatusCode::NO_CONTENT)
}
