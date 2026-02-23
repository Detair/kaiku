//! Connection history API handlers.
//!
//! Provides endpoints for users to view their voice connection quality history.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::AuthUser;

// ============================================================================
// Error Types
// ============================================================================

/// Error types for connectivity operations.
#[derive(Debug, thiserror::Error)]
pub enum ConnectivityError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Session not found")]
    SessionNotFound,
}

impl IntoResponse for ConnectivityError {
    fn into_response(self) -> Response {
        use serde_json::json;

        let (status, code, message) = match &self {
            Self::Database(err) => {
                tracing::error!("Database error: {}", err);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    "Database error".to_string(),
                )
            }
            Self::SessionNotFound => (StatusCode::NOT_FOUND, "SESSION_NOT_FOUND", self.to_string()),
        };

        (status, Json(json!({ "error": code, "message": message }))).into_response()
    }
}

// ============================================================================
// Query Parameters
// ============================================================================

/// Pagination query parameters.
#[derive(Debug, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
pub struct PaginationParams {
    /// Maximum number of items to return (default: 20, max: 100).
    #[serde(default = "default_limit")]
    pub limit: i64,
    /// Number of items to skip (default: 0).
    #[serde(default)]
    pub offset: i64,
}

#[allow(clippy::missing_const_for_fn)]
fn default_limit() -> i64 {
    20
}

// ============================================================================
// Response Types
// ============================================================================

/// 30-day connection summary with daily breakdown.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ConnectionSummary {
    /// Number of days in the period.
    pub period_days: i32,
    /// Average latency over the period (milliseconds).
    pub avg_latency: Option<i16>,
    /// Average packet loss over the period (0.0 - 1.0).
    pub avg_packet_loss: Option<f32>,
    /// Average jitter over the period (milliseconds).
    pub avg_jitter: Option<i16>,
    /// Total number of voice sessions.
    pub total_sessions: i64,
    /// Total time spent in voice (seconds).
    pub total_duration_secs: i64,
    /// Daily statistics breakdown.
    pub daily_stats: Vec<DailyStat>,
}

/// Daily connection statistics.
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub struct DailyStat {
    /// Date of the statistics.
    pub date: NaiveDate,
    /// Average latency for the day (milliseconds).
    pub avg_latency: Option<i16>,
    /// Average packet loss for the day (0.0 - 1.0).
    pub avg_loss: Option<f32>,
    /// Average jitter for the day (milliseconds).
    pub avg_jitter: Option<i16>,
    /// Number of sessions on this day.
    pub session_count: i64,
}

/// Session summary for list view.
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub struct SessionSummary {
    /// Session ID.
    pub id: Uuid,
    /// Channel name (or "DM Call" for DM calls).
    pub channel_name: String,
    /// Guild name (or null for DM calls).
    pub guild_name: Option<String>,
    /// Session start time.
    pub started_at: DateTime<Utc>,
    /// Session end time.
    pub ended_at: DateTime<Utc>,
    /// Average latency (milliseconds).
    pub avg_latency: Option<i16>,
    /// Average packet loss (0.0 - 1.0).
    pub avg_loss: Option<f32>,
    /// Average jitter (milliseconds).
    pub avg_jitter: Option<i16>,
    /// Worst quality score observed (0=poor, 1=fair, 2=good, 3=excellent).
    pub worst_quality: Option<i16>,
}

/// Paginated session list response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SessionListResponse {
    /// List of sessions.
    pub sessions: Vec<SessionSummary>,
    /// Total number of sessions.
    pub total: i64,
    /// Current limit.
    pub limit: i64,
    /// Current offset.
    pub offset: i64,
}

/// Session detail with metrics.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SessionDetail {
    /// Session summary.
    #[serde(flatten)]
    #[schema(inline)]
    pub summary: SessionSummary,
    /// Metric data points.
    pub metrics: Vec<MetricPoint>,
    /// Whether metrics were downsampled.
    pub downsampled: bool,
}

