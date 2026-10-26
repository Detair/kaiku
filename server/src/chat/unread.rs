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

/// SQL list of the read-tracked guild channel types for `channel_type IN ...`
/// (the SQL side of [`db::ChannelType::is_text_like`]), usable inside `concat!`.
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

/// Read-tracked (text, forum, announcement) guild channels the user may view.
///
/// Scoped to one guild, or to every guild the user is a member of when
/// `guild_id` is `None`. Visibility uses the same rules as
/// [`crate::permissions::require_channel_access`], resolved in one batch.
/// Every bulk read-cursor write goes through this so hidden channels never get
/// a cursor or a read event.
pub(crate) async fn viewable_read_tracked_channels(
    pool: &sqlx::PgPool,
    user_id: Uuid,
    guild_id: Option<Uuid>,
) -> sqlx::Result<Vec<Uuid>> {
    let candidates: Vec<(Uuid, Uuid)> = sqlx::query_as(concat!(
        r"SELECT c.guild_id, c.id
          FROM channels c
          INNER JOIN guild_members gm ON gm.guild_id = c.guild_id AND gm.user_id = $1
          WHERE ($2::uuid IS NULL OR c.guild_id = $2)
            AND c.channel_type IN ",
        text_like_channel_types!(),
        r" ORDER BY c.guild_id, c.position"
    ))
    .bind(user_id)
    .bind(guild_id)
    .fetch_all(pool)
    .await?;
    let viewable =
        crate::permissions::filter_viewable_channels_in_guilds(pool, user_id, &candidates).await?;
    Ok(candidates
        .into_iter()
        .map(|(_, channel_id)| channel_id)
        .filter(|id| viewable.contains(id))
        .collect())
}

/// Set the user's read cursor of each channel to its newest top-level message
/// (the channel feed's last entry). Returns the `(channel_id,
/// last_read_message_id)` pairs written, for `ChannelRead` broadcasts.
///
/// With `only_missing`, existing cursors are left alone (join-time setup).
pub(crate) async fn mark_guild_channels_read(
    pool: &sqlx::PgPool,
    user_id: Uuid,
    channel_ids: &[Uuid],
    now: chrono::DateTime<chrono::Utc>,
    only_missing: bool,
) -> sqlx::Result<Vec<(Uuid, Option<Uuid>)>> {
    if channel_ids.is_empty() {
        return Ok(Vec::new());
    }
    macro_rules! upsert {
        ($conflict:literal) => {
            concat!(
                r"INSERT INTO channel_read_state (user_id, channel_id, last_read_at, last_read_message_id)
                  SELECT $1, c.id, $2, (
                      SELECT m.id FROM messages m
                      WHERE m.channel_id = c.id AND m.deleted_at IS NULL AND m.parent_id IS NULL
                      ORDER BY m.created_at DESC LIMIT 1
                  )
                  FROM channels c
                  WHERE c.id = ANY($3)
                  ON CONFLICT (user_id, channel_id) ",
                $conflict,
                r" RETURNING channel_id, last_read_message_id"
            )
        };
    }
    let sql = if only_missing {
        upsert!("DO NOTHING")
    } else {
        upsert!(
            "DO UPDATE SET last_read_at = EXCLUDED.last_read_at, last_read_message_id = EXCLUDED.last_read_message_id"
        )
    };
    sqlx::query_as(sql)
        .bind(user_id)
        .bind(now)
        .bind(channel_ids)
        .fetch_all(pool)
        .await
}

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

    let internal = |what: &'static str| {
        move |e: sqlx::Error| {
            tracing::error!(error = %e, "Failed to bulk mark {what} as read");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to mark {what} as read"),
            )
        }
    };

    // 1. Mark every guild channel the user can open as read (cursor = newest top-level message, the
    //    same message the channel list reports). Hidden channels are left untouched.
    let channel_ids = viewable_read_tracked_channels(&state.db, auth_user.id, None)
        .await
        .map_err(internal("guild channels"))?;
    let guild_rows = mark_guild_channels_read(&state.db, auth_user.id, &channel_ids, now, false)
        .await
        .map_err(internal("guild channels"))?;

    // 2. Mark all DM channels as read
    let dm_rows = crate::chat::queries::mark_all_dms_read(&state.db, auth_user.id, now)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to bulk mark DMs as read");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to mark DMs as read".to_string(),
            )
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
                text_like_channel_types!().contains(name),
                kind.is_text_like(),
                "{name}"
            );
        }
    }
}
