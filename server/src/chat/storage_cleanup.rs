//! Deleting stored objects whose rows are gone.
//!
//! Removing a `file_attachments` row (directly, through a soft-deleted
//! message or through a foreign-key cascade) queues its object keys in
//! `storage_pending_deletes` inside the same transaction (database trigger,
//! migration `20261123000000`). This module deletes the queued objects.
//!
//! It runs as its own periodic task ([`spawn`]), separate from every other
//! sweep, so a slow or hanging object store delays nothing else:
//!
//! - **Claim.** Due keys (`next_attempt_at <= now()`) are claimed with `FOR UPDATE SKIP LOCKED` and
//!   leased for [`LEASE`], so overlapping drains (a second server instance, the post-delete
//!   [`spawn_drain`]) never take the same key. A crashed drain's lease simply runs out.
//! - **Order.** Keys never tried come first, newest first, so a file deleted just now does not wait
//!   behind an old backlog; failed keys follow.
//! - **Bounds.** At most [`TICK_ROWS`] keys and [`TICK_BUDGET`] wall time per tick; every object
//!   delete gives up after [`DELETE_TIMEOUT`]. Keys claimed but not reached in time are released
//!   without counting.
//! - **Backoff.** A failed key is retried after [`backoff`]: 1 min, doubling up to 6 h. It never
//!   blocks other keys and is never dropped; every [`ESCALATE_EVERY`] failed attempts it is logged
//!   at error level.
//! - **Done.** A key leaves the table only after its object delete succeeded or the object was
//!   already gone.
//! - **Backfill.** Attachments of messages deleted before the migration are removed in batches of
//!   [`BACKFILL_BATCH`] rows, one transaction each, skipping rows other transactions hold; the
//!   trigger queues their keys. When nothing is left the backfill records that it finished and
//!   stays idle.
//! - **Gauges.** Each tick records the queue length and the oldest key's age
//!   (`kaiku_storage_pending_deletes`, `kaiku_storage_oldest_pending_delete_seconds`).

use std::time::{Duration, Instant};

use sqlx::PgPool;

use super::S3Client;
use crate::api::AppState;

/// Interval between two worker ticks.
pub const TICK_INTERVAL: Duration = Duration::from_secs(30);
/// Wall-time budget of one tick's object deletes.
pub const TICK_BUDGET: Duration = Duration::from_secs(20);
/// Keys handled per tick at most.
pub const TICK_ROWS: i64 = 200;
/// Keys handled by the post-delete drain ([`spawn_drain`]).
pub const TRY_NOW_ROWS: i64 = 20;
/// Time limit of one object delete (SDK retries included).
pub const DELETE_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a claimed key is reserved for the drain that claimed it.
pub const LEASE: Duration = Duration::from_mins(5);
/// First retry delay; doubles per failed attempt.
pub const BACKOFF_BASE: Duration = Duration::from_mins(1);
/// Longest retry delay.
pub const BACKOFF_MAX: Duration = Duration::from_hours(6);
/// Every this many failed attempts a key is logged at error level.
pub const ESCALATE_EVERY: i32 = 20;
/// Attachment rows removed per backfill batch.
pub const BACKFILL_BATCH: i64 = 200;

/// Something that can delete stored objects (the S3 client; fakes in tests).
pub trait ObjectStore: Sync {
    /// Delete `key`, giving up after `limit`. A missing object is success.
    fn delete_object(
        &self,
        key: &str,
        limit: Duration,
    ) -> impl std::future::Future<Output = Result<(), String>> + Send;
}

impl ObjectStore for S3Client {
    async fn delete_object(&self, key: &str, limit: Duration) -> Result<(), String> {
        self.delete_within(key, limit)
            .await
            .map_err(|e| e.to_string())
    }
}

/// Bounds of one drain.
#[derive(Debug, Clone, Copy)]
pub struct DrainLimits {
    /// Keys claimed at most.
    pub max_rows: i64,
    /// Wall time for the deletes.
    pub budget: Duration,
    /// Time limit of one delete.
    pub per_delete: Duration,
}

impl DrainLimits {
    /// The periodic worker's bounds.
    pub const TICK: Self = Self {
        max_rows: TICK_ROWS,
        budget: TICK_BUDGET,
        per_delete: DELETE_TIMEOUT,
    };
    /// The post-delete drain's bounds.
    pub const TRY_NOW: Self = Self {
        max_rows: TRY_NOW_ROWS,
        budget: TICK_BUDGET,
        per_delete: DELETE_TIMEOUT,
    };
}

/// What one drain did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DrainReport {
    /// Objects deleted (or already gone); their keys left the queue.
    pub deleted: usize,
    /// Deletes that failed; their keys were rescheduled.
    pub failed: usize,
    /// Claimed keys not reached within the budget; released unchanged.
    pub released: usize,
}

