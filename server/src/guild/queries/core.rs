//! Core guild CRUD queries.
//!
//! Covers guild create/read/update/delete plus the handful of helper queries
//! related to guild ownership, membership listings, banner uploads, settings,
//! channel listings with unread state, channel reorder, and bot installations.

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, QueryBuilder, Transaction};
use uuid::Uuid;

use super::super::bots::InstalledBot;
use super::super::error::GuildError;
use super::super::types::{Guild, GuildMember, GuildSettings};

// ============================================================================
// Internal row types
// ============================================================================

/// Tuple of fields returned by `list_guilds_with_member_count`.
#[allow(clippy::type_complexity)]
pub type GuildWithMemberCountRow = (
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
    DateTime<Utc>,
    i64,
);

/// Tuple of fields returned for channel state CTE (unread counts + cursors + last msg id).
pub type ChannelStateRow = (Uuid, i64, Option<Uuid>, Option<Uuid>);

// ============================================================================
// Advisory locks
// ============================================================================

/// Acquire the per-user advisory lock used to serialize guild creation
/// (seed 51).
pub async fn lock_guild_create_for_user(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<(), GuildError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 51))")
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Acquire the per-guild advisory lock used to serialize bot installs
/// (seed 63).
pub async fn lock_bot_install(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
) -> Result<(), GuildError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 63))")
        .bind(guild_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

// ============================================================================
// Guild CRUD
// ============================================================================

/// Count guilds owned by a user (used inside the create-guild advisory lock).
pub async fn count_user_owned_guilds_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<i64, GuildError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM guilds WHERE owner_id = $1")
        .bind(user_id)
        .fetch_one(&mut **tx)
        .await?;
    Ok(count)
}

/// Insert a new guild row inside a transaction.
#[allow(clippy::too_many_arguments)]
pub async fn insert_guild(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
    name: &str,
    owner_id: Uuid,
    description: &Option<String>,
    discoverable: bool,
    tags: &[String],
    banner_url: &Option<String>,
) -> Result<Guild, GuildError> {
    let guild = sqlx::query_as::<_, Guild>(
        r"INSERT INTO guilds (id, name, owner_id, description, discoverable, tags, banner_url)
           VALUES ($1, $2, $3, $4, $5, $6, $7)
           RETURNING id, name, owner_id, icon_url, description, threads_enabled, discoverable, tags, banner_url, plan, created_at",
    )
    .bind(guild_id)
    .bind(name)
    .bind(owner_id)
    .bind(description)
    .bind(discoverable)
    .bind(tags)
    .bind(banner_url)
    .fetch_one(&mut **tx)
    .await?;
    Ok(guild)
}

