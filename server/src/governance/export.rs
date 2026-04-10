//! Data Export Worker
//!
//! Gathers user data from all tables into a versioned JSON archive and uploads to S3.

use std::io::Write;
use std::sync::Arc;

use anyhow::Context;
use chrono::{Duration, Utc};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use super::queries;
use crate::chat::S3Client;
use crate::email::EmailService;

/// Maximum number of messages included in a data export.
const EXPORT_CAP_MESSAGES: i64 = 500_000;
/// Maximum number of reactions included in a data export.
const EXPORT_CAP_REACTIONS: i64 = 500_000;
/// Maximum number of attachment metadata rows included in a data export.
const EXPORT_CAP_ATTACHMENTS: i64 = 100_000;
/// Maximum number of audit log entries included in a data export.
const EXPORT_CAP_AUDIT_LOG: i64 = 100_000;

/// Versioned export manifest.
#[derive(Serialize)]
struct ExportManifest {
    version: &'static str,
    exported_at: String,
    user_id: String,
    sections: Vec<&'static str>,
    truncated_sections: Vec<&'static str>,
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
    queries::mark_export_job_processing(pool, job_id).await?;

    match build_export_archive(pool, user_id).await {
        Ok(tmp) => {
            let s3_key = format!("exports/{user_id}/{job_id}.zip");

            // Stream archive directly to S3 without loading into memory
            let file_size: i64 = s3
                .upload_from_path(&s3_key, tmp.path(), "application/zip")
                .await?
                .try_into()
                .context("Export archive too large")?;

            let expires_at = Utc::now() + Duration::days(7);

            // Mark as completed
            queries::mark_export_job_completed(pool, job_id, &s3_key, file_size, expires_at)
                .await?;

            tracing::info!(
                job_id = %job_id,
                user_id = %user_id,
                file_size = file_size,
                "Export job completed"
            );

            // Send email notification if configured (best-effort, non-fatal)
            if let Some(email) = email_service {
                match crate::db::find_user_by_id(pool, user_id).await {
                    Ok(Some(user)) => {
                        if let Some(user_email) = &user.email {
                            if let Err(e) = email
                                .send_data_export_ready(user_email, &user.username)
                                .await
                            {
                                tracing::warn!(
                                    job_id = %job_id,
                                    user_id = %user_id,
                                    error = %e,
                                    "Failed to send data export notification email"
                                );
                            }
                        }
                    }
                    Ok(None) => {
                        tracing::warn!(
                            job_id = %job_id,
                            user_id = %user_id,
                            "Cannot send export notification: user not found"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            job_id = %job_id,
                            user_id = %user_id,
                            error = %e,
                            "Failed to look up user for export notification email"
                        );
                    }
                }
            }
        }
        Err(e) => {
            // Mark as failed — use if-let so the original error is never discarded
            if let Err(db_err) = queries::mark_export_job_failed(pool, job_id, &e.to_string()).await
            {
                tracing::error!(
                    job_id = %job_id,
                    original_error = %e,
                    db_error = %db_err,
                    "Failed to mark export job as failed; stale-job recovery will handle it"
                );
            }

            return Err(e);
        }
    }

    Ok(())
}

