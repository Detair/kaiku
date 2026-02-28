//! Admin Observability API handlers.
//!
//! Read-only endpoints for the Command Center's observability tab.
//! All routes require `SystemAdminUser` middleware (non-elevated).
//!
//! Design reference: command-center-design-v2 §3–§6, §12

use axum::extract::{Query, State};
use axum::{Extension, Json};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::types::{AdminError, SystemAdminUser};
use crate::api::AppState;
use crate::observability::storage;

// ============================================================================
// Time Range Parsing
// ============================================================================

/// Supported time range values for trend and top-offender queries.
#[derive(Debug, Clone, Copy, Deserialize)]
pub enum TimeRange {
    #[serde(rename = "1h")]
    OneHour,
    #[serde(rename = "6h")]
    SixHours,
    #[serde(rename = "24h")]
    TwentyFourHours,
    #[serde(rename = "7d")]
    SevenDays,
    #[serde(rename = "30d")]
    ThirtyDays,
}

impl TimeRange {
    const fn to_duration(self) -> Duration {
        match self {
            Self::OneHour => Duration::hours(1),
            Self::SixHours => Duration::hours(6),
            Self::TwentyFourHours => Duration::hours(24),
            Self::SevenDays => Duration::days(7),
            Self::ThirtyDays => Duration::days(30),
        }
    }

    /// Compute `(from, to)` timestamps from this range.
    fn to_time_bounds(self) -> (DateTime<Utc>, DateTime<Utc>) {
        let to = Utc::now();
        let from = to - self.to_duration();
        (from, to)
    }
}

// ============================================================================
// Query Parameters
// ============================================================================

/// Trends query parameters.
#[derive(Debug, Deserialize)]
pub struct TrendsParams {
    pub range: TimeRange,
    /// One or more metric names (repeated `metric=...` params).
    #[serde(default)]
    pub metric: Vec<String>,
}

/// Top routes query parameters.
#[derive(Debug, Deserialize)]
pub struct TopRoutesParams {
    pub range: TimeRange,
    /// Sort by `latency` or `errors`.
    #[serde(default = "default_sort")]
    pub sort: String,
    #[serde(default = "default_top_limit")]
    pub limit: i64,
}

fn default_sort() -> String {
    "latency".to_string()
}

const fn default_top_limit() -> i64 {
    10
}

/// Top errors query parameters.
#[derive(Debug, Deserialize)]
pub struct TopErrorsParams {
    pub range: TimeRange,
    #[serde(default = "default_top_limit")]
    pub limit: i64,
}

/// Logs query parameters (cursor-based pagination).
#[derive(Debug, Deserialize)]
pub struct LogsParams {
    pub level: Option<String>,
    pub domain: Option<String>,
    pub service: Option<String>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub search: Option<String>,
    pub cursor: Option<Uuid>,
    #[serde(default = "default_log_limit")]
    pub limit: i64,
}

const fn default_log_limit() -> i64 {
    100
}

/// Traces query parameters (cursor-based pagination).
#[derive(Debug, Deserialize)]
pub struct TracesParams {
    pub status: Option<String>,
    pub domain: Option<String>,
    pub route: Option<String>,
    pub duration_min: Option<i32>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub cursor: Option<Uuid>,
    #[serde(default = "default_log_limit")]
    pub limit: i64,
}

// ============================================================================
// Response Types
// ============================================================================

/// Summary response — vital signs, server metadata, active alert count.
#[derive(Debug, Serialize)]
pub struct SummaryResponse {
    pub vital_signs: VitalSigns,
    pub server_metadata: ServerMetadata,
    pub voice_health_score: Option<f64>,
    pub active_alert_count: i64,
}

