//! Admin audit log handlers.

#![allow(clippy::used_underscore_binding)]

use std::collections::HashSet;

use axum::extract::{Query, State};
use axum::{Extension, Json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::error::AdminError;
use super::queries::{self, AuditLogFilter};
use super::shared::PaginatedResponse;
use super::types::SystemAdminUser;
use crate::api::AppState;

/// Audit log query parameters.
#[derive(Debug, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
pub struct AuditLogParams {
    /// Maximum number of items to return.
    #[serde(default = "default_limit")]
    pub limit: i64,
    /// Number of items to skip.
    #[serde(default)]
    pub offset: i64,
    /// Filter by action prefix (e.g., "admin." for all admin actions).
    pub action: Option<String>,
    /// Filter entries created on or after this date (ISO 8601 format).
    pub from_date: Option<DateTime<Utc>>,
    /// Filter entries created on or before this date (ISO 8601 format).
    pub to_date: Option<DateTime<Utc>>,
    /// Filter by exact action type (e.g., "admin.users.ban").
    pub action_type: Option<String>,
}

#[allow(clippy::missing_const_for_fn)]
fn default_limit() -> i64 {
    50
}

/// Audit log entry response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AuditLogEntryResponse {
    pub id: Uuid,
    pub actor_id: Uuid,
    pub actor_username: Option<String>,
    pub action: String,
    pub target_type: Option<String>,
    pub target_id: Option<Uuid>,
    #[schema(value_type = Option<Object>)]
    pub details: Option<serde_json::Value>,
    pub ip_address: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Get system audit log with pagination and optional filters.
///
/// `GET /api/admin/audit-log`
///
/// Query parameters:
/// - `limit`: Max items to return (default 50, max 100)
/// - `offset`: Number of items to skip
/// - `action`: Filter by action prefix (e.g., "admin." for all admin actions)
/// - `action_type`: Filter by exact action type (e.g., "admin.users.ban")
/// - `from_date`: Filter entries created on or after this date (ISO 8601)
/// - `to_date`: Filter entries created on or before this date (ISO 8601)
#[utoipa::path(
    get,
    path = "/api/admin/audit-log",
    tag = "admin",
    params(AuditLogParams),
    responses((status = 200, body = PaginatedResponse<AuditLogEntryResponse>)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn get_audit_log(
    State(state): State<AppState>,
    Extension(_admin): Extension<SystemAdminUser>,
    Query(params): Query<AuditLogParams>,
) -> Result<Json<PaginatedResponse<AuditLogEntryResponse>>, AdminError> {
    // Clamp limit to reasonable bounds
    let limit = params.limit.clamp(1, 100);
    let offset = params.offset.max(0);

    // Determine action filter (exact action_type takes precedence over prefix)
    let action_filter = params.action_type.as_deref().or(params.action.as_deref());

    // Build dynamic query based on filters
    let filter = AuditLogFilter {
        action_filter,
        exact_action_match: params.action_type.is_some(),
        from_date: params.from_date,
        to_date: params.to_date,
    };
    let (entries, total) = queries::list_audit_log(&state.db, limit, offset, &filter).await?;

    // Collect unique actor IDs for username lookup (deduplicated)
    let actor_ids: Vec<Uuid> = entries
        .iter()
        .map(|e| e.actor_id)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    // Fetch usernames for actors
    let usernames: std::collections::HashMap<Uuid, String> =
        queries::lookup_usernames(&state.db, &actor_ids)
            .await?
            .into_iter()
            .collect();

    let items: Vec<AuditLogEntryResponse> = entries
        .into_iter()
        .map(|e| AuditLogEntryResponse {
            id: e.id,
            actor_id: e.actor_id,
            actor_username: usernames.get(&e.actor_id).cloned(),
            action: e.action,
            target_type: e.target_type,
            target_id: e.target_id,
            details: e.details,
            ip_address: e.ip_address,
            created_at: e.created_at,
        })
        .collect();

    Ok(Json(PaginatedResponse {
        items,
        total,
        limit,
        offset,
    }))
}
