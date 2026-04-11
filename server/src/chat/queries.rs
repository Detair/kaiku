//! Database queries for the chat module.
//!
//! Covers channels, messages, direct messages, permission overrides, and DM
//! search lookups that are not already represented in the shared `db::queries`
//! layer. Functions take `&PgPool` (or `&mut PgConnection` for transactional
//! callers) and return `Result<T, ChatError>` so handlers can use `?` directly.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use super::error::ChatError;
use super::types::{DMParticipant, LastMessagePreview};
use crate::db::Channel;

// ============================================================================
// Internal row types
// ============================================================================

/// Username row used for auto-generating DM channel names.
pub(super) struct UsernameRecord {
    pub username: String,
}

/// Slash-command resolution row used by `messages::create`.
#[derive(sqlx::FromRow)]
pub struct SlashCommandRow {
    pub bot_user_id: Option<Uuid>,
    pub application_id: Uuid,
    pub options: Option<serde_json::Value>,
    pub guild_scoped: bool,
}

/// Reaction aggregate row used to bulk-fetch reactions for a set of messages.
#[derive(sqlx::FromRow)]
pub struct ReactionRow {
    pub message_id: Uuid,
    pub emoji: String,
    pub count: i64,
    pub me: bool,
    pub users: Vec<Uuid>,
}

// ============================================================================
// Channels
// ============================================================================

/// Acquire the per-guild advisory lock used to serialize channel creation.
///
/// The lock is transaction-scoped (released on commit/rollback) and uses
/// seed `55` from the registry in `db/mod.rs`.
pub(super) async fn lock_channel_create(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
) -> Result<(), ChatError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 55))")
        .bind(guild_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Count the channels currently belonging to a guild (used for limit checks).
pub(super) async fn count_channels_in_guild(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
) -> Result<i64, ChatError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM channels WHERE guild_id = $1")
        .bind(guild_id)
        .fetch_one(&mut **tx)
        .await?;
    Ok(count)
}

/// Fetch a channel category's `category_type` (e.g. `"text"` / `"voice"`)
/// to validate that a new channel matches the category restriction.
pub(super) async fn find_channel_category_type(
    tx: &mut Transaction<'_, Postgres>,
    category_id: Uuid,
) -> Result<Option<String>, ChatError> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT category_type::TEXT FROM channel_categories WHERE id = $1")
            .bind(category_id)
            .fetch_optional(&mut **tx)
            .await?;
    Ok(row.map(|(t,)| t))
}

/// Compute the next `position` value for a new channel inside a category
/// (or in the root category when `category_id` is `None`).
pub(super) async fn next_channel_position(
    tx: &mut Transaction<'_, Postgres>,
    category_id: Option<Uuid>,
) -> Result<i32, ChatError> {
    let position: i32 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(position), 0) + 1 FROM channels \
         WHERE category_id IS NOT DISTINCT FROM $1",
    )
    .bind(category_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(position)
}

/// Parameters for `insert_channel`.
pub(super) struct InsertChannelParams<'a> {
    pub name: &'a str,
    pub channel_type: &'a crate::db::ChannelType,
    pub category_id: Option<Uuid>,
    pub guild_id: Option<Uuid>,
    pub topic: Option<&'a str>,
    pub icon_url: Option<&'a str>,
    pub user_limit: Option<i32>,
    pub position: i32,
}

/// Insert a new channel row inside a transaction (used by `channels::create`
/// where the surrounding transaction also performs limit checks).
pub(super) async fn insert_channel(
    tx: &mut Transaction<'_, Postgres>,
    params: InsertChannelParams<'_>,
) -> Result<Channel, ChatError> {
    let channel = sqlx::query_as::<_, Channel>(
        r"INSERT INTO channels (name, channel_type, category_id, guild_id, topic, icon_url, user_limit, position)
          VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
          RETURNING id, name, channel_type, category_id, guild_id, topic, icon_url, user_limit, position, max_screen_shares, created_at, updated_at",
    )
    .bind(params.name)
    .bind(params.channel_type)
    .bind(params.category_id)
    .bind(params.guild_id)
    .bind(params.topic)
    .bind(params.icon_url)
    .bind(params.user_limit)
    .bind(params.position)
    .fetch_one(&mut **tx)
    .await?;
    Ok(channel)
}

