//! Filter Database Queries
//!
//! All database operations for content filter configuration,
//! custom patterns, and moderation action logging.

use sqlx::PgPool;
use uuid::Uuid;

use super::filter_types::{
    FilterAction, FilterCategory, FilterConfigEntry, GuildFilterConfig, GuildFilterPattern,
    ModerationAction,
};

/// Maximum characters of original content stored in moderation log.
const MAX_LOGGED_CONTENT_LEN: usize = 200;

// ============================================================================
// Filter Config Queries
// ============================================================================

/// List all filter configs for a guild.
#[tracing::instrument(skip(pool))]
pub async fn list_filter_configs(
    pool: &PgPool,
    guild_id: Uuid,
) -> sqlx::Result<Vec<GuildFilterConfig>> {
    sqlx::query_as::<_, GuildFilterConfig>(
        "SELECT id, guild_id, category, enabled, action, created_at, updated_at
         FROM guild_filter_configs
         WHERE guild_id = $1
         ORDER BY category",
    )
    .bind(guild_id)
    .fetch_all(pool)
    .await
}

/// Upsert filter configs for a guild (batch, transactional).
#[tracing::instrument(skip(pool, configs))]
pub async fn upsert_filter_configs(
    pool: &PgPool,
    guild_id: Uuid,
    configs: &[FilterConfigEntry],
) -> sqlx::Result<Vec<GuildFilterConfig>> {
    let mut tx = pool.begin().await?;
    let mut results = Vec::new();

    for entry in configs {
        let row = sqlx::query_as::<_, GuildFilterConfig>(
            "INSERT INTO guild_filter_configs (guild_id, category, enabled, action, updated_at)
             VALUES ($1, $2, $3, $4, NOW())
             ON CONFLICT (guild_id, category)
             DO UPDATE SET enabled = $3, action = $4, updated_at = NOW()
             RETURNING id, guild_id, category, enabled, action, created_at, updated_at",
        )
        .bind(guild_id)
        .bind(entry.category)
        .bind(entry.enabled)
        .bind(entry.action)
        .fetch_one(&mut *tx)
        .await?;

        results.push(row);
    }

    tx.commit().await?;
    Ok(results)
}

// ============================================================================
// Custom Pattern Queries
// ============================================================================

/// List all custom patterns for a guild.
#[tracing::instrument(skip(pool))]
pub async fn list_custom_patterns(
    pool: &PgPool,
    guild_id: Uuid,
) -> sqlx::Result<Vec<GuildFilterPattern>> {
    sqlx::query_as::<_, GuildFilterPattern>(
        "SELECT id, guild_id, pattern, is_regex, description, enabled, created_by, created_at, updated_at
         FROM guild_filter_patterns
         WHERE guild_id = $1
         ORDER BY created_at DESC",
    )
    .bind(guild_id)
    .fetch_all(pool)
    .await
}

/// Count custom patterns for a guild.
#[tracing::instrument(skip(pool))]
pub async fn count_custom_patterns(pool: &PgPool, guild_id: Uuid) -> sqlx::Result<i64> {
    let row: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM guild_filter_patterns WHERE guild_id = $1")
            .bind(guild_id)
            .fetch_one(pool)
            .await?;
    Ok(row.0)
}

/// Create a new custom pattern.
#[tracing::instrument(skip(pool))]
pub async fn create_custom_pattern(
    pool: &PgPool,
    guild_id: Uuid,
    pattern: &str,
    is_regex: bool,
    description: Option<&str>,
    created_by: Uuid,
) -> sqlx::Result<GuildFilterPattern> {
    sqlx::query_as::<_, GuildFilterPattern>(
        "INSERT INTO guild_filter_patterns (guild_id, pattern, is_regex, description, created_by)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id, guild_id, pattern, is_regex, description, enabled, created_by, created_at, updated_at",
    )
    .bind(guild_id)
    .bind(pattern)
    .bind(is_regex)
    .bind(description)
    .bind(created_by)
    .fetch_one(pool)
    .await
}

/// Get a single custom pattern by id and guild.
#[tracing::instrument(skip(pool))]
pub async fn get_custom_pattern(
    pool: &PgPool,
    pattern_id: Uuid,
    guild_id: Uuid,
) -> sqlx::Result<Option<GuildFilterPattern>> {
    sqlx::query_as::<_, GuildFilterPattern>(
        "SELECT id, guild_id, pattern, is_regex, description, enabled, created_by, created_at, updated_at
         FROM guild_filter_patterns
         WHERE id = $1 AND guild_id = $2",
    )
    .bind(pattern_id)
    .bind(guild_id)
    .fetch_optional(pool)
    .await
}

