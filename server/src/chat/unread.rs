//! Unread Aggregation API
//!
//! Provides endpoints for querying unread message counts across guilds and DMs.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::AuthUser;
use crate::db;
use crate::ws::{broadcast_to_user, ServerEvent};

/// SQL text of [`UNREAD_MESSAGE_PREDICATE`], usable inside `concat!`.
macro_rules! unread_message_predicate {
    () => {
        "m.deleted_at IS NULL AND m.parent_id IS NULL AND m.user_id IS DISTINCT FROM $1"
    };
}
pub(crate) use unread_message_predicate;

/// SQL text of [`TEXT_LIKE_CHANNEL_TYPES`], usable inside `concat!`.
macro_rules! text_like_channel_types {
    () => {
        "('text', 'forum', 'announcement')"
    };
}
pub(crate) use text_like_channel_types;

/// Which messages count as unread, shared by every unread query.
///
/// A message is unread for the user bound as `$1` (aliased `m`) only if it is
/// not deleted, is not a thread reply (threads track their own read state) and
/// was written by someone else. Each query adds its own lower bound: the read
/// cursor's message time, or the user's join time when there is no cursor.
/// The first two conditions match the partial index
/// `idx_messages_channel_toplevel`.
///
/// Queries embed it at compile time with `concat!` through
/// [`unread_message_predicate!`], so the SQL stays a `&'static str`.
pub const UNREAD_MESSAGE_PREDICATE: &str = unread_message_predicate!();

/// SQL list of the read-tracked guild channel types, for `channel_type IN ...`
/// (the SQL side of [`db::ChannelType::is_text_like`]); embedded with
/// [`text_like_channel_types!`].
pub const TEXT_LIKE_CHANNEL_TYPES: &str = text_like_channel_types!();

/// Get aggregate unread counts for the authenticated user.
///
/// Returns unread counts grouped by guild, plus DM unreads. Every channel entry
/// also carries `last_read_message_id`, the user's server-side read cursor for
/// that channel (`null` when it was never read), so a client can notify only for
/// messages newer than the cursor.
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
        (status = 200, description = "Unread message aggregate; each channel entry has `unread_count` and `last_read_message_id` (the read cursor, `null` if never read)", body = db::UnreadAggregate),
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
/// Batch-marks all guild text, forum and announcement channels and DM channels as read for the
/// current user.
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

    // 1. Mark all read-tracked guild channels as read (cursor = newest top-level message, the same
    //    message the channel list reports).
    let guild_sql = concat!(
        r"INSERT INTO channel_read_state (user_id, channel_id, last_read_at, last_read_message_id)
          SELECT $1, c.id, $2, (
              SELECT m.id FROM messages m
              WHERE m.channel_id = c.id AND m.deleted_at IS NULL AND m.parent_id IS NULL
              ORDER BY m.created_at DESC LIMIT 1
          )
          FROM channels c
          INNER JOIN guild_members gm ON gm.guild_id = c.guild_id AND gm.user_id = $1
          WHERE c.channel_type IN ",
        crate::chat::unread::text_like_channel_types!(),
        r"
          ON CONFLICT (user_id, channel_id)
          DO UPDATE SET last_read_at = EXCLUDED.last_read_at, last_read_message_id = EXCLUDED.last_read_message_id
          RETURNING channel_id, last_read_message_id"
    );
    let guild_rows: Vec<(Uuid, Option<Uuid>)> = sqlx::query_as(guild_sql)
        .bind(auth_user.id)
        .bind(now)
        .fetch_all(&state.db)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to bulk mark guild channels as read");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to mark guild channels as read".to_string(),
            )
        })?;

    // 2. Mark all DM channels as read
    let dm_rows: Vec<(Uuid, Option<Uuid>)> = sqlx::query_as(
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
          RETURNING channel_id, last_read_message_id",
    )
    .bind(auth_user.id)
    .bind(now)
    .fetch_all(&state.db)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to bulk mark DMs as read");
        (StatusCode::INTERNAL_SERVER_ERROR, "Failed to mark DMs as read".to_string())
    })?;

    // 3. Clear phone notifications (C20 `read`; only channels with a prior wake push anything) and
    //    broadcast events to user's other sessions
    for (channel_id, _) in guild_rows.iter().chain(&dm_rows) {
        state.push.notify_read(&state, auth_user.id, *channel_id);
    }
    for (channel_id, last_read_message_id) in &guild_rows {
        let _ = broadcast_to_user(
            &state.redis,
            auth_user.id,
            &ServerEvent::ChannelRead {
                channel_id: *channel_id,
                last_read_message_id: *last_read_message_id,
            },
        )
        .await;
    }

    for (channel_id, last_read_message_id) in &dm_rows {
        let _ = broadcast_to_user(
            &state.redis,
            auth_user.id,
            &ServerEvent::DmRead {
                channel_id: *channel_id,
                last_read_message_id: *last_read_message_id,
            },
        )
        .await;
    }

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::ChannelType;

    #[test]
    fn unread_predicate_excludes_deleted_threads_and_own_messages() {
        assert_eq!(
            UNREAD_MESSAGE_PREDICATE,
            "m.deleted_at IS NULL AND m.parent_id IS NULL AND m.user_id IS DISTINCT FROM $1"
        );
    }

    #[test]
    fn text_like_sql_matches_channel_type() {
        for (kind, name) in [
            (ChannelType::Text, "'text'"),
            (ChannelType::Voice, "'voice'"),
            (ChannelType::Dm, "'dm'"),
            (ChannelType::Forum, "'forum'"),
            (ChannelType::Announcement, "'announcement'"),
        ] {
            assert_eq!(
                TEXT_LIKE_CHANNEL_TYPES.contains(name),
                kind.is_text_like(),
                "{name}"
            );
        }
    }
}