/// Build the export ZIP archive, writing sections to a temp file to reduce peak
/// memory during construction. High-cardinality sections (messages, reactions,
/// attachments, audit log) are explicitly dropped after serialization to limit
/// peak heap usage.
///
/// Those same sections are capped with `LIMIT` to prevent OOM on large accounts.
/// Returns the temp file for streaming upload to S3.
async fn build_export_archive(
    pool: &PgPool,
    user_id: Uuid,
) -> anyhow::Result<tempfile::NamedTempFile> {
    let tmp =
        tempfile::NamedTempFile::new().context("Failed to create temp file for export archive")?;
    let mut zip = ZipWriter::new(std::io::BufWriter::new(
        tmp.as_file()
            .try_clone()
            .context("Failed to clone temp file handle for ZIP writer")?,
    ));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    // 1. Profile
    let profile = queries::export_user_profile(pool, user_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("User not found"))?;

    zip.start_file("profile.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &profile)?;

    // 2. Messages (non-deleted, includes encrypted) — capped
    let mut truncated_sections: Vec<&'static str> = Vec::new();

    let messages = queries::export_user_messages(pool, user_id, EXPORT_CAP_MESSAGES).await?;

    if messages.len() as i64 >= EXPORT_CAP_MESSAGES {
        truncated_sections.push("messages");
        tracing::warn!(
            section = "messages",
            rows = messages.len(),
            user_id = %user_id,
            "Export section truncated at cap"
        );
    } else {
        tracing::info!(
            section = "messages",
            rows = messages.len(),
            user_id = %user_id,
            "Export section collected"
        );
    }
    zip.start_file("messages.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &messages)?;
    drop(messages);

    // 3. Guild memberships (bounded by max_guilds_per_user config)
    let guilds = queries::export_user_guilds(pool, user_id).await?;

    zip.start_file("guilds.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &guilds)?;

    // 4. Friends (bounded by max_friends_per_user config)
    let friends = queries::export_user_friends(pool, user_id).await?;

    zip.start_file("friends.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &friends)?;

    // 5. Preferences (single row)
    let prefs = queries::export_user_preferences(pool, user_id).await?;

    if let Some(p) = &prefs {
        zip.start_file("preferences.json", options)?;
        serde_json::to_writer_pretty(&mut zip, &p.preferences)?;
    }

    // 6. Direct messages (bounded by DM participant limits)
    let direct_messages = queries::export_user_direct_messages(pool, user_id).await?;

    zip.start_file("direct_messages.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &direct_messages)?;

    // 7. Blocked users (bounded by block list limits)
    let blocked_users = queries::export_user_blocked_users(pool, user_id).await?;

    zip.start_file("blocked_users.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &blocked_users)?;

    // 8. Reactions — capped
    let reactions = queries::export_user_reactions(pool, user_id, EXPORT_CAP_REACTIONS).await?;

    if reactions.len() as i64 >= EXPORT_CAP_REACTIONS {
        truncated_sections.push("reactions");
        tracing::warn!(
            section = "reactions",
            rows = reactions.len(),
            user_id = %user_id,
            "Export section truncated at cap"
        );
    } else {
        tracing::info!(
            section = "reactions",
            rows = reactions.len(),
            user_id = %user_id,
            "Export section collected"
        );
    }
    zip.start_file("reactions.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &reactions)?;
    drop(reactions);

    // 9. Attachments (metadata only, no S3 keys) — capped
    let attachments =
        queries::export_user_attachments(pool, user_id, EXPORT_CAP_ATTACHMENTS).await?;

    if attachments.len() as i64 >= EXPORT_CAP_ATTACHMENTS {
        truncated_sections.push("attachments");
        tracing::warn!(
            section = "attachments",
            rows = attachments.len(),
            user_id = %user_id,
            "Export section truncated at cap"
        );
    } else {
        tracing::info!(
            section = "attachments",
            rows = attachments.len(),
            user_id = %user_id,
            "Export section collected"
        );
    }
    zip.start_file("attachments.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &attachments)?;
    drop(attachments);

    // 10. Sessions (bounded by session expiry cleanup — no token_hash)
    let sessions = queries::export_user_sessions(pool, user_id).await?;

    zip.start_file("sessions.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &sessions)?;

    // 11. Devices (bounded by max_devices_per_user config — no raw key material)
    let devices = queries::export_user_devices(pool, user_id).await?;

    zip.start_file("devices.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &devices)?;

    // 12. Key backup metadata (bounded by max_devices_per_user — no encrypted data)
    let key_backups = queries::export_user_key_backups(pool, user_id).await?;

    zip.start_file("key_backups.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &key_backups)?;

    // 13. Audit log — capped
    let audit_log = queries::export_user_audit_log(pool, user_id, EXPORT_CAP_AUDIT_LOG).await?;

    if audit_log.len() as i64 >= EXPORT_CAP_AUDIT_LOG {
        truncated_sections.push("audit_log");
        tracing::warn!(
            section = "audit_log",
            rows = audit_log.len(),
            user_id = %user_id,
            "Export section truncated at cap"
        );
    } else {
        tracing::info!(
            section = "audit_log",
            rows = audit_log.len(),
            user_id = %user_id,
            "Export section collected"
        );
    }
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
        truncated_sections,
    };

    zip.start_file("manifest.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &manifest)?;

    let mut buf_writer = zip
        .finish()
        .map_err(|e| anyhow::anyhow!("Failed to finalize export ZIP archive: {e}"))?;
    buf_writer
        .flush()
        .context("Failed to flush export archive BufWriter")?;
    drop(buf_writer);

    // sync_all is a blocking syscall — run off the async executor
    let file = tmp
        .as_file()
        .try_clone()
        .context("Failed to clone file handle for sync")?;
    tokio::task::spawn_blocking(move || file.sync_all())
        .await
        .context("sync_all task panicked")?
        .context("Failed to sync export archive to disk")?;

    Ok(tmp)
}

/// Recover stale export jobs stuck in `pending`/`processing` after a server crash.
///
/// Jobs older than 1 hour are marked as `failed` so users can retry.
/// Returns the number of recovered jobs.
pub async fn recover_stale_export_jobs(pool: &PgPool) -> anyhow::Result<u64> {
    Ok(queries::recover_stale_export_jobs(pool).await?)
}

/// Cleanup expired export jobs — delete S3 objects and mark as expired.
pub async fn cleanup_expired_exports(pool: &PgPool, s3: &Option<S3Client>) -> anyhow::Result<()> {
    // If S3 is unavailable, skip cleanup entirely to prevent orphaning objects.
    // Marking jobs as expired without deleting files would make them unrecoverable.
    if s3.is_none() {
        tracing::debug!("S3 unavailable — skipping export cleanup to prevent orphaned objects");
        return Ok(());
    }

    let expired_jobs = queries::list_expired_export_jobs(pool).await?;

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
        queries::mark_export_jobs_expired(pool, &updatable_ids).await?;
    }

    let count = updatable_ids.len();
    if count > 0 {
        tracing::debug!(count, "Cleaned up expired export jobs");
    }

    Ok(())
}