#[derive(Debug, Serialize)]
pub struct VitalSigns {
    pub latency_p95_ms: Option<f64>,
    pub error_rate_percent: Option<f64>,
    pub active_ws_connections: Option<i64>,
    pub active_voice_sessions: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ServerMetadata {
    pub version: &'static str,
    pub active_user_count: i64,
    pub guild_count: i64,
}

/// Trends response — metric time series.
#[derive(Debug, Serialize)]
pub struct TrendsResponse {
    pub metrics: Vec<MetricTrend>,
}

#[derive(Debug, Serialize)]
pub struct MetricTrend {
    pub metric_name: String,
    pub datapoints: Vec<storage::TrendDataPoint>,
}

/// Top routes response.
#[derive(Debug, Serialize)]
pub struct TopRoutesResponse {
    pub routes: Vec<RouteEntry>,
}

#[derive(Debug, Serialize)]
pub struct RouteEntry {
    pub route: Option<String>,
    pub request_count: i64,
    pub error_count: i64,
    pub error_rate_percent: f64,
    pub latency_p95_ms: Option<f64>,
}

/// Top errors response.
#[derive(Debug, Serialize)]
pub struct TopErrorsResponse {
    pub error_categories: Vec<ErrorCategoryEntry>,
}

#[derive(Debug, Serialize)]
pub struct ErrorCategoryEntry {
    pub error_type: Option<String>,
    pub count: i64,
    pub avg_p95_ms: Option<f64>,
}

/// Logs response with cursor-based pagination.
#[derive(Debug, Serialize)]
pub struct LogsResponse {
    pub logs: Vec<storage::LogEvent>,
    pub next_cursor: Option<Uuid>,
}

/// Traces response with cursor-based pagination.
#[derive(Debug, Serialize)]
pub struct TracesResponse {
    pub traces: Vec<storage::TraceIndexEntry>,
    pub next_cursor: Option<Uuid>,
}

/// External observability tool links.
#[derive(Debug, Serialize)]
pub struct LinksResponse {
    pub grafana_url: Option<String>,
    pub tempo_url: Option<String>,
    pub loki_url: Option<String>,
    pub prometheus_url: Option<String>,
}

// ============================================================================
// Handlers
// ============================================================================

/// `GET /api/admin/observability/summary`
///
/// Returns vital signs, server metadata, voice health, and recent error count.
#[tracing::instrument(skip(state, _admin))]
pub async fn summary(
    Extension(_admin): Extension<SystemAdminUser>,
    State(state): State<AppState>,
) -> Result<Json<SummaryResponse>, AdminError> {
    let now = Utc::now();
    let five_min_ago = now - Duration::minutes(5);

    // Fetch recent metrics for vital signs (last 5 minutes)
    let latency_p95 = sqlx::query_scalar::<_, Option<f64>>(
        "SELECT AVG(value_p95) FROM telemetry_metric_samples \
         WHERE metric_name = 'kaiku_http_request_duration_ms' \
         AND ts >= $1 AND ts <= $2",
    )
    .bind(five_min_ago)
    .bind(now)
    .fetch_optional(&state.db)
    .await?
    .flatten();

    let error_metrics: Option<(Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT \
             SUM(CASE WHEN metric_name = 'kaiku_http_errors_total' THEN value_count ELSE 0 END), \
             SUM(CASE WHEN metric_name = 'kaiku_http_requests_total' THEN value_count ELSE 0 END) \
         FROM telemetry_metric_samples \
         WHERE metric_name IN ('kaiku_http_errors_total', 'kaiku_http_requests_total') \
         AND ts >= $1 AND ts <= $2",
    )
    .bind(five_min_ago)
    .bind(now)
    .fetch_optional(&state.db)
    .await?;

    let error_rate_percent = error_metrics.and_then(|(errors, total)| {
        let e = errors.unwrap_or(0) as f64;
        let t = total.unwrap_or(0) as f64;
        if t > 0.0 { Some(e / t * 100.0) } else { None }
    });

    // Active WebSocket connections and voice sessions from most recent gauge samples
    let ws_connections = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT value_count FROM telemetry_metric_samples \
         WHERE metric_name = 'kaiku_ws_connections_active' \
         AND ts >= $1 \
         ORDER BY ts DESC LIMIT 1",
    )
    .bind(five_min_ago)
    .fetch_optional(&state.db)
    .await?
    .flatten();

