//! Database queries for the governance module.
//!
//! Covers data export jobs, account deletion lifecycle, and the row types
//! used for serializing user data into export archives. The export row
//! structs live here (rather than in `export.rs`) so the queries can be
//! tested and reused independently of the archive-building logic.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use super::error::GovernanceError;
use crate::db::{self, DataExportJob};

// ============================================================================
// Export row types
// ============================================================================

/// User profile data for export.
#[derive(Serialize)]
pub(super) struct ExportProfile {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub email: Option<String>,
    pub auth_method: String,
    pub avatar_url: Option<String>,
    pub is_bot: bool,
    pub created_at: String,
}

/// Exported message record.
#[derive(Serialize, sqlx::FromRow)]
pub(super) struct ExportMessage {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub content: String,
    pub encrypted: bool,
    pub created_at: DateTime<Utc>,
    pub edited_at: Option<DateTime<Utc>>,
}

/// Exported guild membership.
#[derive(Serialize, sqlx::FromRow)]
pub(super) struct ExportGuildMembership {
    pub guild_id: Uuid,
    pub guild_name: String,
    pub joined_at: DateTime<Utc>,
}

/// Exported friend relationship.
#[derive(Serialize, sqlx::FromRow)]
pub(super) struct ExportFriend {
    pub friend_id: Uuid,
    pub friend_username: String,
    pub since: DateTime<Utc>,
}

/// Exported user preferences.
#[derive(Serialize, sqlx::FromRow)]
pub(super) struct ExportPreferences {
    pub preferences: serde_json::Value,
}

/// Exported DM channel with participants.
#[derive(Serialize, sqlx::FromRow)]
pub(super) struct ExportDirectMessage {
    pub channel_id: Uuid,
    pub channel_name: String,
    pub joined_at: DateTime<Utc>,
    pub participants: Option<String>,
}

/// Exported blocked user.
#[derive(Serialize, sqlx::FromRow)]
pub(super) struct ExportBlockedUser {
    pub blocked_user_id: Uuid,
    pub blocked_username: String,
    pub blocked_at: DateTime<Utc>,
}

/// Exported message reaction.
#[derive(Serialize, sqlx::FromRow)]
pub(super) struct ExportReaction {
    pub id: Uuid,
    pub message_id: Uuid,
    pub emoji: String,
    pub created_at: DateTime<Utc>,
}

/// Exported file attachment metadata (no S3 keys).
#[derive(Serialize, sqlx::FromRow)]
pub(super) struct ExportAttachment {
    pub id: Uuid,
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub created_at: DateTime<Utc>,
}

