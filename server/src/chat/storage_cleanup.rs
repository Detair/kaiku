//! Deleting stored objects whose rows are gone.
//!
//! Removing a `file_attachments` row (directly, through a soft-deleted
//! message or through a foreign-key cascade) queues its object keys in
//! `storage_pending_deletes` inside the same transaction (database trigger,
//! migration `20261123000000`). This module deletes the queued objects:
//! right after a message delete commits ([`spawn_drain`]) and every minute in
//! the auth sweep (`auth::timers`), which retries keys whose delete failed.
//! A key leaves the queue only after its object delete succeeded.

use sqlx::PgPool;

use super::S3Client;
use crate::api::AppState;

/// Keys deleted per drain run.
pub const DRAIN_BATCH: i64 = 100;

/// Delete up to `limit` queued objects, oldest first.
///
/// Returns the number of objects deleted. Without object storage nothing
/// happens (the keys stay queued). A failed delete is logged and the key
/// stays queued with its attempt count raised.
pub async fn drain_pending_deletes(
    pool: &PgPool,
    s3: Option<&S3Client>,
    limit: i64,
) -> sqlx::Result<usize> {
    let Some(s3) = s3 else {
        return Ok(0);
    };
    let keys: Vec<String> = sqlx::query_scalar(
        "SELECT s3_key FROM storage_pending_deletes ORDER BY created_at, s3_key LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let mut deleted = 0;
    for key in keys {
        match s3.delete(&key).await {
            Ok(()) => {
                sqlx::query("DELETE FROM storage_pending_deletes WHERE s3_key = $1")
                    .bind(&key)
                    .execute(pool)
                    .await?;
                deleted += 1;
            }
            Err(e) => {
                tracing::warn!(
                    s3_key = %key,
                    error = %e,
                    "Failed to delete stored object; will retry"
                );
                sqlx::query(
                    "UPDATE storage_pending_deletes
                     SET attempts = attempts + 1, last_attempt_at = NOW()
                     WHERE s3_key = $1",
                )
                .bind(&key)
                .execute(pool)
                .await?;
            }
        }
    }
    Ok(deleted)
}

/// Drain the queue in the background (after a delete committed).
pub fn spawn_drain(state: &AppState) {
    if state.s3.is_none() {
        return;
    }
    let pool = state.db.clone();
    let s3 = state.s3.clone();
    tokio::spawn(async move {
        if let Err(e) = drain_pending_deletes(&pool, s3.as_ref(), DRAIN_BATCH).await {
            tracing::warn!(error = %e, "Draining pending object deletes failed");
        }
    });
}