/// Insert the owner as the first member of a newly-created guild.
pub async fn insert_owner_member(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<(), GuildError> {
    sqlx::query("INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)")
        .bind(guild_id)
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Insert the default `@everyone` role into a freshly-created guild.
pub async fn insert_default_everyone_role(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
    permissions: i64,
) -> Result<(), GuildError> {
    sqlx::query(
        r"INSERT INTO guild_roles (guild_id, name, permissions, position, is_default)
           VALUES ($1, 'everyone', $2, 0, true)",
    )
    .bind(guild_id)
    .bind(permissions)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// List the guilds a user belongs to with their denormalized member count.
pub async fn list_guilds_with_member_count(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Vec<GuildWithMemberCountRow>, GuildError> {
    let rows: Vec<GuildWithMemberCountRow> = sqlx::query_as(
        r"SELECT
            g.id, g.name, g.owner_id, g.icon_url, g.description, g.threads_enabled,
            g.discoverable, g.tags, g.banner_url, g.plan, g.created_at,
            g.member_count::bigint
           FROM guilds g
           INNER JOIN guild_members gm ON g.id = gm.guild_id
           WHERE gm.user_id = $1
           ORDER BY g.created_at",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Fetch a single guild by id.
pub async fn fetch_guild(pool: &PgPool, guild_id: Uuid) -> Result<Guild, GuildError> {
    sqlx::query_as::<_, Guild>(
        "SELECT id, name, owner_id, icon_url, description, threads_enabled, discoverable, tags, banner_url, plan, created_at FROM guilds WHERE id = $1",
    )
    .bind(guild_id)
    .fetch_optional(pool)
    .await?
    .ok_or(GuildError::NotFound)
}

/// Look up the owner id for a guild without fetching the full row.
pub async fn fetch_guild_owner(pool: &PgPool, guild_id: Uuid) -> Result<Uuid, GuildError> {
    let row: Option<(Uuid,)> = sqlx::query_as("SELECT owner_id FROM guilds WHERE id = $1")
        .bind(guild_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.ok_or(GuildError::NotFound)?.0)
}

/// Look up the owner id for a guild (panicking variant — caller already
/// validated the guild exists).
pub async fn fetch_guild_owner_required(pool: &PgPool, guild_id: Uuid) -> Result<Uuid, GuildError> {
    let row: (Uuid,) = sqlx::query_as("SELECT owner_id FROM guilds WHERE id = $1")
        .bind(guild_id)
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

/// Apply a partial update built by `QueryBuilder` to a guild row and return
/// the updated `Guild`.
pub async fn update_guild_dynamic(
    pool: &PgPool,
    mut builder: QueryBuilder<'_, Postgres>,
    guild_id: Uuid,
) -> Result<Guild, GuildError> {
    builder.push(" WHERE id = ");
    builder.push_bind(guild_id);
    builder.push(" RETURNING id, name, owner_id, icon_url, description, threads_enabled, discoverable, tags, banner_url, plan, created_at");

    let updated = builder.build_query_as::<Guild>().fetch_one(pool).await?;
    Ok(updated)
}

/// Delete a guild row.
pub async fn delete_guild(pool: &PgPool, guild_id: Uuid) -> Result<(), GuildError> {
    sqlx::query("DELETE FROM guilds WHERE id = $1")
        .bind(guild_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Update the banner URL for a guild and return the updated row.
pub async fn update_guild_banner(
    pool: &PgPool,
    guild_id: Uuid,
    banner_url: &str,
) -> Result<Guild, GuildError> {
    let updated = sqlx::query_as::<_, Guild>(
        "UPDATE guilds SET banner_url = $1 WHERE id = $2 RETURNING id, name, owner_id, icon_url, description, threads_enabled, discoverable, tags, banner_url, plan, created_at"
    )
    .bind(banner_url)
    .bind(guild_id)
    .fetch_one(pool)
    .await?;
    Ok(updated)
}

// ============================================================================
// Members
// ============================================================================

/// Remove a member from a guild. Returns the number of rows deleted.
pub async fn delete_guild_member(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<u64, GuildError> {
    let result = sqlx::query("DELETE FROM guild_members WHERE guild_id = $1 AND user_id = $2")
        .bind(guild_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// List the members of a guild with their user profile fields.
pub async fn list_guild_members(
    pool: &PgPool,
    guild_id: Uuid,
) -> Result<Vec<GuildMember>, GuildError> {
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
    .fetch_all(pool)
    .await?;
    Ok(members)
}

// ============================================================================
// Channel listing / reorder
// ============================================================================

/// Fetch unread counts, read cursors, and last message ids for a set of text
/// channels in one round trip.
pub async fn fetch_channel_states(
    pool: &PgPool,
    user_id: Uuid,
    channel_ids: &[Uuid],
) -> Result<Vec<ChannelStateRow>, GuildError> {
    let rows = sqlx::query_as::<_, ChannelStateRow>(
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
    .bind(user_id)
    .bind(channel_ids)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Fetch a category's `category_type` for reorder validation.
pub async fn fetch_category_type(
    tx: &mut Transaction<'_, Postgres>,
    category_id: Uuid,
) -> Result<Option<String>, GuildError> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT category_type::TEXT FROM channel_categories WHERE id = $1")
            .bind(category_id)
            .fetch_optional(&mut **tx)
            .await?;
    Ok(row.map(|(t,)| t))
}

/// Fetch a channel's `channel_type` for reorder validation.
pub async fn fetch_channel_type(
    tx: &mut Transaction<'_, Postgres>,
    channel_id: Uuid,
) -> Result<Option<String>, GuildError> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT channel_type::TEXT FROM channels WHERE id = $1")
            .bind(channel_id)
            .fetch_optional(&mut **tx)
            .await?;
    Ok(row.map(|(t,)| t))
}

/// Update a channel's position and category inside a transaction (used by the
/// channel reorder endpoint).
pub async fn update_channel_position(
    tx: &mut Transaction<'_, Postgres>,
    channel_id: Uuid,
    guild_id: Uuid,
    position: i32,
    category_id: Option<Uuid>,
) -> Result<(), GuildError> {
    sqlx::query(
        r"
        UPDATE channels
        SET position = $3, category_id = $4
        WHERE id = $1 AND guild_id = $2
        ",
    )
    .bind(channel_id)
    .bind(guild_id)
    .bind(position)
    .bind(category_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

// ============================================================================
// Channel read state
// ============================================================================

/// Initialize `channel_read_state` rows for all text channels in a guild.
/// Sets `last_read_at` to `NOW()` so pre-existing messages don't appear unread.
pub async fn initialize_channel_read_state(
    pool: &PgPool,
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
    .execute(pool)
    .await?;
    Ok(())
}

/// Mark every text channel in a guild as read for a user. Returns the
/// `(channel_id, last_read_message_id)` pairs that were updated so callers can
/// broadcast `ChannelRead` events.
pub async fn mark_all_guild_channels_read(
    pool: &PgPool,
    user_id: Uuid,
    guild_id: Uuid,
    now: DateTime<Utc>,
) -> Result<Vec<(Uuid, Option<Uuid>)>, GuildError> {
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
    .bind(user_id)
    .bind(guild_id)
    .bind(now)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// ============================================================================
// Settings
// ============================================================================

/// Tuple returned by `fetch_guild_settings`.
///
/// Fields, in order: `threads_enabled`, `discoverable`, `tags`, `banner_url`,
/// `discovery_prompt_dismissed`.
pub type GuildSettingsRow = (bool, bool, Vec<String>, Option<String>, bool);

/// Fetch a per-member view of the guild settings (combines guild flags with
/// the member's `discovery_prompt_dismissed_at` value).
pub async fn fetch_guild_settings(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<GuildSettings, GuildError> {
    let settings: GuildSettingsRow = sqlx::query_as(
        r"SELECT g.threads_enabled, g.discoverable, g.tags, g.banner_url,
                 (gm.discovery_prompt_dismissed_at IS NOT NULL) AS dismissed
          FROM guilds g
          INNER JOIN guild_members gm ON gm.guild_id = g.id AND gm.user_id = $2
          WHERE g.id = $1",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    .ok_or(GuildError::NotFound)?;

    Ok(GuildSettings {
        threads_enabled: settings.0,
        discoverable: settings.1,
        tags: settings.2,
        banner_url: settings.3,
        discovery_prompt_dismissed: settings.4,
    })
}

/// Apply a partial settings update built by `QueryBuilder` and return the
/// updated `(threads_enabled, discoverable, tags, banner_url)` tuple.
pub async fn update_guild_settings_dynamic(
    pool: &PgPool,
    mut builder: QueryBuilder<'_, Postgres>,
    guild_id: Uuid,
) -> Result<(bool, bool, Vec<String>, Option<String>), GuildError> {
    builder
        .push(" WHERE id = ")
        .push_bind(guild_id)
        .push(" RETURNING threads_enabled, discoverable, tags, banner_url");

    let row = builder
        .build_query_as::<(bool, bool, Vec<String>, Option<String>)>()
        .fetch_one(pool)
        .await?;
    Ok(row)
}

/// Fetch the per-member discovery prompt dismissal status.
pub async fn fetch_discovery_prompt_dismissed(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<bool, GuildError> {
    let row: (bool,) = sqlx::query_as(
        "SELECT (discovery_prompt_dismissed_at IS NOT NULL) FROM guild_members WHERE guild_id = $1 AND user_id = $2",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// Mark the discovery setup prompt dismissed for a member.
pub async fn dismiss_discovery_prompt(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<(), GuildError> {
    sqlx::query(
        r"UPDATE guild_members
          SET discovery_prompt_dismissed_at = NOW()
          WHERE guild_id = $1 AND user_id = $2
            AND discovery_prompt_dismissed_at IS NULL",
    )
    .bind(guild_id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(())
}

// ============================================================================
// Usage stats
// ============================================================================

/// Fetch the plan name for a guild (used by the usage endpoint).
pub async fn fetch_guild_plan(pool: &PgPool, guild_id: Uuid) -> Result<String, GuildError> {
    let (plan,): (String,) = sqlx::query_as("SELECT plan FROM guilds WHERE id = $1")
        .bind(guild_id)
        .fetch_optional(pool)
        .await?
        .ok_or(GuildError::NotFound)?;
    Ok(plan)
}

// ============================================================================
// Bots
// ============================================================================

/// Verify that a user id corresponds to a bot user.
pub async fn fetch_bot_user(pool: &PgPool, bot_id: Uuid) -> Result<Option<Uuid>, GuildError> {
    let row = sqlx::query!(
        "SELECT id FROM users WHERE id = $1 AND is_bot = true",
        bot_id
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.id))
}

/// Application info needed to authorize and install a bot.
pub struct BotApplicationRow {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub public: bool,
}

/// Look up the application metadata for a bot user id.
pub async fn fetch_bot_application(
    pool: &PgPool,
    bot_id: Uuid,
) -> Result<Option<BotApplicationRow>, GuildError> {
    let row = sqlx::query!(
        "SELECT id, owner_id, public FROM bot_applications WHERE bot_user_id = $1",
        bot_id
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| BotApplicationRow {
        id: r.id,
        owner_id: r.owner_id,
        public: r.public,
    }))
}

/// Look up the application id for a bot user id (used by remove-bot).
pub async fn fetch_bot_application_id(
    pool: &PgPool,
    bot_id: Uuid,
) -> Result<Option<Uuid>, GuildError> {
    let row = sqlx::query!(
        "SELECT id FROM bot_applications WHERE bot_user_id = $1",
        bot_id
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.id))
}

/// Count installed bots in a guild inside a transaction (used under the
/// bot-install advisory lock).
pub async fn count_guild_bots_tx(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
) -> Result<i64, GuildError> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM guild_bot_installations WHERE guild_id = $1")
            .bind(guild_id)
            .fetch_one(&mut **tx)
            .await?;
    Ok(count)
}

/// Insert a bot installation row, ignoring duplicate `(guild_id, application_id)`.
pub async fn insert_bot_installation(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
    application_id: Uuid,
    installed_by: Uuid,
) -> Result<(), GuildError> {
    sqlx::query(
        "INSERT INTO guild_bot_installations (guild_id, application_id, installed_by) VALUES ($1, $2, $3) ON CONFLICT (guild_id, application_id) DO NOTHING",
    )
    .bind(guild_id)
    .bind(application_id)
    .bind(installed_by)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// List bots installed in a guild (joined with `bot_applications` for
/// metadata).
pub async fn list_guild_bots(
    pool: &PgPool,
    guild_id: Uuid,
) -> Result<Vec<InstalledBot>, GuildError> {
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
    .fetch_all(pool)
    .await?;
    Ok(bots)
}

/// Delete a bot installation. Returns the number of rows affected.
pub async fn delete_bot_installation(
    pool: &PgPool,
    guild_id: Uuid,
    application_id: Uuid,
) -> Result<u64, GuildError> {
    let result = sqlx::query!(
        "DELETE FROM guild_bot_installations WHERE guild_id = $1 AND application_id = $2",
        guild_id,
        application_id
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

// ============================================================================
// Slash commands
// ============================================================================

/// Row returned by `list_guild_slash_commands`
/// (`name, description, bot_name, application_id`).
pub type GuildCommandRow = (String, String, String, Uuid);

/// List slash commands available in a guild (joined with installed bots).
pub async fn list_guild_slash_commands(
    pool: &PgPool,
    guild_id: Uuid,
) -> Result<Vec<GuildCommandRow>, GuildError> {
    let rows: Vec<GuildCommandRow> = sqlx::query_as(
        r"SELECT sc.name, sc.description, ba.name as bot_name, ba.id as application_id
           FROM slash_commands sc
           INNER JOIN bot_applications ba ON sc.application_id = ba.id
           INNER JOIN guild_bot_installations gbi ON ba.id = gbi.application_id
           WHERE gbi.guild_id = $1 AND (sc.guild_id = $1 OR sc.guild_id IS NULL)
           ORDER BY sc.name, (sc.guild_id IS NULL), sc.created_at",
    )
    .bind(guild_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
