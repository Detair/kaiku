//! Database queries for the moderation module.
//!
//! Covers user reports, admin report queue management, content filter
//! configuration, custom patterns, and moderation action logging.

use sqlx::PgPool;
use uuid::Uuid;

use super::error::ModerationError;
use super::filter_types::{
    FilterAction, FilterCategory, FilterConfigEntry, FilterError, GuildFilterConfig,
    GuildFilterPattern, ModerationAction,
};
use super::types::{Report, ReportCategory, ReportStatus, ReportTargetType};

/// Maximum characters of original content stored in moderation log.
const MAX_LOGGED_CONTENT_LEN: usize = 200;

// ============================================================================
// User Report Queries
// ============================================================================

/// Outcome of inserting a new user report.
pub enum InsertReportOutcome {
    Inserted(Report),
    Duplicate,
}

/// Check whether a user with the given id exists.
#[tracing::instrument(skip(pool))]
pub async fn user_exists(pool: &PgPool, user_id: Uuid) -> Result<bool, ModerationError> {
    let exists = sqlx::query_scalar!("SELECT id FROM users WHERE id = $1", user_id)
        .fetch_optional(pool)
        .await?
        .is_some();
    Ok(exists)
}

/// Fetch the author of a message, if it exists.
#[tracing::instrument(skip(pool))]
pub async fn get_message_author(
    pool: &PgPool,
    message_id: Uuid,
) -> Result<Option<Option<Uuid>>, ModerationError> {
    let row = sqlx::query!("SELECT user_id FROM messages WHERE id = $1", message_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.user_id))
}

/// Insert a new user report. Returns `Duplicate` if the unique index on
/// active reports is violated, otherwise the inserted row.
#[tracing::instrument(skip(pool, description))]
pub async fn insert_report(
    pool: &PgPool,
    reporter_id: Uuid,
    target_type: ReportTargetType,
    target_user_id: Uuid,
    target_message_id: Option<Uuid>,
    category: ReportCategory,
    description: Option<String>,
) -> Result<InsertReportOutcome, ModerationError> {
    let result = sqlx::query_as::<_, Report>(
        r"INSERT INTO user_reports (reporter_id, target_type, target_user_id, target_message_id, category, description)
           VALUES ($1, $2, $3, $4, $5, $6)
           RETURNING *",
    )
    .bind(reporter_id)
    .bind(target_type)
    .bind(target_user_id)
    .bind(target_message_id)
    .bind(category)
    .bind(description)
    .fetch_one(pool)
    .await;

    match result {
        Ok(report) => Ok(InsertReportOutcome::Inserted(report)),
        Err(sqlx::Error::Database(ref db_err))
            if db_err.constraint() == Some("idx_reports_no_duplicate_active") =>
        {
            Ok(InsertReportOutcome::Duplicate)
        }
        Err(e) => Err(ModerationError::Database(e)),
    }
}

/// List reports filtered by status/category, ordered newest first.
#[tracing::instrument(skip(pool))]
pub async fn list_reports(
    pool: &PgPool,
    status: Option<ReportStatus>,
    category: Option<ReportCategory>,
    limit: i64,
    offset: i64,
) -> Result<Vec<Report>, ModerationError> {
    let reports = sqlx::query_as::<_, Report>(
        r"SELECT * FROM user_reports
           WHERE ($1::report_status IS NULL OR status = $1)
             AND ($2::report_category IS NULL OR category = $2)
           ORDER BY created_at DESC
           LIMIT $3 OFFSET $4",
    )
    .bind(status)
    .bind(category)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    Ok(reports)
}

/// Count reports matching the optional status/category filter.
#[tracing::instrument(skip(pool))]
pub async fn count_reports(
    pool: &PgPool,
    status: Option<ReportStatus>,
    category: Option<ReportCategory>,
) -> Result<i64, ModerationError> {
    let total = sqlx::query_scalar::<_, Option<i64>>(
        r"SELECT COUNT(*) FROM user_reports
           WHERE ($1::report_status IS NULL OR status = $1)
             AND ($2::report_category IS NULL OR category = $2)",
    )
    .bind(status)
    .bind(category)
    .fetch_one(pool)
    .await?
    .unwrap_or(0);
    Ok(total)
}

/// Fetch a single report by id, returning `NotFound` if missing.
#[tracing::instrument(skip(pool))]
pub async fn get_report(pool: &PgPool, report_id: Uuid) -> Result<Report, ModerationError> {
    sqlx::query_as::<_, Report>("SELECT * FROM user_reports WHERE id = $1")
        .bind(report_id)
        .fetch_optional(pool)
        .await?
        .ok_or(ModerationError::NotFound)
}