/// Update a custom pattern. Returns None if not found or wrong guild.
#[tracing::instrument(skip(pool))]
pub async fn update_custom_pattern(
    pool: &PgPool,
    pattern_id: Uuid,
    guild_id: Uuid,
    pattern: Option<&str>,
    is_regex: Option<bool>,
    description: Option<Option<&str>>,
    enabled: Option<bool>,
) -> sqlx::Result<Option<GuildFilterPattern>> {
    sqlx::query_as::<_, GuildFilterPattern>(
        "UPDATE guild_filter_patterns SET
            pattern = COALESCE($3, pattern),
            is_regex = COALESCE($4, is_regex),
            description = CASE WHEN $5 THEN $6 ELSE description END,
            enabled = COALESCE($7, enabled),
            updated_at = NOW()
         WHERE id = $1 AND guild_id = $2
         RETURNING id, guild_id, pattern, is_regex, description, enabled, created_by, created_at, updated_at",
    )
    .bind(pattern_id)
    .bind(guild_id)
    .bind(pattern)
    .bind(is_regex)
    .bind(description.is_some())
    .bind(description.flatten())
    .bind(enabled)
    .fetch_optional(pool)
    .await
}

/// Delete a custom pattern. Returns true if deleted.
#[tracing::instrument(skip(pool))]
pub async fn delete_custom_pattern(
    pool: &PgPool,
    pattern_id: Uuid,
    guild_id: Uuid,
) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM guild_filter_patterns WHERE id = $1 AND guild_id = $2")
        .bind(pattern_id)
        .bind(guild_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

// ============================================================================
// Moderation Action Log Queries
// ============================================================================

/// Parameters for logging a moderation action.
pub struct LogActionParams<'a> {
    pub guild_id: Uuid,
    pub user_id: Uuid,
    pub channel_id: Uuid,
    pub action: FilterAction,
    pub category: Option<FilterCategory>,
    pub matched_pattern: &'a str,
    pub original_content: &'a str,
    pub custom_pattern_id: Option<Uuid>,
}

/// Log a moderation action.
///
/// Truncates `original_content` to [`MAX_LOGGED_CONTENT_LEN`] characters
/// before storing to limit data retention footprint.
#[tracing::instrument(skip(pool, params))]
pub async fn log_moderation_action(
    pool: &PgPool,
    params: &LogActionParams<'_>,
) -> sqlx::Result<ModerationAction> {
    // Truncate content on char boundary to limit stored data
    let truncated: &str = if params.original_content.len() > MAX_LOGGED_CONTENT_LEN {
        let mut end = MAX_LOGGED_CONTENT_LEN;
        while !params.original_content.is_char_boundary(end) {
            end -= 1;
        }
        &params.original_content[..end]
    } else {
        params.original_content
    };

    sqlx::query_as::<_, ModerationAction>(
        "INSERT INTO moderation_actions (guild_id, user_id, channel_id, action, category, matched_pattern, original_content, custom_pattern_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING id, guild_id, user_id, channel_id, action, category, matched_pattern, original_content, custom_pattern_id, created_at",
    )
    .bind(params.guild_id)
    .bind(params.user_id)
    .bind(params.channel_id)
    .bind(params.action)
    .bind(params.category)
    .bind(params.matched_pattern)
    .bind(truncated)
    .bind(params.custom_pattern_id)
    .fetch_one(pool)
    .await
}

/// List moderation actions for a guild (paginated).
#[tracing::instrument(skip(pool))]
pub async fn list_moderation_log(
    pool: &PgPool,
    guild_id: Uuid,
    limit: i64,
    offset: i64,
) -> sqlx::Result<(Vec<ModerationAction>, i64)> {
    let items = sqlx::query_as::<_, ModerationAction>(
        "SELECT id, guild_id, user_id, channel_id, action, category, matched_pattern, original_content, custom_pattern_id, created_at
         FROM moderation_actions
         WHERE guild_id = $1
         ORDER BY created_at DESC
         LIMIT $2 OFFSET $3",
    )
    .bind(guild_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    let total: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM moderation_actions WHERE guild_id = $1")
            .bind(guild_id)
            .fetch_one(pool)
            .await?;

    Ok((items, total.0))
}
