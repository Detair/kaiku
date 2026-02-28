//! Voice health score computation.
//!
//! Computes a composite 0–100 health score from `connection_metrics` and
//! `connection_sessions` data over a rolling 24-hour window.
//!
//! The score is cached in memory and refreshed every 10 seconds by a
//! background task.
//!
//! Formula (design §6.3):
//!
//! ```text
//! score = join_success_rate * 40
//!       + (1 − packet_loss_p95) * 30
//!       + (1 − jitter_p95_scaled) * 20
//!       + (1 − crash_rate) * 10
//! ```
//!
//! Design reference: §3 (Voice Health Score), §6.3 (Voice Operations)

use std::sync::OnceLock;
use std::time::Duration;

use sqlx::PgPool;
use tokio::sync::RwLock;

/// Cached voice health score (0–100), refreshed every 10 seconds.
static HEALTH_SCORE: OnceLock<RwLock<Option<f64>>> = OnceLock::new();

fn cache() -> &'static RwLock<Option<f64>> {
    HEALTH_SCORE.get_or_init(|| RwLock::new(None))
}

/// Return the most recently computed voice health score (0–100).
///
/// Returns `None` until the first computation completes (within ~10 s of
/// startup) or if there is no connection data in the last 24 hours.
pub async fn get_voice_health_score() -> Option<f64> {
    *cache().read().await
}

/// Spawn the background task that refreshes the voice health score every
/// 10 seconds. Returns a `JoinHandle` that should be stored alongside other
/// background task handles and aborted on graceful shutdown.
pub fn spawn_voice_health_task(pool: PgPool) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        loop {
            interval.tick().await;
            match compute_health_score(&pool).await {
                Ok(score) => {
                    *cache().write().await = Some(score);
                }
                Err(e) => {
                    tracing::debug!(error = %e, "Failed to compute voice health score");
                    // Keep the last-known good value in the cache
                }
            }
        }
    })
}

/// Voice latency SLA threshold in milliseconds (from project spec).
const JITTER_SLA_MS: f64 = 50.0;

/// Weights for the four health score components.
const W_JOIN: f64 = 40.0;
const W_LOSS: f64 = 30.0;
const W_JITTER: f64 = 20.0;
const W_CRASH: f64 = 10.0;

/// Compute the composite health score from the last 24 hours of data.
///
/// Uses admin RLS bypass via transaction-scoped `app.admin_bypass` to read
/// all rows across users.
async fn compute_health_score(pool: &PgPool) -> Result<f64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    crate::db::set_admin_bypass(&mut tx).await?;

    // ── Packet loss p95 and jitter p95 from connection_metrics ────────────
    let quality_row: Option<QualityAggregates> = sqlx::query_as(
        "SELECT \
             PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY packet_loss) AS loss_p95, \
             PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY jitter_ms) AS jitter_p95 \
         FROM connection_metrics \
         WHERE time >= NOW() - INTERVAL '24 hours'",
    )
    .fetch_optional(&mut *tx)
    .await?;

    // ── Session counts and crash rate ─────────────────────────────────────
    let session_row: Option<SessionAggregates> = sqlx::query_as(
        "SELECT \
             COUNT(*) AS total, \
             COUNT(*) FILTER (WHERE worst_quality = 0) AS crashed \
         FROM connection_sessions \
         WHERE ended_at >= NOW() - INTERVAL '24 hours'",
    )
    .fetch_optional(&mut *tx)
    .await?;

    tx.commit().await?;

    // ── Assemble score ────────────────────────────────────────────────────

    // Default to healthy values if no data is available
    let loss_p95: f64 = quality_row.as_ref().and_then(|r| r.loss_p95).unwrap_or(0.0);
    let jitter_p95: f64 = quality_row
        .as_ref()
        .and_then(|r| r.jitter_p95)
        .unwrap_or(0.0);

    let total_sessions: i64 = session_row.as_ref().map_or(0, |r| r.total);
    let crashed_sessions: i64 = session_row.as_ref().map_or(0, |r| r.crashed);

    let crash_rate = if total_sessions > 0 {
        crashed_sessions as f64 / total_sessions as f64
    } else {
        0.0
    };

    // Join success rate: ideally read from kaiku_voice_joins_total metric
    // (outcome=success vs failure). Until metric ingestion exposes per-label
    // counters, hold at 1.0 (neutral) so this component doesn't double-count
    // the crash rate signal. TODO: wire to metric store once available.
    let join_success_rate: f64 = 1.0;

    let jitter_scaled = (jitter_p95 / JITTER_SLA_MS).min(1.0);

    let score = join_success_rate.mul_add(
        W_JOIN,
        (1.0 - loss_p95.min(1.0)).mul_add(
            W_LOSS,
            (1.0 - jitter_scaled).mul_add(W_JITTER, (1.0 - crash_rate) * W_CRASH),
        ),
    );

    // Clamp to [0, 100]
    Ok(score.clamp(0.0, 100.0))
}

#[derive(sqlx::FromRow)]
struct QualityAggregates {
    loss_p95: Option<f64>,
    jitter_p95: Option<f64>,
}

#[derive(sqlx::FromRow)]
struct SessionAggregates {
    total: i64,
    crashed: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_weights_sum_to_100() {
        assert!((W_JOIN + W_LOSS + W_JITTER + W_CRASH - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn jitter_sla_matches_spec() {
        // Project spec: Voice-Latenz < 50ms
        assert!((JITTER_SLA_MS - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn perfect_health_is_100() {
        // All metrics perfect: join=1.0, loss=0.0, jitter=0.0, crash=0.0
        let score = 1.0_f64.mul_add(
            W_JOIN,
            1.0_f64.mul_add(W_LOSS, 1.0_f64.mul_add(W_JITTER, 1.0 * W_CRASH)),
        );
        assert!((score - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn worst_health_is_zero() {
        // All metrics worst: join=0.0, loss=1.0, jitter>=50ms, crash=1.0
        let score = 0.0_f64.mul_add(
            W_JOIN,
            0.0_f64.mul_add(W_LOSS, 0.0_f64.mul_add(W_JITTER, 0.0 * W_CRASH)),
        );
        assert!(score.abs() < f64::EPSILON);
    }
}
