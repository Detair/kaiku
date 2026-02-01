//! Admin-facing report handlers.

use axum::{
    extract::{Path, Query, State},
    Extension, Json,
};
use uuid::Uuid;

use super::types::{
    ListReportsQuery, PaginatedReports, Report, ReportError, ReportResponse,
    ReportStatsResponse, ResolveReportRequest,
};
use crate::admin::ElevatedAdmin;
use crate::api::AppState;
use crate::ws::{broadcast_admin_event, ServerEvent};

/// GET /api/admin/reports
/// List reports with optional status/category filter and pagination.
pub async fn list_reports(
    State(state): State<AppState>,
    Query(query): Query<ListReportsQuery>,
) -> Result<Json<PaginatedReports>, ReportError> {
    let limit = query.limit.clamp(1, 100);
    let offset = query.offset.max(0);

    let reports = sqlx::query_as::<_, Report>(
        r#"SELECT * FROM user_reports
           WHERE ($1::report_status IS NULL OR status = $1)
             AND ($2::report_category IS NULL OR category = $2)
           ORDER BY created_at DESC
           LIMIT $3 OFFSET $4"#,
    )
    .bind(query.status)
    .bind(query.category)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db)
    .await?;

    let total: i64 = sqlx::query_scalar::<_, Option<i64>>(
        r#"SELECT COUNT(*) FROM user_reports
           WHERE ($1::report_status IS NULL OR status = $1)
             AND ($2::report_category IS NULL OR category = $2)"#,
    )
    .bind(query.status)
    .bind(query.category)
    .fetch_one(&state.db)
    .await?
    .unwrap_or(0);

    Ok(Json(PaginatedReports {
        items: reports.into_iter().map(ReportResponse::from).collect(),
        total,
        limit,
        offset,
    }))
}

/// GET /api/admin/reports/:id
/// Get a single report by ID with full details.
pub async fn get_report(
    State(state): State<AppState>,
    Path(report_id): Path<Uuid>,
) -> Result<Json<ReportResponse>, ReportError> {
    let report = sqlx::query_as::<_, Report>("SELECT * FROM user_reports WHERE id = $1")
        .bind(report_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(ReportError::NotFound)?;

    Ok(Json(report.into()))
}

/// POST /api/admin/reports/:id/claim
/// Claim a report for review.
pub async fn claim_report(
    State(state): State<AppState>,
    Extension(elevated): Extension<ElevatedAdmin>,
    Path(report_id): Path<Uuid>,
) -> Result<Json<ReportResponse>, ReportError> {
    let report = sqlx::query_as::<_, Report>(
        r#"UPDATE user_reports
           SET status = 'reviewing', assigned_admin_id = $2, updated_at = NOW()
           WHERE id = $1 AND status = 'pending'
           RETURNING *"#,
    )
    .bind(report_id)
    .bind(elevated.user_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ReportError::NotFound)?;

    Ok(Json(report.into()))
}

/// POST /api/admin/reports/:id/resolve
/// Resolve a report with an action.
pub async fn resolve_report(
    State(state): State<AppState>,
    Path(report_id): Path<Uuid>,
    Json(body): Json<ResolveReportRequest>,
) -> Result<Json<ReportResponse>, ReportError> {
    // Validate resolution_action
    let valid_actions = ["dismissed", "warned", "banned", "escalated"];
    if !valid_actions.contains(&body.resolution_action.as_str()) {
        return Err(ReportError::Validation(format!(
            "Invalid resolution action. Must be one of: {}",
            valid_actions.join(", ")
        )));
    }

    let report = sqlx::query_as::<_, Report>(
        r#"UPDATE user_reports
           SET status = CASE WHEN $2 = 'dismissed' THEN 'dismissed'::report_status ELSE 'resolved'::report_status END,
               resolution_action = $2,
               resolution_note = $3,
               resolved_at = NOW(),
               updated_at = NOW()
           WHERE id = $1 AND status IN ('pending', 'reviewing')
           RETURNING *"#,
    )
    .bind(report_id)
    .bind(&body.resolution_action)
    .bind(&body.resolution_note)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ReportError::NotFound)?;

    // Broadcast resolution to admin events
    let event = ServerEvent::AdminReportResolved {
        report_id: report.id,
    };
    if let Err(e) = broadcast_admin_event(&state.redis, &event).await {
        tracing::warn!("Failed to broadcast admin report resolved event: {}", e);
    }

    Ok(Json(report.into()))
}

/// GET /api/admin/reports/stats
/// Get report counts by status.
pub async fn report_stats(
    State(state): State<AppState>,
) -> Result<Json<ReportStatsResponse>, ReportError> {
    let pending: i64 = sqlx::query_scalar::<_, Option<i64>>("SELECT COUNT(*) FROM user_reports WHERE status ='pending'")
        .fetch_one(&state.db)
        .await?
        .unwrap_or(0);

    let reviewing: i64 = sqlx::query_scalar::<_, Option<i64>>("SELECT COUNT(*) FROM user_reports WHERE status ='reviewing'")
        .fetch_one(&state.db)
        .await?
        .unwrap_or(0);

    let resolved: i64 = sqlx::query_scalar::<_, Option<i64>>("SELECT COUNT(*) FROM user_reports WHERE status ='resolved'")
        .fetch_one(&state.db)
        .await?
        .unwrap_or(0);

    let dismissed: i64 = sqlx::query_scalar::<_, Option<i64>>("SELECT COUNT(*) FROM user_reports WHERE status ='dismissed'")
        .fetch_one(&state.db)
        .await?
        .unwrap_or(0);

    Ok(Json(ReportStatsResponse {
        pending,
        reviewing,
        resolved,
        dismissed,
    }))
}