/// Individual metric data point.
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub struct MetricPoint {
    /// Timestamp of the metric.
    pub time: DateTime<Utc>,
    /// Latency in milliseconds.
    pub latency_ms: i16,
    /// Packet loss ratio (0.0 - 1.0).
    pub packet_loss: f32,
    /// Jitter in milliseconds.
    pub jitter_ms: i16,
    /// Quality score (0=poor, 1=fair, 2=good, 3=excellent).
    pub quality: i16,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Set RLS context for the current user.
async fn set_rls_context(pool: &PgPool, user_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT set_config('app.current_user_id', $1::TEXT, true)")
        .bind(user_id.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

// ============================================================================
// Handlers
// ============================================================================

/// Aggregate stats row from database.
#[derive(Debug, FromRow)]
struct AggregateStats {
    avg_latency: Option<i16>,
    avg_loss: Option<f32>,
    avg_jitter: Option<i16>,
    total_sessions: i64,
    total_duration: i64,
}

/// GET /api/me/connection/summary
///
/// Returns 30-day aggregate stats and daily breakdown for the authenticated user.
#[utoipa::path(
    get,
    path = "/api/me/connection/summary",
    tag = "connectivity",
    responses(
        (status = 200, description = "Connection summary", body = ConnectionSummary),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state))]
pub async fn get_summary(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<ConnectionSummary>, ConnectivityError> {
    // Set RLS context
    set_rls_context(&state.db, auth.id).await?;

    // Get aggregate stats for the last 30 days
    let aggregate = sqlx::query_as::<_, AggregateStats>(
        r"
        SELECT
            AVG(avg_latency)::SMALLINT AS avg_latency,
            AVG(avg_loss)::REAL AS avg_loss,
            AVG(avg_jitter)::SMALLINT AS avg_jitter,
            COUNT(*) AS total_sessions,
            COALESCE(SUM(EXTRACT(EPOCH FROM (ended_at - started_at))::BIGINT), 0)::BIGINT AS total_duration
        FROM connection_sessions
        WHERE user_id = $1
          AND started_at >= NOW() - INTERVAL '30 days'
        ",
    )
    .bind(auth.id)
    .fetch_one(&state.db)
    .await?;

    // Get daily breakdown
    let daily_stats = sqlx::query_as::<_, DailyStat>(
        r"
        SELECT
            DATE(started_at) AS date,
            AVG(avg_latency)::SMALLINT AS avg_latency,
            AVG(avg_loss)::REAL AS avg_loss,
            AVG(avg_jitter)::SMALLINT AS avg_jitter,
            COUNT(*) AS session_count
        FROM connection_sessions
        WHERE user_id = $1
          AND started_at >= NOW() - INTERVAL '30 days'
        GROUP BY DATE(started_at)
        ORDER BY date DESC
        ",
    )
    .bind(auth.id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(ConnectionSummary {
        period_days: 30,
        avg_latency: aggregate.avg_latency,
        avg_packet_loss: aggregate.avg_loss,
        avg_jitter: aggregate.avg_jitter,
        total_sessions: aggregate.total_sessions,
        total_duration_secs: aggregate.total_duration,
        daily_stats,
    }))
}

/// GET /api/me/connection/sessions
///
/// Returns paginated list of session summaries for the authenticated user.
#[utoipa::path(
    get,
    path = "/api/me/connection/sessions",
    tag = "connectivity",
    params(PaginationParams),
    responses(
        (status = 200, description = "Active sessions", body = SessionListResponse),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state))]