/// Retry delay after `attempts` failed attempts (1 min, 2 min, 4 min, …,
/// at most 6 h).
#[must_use]
pub fn backoff(attempts: i32) -> Duration {
    let exp = u32::try_from(attempts.saturating_sub(1))
        .unwrap_or(0)
        .min(16);
    BACKOFF_BASE
        .saturating_mul(2u32.saturating_pow(exp))
        .min(BACKOFF_MAX)
}

/// Whether reaching `attempts` failed attempts is an escalation step.
#[must_use]
pub const fn escalates(attempts: i32) -> bool {
    attempts > 0 && attempts % ESCALATE_EVERY == 0
}

const fn interval_secs(d: Duration) -> f64 {
    d.as_secs_f64()
}

/// Claim up to `limit` due keys: never-tried first, newest first.
async fn claim(pool: &PgPool, limit: i64) -> sqlx::Result<Vec<(String, i32)>> {
    sqlx::query_as(
        "UPDATE storage_pending_deletes d
         SET next_attempt_at = NOW() + make_interval(secs => $2)
         FROM (
             SELECT s3_key FROM storage_pending_deletes
             WHERE next_attempt_at <= NOW()
             ORDER BY attempts, created_at DESC, s3_key
             LIMIT $1
             FOR UPDATE SKIP LOCKED
         ) due
         WHERE d.s3_key = due.s3_key
         RETURNING d.s3_key, d.attempts",
    )
    .bind(limit)
    .bind(interval_secs(LEASE))
    .fetch_all(pool)
    .await
}

/// Delete up to `limits.max_rows` due objects within `limits.budget`.
///
/// Without object storage nothing happens (the keys stay queued).
pub async fn drain_pending_deletes<S: ObjectStore>(
    pool: &PgPool,
    store: Option<&S>,
    limits: DrainLimits,
) -> sqlx::Result<DrainReport> {
    let mut report = DrainReport::default();
    let Some(store) = store else {
        return Ok(report);
    };
    let started = Instant::now();
    let mut claimed = claim(pool, limits.max_rows).await?;
    // RETURNING has no order; keep the claim order.
    claimed.sort_by_key(|(_, attempts)| *attempts);

    let mut rest = claimed.into_iter();
    for (key, attempts) in rest.by_ref() {
        let left = limits.budget.saturating_sub(started.elapsed());
        if left.is_zero() {
            release(pool, &key).await?;
            report.released += 1;
            break;
        }
        match store.delete_object(&key, limits.per_delete.min(left)).await {
            Ok(()) => {
                sqlx::query("DELETE FROM storage_pending_deletes WHERE s3_key = $1")
                    .bind(&key)
                    .execute(pool)
                    .await?;
                report.deleted += 1;
            }
            Err(e) => {
                let attempts = attempts.saturating_add(1);
                tracing::warn!(
                    s3_key = %key,
                    attempts,
                    error = %e,
                    "Failed to delete stored object; will retry"
                );
                if escalates(attempts) {
                    tracing::error!(
                        s3_key = %key,
                        attempts,
                        "Stored object still not deleted after repeated attempts; kept queued"
                    );
                }
                sqlx::query(
                    "UPDATE storage_pending_deletes
                     SET attempts = $2,
                         last_attempt_at = NOW(),
                         next_attempt_at = NOW() + make_interval(secs => $3)
                     WHERE s3_key = $1",
                )
                .bind(&key)
                .bind(attempts)
                .bind(interval_secs(backoff(attempts)))
                .execute(pool)
                .await?;
                report.failed += 1;
            }
        }
    }
    for (key, _) in rest {
        release(pool, &key).await?;
        report.released += 1;
    }
    Ok(report)
}

/// Give a claimed key back without counting an attempt.
async fn release(pool: &PgPool, key: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE storage_pending_deletes SET next_attempt_at = NOW() WHERE s3_key = $1")
        .bind(key)
        .execute(pool)
        .await?;
    Ok(())
}