/// Exported session record (no `token_hash`).
#[derive(Serialize, sqlx::FromRow)]
pub(super) struct ExportSession {
    pub id: Uuid,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// Exported E2EE device registration (no raw key material).
#[derive(Serialize, sqlx::FromRow)]
pub(super) struct ExportDevice {
    pub id: Uuid,
    pub device_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub is_verified: bool,
}

/// Exported key backup metadata (no encrypted data).
#[derive(Serialize, sqlx::FromRow)]
pub(super) struct ExportKeyBackup {
    pub version: i32,
    pub created_at: DateTime<Utc>,
}

/// Exported audit log entry.
#[derive(Serialize, sqlx::FromRow)]
pub(super) struct ExportAuditLogEntry {
    pub action: String,
    pub target_type: Option<String>,
    pub target_id: Option<Uuid>,
    pub details: Option<serde_json::Value>,
    pub ip_address: Option<String>,
    pub created_at: DateTime<Utc>,
}

// ============================================================================
// Data Export Job Queries
// ============================================================================

/// Outcome of inserting a new export job.
pub enum InsertExportJobOutcome {
    Inserted(DataExportJob),
    AlreadyPending,
}

/// Mark stale pending/processing export jobs for a user as failed.
///
/// Used to recover from server crashes that left jobs hanging — without this
/// recovery the unique partial index would block the user from retrying.
#[tracing::instrument(skip(pool))]
pub async fn fail_stale_export_jobs_for_user(
    pool: &PgPool,
    user_id: Uuid,
    interval: &str,
) -> Result<(), GovernanceError> {
    sqlx::query(
        "UPDATE data_export_jobs
         SET status = 'failed',
             error_message = COALESCE(error_message, 'Job stale after restart; please retry'),
             completed_at = NOW()
         WHERE user_id = $1
           AND status IN ('pending', 'processing')
           AND created_at < NOW() - CAST($2 AS INTERVAL)",
    )
    .bind(user_id)
    .bind(interval)
    .execute(pool)
    .await?;
    Ok(())
}

/// Check whether the user has an existing pending/processing export job.
#[tracing::instrument(skip(pool))]
pub async fn has_active_export_job(pool: &PgPool, user_id: Uuid) -> Result<bool, GovernanceError> {
    let existing = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM data_export_jobs
         WHERE user_id = $1 AND status IN ('pending', 'processing')
         LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(existing.is_some())
}

/// Insert a new export job for a user.
///
/// Returns `AlreadyPending` if the unique partial index on active jobs is
/// violated by a concurrent request.
#[tracing::instrument(skip(pool))]
pub async fn insert_export_job(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<InsertExportJobOutcome, GovernanceError> {
    let result = sqlx::query_as::<_, DataExportJob>(
        "INSERT INTO data_export_jobs (user_id) VALUES ($1) RETURNING *",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await;

    match result {
        Ok(job) => Ok(InsertExportJobOutcome::Inserted(job)),
        Err(sqlx::Error::Database(ref db_err)) if db_err.is_unique_violation() => {
            Ok(InsertExportJobOutcome::AlreadyPending)
        }
        Err(e) => Err(GovernanceError::Database(e)),
    }
}

/// Fetch the most recent export job for a user.
#[tracing::instrument(skip(pool))]
pub async fn latest_export_job(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<DataExportJob, GovernanceError> {
    sqlx::query_as::<_, DataExportJob>(
        "SELECT * FROM data_export_jobs
         WHERE user_id = $1
         ORDER BY created_at DESC
         LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    .ok_or(GovernanceError::ExportNotFound)
}

/// Fetch the most recent completed export job for a user.
#[tracing::instrument(skip(pool))]
pub async fn latest_completed_export_job(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<DataExportJob, GovernanceError> {
    sqlx::query_as::<_, DataExportJob>(
        "SELECT * FROM data_export_jobs
         WHERE user_id = $1 AND status = 'completed'
         ORDER BY created_at DESC
         LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    .ok_or(GovernanceError::ExportNotFound)
}

/// Mark an export job as `processing`.
#[tracing::instrument(skip(pool))]
pub async fn mark_export_job_processing(pool: &PgPool, job_id: Uuid) -> sqlx::Result<()> {
    sqlx::query("UPDATE data_export_jobs SET status = 'processing' WHERE id = $1")
        .bind(job_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Mark an export job as `completed` with the uploaded archive metadata.
#[tracing::instrument(skip(pool))]
pub async fn mark_export_job_completed(
    pool: &PgPool,
    job_id: Uuid,
    s3_key: &str,
    file_size: i64,
    expires_at: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE data_export_jobs
         SET status = 'completed', s3_key = $1, file_size_bytes = $2,
             expires_at = $3, completed_at = NOW()
         WHERE id = $4",
    )
    .bind(s3_key)
    .bind(file_size)
    .bind(expires_at)
    .bind(job_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Mark an export job as `failed` with the given error message.
#[tracing::instrument(skip(pool, error_message))]
pub async fn mark_export_job_failed(
    pool: &PgPool,
    job_id: Uuid,
    error_message: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE data_export_jobs
         SET status = 'failed', error_message = $1, completed_at = NOW()
         WHERE id = $2",
    )
    .bind(error_message)
    .bind(job_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Recover any stale `pending`/`processing` export jobs older than 1 hour.
/// Returns the number of recovered jobs.
#[tracing::instrument(skip(pool))]
pub async fn recover_stale_export_jobs(pool: &PgPool) -> sqlx::Result<u64> {
    let result = sqlx::query(
        "UPDATE data_export_jobs
         SET status = 'failed',
             error_message = COALESCE(error_message, 'Job stale after restart; please retry'),
             completed_at = NOW()
         WHERE status IN ('pending', 'processing')
           AND created_at < NOW() - INTERVAL '1 hour'",
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// List completed export jobs whose `expires_at` has passed.
#[tracing::instrument(skip(pool))]
pub async fn list_expired_export_jobs(pool: &PgPool) -> sqlx::Result<Vec<(Uuid, Option<String>)>> {
    sqlx::query_as(
        "SELECT id, s3_key FROM data_export_jobs
         WHERE status = 'completed' AND expires_at < NOW()",
    )
    .fetch_all(pool)
    .await
}

/// Mark the given export jobs as `expired` and clear their S3 keys.
#[tracing::instrument(skip(pool, job_ids))]
pub async fn mark_export_jobs_expired(pool: &PgPool, job_ids: &[Uuid]) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE data_export_jobs SET status = 'expired', s3_key = NULL
         WHERE id = ANY($1)",
    )
    .bind(job_ids)
    .execute(pool)
    .await?;
    Ok(())
}

// ============================================================================
// Account Deletion Queries
// ============================================================================

/// List the (id, name) of guilds owned by the user.
#[tracing::instrument(skip(pool))]
pub async fn list_owned_guilds(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Vec<(Uuid, String)>, GovernanceError> {
    let rows = sqlx::query_as("SELECT id, name FROM guilds WHERE owner_id = $1")
        .bind(user_id)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

/// Schedule an account deletion by setting the deletion timestamps.
#[tracing::instrument(skip(pool))]
pub async fn schedule_account_deletion(
    pool: &PgPool,
    user_id: Uuid,
    requested_at: DateTime<Utc>,
    scheduled_at: DateTime<Utc>,
) -> Result<(), GovernanceError> {
    sqlx::query(
        "UPDATE users SET deletion_requested_at = $1, deletion_scheduled_at = $2 WHERE id = $3",
    )
    .bind(requested_at)
    .bind(scheduled_at)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Delete every session for a user (force logout everywhere).
#[tracing::instrument(skip(pool))]
pub async fn delete_all_user_sessions(pool: &PgPool, user_id: Uuid) -> Result<(), GovernanceError> {
    sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Cancel a pending account deletion. Returns the number of rows affected.
#[tracing::instrument(skip(pool))]
pub async fn cancel_account_deletion(pool: &PgPool, user_id: Uuid) -> Result<u64, GovernanceError> {
    let result = sqlx::query(
        "UPDATE users
         SET deletion_requested_at = NULL, deletion_scheduled_at = NULL
         WHERE id = $1 AND deletion_scheduled_at IS NOT NULL",
    )
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// List users whose deletion grace period has expired.
#[tracing::instrument(skip(pool))]
pub async fn list_due_deletions(pool: &PgPool) -> sqlx::Result<Vec<(Uuid, String)>> {
    sqlx::query_as(
        "SELECT id, username FROM users
         WHERE deletion_scheduled_at IS NOT NULL AND deletion_scheduled_at <= NOW()",
    )
    .fetch_all(pool)
    .await
}

/// Fetch the avatar URL for a user.
#[tracing::instrument(skip(pool))]
pub async fn get_user_avatar_url(pool: &PgPool, user_id: Uuid) -> sqlx::Result<Option<String>> {
    let row: Option<Option<String>> =
        sqlx::query_scalar("SELECT avatar_url FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.flatten())
}

/// List the S3 keys for all file attachments authored by the user.
#[tracing::instrument(skip(pool))]
pub async fn list_user_attachment_s3_keys(
    pool: &PgPool,
    user_id: Uuid,
) -> sqlx::Result<Vec<String>> {
    sqlx::query_scalar(
        "SELECT fa.s3_key FROM file_attachments fa
         JOIN messages m ON m.id = fa.message_id
         WHERE m.user_id = $1",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

/// List the S3 keys for the user's data export archives.
#[tracing::instrument(skip(pool))]
pub async fn list_user_export_s3_keys(pool: &PgPool, user_id: Uuid) -> sqlx::Result<Vec<String>> {
    sqlx::query_scalar(
        "SELECT s3_key FROM data_export_jobs
         WHERE user_id = $1 AND s3_key IS NOT NULL",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

/// Delete a user row, cascading to all dependent tables. Returns rows affected.
#[tracing::instrument(skip(pool))]
pub async fn delete_user(pool: &PgPool, user_id: Uuid) -> sqlx::Result<u64> {
    let result = sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

// ============================================================================
// Export Archive Queries
// ============================================================================

/// Fetch a user's profile for export. Returns `None` if the user is missing.
#[tracing::instrument(skip(pool))]
pub(super) async fn export_user_profile(
    pool: &PgPool,
    user_id: Uuid,
) -> sqlx::Result<Option<ExportProfile>> {
    let user = match db::find_user_by_id(pool, user_id).await? {
        Some(u) => u,
        None => return Ok(None),
    };
    Ok(Some(ExportProfile {
        id: user.id.to_string(),
        username: user.username.clone(),
        display_name: user.display_name,
        email: user.email,
        auth_method: format!("{:?}", user.auth_method),
        avatar_url: user.avatar_url,
        is_bot: user.is_bot,
        created_at: user.created_at.to_rfc3339(),
    }))
}

/// Fetch up to `limit` non-deleted messages authored by the user.
#[tracing::instrument(skip(pool))]
pub(super) async fn export_user_messages(
    pool: &PgPool,
    user_id: Uuid,
    limit: i64,
) -> sqlx::Result<Vec<ExportMessage>> {
    sqlx::query_as(
        "SELECT id, channel_id, content, encrypted, created_at, edited_at
         FROM messages
         WHERE user_id = $1 AND deleted_at IS NULL
         ORDER BY created_at ASC
         LIMIT $2",
    )
    .bind(user_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Fetch the user's guild memberships for export.
#[tracing::instrument(skip(pool))]
pub(super) async fn export_user_guilds(
    pool: &PgPool,
    user_id: Uuid,
) -> sqlx::Result<Vec<ExportGuildMembership>> {
    sqlx::query_as(
        "SELECT gm.guild_id, g.name as guild_name, gm.joined_at
         FROM guild_members gm
         JOIN guilds g ON g.id = gm.guild_id
         WHERE gm.user_id = $1
         ORDER BY gm.joined_at ASC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

/// Fetch the user's accepted friendships for export.
#[tracing::instrument(skip(pool))]
pub(super) async fn export_user_friends(
    pool: &PgPool,
    user_id: Uuid,
) -> sqlx::Result<Vec<ExportFriend>> {
    sqlx::query_as(
        "SELECT
            CASE WHEN fr.requester_id = $1 THEN fr.addressee_id ELSE fr.requester_id END as friend_id,
            u.username as friend_username,
            fr.updated_at as since
         FROM friendships fr
         JOIN users u ON u.id = CASE WHEN fr.requester_id = $1 THEN fr.addressee_id ELSE fr.requester_id END
         WHERE (fr.requester_id = $1 OR fr.addressee_id = $1)
           AND fr.status = 'accepted'
         ORDER BY fr.updated_at ASC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

/// Fetch the user's preferences row for export, if any.
#[tracing::instrument(skip(pool))]
pub(super) async fn export_user_preferences(
    pool: &PgPool,
    user_id: Uuid,
) -> sqlx::Result<Option<ExportPreferences>> {
    sqlx::query_as("SELECT preferences FROM user_preferences WHERE user_id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
}

/// Fetch the user's DM channels and aggregated participant lists for export.
#[tracing::instrument(skip(pool))]
pub(super) async fn export_user_direct_messages(
    pool: &PgPool,
    user_id: Uuid,
) -> sqlx::Result<Vec<ExportDirectMessage>> {
    sqlx::query_as(
        "SELECT
            dp.channel_id,
            c.name as channel_name,
            dp.joined_at,
            string_agg(u.username, ', ' ORDER BY u.username) as participants
         FROM dm_participants dp
         JOIN channels c ON c.id = dp.channel_id
         JOIN dm_participants dp2 ON dp2.channel_id = dp.channel_id AND dp2.user_id != $1
         JOIN users u ON u.id = dp2.user_id
         WHERE dp.user_id = $1
         GROUP BY dp.channel_id, c.name, dp.joined_at
         ORDER BY dp.joined_at ASC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

/// Fetch the user's blocked users for export.
#[tracing::instrument(skip(pool))]
pub(super) async fn export_user_blocked_users(
    pool: &PgPool,
    user_id: Uuid,
) -> sqlx::Result<Vec<ExportBlockedUser>> {
    sqlx::query_as(
        "SELECT
            fr.addressee_id as blocked_user_id,
            u.username as blocked_username,
            fr.updated_at as blocked_at
         FROM friendships fr
         JOIN users u ON u.id = fr.addressee_id
         WHERE fr.requester_id = $1 AND fr.status = 'blocked'
         ORDER BY fr.updated_at ASC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

/// Fetch up to `limit` of the user's message reactions for export.
#[tracing::instrument(skip(pool))]
pub(super) async fn export_user_reactions(
    pool: &PgPool,
    user_id: Uuid,
    limit: i64,
) -> sqlx::Result<Vec<ExportReaction>> {
    sqlx::query_as(
        "SELECT id, message_id, emoji, created_at
         FROM message_reactions
         WHERE user_id = $1
         ORDER BY created_at ASC
         LIMIT $2",
    )
    .bind(user_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Fetch up to `limit` attachment metadata rows for the user's messages.
#[tracing::instrument(skip(pool))]
pub(super) async fn export_user_attachments(
    pool: &PgPool,
    user_id: Uuid,
    limit: i64,
) -> sqlx::Result<Vec<ExportAttachment>> {
    sqlx::query_as(
        "SELECT fa.id, fa.filename, fa.mime_type, fa.size_bytes, fa.created_at
         FROM file_attachments fa
         JOIN messages m ON m.id = fa.message_id
         WHERE m.user_id = $1
         ORDER BY fa.created_at ASC
         LIMIT $2",
    )
    .bind(user_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Fetch the user's session metadata for export (no token hashes).
#[tracing::instrument(skip(pool))]
pub(super) async fn export_user_sessions(
    pool: &PgPool,
    user_id: Uuid,
) -> sqlx::Result<Vec<ExportSession>> {
    sqlx::query_as(
        "SELECT id, host(ip_address) as ip_address, user_agent, created_at, expires_at
         FROM sessions
         WHERE user_id = $1
         ORDER BY created_at ASC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

/// Fetch the user's E2EE devices for export (no key material).
#[tracing::instrument(skip(pool))]
pub(super) async fn export_user_devices(
    pool: &PgPool,
    user_id: Uuid,
) -> sqlx::Result<Vec<ExportDevice>> {
    sqlx::query_as(
        "SELECT id, device_name, created_at, last_seen_at, is_verified
         FROM user_devices
         WHERE user_id = $1
         ORDER BY created_at ASC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

/// Fetch the user's key backup metadata for export (no encrypted data).
#[tracing::instrument(skip(pool))]
pub(super) async fn export_user_key_backups(
    pool: &PgPool,
    user_id: Uuid,
) -> sqlx::Result<Vec<ExportKeyBackup>> {
    sqlx::query_as(
        "SELECT version, created_at
         FROM key_backups
         WHERE user_id = $1
         ORDER BY created_at ASC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

/// Fetch up to `limit` audit log entries authored by the user for export.
#[tracing::instrument(skip(pool))]
pub(super) async fn export_user_audit_log(
    pool: &PgPool,
    user_id: Uuid,
    limit: i64,
) -> sqlx::Result<Vec<ExportAuditLogEntry>> {
    sqlx::query_as(
        "SELECT action, target_type, target_id, details,
                host(ip_address) as ip_address, created_at
         FROM system_audit_log
         WHERE actor_id = $1
         ORDER BY created_at ASC
         LIMIT $2",
    )
    .bind(user_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}