/// Check whether a user is a member of the given guild.
pub async fn is_guild_member(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<bool, ChatError> {
    let exists = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM guild_members WHERE guild_id = $1 AND user_id = $2) as "exists!""#,
        guild_id,
        user_id,
    )
    .fetch_one(pool)
    .await?;
    Ok(exists)
}

/// Forward-only upsert into `channel_read_state` for a guild channel.
///
/// The cursor only moves forward — stale requests cannot rewind it.
pub async fn upsert_channel_read_state(
    pool: &PgPool,
    user_id: Uuid,
    channel_id: Uuid,
    last_read_at: DateTime<Utc>,
    last_read_message_id: Uuid,
) -> Result<(), ChatError> {
    sqlx::query(
        r"INSERT INTO channel_read_state (user_id, channel_id, last_read_at, last_read_message_id)
          VALUES ($1, $2, $3, $4)
          ON CONFLICT (user_id, channel_id)
          DO UPDATE SET last_read_at = EXCLUDED.last_read_at,
                        last_read_message_id = EXCLUDED.last_read_message_id
          WHERE channel_read_state.last_read_message_id IS NULL
             OR (SELECT created_at FROM messages WHERE id = EXCLUDED.last_read_message_id)
                > (SELECT created_at FROM messages WHERE id = channel_read_state.last_read_message_id)",
    )
    .bind(user_id)
    .bind(channel_id)
    .bind(last_read_at)
    .bind(last_read_message_id)
    .execute(pool)
    .await?;
    Ok(())
}

// ============================================================================
// Messages
// ============================================================================

/// Return the participant `user_id`s of a DM channel. Used by message-create
/// to enforce block checks across DM peers.
pub async fn list_dm_participant_ids(
    pool: &PgPool,
    channel_id: Uuid,
) -> Result<Vec<Uuid>, ChatError> {
    let ids = sqlx::query_scalar!(
        "SELECT user_id FROM dm_participants WHERE channel_id = $1",
        channel_id,
    )
    .fetch_all(pool)
    .await?;
    Ok(ids)
}

/// Insert a built-in `/ping` reply message and return its `(id, created_at)`.
pub async fn insert_ping_message(
    pool: &PgPool,
    channel_id: Uuid,
    user_id: Uuid,
    content: &str,
) -> Result<(Uuid, DateTime<Utc>), ChatError> {
    let row: (Uuid, DateTime<Utc>) = sqlx::query_as(
        r"INSERT INTO messages (channel_id, user_id, content)
          VALUES ($1, $2, $3)
          RETURNING id, created_at",
    )
    .bind(channel_id)
    .bind(user_id)
    .bind(content)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// Resolve a slash command name to one or more bot installations in a guild.
///
/// Guild-scoped commands take priority over global commands; same-priority
/// matches are returned in deterministic order so the caller can detect
/// ambiguity.
pub async fn list_slash_command_candidates(
    pool: &PgPool,
    guild_id: Uuid,
    command_name: &str,
) -> Result<Vec<SlashCommandRow>, ChatError> {
    let commands = sqlx::query_as::<_, SlashCommandRow>(
        r"SELECT ba.bot_user_id, sc.application_id, sc.options, (sc.guild_id IS NOT NULL) AS guild_scoped
          FROM slash_commands sc
          JOIN bot_applications ba ON ba.id = sc.application_id
          JOIN guild_bot_installations gbi ON gbi.application_id = sc.application_id
          WHERE gbi.guild_id = $1
            AND sc.name = $2
            AND (sc.guild_id = $1 OR sc.guild_id IS NULL)
          ORDER BY (sc.guild_id IS NOT NULL) DESC, sc.created_at ASC, sc.id ASC",
    )
    .bind(guild_id)
    .bind(command_name)
    .fetch_all(pool)
    .await?;
    Ok(commands)
}

/// Look up display names (or usernames) for a set of bot user ids. Used to
/// produce a friendly "ambiguous command" error message.
pub async fn list_bot_display_names(
    pool: &PgPool,
    bot_ids: &[Uuid],
) -> Result<Vec<String>, ChatError> {
    let names: Vec<Option<String>> =
        sqlx::query_scalar("SELECT COALESCE(display_name, username) FROM users WHERE id = ANY($1)")
            .bind(bot_ids)
            .fetch_all(pool)
            .await?;
    Ok(names.into_iter().flatten().collect())
}

/// Whether the guild allows thread replies on its channels.
pub async fn is_guild_threads_enabled(pool: &PgPool, guild_id: Uuid) -> Result<bool, ChatError> {
    let row: (bool,) = sqlx::query_as("SELECT threads_enabled FROM guilds WHERE id = $1")
        .bind(guild_id)
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

/// Whether a specific message is currently pinned in its channel.
pub async fn is_message_pinned(
    pool: &PgPool,
    channel_id: Uuid,
    message_id: Uuid,
) -> Result<bool, ChatError> {
    let pinned: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM channel_pins WHERE channel_id = $1 AND message_id = $2)",
    )
    .bind(channel_id)
    .bind(message_id)
    .fetch_one(pool)
    .await?;
    Ok(pinned)
}

