//! Admin-facing report handlers.

use axum::extract::{Path, Query, State};
use axum::{Extension, Json};
use uuid::Uuid;

use super::error::ModerationError;
use super::queries;
use super::types::{
    ListReportsQuery, PaginatedReports, ReportResponse, ReportStatsResponse, ResolveReportRequest,
};
use crate::admin::ElevatedAdmin;
use crate::api::AppState;
use crate::ws::{broadcast_admin_event, ServerEvent};

/// GET /api/admin/reports
/// List reports with optional status/category filter and pagination.
#[utoipa::path(
    get,
    path = "/api/admin/reports",
    tag = "moderation",
    params(ListReportsQuery),
    responses((status = 200, body = PaginatedReports)),
    security(("bearer_auth" = []))
)]
pub async fn list_reports(
    State(state): State<AppState>,
    Query(query): Query<ListReportsQuery>,
) -> Result<Json<PaginatedReports>, ModerationError> {
    let limit = query.limit.clamp(1, 100);
    let offset = query.offset.max(0);

    let reports =
        queries::list_reports(&state.db, query.status, query.category, limit, offset).await?;
    let total = queries::count_reports(&state.db, query.status, query.category).await?;

    Ok(Json(PaginatedReports {
        items: reports.into_iter().map(ReportResponse::from).collect(),
        total,
        limit,
        offset,
    }))
}

/// GET /api/admin/reports/:id
/// Get a single report by ID with full details.
#[utoipa::path(
    get,
    path = "/api/admin/reports/{id}",
    tag = "moderation",
    params(("id" = Uuid, Path, description = "Report ID")),
    responses((status = 200, body = ReportResponse)),
    security(("bearer_auth" = []))
)]
pub async fn get_report(
    State(state): State<AppState>,
    Path(report_id): Path<Uuid>,
) -> Result<Json<ReportResponse>, ModerationError> {
    let report = queries::get_report(&state.db, report_id).await?;
    Ok(Json(report.into()))
}

/// POST /api/admin/reports/:id/claim
/// Claim a report for review.
#[utoipa::path(
    post,
    path = "/api/admin/reports/{id}/claim",
    tag = "moderation",
    params(("id" = Uuid, Path, description = "Report ID")),
    responses((status = 200, body = ReportResponse)),
    security(("bearer_auth" = []))
)]
pub async fn claim_report(
    State(state): State<AppState>,
    Extension(elevated): Extension<ElevatedAdmin>,
    Path(report_id): Path<Uuid>,
) -> Result<Json<ReportResponse>, ModerationError> {
    let report = queries::claim_report(&state.db, report_id, elevated.user_id).await?;
    Ok(Json(report.into()))
}

/// POST /api/admin/reports/:id/resolve
/// Resolve a report with an action.
#[utoipa::path(
    post,
    path = "/api/admin/reports/{id}/resolve",
    tag = "moderation",
    params(("id" = Uuid, Path, description = "Report ID")),
    request_body = ResolveReportRequest,
    responses((status = 200, body = ReportResponse)),
    security(("bearer_auth" = []))
)]
pub async fn resolve_report(
    State(state): State<AppState>,
    Path(report_id): Path<Uuid>,
    Json(body): Json<ResolveReportRequest>,
) -> Result<Json<ReportResponse>, ModerationError> {
    // Validate resolution_action
    let valid_actions = ["dismissed", "warned", "banned", "escalated"];
    if !valid_actions.contains(&body.resolution_action.as_str()) {
        return Err(ModerationError::Validation(format!(
            "Invalid resolution action. Must be one of: {}",
            valid_actions.join(", ")
        )));
    }

    let report = queries::resolve_report(
        &state.db,
        report_id,
        &body.resolution_action,
        body.resolution_note.as_deref(),
    )
    .await?;

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
#[utoipa::path(
    get,
    path = "/api/admin/reports/stats",
    tag = "moderation",
    responses((status = 200, body = ReportStatsResponse)),
    security(("bearer_auth" = []))
)]
pub async fn report_stats(
    State(state): State<AppState>,
) -> Result<Json<ReportStatsResponse>, ModerationError> {
    let counts = queries::count_reports_by_status(&state.db).await?;
    Ok(Json(ReportStatsResponse {
        pending: counts.pending,
        reviewing: counts.reviewing,
        resolved: counts.resolved,
        dismissed: counts.dismissed,
    }))
}
