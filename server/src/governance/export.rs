//! Data Export Worker
//!
//! Gathers user data from all tables into a versioned JSON archive and uploads to S3.

use std::sync::Arc;

use anyhow::Context;
use chrono::{Duration, Utc};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::chat::S3Client;
use crate::email::EmailService;

/// Versioned export manifest.
#[derive(Serialize)]
struct ExportManifest {
    version: &'static str,
    exported_at: String,
    user_id: String,
    sections: Vec<&'static str>,
}

/// User profile data for export.
#[derive(Serialize)]
struct ExportProfile {
    id: String,
    username: String,
    display_name: String,
    email: Option<String>,
    auth_method: String,
    avatar_url: Option<String>,
    is_bot: bool,
    created_at: String,
}

/// Exported message record.
#[derive(Serialize, sqlx::FromRow)]
struct ExportMessage {
    id: Uuid,
    channel_id: Uuid,
    content: String,
    encrypted: bool,
    created_at: chrono::DateTime<chrono::Utc>,
    edited_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Exported guild membership.
#[derive(Serialize, sqlx::FromRow)]
struct ExportGuildMembership {
    guild_id: Uuid,
    guild_name: String,
    joined_at: chrono::DateTime<chrono::Utc>,
}

/// Exported friend relationship.
#[derive(Serialize, sqlx::FromRow)]
struct ExportFriend {
    friend_id: Uuid,
    friend_username: String,
    since: chrono::DateTime<chrono::Utc>,
}

/// Exported user preferences.
#[derive(Serialize, sqlx::FromRow)]
struct ExportPreferences {
    preferences: serde_json::Value,
}

/// Exported DM channel with participants.
#[derive(Serialize, sqlx::FromRow)]
struct ExportDirectMessage {
    channel_id: Uuid,
    channel_name: String,
    joined_at: chrono::DateTime<chrono::Utc>,
    participants: Option<String>,
}

/// Exported blocked user.
#[derive(Serialize, sqlx::FromRow)]
struct ExportBlockedUser {
    blocked_user_id: Uuid,
    blocked_username: String,
    blocked_at: chrono::DateTime<chrono::Utc>,
}

/// Exported message reaction.
#[derive(Serialize, sqlx::FromRow)]
struct ExportReaction {
    id: Uuid,
    message_id: Uuid,
    emoji: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

/// Exported file attachment metadata (no S3 keys).
#[derive(Serialize, sqlx::FromRow)]
struct ExportAttachment {
    id: Uuid,
    filename: String,
    mime_type: String,
    size_bytes: i64,
    created_at: chrono::DateTime<chrono::Utc>,
}

/// Exported session record (no `token_hash`).
#[derive(Serialize, sqlx::FromRow)]
struct ExportSession {
    id: Uuid,
    ip_address: Option<String>,
    user_agent: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    expires_at: chrono::DateTime<chrono::Utc>,
}

/// Exported E2EE device registration (no raw key material).
#[derive(Serialize, sqlx::FromRow)]
struct ExportDevice {
    id: Uuid,
    device_name: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    last_seen_at: chrono::DateTime<chrono::Utc>,
    is_verified: bool,
}

/// Exported key backup metadata (no encrypted data).
#[derive(Serialize, sqlx::FromRow)]
struct ExportKeyBackup {
    version: i32,
    created_at: chrono::DateTime<chrono::Utc>,
}

/// Exported audit log entry.
#[derive(Serialize, sqlx::FromRow)]
struct ExportAuditLogEntry {
    action: String,
    target_type: Option<String>,
    target_id: Option<Uuid>,
    details: Option<serde_json::Value>,
    ip_address: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
}

/// Process a data export job.
pub async fn process_export_job(
    pool: &PgPool,
    s3: &S3Client,
    email_service: &Option<Arc<EmailService>>,
    job_id: Uuid,
    user_id: Uuid,
) -> anyhow::Result<()> {
    // Mark job as processing
    sqlx::query("UPDATE data_export_jobs SET status = 'processing' WHERE id = $1")
        .bind(job_id)
        .execute(pool)
        .await?;

    match build_export_archive(pool, user_id).await {
        Ok(archive_data) => {
            let file_size = archive_data.len() as i64;
            let s3_key = format!("exports/{user_id}/{job_id}.zip");

            // Upload to S3
            s3.upload(&s3_key, archive_data, "application/zip").await?;

            let expires_at = Utc::now() + Duration::days(7);

            // Mark as completed
            sqlx::query(
                "UPDATE data_export_jobs
                 SET status = 'completed', s3_key = $1, file_size_bytes = $2,
                     expires_at = $3, completed_at = NOW()
                 WHERE id = $4",
            )
            .bind(&s3_key)
            .bind(file_size)
            .bind(expires_at)
            .bind(job_id)
            .execute(pool)
            .await?;

            tracing::info!(
                job_id = %job_id,
                user_id = %user_id,
                file_size = file_size,
                "Export job completed"
            );

            // Send email notification if configured
            if let Some(email) = email_service {
                if let Ok(Some(user)) = crate::db::find_user_by_id(pool, user_id).await {
                    if let Some(user_email) = &user.email {
                        let _ = email
                            .send_data_export_ready(user_email, &user.username)
                            .await;
                    }
                }
            }
        }
        Err(e) => {
            // Mark as failed
            sqlx::query(
                "UPDATE data_export_jobs
                 SET status = 'failed', error_message = $1, completed_at = NOW()
                 WHERE id = $2",
            )
            .bind(e.to_string())
            .bind(job_id)
            .execute(pool)
            .await?;

            return Err(e);
        }
    }

    Ok(())
}

/// Build the export ZIP archive, writing sections to a temp file to reduce peak
/// memory during construction (each section is dropped after serialization).
///
/// High-cardinality sections (messages, reactions, attachments, audit log) are
/// capped with `LIMIT` to prevent OOM on large accounts. The finished archive
/// is read back into memory for S3 upload.
async fn build_export_archive(pool: &PgPool, user_id: Uuid) -> anyhow::Result<Vec<u8>> {
    let tmp = tempfile::NamedTempFile::new()
        .context("Failed to create temp file for export archive")?;
    let mut zip = ZipWriter::new(std::io::BufWriter::new(tmp.as_file().try_clone()?));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    // 1. Profile
    let user = crate::db::find_user_by_id(pool, user_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("User not found"))?;

    let profile = ExportProfile {
        id: user.id.to_string(),
        username: user.username.clone(),
        display_name: user.display_name,
        email: user.email,
        auth_method: format!("{:?}", user.auth_method),
        avatar_url: user.avatar_url,
        is_bot: user.is_bot,
        created_at: user.created_at.to_rfc3339(),
    };

    zip.start_file("profile.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &profile)?;

    // 2. Messages (non-deleted, includes encrypted) — capped at 500K rows
    let messages: Vec<ExportMessage> = sqlx::query_as(
        "SELECT id, channel_id, content, encrypted, created_at, edited_at
         FROM messages
         WHERE user_id = $1 AND deleted_at IS NULL
         ORDER BY created_at ASC
         LIMIT 500000",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    tracing::info!(section = "messages", rows = messages.len(), "Export section collected");
    zip.start_file("messages.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &messages)?;
    drop(messages);

    // 3. Guild memberships (bounded by max_guilds_per_user config)
    let guilds: Vec<ExportGuildMembership> = sqlx::query_as(
        "SELECT gm.guild_id, g.name as guild_name, gm.joined_at
         FROM guild_members gm
         JOIN guilds g ON g.id = gm.guild_id
         WHERE gm.user_id = $1
         ORDER BY gm.joined_at ASC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    zip.start_file("guilds.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &guilds)?;

    // 4. Friends (bounded by max_friends_per_user config)
    let friends: Vec<ExportFriend> = sqlx::query_as(
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
    .await?;

    zip.start_file("friends.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &friends)?;

    // 5. Preferences (single row)
    let prefs: Option<ExportPreferences> =
        sqlx::query_as("SELECT preferences FROM user_preferences WHERE user_id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;

    if let Some(p) = &prefs {
        zip.start_file("preferences.json", options)?;
        serde_json::to_writer_pretty(&mut zip, &p.preferences)?;
    }

    // 6. Direct messages (bounded by DM participant limits)
    let direct_messages: Vec<ExportDirectMessage> = sqlx::query_as(
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
    .await?;

    zip.start_file("direct_messages.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &direct_messages)?;

    // 7. Blocked users (bounded by block list limits)
    let blocked_users: Vec<ExportBlockedUser> = sqlx::query_as(
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
    .await?;

    zip.start_file("blocked_users.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &blocked_users)?;

    // 8. Reactions — capped at 500K rows
    let reactions: Vec<ExportReaction> = sqlx::query_as(
        "SELECT id, message_id, emoji, created_at
         FROM message_reactions
         WHERE user_id = $1
         ORDER BY created_at ASC
         LIMIT 500000",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    tracing::info!(section = "reactions", rows = reactions.len(), "Export section collected");
    zip.start_file("reactions.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &reactions)?;
    drop(reactions);

    // 9. Attachments (metadata only, no S3 keys) — capped at 100K rows
    let attachments: Vec<ExportAttachment> = sqlx::query_as(
        "SELECT fa.id, fa.filename, fa.mime_type, fa.size_bytes, fa.created_at
         FROM file_attachments fa
         JOIN messages m ON m.id = fa.message_id
         WHERE m.user_id = $1
         ORDER BY fa.created_at ASC
         LIMIT 100000",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    tracing::info!(section = "attachments", rows = attachments.len(), "Export section collected");
    zip.start_file("attachments.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &attachments)?;
    drop(attachments);

    // 10. Sessions (bounded by session expiry cleanup — no token_hash)
    let sessions: Vec<ExportSession> = sqlx::query_as(
        "SELECT id, host(ip_address) as ip_address, user_agent, created_at, expires_at
         FROM sessions
         WHERE user_id = $1
         ORDER BY created_at ASC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    zip.start_file("sessions.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &sessions)?;

    // 11. Devices (bounded by max_devices_per_user config — no raw key material)
    let devices: Vec<ExportDevice> = sqlx::query_as(
        "SELECT id, device_name, created_at, last_seen_at, is_verified
         FROM user_devices
         WHERE user_id = $1
         ORDER BY created_at ASC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    zip.start_file("devices.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &devices)?;

    // 12. Key backup metadata (bounded by max_devices_per_user — no encrypted data)
    let key_backups: Vec<ExportKeyBackup> = sqlx::query_as(
        "SELECT version, created_at
         FROM key_backups
         WHERE user_id = $1
         ORDER BY created_at ASC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    zip.start_file("key_backups.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &key_backups)?;

    // 13. Audit log — capped at 100K rows
    let audit_log: Vec<ExportAuditLogEntry> = sqlx::query_as(
        "SELECT action, target_type, target_id, details,
                host(ip_address) as ip_address, created_at
         FROM system_audit_log
         WHERE actor_id = $1
         ORDER BY created_at ASC
         LIMIT 100000",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    tracing::info!(section = "audit_log", rows = audit_log.len(), "Export section collected");
    zip.start_file("audit_log.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &audit_log)?;
    drop(audit_log);

    // Manifest
    let manifest = ExportManifest {
        version: "1.1",
        exported_at: Utc::now().to_rfc3339(),
        user_id: user_id.to_string(),
        sections: vec![
            "profile",
            "messages",
            "guilds",
            "friends",
            "preferences",
            "direct_messages",
            "blocked_users",
            "reactions",
            "attachments",
            "sessions",
            "devices",
            "key_backups",
            "audit_log",
        ],
    };

    zip.start_file("manifest.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &manifest)?;

    zip.finish()
        .map_err(|e| anyhow::anyhow!("Failed to finalize export ZIP archive: {e}"))?;

    // Read finished archive from temp file for S3 upload
    let archive_data = std::fs::read(tmp.path())?;
    Ok(archive_data)
}

/// Recover stale export jobs stuck in `pending`/`processing` after a server crash.
///
/// Jobs older than 1 hour are marked as `failed` so users can retry.
/// Returns the number of recovered jobs.
pub async fn recover_stale_export_jobs(pool: &PgPool) -> anyhow::Result<u64> {
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

/// Cleanup expired export jobs — delete S3 objects and mark as expired.
pub async fn cleanup_expired_exports(pool: &PgPool, s3: &Option<S3Client>) -> anyhow::Result<()> {
    // If S3 is unavailable, skip cleanup entirely to prevent orphaning objects.
    // Marking jobs as expired without deleting files would make them unrecoverable.
    if s3.is_none() {
        tracing::debug!("S3 unavailable — skipping export cleanup to prevent orphaned objects");
        return Ok(());
    }

    let expired_jobs: Vec<(Uuid, Option<String>)> = sqlx::query_as(
        "SELECT id, s3_key FROM data_export_jobs
         WHERE status = 'completed' AND expires_at < NOW()",
    )
    .fetch_all(pool)
    .await?;

    if expired_jobs.is_empty() {
        return Ok(());
    }
    let mut updatable_ids = Vec::new();

    for (job_id, s3_key) in &expired_jobs {
        match (s3, s3_key.as_deref()) {
            (Some(s3_client), Some(key)) => match s3_client.delete(key).await {
                Ok(()) => updatable_ids.push(*job_id),
                Err(e) => {
                    tracing::warn!(
                        job_id = %job_id,
                        s3_key = %key,
                        error = %e,
                        "Failed to delete expired export from S3; keeping job retryable"
                    );
                }
            },
            _ => {
                updatable_ids.push(*job_id);
            }
        }
    }

    if !updatable_ids.is_empty() {
        sqlx::query(
            "UPDATE data_export_jobs SET status = 'expired', s3_key = NULL
             WHERE id = ANY($1)",
        )
        .bind(&updatable_ids)
        .execute(pool)
        .await?;
    }

    let count = updatable_ids.len();
    if count > 0 {
        tracing::debug!(count, "Cleaned up expired export jobs");
    }

    Ok(())
}