/// Claim a pending report for the given admin. Returns `NotFound` if the
/// report does not exist or is no longer pending.
#[tracing::instrument(skip(pool))]
pub async fn claim_report(
    pool: &PgPool,
    report_id: Uuid,
    admin_id: Uuid,
) -> Result<Report, ModerationError> {
    sqlx::query_as::<_, Report>(
        r"UPDATE user_reports
           SET status = 'reviewing', assigned_admin_id = $2, updated_at = NOW()
           WHERE id = $1 AND status = 'pending'
           RETURNING *",
    )
    .bind(report_id)
    .bind(admin_id)
    .fetch_optional(pool)
    .await?
    .ok_or(ModerationError::NotFound)
}

/// Resolve a pending or reviewing report with the given action/note.
/// Returns `NotFound` if no eligible report exists.
#[tracing::instrument(skip(pool))]
pub async fn resolve_report(
    pool: &PgPool,
    report_id: Uuid,
    resolution_action: &str,
    resolution_note: Option<&str>,
) -> Result<Report, ModerationError> {
    sqlx::query_as::<_, Report>(
        r"UPDATE user_reports
           SET status = CASE WHEN $2 = 'dismissed' THEN 'dismissed'::report_status ELSE 'resolved'::report_status END,
               resolution_action = $2,
               resolution_note = $3,
               resolved_at = NOW(),
               updated_at = NOW()
           WHERE id = $1 AND status IN ('pending', 'reviewing')
           RETURNING *",
    )
    .bind(report_id)
    .bind(resolution_action)
    .bind(resolution_note)
    .fetch_optional(pool)
    .await?
    .ok_or(ModerationError::NotFound)
}

/// Aggregate report counts by status.
pub struct ReportStatusCounts {
    pub pending: i64,
    pub reviewing: i64,
    pub resolved: i64,
    pub dismissed: i64,
}

/// Count reports for each status bucket in a single batched call.
#[tracing::instrument(skip(pool))]
pub async fn count_reports_by_status(pool: &PgPool) -> Result<ReportStatusCounts, ModerationError> {
    let pending: i64 = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT COUNT(*) FROM user_reports WHERE status ='pending'",
    )
    .fetch_one(pool)
    .await?
    .unwrap_or(0);

    let reviewing: i64 = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT COUNT(*) FROM user_reports WHERE status ='reviewing'",
    )
    .fetch_one(pool)
    .await?
    .unwrap_or(0);

    let resolved: i64 = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT COUNT(*) FROM user_reports WHERE status ='resolved'",
    )
    .fetch_one(pool)
    .await?
    .unwrap_or(0);

    let dismissed: i64 = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT COUNT(*) FROM user_reports WHERE status ='dismissed'",
    )
    .fetch_one(pool)
    .await?
    .unwrap_or(0);

    Ok(ReportStatusCounts {
        pending,
        reviewing,
        resolved,
        dismissed,
    })
}

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
) -> Result<Vec<GuildFilterConfig>, FilterError> {
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
pub async fn count_custom_patterns(pool: &PgPool, guild_id: Uuid) -> Result<i64, FilterError> {
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
) -> Result<GuildFilterPattern, FilterError> {
    let row = sqlx::query_as::<_, GuildFilterPattern>(
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
    .await?;
    Ok(row)
}

/// Get a single custom pattern by id and guild.
#[tracing::instrument(skip(pool))]
pub async fn get_custom_pattern(
    pool: &PgPool,
    pattern_id: Uuid,
    guild_id: Uuid,
) -> Result<Option<GuildFilterPattern>, FilterError> {
    let row = sqlx::query_as::<_, GuildFilterPattern>(
        "SELECT id, guild_id, pattern, is_regex, description, enabled, created_by, created_at, updated_at
         FROM guild_filter_patterns
         WHERE id = $1 AND guild_id = $2",
    )
    .bind(pattern_id)
    .bind(guild_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Update a custom pattern. Returns None if not found or wrong guild.
#[tracing::instrument(skip(pool))]
#[allow(clippy::too_many_arguments)]
pub async fn update_custom_pattern(
    pool: &PgPool,
    pattern_id: Uuid,
    guild_id: Uuid,
    pattern: Option<&str>,
    is_regex: Option<bool>,
    description: Option<Option<&str>>,
    enabled: Option<bool>,
) -> Result<Option<GuildFilterPattern>, FilterError> {
    let row = sqlx::query_as::<_, GuildFilterPattern>(
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
    .await?;
    Ok(row)
}

/// Delete a custom pattern. Returns true if deleted.
#[tracing::instrument(skip(pool))]
pub async fn delete_custom_pattern(
    pool: &PgPool,
    pattern_id: Uuid,
    guild_id: Uuid,
) -> Result<bool, FilterError> {
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

/// List moderation actions for a guild (paginated). Returns `(items, total)`.
#[tracing::instrument(skip(pool))]
pub async fn list_moderation_log(
    pool: &PgPool,
    guild_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<(Vec<ModerationAction>, i64), FilterError> {
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