    let voice_sessions = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT value_count FROM telemetry_metric_samples \
         WHERE metric_name = 'kaiku_voice_sessions_active' \
         AND ts >= $1 \
         ORDER BY ts DESC LIMIT 1",
    )
    .bind(five_min_ago)
    .fetch_optional(&state.db)
    .await?
    .flatten();

    // Server metadata from core tables
    let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await?;
    let guild_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM guilds")
        .fetch_one(&state.db)
        .await?;

    // Recent error count (last 5 minutes)
    let active_alert_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM telemetry_log_events \
         WHERE level = 'ERROR' AND ts >= $1",
    )
    .bind(five_min_ago)
    .fetch_one(&state.db)
    .await?;

    // Voice health score (cached, refreshed every 10s)
    let voice_health_score = crate::observability::voice::get_voice_health_score().await;

    Ok(Json(SummaryResponse {
        vital_signs: VitalSigns {
            latency_p95_ms: latency_p95,
            error_rate_percent,
            active_ws_connections: ws_connections,
            active_voice_sessions: voice_sessions,
        },
        server_metadata: ServerMetadata {
            version: env!("CARGO_PKG_VERSION"),
            active_user_count: user_count,
            guild_count,
        },
        voice_health_score,
        active_alert_count,
    }))
}

/// `GET /api/admin/observability/trends`
///
/// Returns time-series data for requested metrics over the given range.
#[tracing::instrument(skip(state, _admin))]
pub async fn trends(
    Extension(_admin): Extension<SystemAdminUser>,
    State(state): State<AppState>,
    Query(params): Query<TrendsParams>,
) -> Result<Json<TrendsResponse>, AdminError> {
    if params.metric.is_empty() {
        return Err(AdminError::Validation(
            "At least one 'metric' parameter is required".into(),
        ));
    }

    if params.metric.len() > 10 {
        return Err(AdminError::Validation(
            "At most 10 metrics can be requested at once".into(),
        ));
    }

    let (from, to) = params.range.to_time_bounds();

    let mut metrics = Vec::with_capacity(params.metric.len());
    for metric_name in &params.metric {
        let datapoints = storage::query_trends(&state.db, metric_name, from, to).await?;
        metrics.push(MetricTrend {
            metric_name: metric_name.clone(),
            datapoints,
        });
    }

    Ok(Json(TrendsResponse { metrics }))
}

/// `GET /api/admin/observability/top-routes`
///
/// Returns top routes ranked by latency or error count.
#[tracing::instrument(skip(state, _admin))]
pub async fn top_routes(
    Extension(_admin): Extension<SystemAdminUser>,
    State(state): State<AppState>,
    Query(params): Query<TopRoutesParams>,
) -> Result<Json<TopRoutesResponse>, AdminError> {
    let (from, to) = params.range.to_time_bounds();
    let sort_by_errors = params.sort == "errors";
    let limit = params.limit.clamp(1, 10);

    let raw = storage::query_top_routes(&state.db, from, to, sort_by_errors, limit).await?;

    let routes = raw
        .into_iter()
        .map(|r| {
            let req = r.request_count.unwrap_or(0);
            let err = r.error_count.unwrap_or(0);
            let error_rate = if req > 0 {
                err as f64 / req as f64 * 100.0
            } else {
                0.0
            };
            RouteEntry {
                route: r.route,
                request_count: req,
                error_count: err,
                error_rate_percent: error_rate,
                latency_p95_ms: r.avg_p95,
            }
        })
        .collect();

    Ok(Json(TopRoutesResponse { routes }))
}

/// `GET /api/admin/observability/top-errors`
///
/// Returns top error categories ranked by count.
#[tracing::instrument(skip(state, _admin))]
pub async fn top_errors(
    Extension(_admin): Extension<SystemAdminUser>,
    State(state): State<AppState>,
    Query(params): Query<TopErrorsParams>,
) -> Result<Json<TopErrorsResponse>, AdminError> {
    let (from, to) = params.range.to_time_bounds();
    let limit = params.limit.clamp(1, 10);

    let raw = storage::query_top_errors(&state.db, from, to, limit).await?;

    let error_categories = raw
        .into_iter()
        .map(|r| ErrorCategoryEntry {
            error_type: r.route, // query_top_errors aliases error.type into route field
            count: r.error_count.unwrap_or(0),
            avg_p95_ms: r.avg_p95,
        })
        .collect();

    Ok(Json(TopErrorsResponse { error_categories }))
}