/// One backfill batch: remove up to `batch` attachment rows of
/// soft-deleted messages (the trigger queues their objects).
///
/// Returns `None` once the backfill has finished (recorded, then idle),
/// otherwise the number of rows removed. Rows locked by other transactions
/// are skipped and picked up by a later batch.
pub async fn backfill_batch(pool: &PgPool, batch: i64) -> sqlx::Result<Option<u64>> {
    let finished: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT finished_at FROM storage_cleanup_backfill")
            .fetch_optional(pool)
            .await?
            .flatten();
    if finished.is_some() {
        return Ok(None);
    }
    let mut tx = pool.begin().await?;
    let removed = sqlx::query(
        "DELETE FROM file_attachments
         WHERE id IN (
             SELECT fa.id
             FROM file_attachments fa
             JOIN messages m ON m.id = fa.message_id
             WHERE m.deleted_at IS NOT NULL
             LIMIT $1
             FOR UPDATE OF fa SKIP LOCKED
         )",
    )
    .bind(batch)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if removed == 0 {
        // Finished only when nothing is left at all, not merely skipped.
        let left: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                 SELECT 1 FROM file_attachments fa
                 JOIN messages m ON m.id = fa.message_id
                 WHERE m.deleted_at IS NOT NULL
             )",
        )
        .fetch_one(&mut *tx)
        .await?;
        if !left {
            sqlx::query("UPDATE storage_cleanup_backfill SET finished_at = NOW()")
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            tracing::info!("Clean-up of files of earlier deleted messages finished");
            return Ok(None);
        }
    }
    tx.commit().await?;
    Ok(Some(removed))
}

/// Record the queue length and the oldest key's age.
async fn record_queue_state(pool: &PgPool) -> sqlx::Result<()> {
    let (pending, oldest): (i64, Option<f64>) = sqlx::query_as(
        "SELECT COUNT(*), EXTRACT(EPOCH FROM NOW() - MIN(created_at))::float8
         FROM storage_pending_deletes",
    )
    .fetch_one(pool)
    .await?;
    let pending = u64::try_from(pending).unwrap_or(0);
    // Whole seconds are enough for an age gauge.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let oldest = oldest.unwrap_or(0.0).max(0.0) as u64;
    crate::observability::metrics::set_storage_pending_deletes(pending, oldest);
    tracing::debug!(
        pending,
        oldest_age_seconds = oldest,
        "Pending object deletes"
    );
    Ok(())
}

/// One worker tick: a backfill batch, a bounded drain, the gauges.
pub async fn run_tick<S: ObjectStore>(pool: &PgPool, store: Option<&S>) {
    if let Err(e) = backfill_batch(pool, BACKFILL_BATCH).await {
        tracing::warn!(error = %e, "Clean-up of files of earlier deleted messages failed");
    }
    match drain_pending_deletes(pool, store, DrainLimits::TICK).await {
        Ok(r) if r.deleted + r.failed + r.released > 0 => tracing::debug!(
            deleted = r.deleted,
            failed = r.failed,
            released = r.released,
            "Object clean-up tick"
        ),
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %e, "Draining pending object deletes failed"),
    }
    if let Err(e) = record_queue_state(pool).await {
        tracing::warn!(error = %e, "Reading the object clean-up queue failed");
    }
}

/// Remove the attachment rows of every message `user_id` wrote.
///
/// The trigger queues their objects. For account deletion (self-service
/// and admin): the messages stay anonymized, their files go. Call in the
/// transaction that deletes the user row.
pub async fn remove_attachments_of_user(
    conn: &mut sqlx::PgConnection,
    user_id: uuid::Uuid,
) -> sqlx::Result<u64> {
    Ok(sqlx::query(
        "DELETE FROM file_attachments fa USING messages m
         WHERE m.id = fa.message_id AND m.user_id = $1",
    )
    .bind(user_id)
    .execute(conn)
    .await?
    .rows_affected())
}

/// Start the object-cleanup worker (its own task, every [`TICK_INTERVAL`]).
pub fn spawn(state: &AppState) -> tokio::task::JoinHandle<()> {
    let pool = state.db.clone();
    let s3 = state.s3.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(TICK_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            run_tick(&pool, s3.as_ref()).await;
        }
    })
}

/// Try the newest due keys right after a delete committed (background).
pub fn spawn_drain(state: &AppState) {
    if state.s3.is_none() {
        return;
    }
    let pool = state.db.clone();
    let s3 = state.s3.clone();
    tokio::spawn(async move {
        if let Err(e) = drain_pending_deletes(&pool, s3.as_ref(), DrainLimits::TRY_NOW).await {
            tracing::warn!(error = %e, "Draining pending object deletes failed");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_from_one_minute_up_to_six_hours() {
        let mins: Vec<u64> = (1..=10).map(|a| backoff(a).as_secs() / 60).collect();
        assert_eq!(mins, [1, 2, 4, 8, 16, 32, 64, 128, 256, 360]);
        assert_eq!(backoff(0), BACKOFF_BASE);
        assert_eq!(backoff(i32::MAX), BACKOFF_MAX);
    }

    #[test]
    fn escalation_every_twenty_attempts() {
        let steps: Vec<i32> = (0..=60).filter(|a| escalates(*a)).collect();
        assert_eq!(steps, [20, 40, 60]);
    }
}
