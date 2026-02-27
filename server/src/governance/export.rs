//! Data Export Worker
//!
//! Gathers user data from all tables into a versioned JSON archive and uploads to S3.

use std::sync::Arc;

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

/// Build the export ZIP archive in memory.
async fn build_export_archive(pool: &PgPool, user_id: Uuid) -> anyhow::Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut zip = ZipWriter::new(std::io::Cursor::new(&mut buf));
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

    // 2. Messages (non-deleted, includes encrypted)
    let messages: Vec<ExportMessage> = sqlx::query_as(
        "SELECT id, channel_id, content, encrypted, created_at, edited_at
         FROM messages
         WHERE user_id = $1 AND deleted_at IS NULL
         ORDER BY created_at ASC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    zip.start_file("messages.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &messages)?;

    // 3. Guild memberships
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

    // 4. Friends
    let friends: Vec<ExportFriend> = sqlx::query_as(
        "SELECT
            CASE WHEN fr.requester_id = $1 THEN fr.addressee_id ELSE fr.requester_id END as friend_id,
            u.username as friend_username,
            fr.accepted_at as since
         FROM friend_requests fr
         JOIN users u ON u.id = CASE WHEN fr.requester_id = $1 THEN fr.addressee_id ELSE fr.requester_id END
         WHERE (fr.requester_id = $1 OR fr.addressee_id = $1)
           AND fr.status = 'accepted'
         ORDER BY fr.accepted_at ASC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    zip.start_file("friends.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &friends)?;

    // 5. Preferences
    let prefs: Option<ExportPreferences> =
        sqlx::query_as("SELECT preferences FROM user_preferences WHERE user_id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;

    if let Some(p) = &prefs {
        zip.start_file("preferences.json", options)?;
        serde_json::to_writer_pretty(&mut zip, &p.preferences)?;
    }

    // 6. Manifest
    let manifest = ExportManifest {
        version: "1.0",
        exported_at: Utc::now().to_rfc3339(),
        user_id: user_id.to_string(),
        sections: vec!["profile", "messages", "guilds", "friends", "preferences"],
    };

    zip.start_file("manifest.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &manifest)?;

    zip.finish()?;

    Ok(buf)
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