/// `GET /api/admin/observability/logs`
///
/// Returns paginated log events with optional filters.
#[tracing::instrument(skip(state, _admin))]
pub async fn logs(
    Extension(_admin): Extension<SystemAdminUser>,
    State(state): State<AppState>,
    Query(params): Query<LogsParams>,
) -> Result<Json<LogsResponse>, AdminError> {
    let now = Utc::now();
    let limit = params.limit.clamp(1, 100);

    let filter = storage::LogFilter {
        level: params.level,
        domain: params.domain,
        service: params.service,
        search: params.search,
        from: params.from.unwrap_or(now - Duration::hours(24)),
        to: params.to.unwrap_or(now),
        cursor: params.cursor,
        limit,
    };

    let items = storage::query_logs(&state.db, &filter).await?;
    // Only provide next_cursor when the page is full (more results likely exist)
    let next_cursor = if items.len() as i64 == limit {
        items.last().map(|l| l.id)
    } else {
        None
    };

    Ok(Json(LogsResponse {
        logs: items,
        next_cursor,
    }))
}

/// `GET /api/admin/observability/traces`
///
/// Returns paginated trace index entries with optional filters.
#[tracing::instrument(skip(state, _admin))]
pub async fn traces(
    Extension(_admin): Extension<SystemAdminUser>,
    State(state): State<AppState>,
    Query(params): Query<TracesParams>,
) -> Result<Json<TracesResponse>, AdminError> {
    let now = Utc::now();
    let limit = params.limit.clamp(1, 100);

    // Map "error"/"slow" status filter to is_error/duration_min
    let (is_error, duration_min) = match params.status.as_deref() {
        Some("error") => (true, params.duration_min),
        Some("slow") => (false, Some(params.duration_min.unwrap_or(1000))),
        _ => (false, params.duration_min),
    };

    let filter = storage::TraceFilter {
        status_code: None,
        is_error,
        domain: params.domain,
        route: params.route,
        duration_min,
        from: params.from.unwrap_or(now - Duration::hours(24)),
        to: params.to.unwrap_or(now),
        cursor: params.cursor,
        limit,
    };

    let items = storage::query_traces(&state.db, &filter).await?;
    let next_cursor = if items.len() as i64 == limit {
        items.last().map(|t| t.id)
    } else {
        None
    };

    Ok(Json(TracesResponse {
        traces: items,
        next_cursor,
    }))
}

/// `GET /api/admin/observability/links`
///
/// Returns configured external observability tool URLs from environment.
#[tracing::instrument(skip(_admin))]
pub async fn links(
    Extension(_admin): Extension<SystemAdminUser>,
) -> Json<LinksResponse> {
    Json(LinksResponse {
        grafana_url: std::env::var("GRAFANA_URL").ok(),
        tempo_url: std::env::var("TEMPO_URL").ok(),
        loki_url: std::env::var("LOKI_URL").ok(),
        prometheus_url: std::env::var("PROMETHEUS_URL").ok(),
    })
}

// ============================================================================
// Router
// ============================================================================

/// Build the observability sub-router.
///
/// All routes are mounted under `/api/admin/observability/*` and require
/// `SystemAdminUser` middleware (non-elevated).
pub fn router() -> axum::Router<AppState> {
    use axum::routing::get;

    axum::Router::new()
        .route("/summary", get(summary))
        .route("/trends", get(trends))
        .route("/top-routes", get(top_routes))
        .route("/top-errors", get(top_errors))
        .route("/logs", get(logs))
        .route("/traces", get(traces))
        .route("/links", get(links))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_range_durations() {
        assert_eq!(TimeRange::OneHour.to_duration(), Duration::hours(1));
        assert_eq!(TimeRange::SixHours.to_duration(), Duration::hours(6));
        assert_eq!(TimeRange::TwentyFourHours.to_duration(), Duration::hours(24));
        assert_eq!(TimeRange::SevenDays.to_duration(), Duration::days(7));
        assert_eq!(TimeRange::ThirtyDays.to_duration(), Duration::days(30));
    }

    #[test]
    fn time_range_bounds_are_sane() {
        let (from, to) = TimeRange::OneHour.to_time_bounds();
        let diff = to - from;
        assert!((diff.num_minutes() - 60).abs() <= 1);
    }

    #[test]
    fn default_limits() {
        assert_eq!(default_top_limit(), 10);
        assert_eq!(default_log_limit(), 100);
    }

    #[test]
    fn error_rate_calculation() {
        // 5 errors out of 100 requests = 5%
        let req: i64 = 100;
        let err: i64 = 5;
        let rate = err as f64 / req as f64 * 100.0;
        assert!((rate - 5.0).abs() < f64::EPSILON);
    }
}
