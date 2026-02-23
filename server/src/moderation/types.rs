//! Moderation Types

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

// ============================================================================
// Database Enums
// ============================================================================

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema,
)]
#[sqlx(type_name = "report_category", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum ReportCategory {
    Harassment,
    Spam,
    InappropriateContent,
    Impersonation,
    Other,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema,
)]
#[sqlx(type_name = "report_status", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum ReportStatus {
    Pending,
    Reviewing,
    Resolved,
    Dismissed,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema,
)]
#[sqlx(type_name = "report_target_type", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum ReportTargetType {
    User,
    Message,
}

// ============================================================================
// Request Types
// ============================================================================

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct CreateReportRequest {
    pub target_type: ReportTargetType,
    pub target_user_id: Uuid,
    pub target_message_id: Option<Uuid>,
    pub category: ReportCategory,
    #[validate(length(max = 500, message = "Description must be at most 500 characters"))]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ResolveReportRequest {
    /// One of: dismissed, warned, banned, escalated
    pub resolution_action: String,
    pub resolution_note: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
pub struct ListReportsQuery {
    pub status: Option<ReportStatus>,
    pub category: Option<ReportCategory>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

const fn default_limit() -> i64 {
    20
}

// ============================================================================
// Response Types
// ============================================================================

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Report {
    pub id: Uuid,
    pub reporter_id: Uuid,
    pub target_type: ReportTargetType,
    pub target_user_id: Uuid,
    pub target_message_id: Option<Uuid>,
    pub category: ReportCategory,
    pub description: Option<String>,
    pub status: ReportStatus,
    pub assigned_admin_id: Option<Uuid>,
    pub resolution_action: Option<String>,
    pub resolution_note: Option<String>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ReportResponse {
    pub id: Uuid,
    pub reporter_id: Uuid,
    pub target_type: ReportTargetType,
    pub target_user_id: Uuid,
    pub target_message_id: Option<Uuid>,
    pub category: ReportCategory,
    pub description: Option<String>,
    pub status: ReportStatus,
    pub assigned_admin_id: Option<Uuid>,
    pub resolution_action: Option<String>,
    pub resolution_note: Option<String>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Report> for ReportResponse {
    fn from(r: Report) -> Self {
        Self {
            id: r.id,
            reporter_id: r.reporter_id,
            target_type: r.target_type,
            target_user_id: r.target_user_id,
            target_message_id: r.target_message_id,
            category: r.category,
            description: r.description,
            status: r.status,
            assigned_admin_id: r.assigned_admin_id,
            resolution_action: r.resolution_action,
            resolution_note: r.resolution_note,
            resolved_at: r.resolved_at,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ReportStatsResponse {
    pub pending: i64,
    pub reviewing: i64,
    pub resolved: i64,
    pub dismissed: i64,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PaginatedReports {
    pub items: Vec<ReportResponse>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug, thiserror::Error)]
pub enum ReportError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Report not found")]
    NotFound,

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Rate limited: too many reports")]
    RateLimited,

    #[error("Duplicate report: you already have an active report for this target")]
    Duplicate,
}

impl IntoResponse for ReportError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            Self::Database(err) => {
                tracing::error!("Database error: {}", err);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    "Database error".to_string(),
                )
            }
            Self::NotFound => (StatusCode::NOT_FOUND, "REPORT_NOT_FOUND", self.to_string()),
            Self::Validation(msg) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR", msg.clone()),
            Self::RateLimited => (
                StatusCode::TOO_MANY_REQUESTS,
                "RATE_LIMITED",
                self.to_string(),
            ),
            Self::Duplicate => (StatusCode::CONFLICT, "DUPLICATE_REPORT", self.to_string()),
        };

        (
            status,
            Json(serde_json::json!({ "error": code, "message": message })),
        )
            .into_response()
    }
}
