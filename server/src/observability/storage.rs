//! Native telemetry storage — insert and query helpers.
//!
//! All queries enforce:
//! - Max page size: 100
//! - Max time range: 30 days
//! - Required time filters
//!
//! Uses runtime-checked queries (`sqlx::query` / `sqlx::query_as`) instead of
//! compile-time macros because the telemetry tables do not exist in the offline
//! sqlx cache yet.
//!
//! Design reference: §11 (Data Model), §12 (API Design), §16 (Performance)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

// ============================================================================
// Constants
// ============================================================================

const MAX_PAGE_SIZE: i64 = 100;
const MAX_TIME_RANGE_DAYS: i64 = 30;
/// Max rows for live trend queries (one per minute over 24h).
const MAX_TREND_ROWS: i64 = 1440;

// ============================================================================
// Types
// ============================================================================

/// A single metric sample row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSample {
    pub ts: DateTime<Utc>,
    pub metric_name: String,
    pub scope: String,
    pub labels: serde_json::Value,
    pub value_count: Option<i64>,
    pub value_sum: Option<f64>,
    pub value_p50: Option<f64>,
    pub value_p95: Option<f64>,
    pub value_p99: Option<f64>,
}

/// A single log event row.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct LogEvent {
    pub id: Uuid,
    pub ts: DateTime<Utc>,
    pub level: String,
    pub service: String,
    pub domain: String,
    pub event: String,
    pub message: String,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub attrs: serde_json::Value,
}

/// A single trace index row.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TraceIndexEntry {
    pub id: Uuid,
    pub trace_id: String,
    pub span_name: String,
    pub domain: String,
    pub route: Option<String>,
    pub status_code: Option<String>,
    pub duration_ms: i32,
    pub ts: DateTime<Utc>,
    pub service: String,
}

/// Time-series data point for trend queries.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TrendDataPoint {
    pub ts: DateTime<Utc>,
    pub metric_name: String,
    pub value_count: Option<i64>,
    pub value_sum: Option<f64>,
    pub value_p50: Option<f64>,
    pub value_p95: Option<f64>,
    pub value_p99: Option<f64>,
}

/// Daily rollup data point for 7d/30d trend queries.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TrendRollup {
    pub day: DateTime<Utc>,
    pub metric_name: String,
    pub scope: String,
    pub route: Option<String>,
    pub sample_count: Option<i64>,
    pub avg_p95: Option<f64>,
    pub max_p95: Option<f64>,
    pub total_count: Option<i64>,
    pub error_count: Option<i64>,
}

/// Route ranking entry for top-routes / top-errors queries.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TopRouteEntry {
    pub route: Option<String>,
    pub request_count: Option<i64>,
    pub error_count: Option<i64>,
    pub avg_p95: Option<f64>,
    pub max_p95: Option<f64>,
}

/// Log query filters.
#[derive(Debug, Clone, Default)]
pub struct LogFilter {
    pub level: Option<String>,
    pub domain: Option<String>,
    pub service: Option<String>,
    pub search: Option<String>,
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub cursor: Option<Uuid>,
    pub limit: i64,
}

/// Trace query filters.
#[derive(Debug, Clone, Default)]
pub struct TraceFilter {
    pub status_code: Option<String>,
    /// When true, filter to any 5xx status code (overrides `status_code`).
    pub is_error: bool,
    pub domain: Option<String>,
    pub route: Option<String>,
    pub duration_min: Option<i32>,
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub cursor: Option<Uuid>,
    pub limit: i64,
}

/// Parameters for inserting a metric sample.
#[derive(Debug, Clone)]
pub struct InsertMetricSample<'a> {
    pub ts: DateTime<Utc>,
    pub metric_name: &'a str,
    pub scope: &'a str,
    pub labels: &'a serde_json::Value,
    pub value_count: Option<i64>,
    pub value_sum: Option<f64>,
    pub value_p50: Option<f64>,
    pub value_p95: Option<f64>,
    pub value_p99: Option<f64>,
}

/// Parameters for inserting a log event.
#[derive(Debug, Clone)]
pub struct InsertLogEvent<'a> {
    pub ts: DateTime<Utc>,
    pub level: &'a str,
    pub service: &'a str,
    pub domain: &'a str,
    pub event: &'a str,
    pub message: &'a str,
    pub trace_id: Option<&'a str>,
    pub span_id: Option<&'a str>,
    pub attrs: &'a serde_json::Value,
}

