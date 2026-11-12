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
//! - **Two lanes.** Each tick's row budget is split in half: the *fresh* lane takes never-tried
//!   keys, newest first, so a file deleted just now does not wait behind an old backlog; the
//!   *retry* lane takes due retries (`attempts > 0`, `next_attempt_at <= now()`), the longest-due
//!   first, so a key that failed is retried once its backoff ran out however many new keys arrive.
//!   A lane may use what the other leaves unused. Within the tick the two lanes alternate, so a
//!   slow store shares the wall time between them.
//! - **Try now.** After a delete commits, [`spawn_drain`] tries exactly the keys that delete queued
//!   (fresh only), nothing older.
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
pub const TRY_NOW_ROWS: i64 = 50;
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

/// Which keys a claim may take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lane {
    /// Never-tried keys, newest first.
    Fresh,
    /// Due retries (`attempts > 0`), longest-due first.
    Retry,
}

/// Claim up to `limit` due keys of one lane (optionally only `only`), and
/// lease them. One atomic statement; committed before any store call.
async fn claim(
    pool: &PgPool,
    lane: Lane,
    limit: i64,
    only: Option<&[String]>,
) -> sqlx::Result<Vec<(String, i32)>> {
    if limit <= 0 {
        return Ok(Vec::new());
    }
    // Two literal statements (one per lane); only the filter and order differ.
    let sql = match lane {
        Lane::Fresh => {
            "UPDATE storage_pending_deletes d
             SET next_attempt_at = NOW() + make_interval(secs => $2)
             FROM (
                 SELECT s3_key FROM storage_pending_deletes
                 WHERE next_attempt_at <= NOW() AND attempts = 0
                   AND ($3::text[] IS NULL OR s3_key = ANY($3))
                 ORDER BY created_at DESC, s3_key
                 LIMIT $1
                 FOR UPDATE SKIP LOCKED
             ) due
             WHERE d.s3_key = due.s3_key
             RETURNING d.s3_key, d.attempts"
        }
        Lane::Retry => {
            "UPDATE storage_pending_deletes d
             SET next_attempt_at = NOW() + make_interval(secs => $2)
             FROM (
                 SELECT s3_key FROM storage_pending_deletes
                 WHERE next_attempt_at <= NOW() AND attempts > 0
                   AND ($3::text[] IS NULL OR s3_key = ANY($3))
                 ORDER BY next_attempt_at, s3_key
                 LIMIT $1
                 FOR UPDATE SKIP LOCKED
             ) due
             WHERE d.s3_key = due.s3_key
             RETURNING d.s3_key, d.attempts"
        }
    };
    sqlx::query_as(sql)
        .bind(limit)
        .bind(interval_secs(LEASE))
        .bind(only)
        .fetch_all(pool)
        .await
}

/// Claim one tick's keys: half the budget per lane, each lane may use what
/// the other leaves. Returns `(fresh, retries)`.
async fn claim_lanes(
    pool: &PgPool,
    max_rows: i64,
) -> sqlx::Result<(Vec<(String, i32)>, Vec<(String, i32)>)> {
    let half = max_rows / 2;
    let mut retries = claim(pool, Lane::Retry, half, None).await?;
    let fresh = claim(pool, Lane::Fresh, max_rows - len_i64(&retries), None).await?;
    let left = max_rows - len_i64(&retries) - len_i64(&fresh);
    retries.extend(claim(pool, Lane::Retry, left, None).await?);
    Ok((fresh, retries))
}

fn len_i64<T>(v: &[T]) -> i64 {
    i64::try_from(v.len()).unwrap_or(i64::MAX)
}

/// The two lanes alternating: fresh, retry, fresh, retry, …
fn interleave<T>(a: Vec<T>, b: Vec<T>) -> Vec<T> {
    let mut out = Vec::with_capacity(a.len() + b.len());
    let (mut a, mut b) = (a.into_iter(), b.into_iter());
    loop {
        match (a.next(), b.next()) {
            (None, None) => return out,
            (x, y) => out.extend(x.into_iter().chain(y)),
        }
    }
}

/// Delete up to `limits.max_rows` due objects within `limits.budget`
/// (both lanes, see the module docs).
///
/// Without object storage nothing happens (the keys stay queued).
pub async fn drain_pending_deletes<S: ObjectStore>(
    pool: &PgPool,
    store: Option<&S>,
    limits: DrainLimits,
) -> sqlx::Result<DrainReport> {
    let Some(store) = store else {
        return Ok(DrainReport::default());
    };
    let (fresh, retries) = claim_lanes(pool, limits.max_rows).await?;
    delete_claimed(pool, store, limits, interleave(fresh, retries), None).await
}