/// Remove the channel pin (if any) for a message. Returns whether a row was
/// actually deleted so the caller can decide whether to broadcast.
pub async fn delete_channel_pin(
    pool: &PgPool,
    channel_id: Uuid,
    message_id: Uuid,
) -> Result<bool, ChatError> {
    let result = sqlx::query("DELETE FROM channel_pins WHERE channel_id = $1 AND message_id = $2")
        .bind(channel_id)
        .bind(message_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Bulk-fetch reactions for a set of messages, grouped by emoji. The
/// `me` flag is true when the requesting user has reacted with that emoji.
pub async fn list_reactions_for_messages(
    pool: &PgPool,
    requesting_user_id: Uuid,
    message_ids: &[Uuid],
) -> Result<Vec<ReactionRow>, ChatError> {
    let rows = sqlx::query_as::<_, ReactionRow>(
        r"SELECT
            message_id,
            emoji,
            COUNT(*)::bigint as count,
            BOOL_OR(user_id = $1) as me,
            array_agg(user_id) as users
          FROM message_reactions
          WHERE message_id = ANY($2)
          GROUP BY message_id, emoji
          ORDER BY MIN(created_at)",
    )
    .bind(requesting_user_id)
    .bind(message_ids)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Bulk-fetch the set of message ids that are currently pinned (intersected
/// with the supplied list).
pub async fn list_pinned_message_ids(
    pool: &PgPool,
    message_ids: &[Uuid],
) -> Result<HashSet<Uuid>, ChatError> {
    let ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT message_id FROM channel_pins WHERE message_id = ANY($1)")
            .bind(message_ids)
            .fetch_all(pool)
            .await?;
    Ok(ids.into_iter().collect())
}

/// Find the id of the latest non-deleted reply in a thread, if any.
pub async fn latest_thread_reply_id(
    pool: &PgPool,
    parent_id: Uuid,
) -> Result<Option<Uuid>, ChatError> {
    let id: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM messages \
         WHERE parent_id = $1 AND deleted_at IS NULL \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(parent_id)
    .fetch_optional(pool)
    .await?;
    Ok(id)
}

// ============================================================================
// Direct Messages
// ============================================================================

/// Find an existing 1:1 DM channel between two users (in either direction).
pub async fn find_direct_dm_channel(
    pool: &PgPool,
    user1_id: Uuid,
    user2_id: Uuid,
) -> Result<Option<Channel>, ChatError> {
    let existing = sqlx::query_as::<_, Channel>(
        r"SELECT c.id, c.name, c.channel_type, c.category_id, c.guild_id,
                 c.topic, c.icon_url, c.user_limit, c.position, c.max_screen_shares, c.created_at, c.updated_at
          FROM channels c
          JOIN dm_participants p1 ON c.id = p1.channel_id AND p1.user_id = $1
          JOIN dm_participants p2 ON c.id = p2.channel_id AND p2.user_id = $2
          WHERE c.channel_type = 'dm' AND c.guild_id IS NULL
          AND (SELECT COUNT(*) FROM dm_participants WHERE channel_id = c.id) = 2",
    )
    .bind(user1_id)
    .bind(user2_id)
    .fetch_optional(pool)
    .await?;
    Ok(existing)
}

/// Fetch the usernames of two users (used to auto-generate a 1:1 DM name).
pub(super) async fn list_usernames_for_pair(
    pool: &PgPool,
    user1_id: Uuid,
    user2_id: Uuid,
) -> Result<Vec<UsernameRecord>, ChatError> {
    let names = sqlx::query_as!(
        UsernameRecord,
        "SELECT username FROM users WHERE id = $1 OR id = $2 ORDER BY username",
        user1_id,
        user2_id,
    )
    .fetch_all(pool)
    .await?;
    Ok(names)
}

/// Fetch usernames for an arbitrary set of user ids (used by group DM
/// auto-naming).
pub(super) async fn list_usernames_for_ids(
    pool: &PgPool,
    user_ids: &[Uuid],
) -> Result<Vec<UsernameRecord>, ChatError> {
    let names = sqlx::query_as!(
        UsernameRecord,
        "SELECT username FROM users WHERE id = ANY($1) ORDER BY username",
        user_ids,
    )
    .fetch_all(pool)
    .await?;
    Ok(names)
}

/// Insert a new DM channel row with the given id and name.
pub async fn insert_dm_channel(
    pool: &PgPool,
    channel_id: Uuid,
    name: &str,
) -> Result<Channel, ChatError> {
    let channel = sqlx::query_as::<_, Channel>(
        r"INSERT INTO channels (id, name, channel_type, guild_id, position)
          VALUES ($1, $2, 'dm', NULL, 0)
          RETURNING id, name, channel_type, category_id, guild_id, topic, icon_url, user_limit, position, max_screen_shares, created_at, updated_at",
    )
    .bind(channel_id)
    .bind(name)
    .fetch_one(pool)
    .await?;
    Ok(channel)
}

/// Insert two participants into a DM channel in a single statement.
pub async fn insert_dm_participants_pair(
    pool: &PgPool,
    channel_id: Uuid,
    user1_id: Uuid,
    user2_id: Uuid,
) -> Result<(), ChatError> {
    sqlx::query!(
        "INSERT INTO dm_participants (channel_id, user_id) VALUES ($1, $2), ($1, $3)",
        channel_id,
        user1_id,
        user2_id,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Insert a single participant row into a DM channel.
pub async fn insert_dm_participant(
    pool: &PgPool,
    channel_id: Uuid,
    user_id: Uuid,
) -> Result<(), ChatError> {
    sqlx::query!(
        "INSERT INTO dm_participants (channel_id, user_id) VALUES ($1, $2)",
        channel_id,
        user_id,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// List participants of a DM channel joined with the user profile.
pub async fn list_dm_participants(
    pool: &PgPool,
    channel_id: Uuid,
) -> Result<Vec<DMParticipant>, ChatError> {
    let participants = sqlx::query_as!(
        DMParticipant,
        r#"SELECT
            u.id as user_id,
            u.username,
            u.display_name,
            u.avatar_url,
            dp.joined_at
           FROM dm_participants dp
           JOIN users u ON u.id = dp.user_id
           WHERE dp.channel_id = $1
           ORDER BY dp.joined_at ASC"#,
        channel_id,
    )
    .fetch_all(pool)
    .await?;
    Ok(participants)
}

/// List all DM channels a user participates in, ordered by most recent
/// activity.
pub async fn list_user_dm_channels(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Vec<Channel>, ChatError> {
    let channels = sqlx::query_as::<_, Channel>(
        r"SELECT c.id, c.name, c.channel_type, c.category_id, c.guild_id,
                 c.topic, c.icon_url, c.user_limit, c.position, c.max_screen_shares, c.created_at, c.updated_at
          FROM channels c
          JOIN dm_participants dp ON c.id = dp.channel_id
          WHERE dp.user_id = $1 AND c.channel_type = 'dm'
          ORDER BY c.updated_at DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(channels)
}

/// Fetch the latest message preview for a DM channel.
pub async fn fetch_last_message_preview(
    pool: &PgPool,
    channel_id: Uuid,
) -> Result<Option<LastMessagePreview>, ChatError> {
    let last_message = sqlx::query_as::<_, LastMessagePreview>(
        "SELECT m.id, m.content, m.user_id, u.username, m.created_at
         FROM messages m
         LEFT JOIN users u ON u.id = m.user_id
         WHERE m.channel_id = $1
         ORDER BY m.created_at DESC
         LIMIT 1",
    )
    .bind(channel_id)
    .fetch_optional(pool)
    .await?;
    Ok(last_message)
}

/// Compute the unread message count for a user in a DM channel.
///
/// If the user has never read the channel, every message counts as unread.
pub async fn dm_unread_count(
    pool: &PgPool,
    user_id: Uuid,
    channel_id: Uuid,
) -> Result<i64, ChatError> {
    let read_state_row = sqlx::query!(
        r#"SELECT last_read_at FROM dm_read_state
               WHERE user_id = $1 AND channel_id = $2"#,
        user_id,
        channel_id
    )
    .fetch_optional(pool)
    .await?;

    let count = if let Some(read_state) = read_state_row {
        sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!" FROM messages
                   WHERE channel_id = $1 AND created_at > $2"#,
            channel_id,
            read_state.last_read_at
        )
        .fetch_one(pool)
        .await?
    } else {
        // No read state = all messages are unread
        sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!" FROM messages WHERE channel_id = $1"#,
            channel_id
        )
        .fetch_one(pool)
        .await?
    };

    Ok(count)
}

/// Whether a user is a participant of a DM channel.
pub async fn is_dm_participant(
    pool: &PgPool,
    channel_id: Uuid,
    user_id: Uuid,
) -> Result<bool, ChatError> {
    let exists = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM dm_participants WHERE channel_id = $1 AND user_id = $2) as "exists!""#,
        channel_id,
        user_id,
    )
    .fetch_one(pool)
    .await?;
    Ok(exists)
}

/// Remove a user from a DM channel. Returns whether a participant row was
/// deleted (used to distinguish "not a member" from a successful leave).
pub async fn remove_dm_participant(
    pool: &PgPool,
    channel_id: Uuid,
    user_id: Uuid,
) -> Result<bool, ChatError> {
    let result = sqlx::query!(
        "DELETE FROM dm_participants WHERE channel_id = $1 AND user_id = $2",
        channel_id,
        user_id,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Number of remaining participants in a DM channel.
pub async fn count_dm_participants(pool: &PgPool, channel_id: Uuid) -> Result<i64, ChatError> {
    let count = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!" FROM dm_participants WHERE channel_id = $1"#,
        channel_id,
    )
    .fetch_one(pool)
    .await?;
    Ok(count)
}

/// Update the display name of a (group) DM channel.
pub async fn update_dm_channel_name(
    pool: &PgPool,
    channel_id: Uuid,
    name: &str,
) -> Result<Channel, ChatError> {
    let channel = sqlx::query_as::<_, Channel>(
        r"UPDATE channels SET name = $1, updated_at = NOW()
          WHERE id = $2
          RETURNING id, name, channel_type, category_id, guild_id, topic, icon_url, user_limit, position, max_screen_shares, created_at, updated_at",
    )
    .bind(name)
    .bind(channel_id)
    .fetch_one(pool)
    .await?;
    Ok(channel)
}

/// Persist the S3 key for a channel icon.
pub async fn set_channel_icon(
    pool: &PgPool,
    channel_id: Uuid,
    s3_key: &str,
) -> Result<(), ChatError> {
    sqlx::query!(
        "UPDATE channels SET icon_url = $1, updated_at = NOW() WHERE id = $2",
        s3_key,
        channel_id,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Forward-only upsert into `dm_read_state` for a single DM channel.
pub async fn upsert_dm_read_state(
    pool: &PgPool,
    user_id: Uuid,
    channel_id: Uuid,
    last_read_at: DateTime<Utc>,
    last_read_message_id: Uuid,
) -> Result<(), ChatError> {
    sqlx::query(
        r"INSERT INTO dm_read_state (user_id, channel_id, last_read_at, last_read_message_id)
          VALUES ($1, $2, $3, $4)
          ON CONFLICT (user_id, channel_id)
          DO UPDATE SET last_read_at = EXCLUDED.last_read_at,
                        last_read_message_id = EXCLUDED.last_read_message_id
          WHERE dm_read_state.last_read_message_id IS NULL
             OR (SELECT created_at FROM messages WHERE id = EXCLUDED.last_read_message_id)
                > (SELECT created_at FROM messages WHERE id = dm_read_state.last_read_message_id)",
    )
    .bind(user_id)
    .bind(channel_id)
    .bind(last_read_at)
    .bind(last_read_message_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Bulk upsert `dm_read_state` for every DM channel a user participates in.
///
/// Returns `(channel_id, last_read_message_id)` pairs so the caller can
/// fan out broadcast events to other sessions.
pub async fn mark_all_dms_read(
    pool: &PgPool,
    user_id: Uuid,
    last_read_at: DateTime<Utc>,
) -> Result<Vec<(Uuid, Option<Uuid>)>, ChatError> {
    let rows: Vec<(Uuid, Option<Uuid>)> = sqlx::query_as(
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
    .bind(user_id)
    .bind(last_read_at)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// ============================================================================
// DM Search
// ============================================================================

/// Fetch `(id, name)` rows for a set of channel ids. Used by DM search to
/// build a channel-name lookup map for the result list.
pub async fn list_channel_names_by_ids(
    pool: &PgPool,
    channel_ids: &[Uuid],
) -> Result<Vec<(Uuid, String)>, ChatError> {
    let rows: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id, name FROM channels WHERE id = ANY($1)")
            .bind(channel_ids)
            .fetch_all(pool)
            .await?;
    Ok(rows)
}

// ============================================================================
// Channel permission overrides
// ============================================================================

/// Row layout returned by override queries:
/// `(id, channel_id, role_id, allow_permissions, deny_permissions)`.
pub(super) type OverrideRow = (Uuid, Uuid, Uuid, i64, i64);

/// List all permission overrides configured on a channel.
pub async fn list_channel_overrides(
    pool: &PgPool,
    channel_id: Uuid,
) -> Result<Vec<OverrideRow>, ChatError> {
    let overrides = sqlx::query_as::<_, OverrideRow>(
        r"SELECT id, channel_id, role_id, allow_permissions, deny_permissions
          FROM channel_overrides
          WHERE channel_id = $1",
    )
    .bind(channel_id)
    .fetch_all(pool)
    .await?;
    Ok(overrides)
}

/// Fetch a channel's `guild_id` (used by override handlers to validate that a
/// channel actually belongs to a guild).
pub async fn find_channel_guild_id(
    pool: &PgPool,
    channel_id: Uuid,
) -> Result<Option<Option<Uuid>>, ChatError> {
    let row: Option<(Option<Uuid>,)> =
        sqlx::query_as("SELECT guild_id FROM channels WHERE id = $1")
            .bind(channel_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(g,)| g))
}

/// Whether a role with the given id exists in the given guild.
pub async fn guild_role_exists(
    pool: &PgPool,
    role_id: Uuid,
    guild_id: Uuid,
) -> Result<bool, ChatError> {
    let exists: Option<(i32,)> =
        sqlx::query_as("SELECT 1 FROM guild_roles WHERE id = $1 AND guild_id = $2")
            .bind(role_id)
            .bind(guild_id)
            .fetch_optional(pool)
            .await?;
    Ok(exists.is_some())
}

/// Upsert a permission override for a (channel, role) pair.
pub async fn upsert_channel_override(
    pool: &PgPool,
    channel_id: Uuid,
    role_id: Uuid,
    allow: i64,
    deny: i64,
) -> Result<OverrideRow, ChatError> {
    let row = sqlx::query_as::<_, OverrideRow>(
        r"INSERT INTO channel_overrides (channel_id, role_id, allow_permissions, deny_permissions)
          VALUES ($1, $2, $3, $4)
          ON CONFLICT (channel_id, role_id) DO UPDATE SET
              allow_permissions = $3,
              deny_permissions = $4
          RETURNING id, channel_id, role_id, allow_permissions, deny_permissions",
    )
    .bind(channel_id)
    .bind(role_id)
    .bind(allow)
    .bind(deny)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// Delete a permission override for a (channel, role) pair. Returns whether
/// a row was actually deleted.
pub async fn delete_channel_override(
    pool: &PgPool,
    channel_id: Uuid,
    role_id: Uuid,
) -> Result<bool, ChatError> {
    let result =
        sqlx::query("DELETE FROM channel_overrides WHERE channel_id = $1 AND role_id = $2")
            .bind(channel_id)
            .bind(role_id)
            .execute(pool)
            .await?;
    Ok(result.rows_affected() > 0)
}