/// Parameters for inserting a trace index entry.
#[derive(Debug, Clone)]
pub struct InsertTraceEntry<'a> {
    pub trace_id: &'a str,
    pub span_name: &'a str,
    pub domain: &'a str,
    pub route: Option<&'a str>,
    pub status_code: Option<&'a str>,
    pub duration_ms: i32,
    pub ts: DateTime<Utc>,
    pub service: &'a str,
}

// ============================================================================
// Insert helpers
// ============================================================================

/// Insert a pre-aggregated metric sample.
#[tracing::instrument(skip(pool, params))]
pub async fn insert_metric_sample(
    pool: &PgPool,
    params: &InsertMetricSample<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO telemetry_metric_samples \
         (ts, metric_name, scope, labels, value_count, value_sum, value_p50, value_p95, value_p99) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(params.ts)
    .bind(params.metric_name)
    .bind(params.scope)
    .bind(params.labels)
    .bind(params.value_count)
    .bind(params.value_sum)
    .bind(params.value_p50)
    .bind(params.value_p95)
    .bind(params.value_p99)
    .execute(pool)
    .await?;

    Ok(())
}

/// Insert a curated log event (WARN/ERROR only).
#[tracing::instrument(skip(pool, params))]
pub async fn insert_log_event(
    pool: &PgPool,
    params: &InsertLogEvent<'_>,
) -> Result<Uuid, sqlx::Error> {
    let row: (Uuid,) = sqlx::query_as(
        "INSERT INTO telemetry_log_events \
         (ts, level, service, domain, event, message, trace_id, span_id, attrs) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
         RETURNING id",
    )
    .bind(params.ts)
    .bind(params.level)
    .bind(params.service)
    .bind(params.domain)
    .bind(params.event)
    .bind(params.message)
    .bind(params.trace_id)
    .bind(params.span_id)
    .bind(params.attrs)
    .fetch_one(pool)
    .await?;

    Ok(row.0)
}

/// Insert a trace index entry (metadata only, no span payload).
#[tracing::instrument(skip(pool, params))]
pub async fn insert_trace_index_entry(
    pool: &PgPool,
    params: &InsertTraceEntry<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO telemetry_trace_index \
         (trace_id, span_name, domain, route, status_code, duration_ms, ts, service) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(params.trace_id)
    .bind(params.span_name)
    .bind(params.domain)
    .bind(params.route)
    .bind(params.status_code)
    .bind(params.duration_ms)
    .bind(params.ts)
    .bind(params.service)
    .execute(pool)
    .await?;

    Ok(())
}

// ============================================================================
// Query helpers
// ============================================================================

