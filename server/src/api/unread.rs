//! Unread Aggregation API
//!
//! Provides endpoints for querying unread message counts across guilds and DMs.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use uuid::Uuid;

use super::AppState;
use crate::auth::AuthUser;
use crate::db;
use crate::ws::{broadcast_to_user, ServerEvent};

/// Get aggregate unread counts for the authenticated user.
///
/// Returns unread counts grouped by guild, plus DM unreads.
/// This is the primary endpoint for the Home unread dashboard.
///
/// # Route
/// `GET /api/me/unread`
///
/// # Authentication
/// Requires valid JWT token.
///
/// # Returns
/// - 200 OK: `UnreadAggregate` with guild and DM unread counts
/// - 500 Internal Server Error: Database error
#[utoipa::path(
    get,
    path = "/api/me/unread",
    tag = "unread",
    responses(
        (status = 200, description = "Unread message aggregate"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state))]
pub async fn get_unread_aggregate(
    auth_user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<db::UnreadAggregate>, (StatusCode, String)> {
    let aggregate = db::get_unread_aggregate(&state.db, auth_user.id)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, user_id = %auth_user.id, "Failed to fetch unread aggregate");
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch unread counts".to_string())
        })?;

    Ok(Json(aggregate))
}

/// Mark all messages as read (guilds + DMs).
///
/// Batch-marks all guild text channels and DM channels as read for the current user.
///
/// # Route
/// `POST /api/me/read-all`
#[utoipa::path(
    post,
    path = "/api/me/read-all",
    tag = "unread",
    responses(
        (status = 204, description = "All marked as read"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state))]
pub async fn mark_all_read(
    auth_user: AuthUser,
    State(state): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let now = chrono::Utc::now();

    // 1. Mark all guild text channels as read
    let guild_rows: Vec<(Uuid,)> = sqlx::query_as(
        r"INSERT INTO channel_read_state (user_id, channel_id, last_read_at, last_read_message_id)
          SELECT $1, c.id, $2, (
              SELECT m.id FROM messages m
              WHERE m.channel_id = c.id AND m.deleted_at IS NULL
              ORDER BY m.created_at DESC LIMIT 1
          )
          FROM channels c
          INNER JOIN guild_members gm ON gm.guild_id = c.guild_id AND gm.user_id = $1
          WHERE c.channel_type = 'text'
          ON CONFLICT (user_id, channel_id)
          DO UPDATE SET last_read_at = EXCLUDED.last_read_at, last_read_message_id = EXCLUDED.last_read_message_id
          RETURNING channel_id",
    )
    .bind(auth_user.id)
    .bind(now)
    .fetch_all(&state.db)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to bulk mark guild channels as read");
        (StatusCode::INTERNAL_SERVER_ERROR, "Failed to mark guild channels as read".to_string())
    })?;

    // 2. Mark all DM channels as read
    let dm_rows: Vec<(Uuid,)> = sqlx::query_as(
        r"INSERT INTO dm_read_state (user_id, channel_id, last_read_at, last_read_message_id)
          SELECT $1, dp.channel_id, $2, (
              SELECT m.id FROM messages m
              WHERE m.channel_id = dp.channel_id AND m.deleted_at IS NULL
              ORDER BY m.created_at DESC LIMIT 1
          )
          FROM dm_participants dp
          INNER JOIN channels c ON c.id = dp.channel_id
          WHERE dp.user_id = $1 AND c.channel_type = 'dm'
          ON CONFLICT (user_id, channel_id)
          DO UPDATE SET last_read_at = EXCLUDED.last_read_at, last_read_message_id = EXCLUDED.last_read_message_id
          RETURNING channel_id",
    )
    .bind(auth_user.id)
    .bind(now)
    .fetch_all(&state.db)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to bulk mark DMs as read");
        (StatusCode::INTERNAL_SERVER_ERROR, "Failed to mark DMs as read".to_string())
    })?;

    // 3. Broadcast events to user's other sessions
    for (channel_id,) in &guild_rows {
        let _ = broadcast_to_user(
            &state.redis,
            auth_user.id,
            &ServerEvent::ChannelRead {
                channel_id: *channel_id,
                last_read_message_id: None,
            },
        )
        .await;
    }

    for (channel_id,) in &dm_rows {
        let _ = broadcast_to_user(
            &state.redis,
            auth_user.id,
            &ServerEvent::DmRead {
                channel_id: *channel_id,
                last_read_message_id: None,
            },
        )
        .await;
    }

    Ok(StatusCode::NO_CONTENT)
}