pub async fn get_sessions(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(params): Query<PaginationParams>,
) -> Result<Json<SessionListResponse>, ConnectivityError> {
    // Validate pagination
    let limit = params.limit.clamp(1, 100);
    let offset = params.offset.max(0);

    // Set RLS context
    set_rls_context(&state.db, auth.id).await?;

    // Get total count
    let total: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM connection_sessions WHERE user_id = $1")
            .bind(auth.id)
            .fetch_one(&state.db)
            .await?;

    // Get sessions with channel and guild names
    let sessions = sqlx::query_as::<_, SessionSummary>(
        r"
        SELECT
            s.id,
            COALESCE(c.name, 'DM Call') AS channel_name,
            g.name AS guild_name,
            s.started_at,
            s.ended_at,
            s.avg_latency,
            s.avg_loss,
            s.avg_jitter,
            s.worst_quality
        FROM connection_sessions s
        LEFT JOIN channels c ON c.id = s.channel_id
        LEFT JOIN guilds g ON g.id = s.guild_id
        WHERE s.user_id = $1
        ORDER BY s.started_at DESC
        LIMIT $2 OFFSET $3
        ",
    )
    .bind(auth.id)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(SessionListResponse {
        sessions,
        total,
        limit,
        offset,
    }))
}

/// GET `/api/me/connection/sessions/:session_id`
///
/// Returns session detail with metrics (downsampled if >200 points).
#[utoipa::path(
    get,
    path = "/api/me/connection/sessions/{session_id}",
    tag = "connectivity",
    params(("session_id" = Uuid, Path, description = "Session ID")),
    responses(
        (status = 200, description = "Session detail", body = SessionDetail),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state))]
pub async fn get_session_detail(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(session_id): Path<Uuid>,
) -> Result<Json<SessionDetail>, ConnectivityError> {
    // Set RLS context
    set_rls_context(&state.db, auth.id).await?;

    // Get session summary
    let summary = sqlx::query_as::<_, SessionSummary>(
        r"
        SELECT
            s.id,
            COALESCE(c.name, 'DM Call') AS channel_name,
            g.name AS guild_name,
            s.started_at,
            s.ended_at,
            s.avg_latency,
            s.avg_loss,
            s.avg_jitter,
            s.worst_quality
        FROM connection_sessions s
        LEFT JOIN channels c ON c.id = s.channel_id
        LEFT JOIN guilds g ON g.id = s.guild_id
        WHERE s.id = $1 AND s.user_id = $2
        ",
    )
    .bind(session_id)
    .bind(auth.id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ConnectivityError::SessionNotFound)?;

    // Count metrics for this session
    let metric_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM connection_metrics WHERE session_id = $1")
            .bind(session_id)
            .fetch_one(&state.db)
            .await?;

    const MAX_POINTS: i64 = 200;
    let downsampled = metric_count > MAX_POINTS;

    // Get metrics (downsampled if needed)
    let metrics = if downsampled {
        // Calculate bucket size to get ~200 points
        let bucket_seconds = ((metric_count as f64 / MAX_POINTS as f64).ceil() as i64).max(1);
        let bucket_interval = format!("{bucket_seconds} seconds");

        sqlx::query_as::<_, MetricPoint>(
            r"
            SELECT
                time_bucket($1::INTERVAL, time) AS time,
                AVG(latency_ms)::SMALLINT AS latency_ms,
                AVG(packet_loss)::REAL AS packet_loss,
                AVG(jitter_ms)::SMALLINT AS jitter_ms,
                MIN(quality)::SMALLINT AS quality
            FROM connection_metrics
            WHERE session_id = $2
            GROUP BY time_bucket($1::INTERVAL, time)
            ORDER BY time ASC
            ",
        )
        .bind(&bucket_interval)
        .bind(session_id)
        .fetch_all(&state.db)
        .await?
    } else {
        // Return all metrics
        sqlx::query_as::<_, MetricPoint>(
            r"
            SELECT
                time,
                latency_ms,
                packet_loss,
                jitter_ms,
                quality
            FROM connection_metrics
            WHERE session_id = $1
            ORDER BY time ASC
            ",
        )
        .bind(session_id)
        .fetch_all(&state.db)
        .await?
    };

    Ok(Json(SessionDetail {
        summary,
        metrics,
        downsampled,
    }))
}