/// Delete the claimed keys in order; stop early at the wall-time budget or
/// when `stop` fires (after the store call in progress), releasing the rest.
async fn delete_claimed<S: ObjectStore>(
    pool: &PgPool,
    store: &S,
    limits: DrainLimits,
    claimed: Vec<(String, i32)>,
    stop: Option<&tokio::sync::watch::Receiver<bool>>,
) -> sqlx::Result<DrainReport> {
    let mut report = DrainReport::default();
    let started = Instant::now();
    let mut rest = claimed.into_iter();
    for (key, attempts) in rest.by_ref() {
        let left = limits.budget.saturating_sub(started.elapsed());
        if left.is_zero() || stop.is_some_and(|s| *s.borrow()) {
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
    let (_keep, never) = tokio::sync::watch::channel(false);
    run_tick_until(pool, store, DrainLimits::TICK, &never).await;
}

async fn run_tick_until<S: ObjectStore>(
    pool: &PgPool,
    store: Option<&S>,
    limits: DrainLimits,
    stop: &tokio::sync::watch::Receiver<bool>,
) {
    if let Err(e) = backfill_batch(pool, BACKFILL_BATCH).await {
        tracing::warn!(error = %e, "Clean-up of files of earlier deleted messages failed");
    }
    let drained = match store {
        Some(store) => match claim_lanes(pool, limits.max_rows).await {
            Ok((fresh, retries)) => {
                delete_claimed(pool, store, limits, interleave(fresh, retries), Some(stop)).await
            }
            Err(e) => Err(e),
        },
        None => Ok(DrainReport::default()),
    };
    match drained {
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
///
/// Returns the queued object keys (for [`spawn_drain`] after the commit).
pub async fn remove_attachments_of_user(
    conn: &mut sqlx::PgConnection,
    user_id: uuid::Uuid,
) -> sqlx::Result<Vec<String>> {
    let rows: Vec<(String, Option<String>, Option<String>)> = sqlx::query_as(
        "DELETE FROM file_attachments fa USING messages m
         WHERE m.id = fa.message_id AND m.user_id = $1
         RETURNING fa.s3_key, fa.thumbnail_s3_key, fa.medium_s3_key",
    )
    .bind(user_id)
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .flat_map(|(a, b, c)| [Some(a), b, c])
        .flatten()
        .filter(|k| !k.is_empty())
        .collect())
}

/// The running object-cleanup worker.
///
/// [`CleanupWorker::shutdown`] stops it and waits; dropping the handle
/// stops it after the current tick without waiting.
#[must_use = "keep the handle to shut the worker down"]
pub struct CleanupWorker {
    stop: tokio::sync::watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

impl CleanupWorker {
    /// Stop the worker and wait for it: a tick in progress ends after the
    /// object-store call it is in (at most [`DELETE_TIMEOUT`]) and releases
    /// the keys it has not reached. A killed tick loses nothing either: its
    /// keys come back when their [`LEASE`] runs out.
    pub async fn shutdown(self) {
        let _ = self.stop.send(true);
        if let Err(e) = self.task.await {
            if e.is_panic() {
                tracing::error!(error = %e, "Object clean-up worker panicked");
            }
        }
    }
}

/// Start the object-cleanup worker (its own task, every [`TICK_INTERVAL`]).
pub fn spawn(state: &AppState) -> CleanupWorker {
    spawn_with(
        state.db.clone(),
        state.s3.clone(),
        TICK_INTERVAL,
        DrainLimits::TICK,
    )
}

/// [`spawn`] with an explicit store, interval and bounds (tests).
pub fn spawn_with<S: ObjectStore + Send + 'static>(
    pool: PgPool,
    store: Option<S>,
    every: Duration,
    limits: DrainLimits,
) -> CleanupWorker {
    let (stop, mut stopped) = tokio::sync::watch::channel(false);
    let task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(every);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = interval.tick() => {}
                _ = stopped.changed() => return,
            }
            if *stopped.borrow() {
                return;
            }
            run_tick_until(&pool, store.as_ref(), limits, &stopped).await;
        }
    });
    CleanupWorker { stop, task }
}

/// Try the keys a just-committed delete queued (and only those), in the
/// background. Older keys are the worker's.
pub fn spawn_drain(state: &AppState, keys: Vec<String>) {
    if state.s3.is_none() || keys.is_empty() {
        return;
    }
    let pool = state.db.clone();
    let s3 = state.s3.clone();
    tokio::spawn(async move {
        let result = async {
            let mut claimed = Vec::new();
            for chunk in keys.chunks(usize::try_from(TRY_NOW_ROWS).unwrap_or(50)) {
                claimed.extend(claim(&pool, Lane::Fresh, TRY_NOW_ROWS, Some(chunk)).await?);
            }
            match s3.as_ref() {
                Some(s3) => delete_claimed(&pool, s3, DrainLimits::TRY_NOW, claimed, None).await,
                None => Ok(DrainReport::default()),
            }
        }
        .await;
        if let Err(e) = result {
            tracing::warn!(error = %e, "Draining pending object deletes failed");
        }
    });
}

/// Object keys (original and variants) of a message's attachments; read
/// before deleting it, to hand to [`spawn_drain`] afterwards.
pub async fn attachment_keys_of_message(
    pool: &PgPool,
    message_id: uuid::Uuid,
) -> sqlx::Result<Vec<String>> {
    sqlx::query_scalar(
        "SELECT k FROM file_attachments fa,
             unnest(ARRAY[fa.s3_key, fa.thumbnail_s3_key, fa.medium_s3_key]) AS k
         WHERE fa.message_id = $1 AND k IS NOT NULL AND k <> ''",
    )
    .bind(message_id)
    .fetch_all(pool)
    .await
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