/// Query metric trend data points for a given metric and time range.
///
/// For ranges <= 24h, queries `telemetry_metric_samples` directly (capped at
/// 1440 rows). For 7d/30d, queries the `telemetry_trend_rollups` materialized
/// view. p99 is not available at daily rollup granularity and is set to NULL.
#[tracing::instrument(skip(pool))]
pub async fn query_trends(
    pool: &PgPool,
    metric_name: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<Vec<TrendDataPoint>, sqlx::Error> {
    let range = to - from;

    if range.num_days() <= 1 {
        // Live query — fine-grained data, capped to prevent unbounded results
        sqlx::query_as::<_, TrendDataPoint>(
            "SELECT ts, metric_name, value_count, value_sum, value_p50, value_p95, value_p99 \
             FROM telemetry_metric_samples \
             WHERE metric_name = $1 AND ts >= $2 AND ts <= $3 \
             ORDER BY ts ASC \
             LIMIT $4",
        )
        .bind(metric_name)
        .bind(from)
        .bind(to)
        .bind(MAX_TREND_ROWS)
        .fetch_all(pool)
        .await
    } else {
        // Rollup query — daily aggregates (p99 not available at this granularity)
        sqlx::query_as::<_, TrendDataPoint>(
            "SELECT \
                 day AS ts, \
                 metric_name, \
                 total_count AS value_count, \
                 NULL::double precision AS value_sum, \
                 NULL::double precision AS value_p50, \
                 avg_p95 AS value_p95, \
                 NULL::double precision AS value_p99 \
             FROM telemetry_trend_rollups \
             WHERE metric_name = $1 AND day >= $2 AND day <= $3 \
             ORDER BY day ASC",
        )
        .bind(metric_name)
        .bind(from)
        .bind(to)
        .fetch_all(pool)
        .await
    }
}

/// Query paginated log events with filters.
///
/// Uses composite `(ts DESC, id DESC)` ordering with a subquery-based cursor
/// for stable chronological pagination despite UUID v4 primary keys.
///
/// **Search performance note:** The `ILIKE '%term%'` search uses a leading
/// wildcard which prevents B-tree index usage, causing a sequential scan
/// within the time range. For large tables, consider adding a GIN trigram
/// index (`gin_trgm_ops`) or full-text search index in a future iteration.
#[tracing::instrument(skip(pool))]
pub async fn query_logs(pool: &PgPool, filter: &LogFilter) -> Result<Vec<LogEvent>, sqlx::Error> {
    let limit = filter.limit.min(MAX_PAGE_SIZE);
    let from = clamp_from_time(filter.from, filter.to);
    let search = filter.search.as_deref().map(escape_ilike_pattern);

    sqlx::query_as::<_, LogEvent>(
        "SELECT id, ts, level, service, domain, event, message, trace_id, span_id, attrs \
         FROM telemetry_log_events \
         WHERE ts >= $1 \
           AND ts <= $2 \
           AND ($3::text IS NULL OR level = $3) \
           AND ($4::text IS NULL OR domain = $4) \
           AND ($5::text IS NULL OR service = $5) \
           AND ($6::text IS NULL OR event ILIKE '%' || $6 || '%' OR message ILIKE '%' || $6 || '%') \
           AND ($7::uuid IS NULL OR (ts, id) < ((SELECT ts FROM telemetry_log_events WHERE id = $7), $7)) \
         ORDER BY ts DESC, id DESC \
         LIMIT $8",
    )
    .bind(from)
    .bind(filter.to)
    .bind(filter.level.as_deref())
    .bind(filter.domain.as_deref())
    .bind(filter.service.as_deref())
    .bind(search.as_deref())
    .bind(filter.cursor)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Query paginated trace index entries with filters.
///
/// Uses composite `(ts DESC, id DESC)` ordering with a subquery-based cursor
/// for stable chronological pagination despite UUID v4 primary keys.
#[tracing::instrument(skip(pool))]
pub async fn query_traces(
    pool: &PgPool,
    filter: &TraceFilter,
) -> Result<Vec<TraceIndexEntry>, sqlx::Error> {
    let limit = filter.limit.min(MAX_PAGE_SIZE);
    let from = clamp_from_time(filter.from, filter.to);

    sqlx::query_as::<_, TraceIndexEntry>(
        "SELECT id, trace_id, span_name, domain, route, status_code, duration_ms, ts, service \
         FROM telemetry_trace_index \
         WHERE ts >= $1 \
           AND ts <= $2 \
           AND ($3::text IS NULL OR status_code = $3) \
           AND ($4::bool IS NOT TRUE OR status_code LIKE '5%') \
           AND ($5::text IS NULL OR domain = $5) \
           AND ($6::text IS NULL OR route = $6) \
           AND ($7::int IS NULL OR duration_ms >= $7) \
           AND ($8::uuid IS NULL OR (ts, id) < ((SELECT ts FROM telemetry_trace_index WHERE id = $8), $8)) \
         ORDER BY ts DESC, id DESC \
         LIMIT $9",
    )
    .bind(from)
    .bind(filter.to)
    .bind(filter.status_code.as_deref())
    .bind(filter.is_error)
    .bind(filter.domain.as_deref())
    .bind(filter.route.as_deref())
    .bind(filter.duration_min)
    .bind(filter.cursor)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Query top routes ranked by p95 latency or error count.
#[tracing::instrument(skip(pool))]
pub async fn query_top_routes(
    pool: &PgPool,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    sort_by_errors: bool,
    limit: i64,
) -> Result<Vec<TopRouteEntry>, sqlx::Error> {
    let limit = limit.min(MAX_PAGE_SIZE);
    let from = clamp_from_time(from, to);

    // Two separate query strings to avoid format! SQL injection pattern.
    // Safe status_code cast: only cast values matching digit-only pattern.
    const BASE: &str = "\
        SELECT \
             labels->>'http.route' AS route, \
             SUM(value_count) AS request_count, \
             SUM(CASE \
                 WHEN labels->>'http.response.status_code' ~ '^\\d+$' \
                      AND (labels->>'http.response.status_code')::int >= 500 \
                 THEN value_count ELSE 0 END) AS error_count, \
             AVG(value_p95) AS avg_p95, \
             MAX(value_p95) AS max_p95 \
         FROM telemetry_metric_samples \
         WHERE metric_name = 'kaiku_http_request_duration_ms' \
           AND ts >= $1 AND ts <= $2 \
           AND labels->>'http.route' IS NOT NULL \
         GROUP BY labels->>'http.route'";

    const ORDER_BY_ERRORS: &str = " ORDER BY error_count DESC NULLS LAST LIMIT $3";
    const ORDER_BY_P95: &str = " ORDER BY avg_p95 DESC NULLS LAST LIMIT $3";

    let sql = if sort_by_errors {
        format!("{BASE}{ORDER_BY_ERRORS}")
    } else {
        format!("{BASE}{ORDER_BY_P95}")
    };

    sqlx::query_as::<_, TopRouteEntry>(&sql)
        .bind(from)
        .bind(to)
        .bind(limit)
        .fetch_all(pool)
        .await
}

/// Query top error categories grouped by `error.type` label.
#[tracing::instrument(skip(pool))]
pub async fn query_top_errors(
    pool: &PgPool,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    limit: i64,
) -> Result<Vec<TopRouteEntry>, sqlx::Error> {
    let limit = limit.min(MAX_PAGE_SIZE);
    let from = clamp_from_time(from, to);

    sqlx::query_as::<_, TopRouteEntry>(
        "SELECT \
             labels->>'error.type' AS route, \
             SUM(value_count) AS request_count, \
             SUM(value_count) AS error_count, \
             AVG(value_p95) AS avg_p95, \
             MAX(value_p95) AS max_p95 \
         FROM telemetry_metric_samples \
         WHERE metric_name = 'kaiku_http_errors_total' \
           AND ts >= $1 AND ts <= $2 \
           AND labels->>'error.type' IS NOT NULL \
         GROUP BY labels->>'error.type' \
         ORDER BY error_count DESC NULLS LAST \
         LIMIT $3",
    )
    .bind(from)
    .bind(to)
    .bind(limit)
    .fetch_all(pool)
    .await
}

// ============================================================================
// Helpers
// ============================================================================

/// Clamp the `from` timestamp to ensure the time range does not exceed 30 days.
fn clamp_from_time(from: DateTime<Utc>, to: DateTime<Utc>) -> DateTime<Utc> {
    let max_from = to - chrono::Duration::days(MAX_TIME_RANGE_DAYS);
    if from < max_from {
        max_from
    } else {
        from
    }
}

/// Escape ILIKE pattern metacharacters (`%` and `_`) in user-supplied search text.
fn escape_ilike_pattern(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_from_time_within_range() {
        let to = Utc::now();
        let from = to - chrono::Duration::days(7);
        let clamped = clamp_from_time(from, to);
        assert_eq!(clamped, from);
    }

    #[test]
    fn clamp_from_time_exceeds_range() {
        let to = Utc::now();
        let from = to - chrono::Duration::days(60);
        let clamped = clamp_from_time(from, to);
        let expected = to - chrono::Duration::days(MAX_TIME_RANGE_DAYS);
        assert_eq!(clamped, expected);
    }

    #[test]
    fn max_page_size_is_100() {
        assert_eq!(MAX_PAGE_SIZE, 100);
    }

    #[test]
    fn escape_ilike_pattern_handles_metacharacters() {
        assert_eq!(escape_ilike_pattern("hello%world"), "hello\\%world");
        assert_eq!(escape_ilike_pattern("test_value"), "test\\_value");
        assert_eq!(escape_ilike_pattern("normal"), "normal");
        assert_eq!(escape_ilike_pattern("a\\b%c_d"), "a\\\\b\\%c\\_d");
    }
}
