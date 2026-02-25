//! Account Deletion Worker
//!
//! Processes accounts whose 30-day grace period has expired.
//! Collects S3 objects, then deletes the user row — DB cascades and
//! SET NULL handle the rest.

use sqlx::PgPool;
use uuid::Uuid;

use crate::chat::S3Client;

/// S3 keys that belong to a user and must be cleaned up before deletion.
struct UserS3Objects {
    /// Avatar image key (e.g. `avatars/{user_id}/...`).
    avatar_key: Option<String>,
    /// File attachment keys from the user's messages.
    attachment_keys: Vec<String>,
    /// Data export archive keys.
    export_keys: Vec<String>,
}

/// Collect all S3 keys associated with a user.
async fn collect_user_s3_keys(pool: &PgPool, user_id: Uuid) -> anyhow::Result<UserS3Objects> {
    // Avatar — avatar_url stores a full URL, extract the S3 key portion
    let avatar_key: Option<String> =
        sqlx::query_scalar("SELECT avatar_url FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await?
            .flatten()
            .and_then(|url: String| url.find("avatars/").map(|pos| url[pos..].to_string()));

    // File attachments on the user's messages
    let attachment_keys: Vec<String> = sqlx::query_scalar(
        "SELECT fa.s3_key FROM file_attachments fa
         JOIN messages m ON m.id = fa.message_id
         WHERE m.user_id = $1",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    // Data export archives
    let export_keys: Vec<String> = sqlx::query_scalar(
        "SELECT s3_key FROM data_export_jobs
         WHERE user_id = $1 AND s3_key IS NOT NULL",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(UserS3Objects {
        avatar_key,
        attachment_keys,
        export_keys,
    })
}

/// Delete collected S3 objects, logging but not failing on individual errors.
async fn delete_s3_objects(s3: &S3Client, objects: &UserS3Objects, user_id: Uuid) {
    let all_keys = objects
        .avatar_key
        .iter()
        .chain(&objects.attachment_keys)
        .chain(&objects.export_keys);

    for key in all_keys {
        if let Err(e) = s3.delete(key).await {
            tracing::warn!(
                user_id = %user_id,
                s3_key = %key,
                error = %e,
                "Failed to delete S3 object during account deletion"
            );
        }
    }
}

/// Process accounts whose deletion grace period has expired.
///
/// For each due account:
/// 1. Collect S3 keys (avatar, attachments, exports)
/// 2. Delete the user row (cascades handle DB cleanup, SET NULL anonymizes messages)
/// 3. Clean up S3 objects
pub async fn process_pending_deletions(pool: &PgPool, s3: &Option<S3Client>) -> anyhow::Result<()> {
    let due_users: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, username FROM users
         WHERE deletion_scheduled_at IS NOT NULL AND deletion_scheduled_at <= NOW()",
    )
    .fetch_all(pool)
    .await?;

    if due_users.is_empty() {
        return Ok(());
    }

    for (user_id, username) in &due_users {
        tracing::info!(
            user_id = %user_id,
            username = %username,
            "Processing account deletion"
        );

        // Collect S3 keys before deleting the user (FK relationships still intact)
        let s3_objects = collect_user_s3_keys(pool, *user_id).await?;

        // Delete the user row — cascades handle everything:
        //   CASCADE: sessions, guild_members, channel_members, user_keys, user_roles,
        //            favorites, pins, read_state, preferences, friend_requests,
        //            mfa_backup_codes, password_reset_tokens, device_transfers,
        //            prekeys, user_blocks, user_reports, bot_installations, etc.
        //   SET NULL: messages.user_id, content filters, audit logs, etc.
        let result = sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            tracing::warn!(user_id = %user_id, "User already deleted, skipping");
            continue;
        }

        // Clean up S3 objects (best-effort, logged on failure)
        if let Some(s3) = s3 {
            delete_s3_objects(s3, &s3_objects, *user_id).await;
        }

        tracing::info!(
            user_id = %user_id,
            username = %username,
            attachments_cleaned = s3_objects.attachment_keys.len(),
            exports_cleaned = s3_objects.export_keys.len(),
            "Account deletion completed"
        );
    }

    let count = due_users.len();
    tracing::info!(count, "Processed pending account deletions");

    Ok(())
}
