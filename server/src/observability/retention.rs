//! Telemetry retention and rollup refresh jobs.
//!
//! Runs hourly to:
//! 1. Refresh the `telemetry_trend_rollups` materialized view concurrently.
//! 2. Hard-delete rows older than 30 days from all native telemetry tables.
//!
//! Design reference: §11.5 (Retention Policies)

use std::time::{Duration, Instant};

use sqlx::PgPool;

const RETENTION_DAYS: i32 = 30;
const DELETE_BATCH_SIZE: i64 = 10_000;

/// Start the hourly retention and rollup refresh background task.
///
/// This spawns a tokio task that runs every hour. The first tick is consumed
/// immediately to avoid running a retention cycle during startup when the
/// server is handling its initial request burst.
///
/// The returned `JoinHandle` should be stored alongside other background
/// task handles in `main`.
pub fn spawn_retention_task(pool: PgPool) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(3600));
        interval.tick().await; // consume immediate first tick
        loop {
            interval.tick().await;
            run_retention_cycle(&pool).await;
        }
    })
}

/// Execute one retention + rollup refresh cycle.
///
/// Refreshes the materialized view *before* purging so that boundary-day data
/// is captured in the rollup before deletion. Logs execution time and rows
/// deleted via tracing (not to native telemetry tables, to avoid circular
/// ingestion).
#[tracing::instrument(skip(pool))]
async fn run_retention_cycle(pool: &PgPool) {
    let start = Instant::now();

    // Refresh rollups FIRST so boundary-day data is captured before deletion
    refresh_trend_rollups(pool).await;

    let metrics_deleted = purge_old_metric_samples(pool).await;
    let logs_deleted = purge_old_log_events(pool).await;
    let traces_deleted = purge_old_trace_index(pool).await;

    let elapsed = start.elapsed();
    tracing::info!(
        elapsed_ms = elapsed.as_millis() as u64,
        metrics_deleted,
        logs_deleted,
        traces_deleted,
        "Telemetry retention cycle completed"
    );
}

/// Delete metric samples older than 30 days.
///
/// Attempts `TimescaleDB` `drop_chunks` first for efficient chunk-level deletion.
/// Falls back to batched `DELETE` if `TimescaleDB` is not available.
async fn purge_old_metric_samples(pool: &PgPool) -> i64 {
    // Try TimescaleDB drop_chunks first (much faster for hypertables)
    let ts_result = sqlx::query(
        "SELECT drop_chunks('telemetry_metric_samples', older_than => INTERVAL '30 days')",
    )
    .execute(pool)
    .await;

    match ts_result {
        Ok(_) => {
            tracing::debug!("Used TimescaleDB drop_chunks for metric samples");
            // drop_chunks doesn't return affected row count easily, report 0
            0
        }
        Err(_) => {
            // Fallback: batched DELETE to avoid long-held locks
            purge_in_batches(
                pool,
                "DELETE FROM telemetry_metric_samples WHERE ctid IN (\
                     SELECT ctid FROM telemetry_metric_samples \
                     WHERE ts < NOW() - make_interval(days => $1) LIMIT $2\
                 )",
                "metric samples",
            )
            .await
        }
    }
}

/// Delete log events older than 30 days in batches.
async fn purge_old_log_events(pool: &PgPool) -> i64 {
    purge_in_batches(
        pool,
        "DELETE FROM telemetry_log_events WHERE id IN (\
             SELECT id FROM telemetry_log_events \
             WHERE ts < NOW() - make_interval(days => $1) LIMIT $2\
         )",
        "log events",
    )
    .await
}

/// Delete trace index entries older than 30 days in batches.
async fn purge_old_trace_index(pool: &PgPool) -> i64 {
    purge_in_batches(
        pool,
        "DELETE FROM telemetry_trace_index WHERE id IN (\
             SELECT id FROM telemetry_trace_index \
             WHERE ts < NOW() - make_interval(days => $1) LIMIT $2\
         )",
        "trace index entries",
    )
    .await
}

/// Execute batched DELETEs to avoid holding table-level locks for too long.
///
/// Deletes up to [`DELETE_BATCH_SIZE`] rows per iteration until no more rows
/// match the retention cutoff. The SQL must accept `$1` (retention days) and
/// `$2` (batch size limit).
async fn purge_in_batches(pool: &PgPool, sql: &str, table_label: &str) -> i64 {
    let mut total_deleted: i64 = 0;
    loop {
        match sqlx::query(sql)
            .bind(RETENTION_DAYS)
            .bind(DELETE_BATCH_SIZE)
            .execute(pool)
            .await
        {
            Ok(result) => {
                let deleted = result.rows_affected() as i64;
                total_deleted += deleted;
                if deleted < DELETE_BATCH_SIZE {
                    break;
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, table = table_label, "Failed to purge old {table_label}");
                break;
            }
        }
    }
    total_deleted
}

/// Refresh the trend rollups materialized view concurrently.
///
/// `CONCURRENTLY` allows reads during refresh (requires the unique index).
async fn refresh_trend_rollups(pool: &PgPool) {
    let start = Instant::now();
    match sqlx::query("REFRESH MATERIALIZED VIEW CONCURRENTLY telemetry_trend_rollups")
        .execute(pool)
        .await
    {
        Ok(_) => {
            tracing::debug!(
                elapsed_ms = start.elapsed().as_millis() as u64,
                "Refreshed telemetry_trend_rollups"
            );
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to refresh telemetry_trend_rollups");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_days_is_30() {
        assert_eq!(RETENTION_DAYS, 30);
    }
}
