//! Admin guild management handlers (list, details, suspend, delete, export, bulk, page limits).

#![allow(clippy::used_underscore_binding)]

use std::fmt::Write;
use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::{Extension, Json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use tracing::warn;
use utoipa::ToSchema;
use uuid::Uuid;

use super::error::AdminError;
use super::queries;
use super::shared::{escape_csv, DeleteResponse, PaginatedResponse, PaginationParams};
use super::types::{
    BulkActionFailure, BulkSuspendRequest, BulkSuspendResponse, ElevatedAdmin, SuspendGuildRequest,
    SystemAdminUser,
};
use crate::api::AppState;
use crate::permissions::queries::write_audit_log;
use crate::ws::{broadcast_admin_event, ServerEvent};

/// Guild summary for admin listing.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GuildSummary {
    pub id: Uuid,
    pub name: String,
    pub owner_id: Uuid,
    pub icon_url: Option<String>,
    pub member_count: i64,
    pub created_at: DateTime<Utc>,
    pub suspended_at: Option<DateTime<Utc>>,
}

/// Guild member info for detail view.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GuildMemberInfo {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub joined_at: DateTime<Utc>,
}

/// Guild owner info for detail view.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GuildOwnerInfo {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
}

/// Detailed guild information response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GuildDetailsResponse {
    pub id: Uuid,
    pub name: String,
    pub icon_url: Option<String>,
    pub member_count: i64,
    pub created_at: DateTime<Utc>,
    pub suspended_at: Option<DateTime<Utc>>,
    pub owner: GuildOwnerInfo,
    pub top_members: Vec<GuildMemberInfo>,
}

/// Guild suspend response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SuspendResponse {
    pub suspended: bool,
    pub guild_id: Uuid,
}

/// List all guilds with pagination and optional search.
///
/// `GET /api/admin/guilds`
#[utoipa::path(
    get,
    path = "/api/admin/guilds",
    tag = "admin",
    params(PaginationParams),
    responses((status = 200, body = PaginatedResponse<GuildSummary>)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn list_guilds(
    State(state): State<AppState>,
    Extension(_admin): Extension<SystemAdminUser>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<PaginatedResponse<GuildSummary>>, AdminError> {
    // Clamp limit to reasonable bounds
    let limit = params.limit.clamp(1, 100);
    let offset = params.offset.max(0);

    // Prepare search pattern if provided
    let search_pattern = params
        .search
        .as_ref()
        .map(|s| format!("%{}%", s.to_lowercase()));

    let total = queries::count_guilds_filtered(&state.db, search_pattern.as_deref()).await?;
    let guilds =
        queries::list_guilds_filtered(&state.db, limit, offset, search_pattern.as_deref()).await?;

    let items: Vec<GuildSummary> = guilds
        .into_iter()
        .map(
            |(id, name, owner_id, icon_url, member_count, created_at, suspended_at)| GuildSummary {
                id,
                name,
                owner_id,
                icon_url,
                member_count,
                created_at,
                suspended_at,
            },
        )
        .collect();

    Ok(Json(PaginatedResponse {
        items,
        total,
        limit,
        offset,
    }))
}

/// Get detailed guild information.
///
/// `GET /api/admin/guilds/:id/details`
#[utoipa::path(
    get,
    path = "/api/admin/guilds/{id}/details",
    tag = "admin",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 200, body = GuildDetailsResponse)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn get_guild_details(
    State(state): State<AppState>,
    Extension(_admin): Extension<SystemAdminUser>,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<GuildDetailsResponse>, AdminError> {
    // Get basic guild info with member count
    let guild = queries::get_guild_details(&state.db, guild_id)
        .await?
        .ok_or_else(|| AdminError::NotFound("Guild not found".to_string()))?;

    // Get owner info
    let owner = queries::get_guild_owner(&state.db, guild.owner_id).await?;

    // Get top 5 members (excluding owner, most recent first)
    let top_members_rows =
        queries::list_top_guild_members(&state.db, guild_id, guild.owner_id).await?;

    let top_members: Vec<GuildMemberInfo> = top_members_rows
        .into_iter()
        .map(|row| GuildMemberInfo {
            user_id: row.user_id,
            username: row.username,
            display_name: row.display_name,
            avatar_url: row.avatar_url,
            joined_at: row.joined_at,
        })
        .collect();

    Ok(Json(GuildDetailsResponse {
        id: guild.id,
        name: guild.name,
        icon_url: guild.icon_url,
        member_count: guild.member_count,
        created_at: guild.created_at,
        suspended_at: guild.suspended_at,
        owner: GuildOwnerInfo {
            user_id: owner.id,
            username: owner.username,
            display_name: owner.display_name,
            avatar_url: owner.avatar_url,
        },
        top_members,
    }))
}

/// Suspend a guild.
///
/// `POST /api/admin/guilds/:id/suspend`
#[utoipa::path(
    post,
    path = "/api/admin/guilds/{id}/suspend",
    tag = "admin",
    params(("id" = Uuid, Path, description = "Guild ID")),
    request_body = SuspendGuildRequest,
    responses((status = 200, description = "Guild suspended", body = SuspendResponse)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn suspend_guild(
    State(state): State<AppState>,
    Extension(admin): Extension<SystemAdminUser>,
    Extension(_elevated): Extension<ElevatedAdmin>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(guild_id): Path<Uuid>,
    Json(body): Json<SuspendGuildRequest>,
) -> Result<Json<SuspendResponse>, AdminError> {
    // Get guild name for the event
    let guild_name = queries::find_guild(&state.db, guild_id)
        .await?
        .map(|(_, name)| name)
        .ok_or_else(|| AdminError::NotFound("Guild".to_string()))?;

    let affected = queries::suspend_guild(&state.db, guild_id, admin.user_id, &body.reason).await?;

    if affected == 0 {
        return Err(AdminError::Validation(
            "Guild is already suspended".to_string(),
        ));
    }

    // Log the action
    let ip_address = addr.ip().to_string();
    write_audit_log(
        &state.db,
        admin.user_id,
        "admin.guilds.suspend",
        Some("guild"),
        Some(guild_id),
        Some(serde_json::json!({"reason": body.reason})),
        Some(&ip_address),
    )
    .await?;

    // Broadcast admin event
    if let Err(e) = broadcast_admin_event(
        &state.redis,
        &ServerEvent::AdminGuildSuspended {
            guild_id,
            guild_name: guild_name.clone(),
        },
    )
    .await
    {
        warn!(guild_id = %guild_id, error = %e, "Failed to broadcast guild suspend event");
    }

    Ok(Json(SuspendResponse {
        suspended: true,
        guild_id,
    }))
}

/// Unsuspend a guild.
///
/// `DELETE /api/admin/guilds/:id/suspend`
#[utoipa::path(
    delete,
    path = "/api/admin/guilds/{id}/suspend",
    tag = "admin",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses(
        (status = 200, description = "Guild unsuspended", body = SuspendResponse),
        (status = 404, description = "Guild not found or not suspended"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state))]
pub async fn unsuspend_guild(
    State(state): State<AppState>,
    Extension(admin): Extension<SystemAdminUser>,
    Extension(_elevated): Extension<ElevatedAdmin>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<SuspendResponse>, AdminError> {
    // Get guild name for the event
    let guild_name = queries::get_guild_name(&state.db, guild_id)
        .await?
        .unwrap_or_else(|| "Unknown".to_string());

    let affected = queries::unsuspend_guild(&state.db, guild_id).await?;

    if affected == 0 {
        return Err(AdminError::NotFound("Suspended guild".to_string()));
    }

    // Log the action
    let ip_address = addr.ip().to_string();
    write_audit_log(
        &state.db,
        admin.user_id,
        "admin.guilds.unsuspend",
        Some("guild"),
        Some(guild_id),
        None,
        Some(&ip_address),
    )
    .await?;

    // Broadcast admin event
    if let Err(e) = broadcast_admin_event(
        &state.redis,
        &ServerEvent::AdminGuildUnsuspended {
            guild_id,
            guild_name: guild_name.clone(),
        },
    )
    .await
    {
        warn!(guild_id = %guild_id, error = %e, "Failed to broadcast guild unsuspend event");
    }

    Ok(Json(SuspendResponse {
        suspended: false,
        guild_id,
    }))
}

/// Suspend multiple guilds at once.
///
/// `POST /api/admin/guilds/bulk-suspend`
#[utoipa::path(
    post,
    path = "/api/admin/guilds/bulk-suspend",
    tag = "admin",
    request_body = BulkSuspendRequest,
    responses((status = 200, body = BulkSuspendResponse)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn bulk_suspend_guilds(
    State(state): State<AppState>,
    Extension(admin): Extension<ElevatedAdmin>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<BulkSuspendRequest>,
) -> Result<Json<BulkSuspendResponse>, AdminError> {
    // Validate request
    if body.guild_ids.is_empty() {
        return Err(AdminError::Validation("No guild IDs provided".to_string()));
    }
    if body.guild_ids.len() > 100 {
        return Err(AdminError::Validation(
            "Cannot suspend more than 100 guilds at once".to_string(),
        ));
    }
    if body.reason.trim().is_empty() {
        return Err(AdminError::Validation("Reason is required".to_string()));
    }

    let mut suspended_count = 0;
    let mut already_suspended = 0;
    let mut failed: Vec<BulkActionFailure> = Vec::new();
    let ip_address = addr.ip().to_string();

    for guild_id in &body.guild_ids {
        // Check if guild exists and get current status
        let status = queries::get_guild_suspension_status(&state.db, *guild_id).await?;

        match status {
            None => {
                failed.push(BulkActionFailure {
                    id: *guild_id,
                    reason: "Guild not found".to_string(),
                });
            }
            Some(Some(_)) => {
                already_suspended += 1;
            }
            Some(None) => {
                // Suspend the guild
                match queries::bulk_suspend_guild(&state.db, *guild_id, &body.reason).await {
                    Ok(_) => {
                        suspended_count += 1;
                    }
                    Err(e) => {
                        failed.push(BulkActionFailure {
                            id: *guild_id,
                            reason: format!("Database error: {e}"),
                        });
                    }
                }
            }
        }
    }

    // Log the bulk action
    write_audit_log(
        &state.db,
        admin.user_id,
        "admin.guilds.bulk_suspend",
        Some("guild"),
        None,
        Some(serde_json::json!({
            "guild_count": body.guild_ids.len(),
            "suspended_count": suspended_count,
            "already_suspended": already_suspended,
            "failed_count": failed.len(),
            "reason": body.reason
        })),
        Some(&ip_address),
    )
    .await?;

    Ok(Json(BulkSuspendResponse {
        suspended_count,
        already_suspended,
        failed,
    }))
}

/// Permanently delete a guild and all associated data.
///
/// `DELETE /api/admin/guilds/:id`
#[utoipa::path(
    delete,
    path = "/api/admin/guilds/{id}",
    tag = "admin",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 200, description = "Guild deleted", body = DeleteResponse)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn delete_guild(
    State(state): State<AppState>,
    Extension(admin): Extension<SystemAdminUser>,
    Extension(_elevated): Extension<ElevatedAdmin>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<DeleteResponse>, AdminError> {
    // Check guild exists and get name
    let guild_name = queries::find_guild(&state.db, guild_id)
        .await?
        .map(|(_, name)| name)
        .ok_or_else(|| AdminError::NotFound("Guild".to_string()))?;

    // Delete guild (cascades to channels, messages, roles, members, invites, etc.)
    let deleted = queries::delete_guild(&state.db, guild_id).await?;

    if deleted == 0 {
        return Err(AdminError::NotFound("Guild".to_string()));
    }

    // Log the action
    let ip_address = addr.ip().to_string();
    write_audit_log(
        &state.db,
        admin.user_id,
        "admin.guilds.delete",
        Some("guild"),
        Some(guild_id),
        Some(serde_json::json!({"guild_name": guild_name})),
        Some(&ip_address),
    )
    .await?;

    // Broadcast admin event
    if let Err(e) = broadcast_admin_event(
        &state.redis,
        &ServerEvent::AdminGuildDeleted {
            guild_id,
            guild_name: guild_name.clone(),
        },
    )
    .await
    {
        warn!(guild_id = %guild_id, error = %e, "Failed to broadcast guild delete event");
    }

    Ok(Json(DeleteResponse {
        deleted: true,
        id: guild_id,
    }))
}

/// Export guilds to CSV.
///
/// `GET /api/admin/guilds/export`
#[utoipa::path(
    get,
    path = "/api/admin/guilds/export",
    tag = "admin",
    responses((status = 200, description = "CSV file download")),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn export_guilds_csv(
    State(state): State<AppState>,
    Extension(_admin): Extension<SystemAdminUser>,
    Query(params): Query<PaginationParams>,
) -> Result<impl IntoResponse, AdminError> {
    // Build search condition (use empty string to match all if no search)
    let search_pattern = params
        .search
        .as_ref()
        .map(|s| format!("%{}%", s.to_lowercase()));

    // Query all matching guilds (no pagination for export)
    let guilds = queries::export_guilds(&state.db, search_pattern.as_deref()).await?;

    // Build CSV content
    let mut csv =
        String::from("id,name,owner_id,member_count,created_at,is_suspended,suspended_at\n");
    for guild in guilds {
        let suspended_at_str: String = guild
            .suspended_at
            .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_default();
        writeln!(
            csv,
            "{},{},{},{},{},{},{}",
            guild.id,
            escape_csv(&guild.name),
            guild.owner_id,
            guild.member_count,
            guild.created_at.format("%Y-%m-%d %H:%M:%S"),
            guild.suspended_at.is_some(),
            suspended_at_str
        )
        .expect("write to String is infallible");
    }

    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"guilds_export.csv\"",
            ),
        ],
        csv,
    ))
}

// ============================================================================
// Per-Guild Page Limits
// ============================================================================

/// Request to set per-guild page limits.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SetGuildPageLimitsRequest {
    /// Maximum pages (null = reset to instance default, min 1).
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub max_pages: Option<Option<i32>>,
    /// Maximum revisions per page (null = reset to instance default, min 5).
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub max_revisions: Option<Option<i32>>,
}

#[allow(clippy::option_option)]
fn deserialize_double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// Guild page limits response.
#[derive(Debug, Serialize, ToSchema)]
pub struct GuildPageLimitsResponse {
    pub guild_id: Uuid,
    pub max_pages: Option<i32>,
    pub max_revisions: Option<i32>,
    pub instance_default_pages: i64,
    pub instance_default_revisions: i64,
}

/// Get per-guild page limits.
///
/// GET /api/admin/guilds/:id/page-limits
#[utoipa::path(
    get,
    path = "/api/admin/guilds/{id}/page-limits",
    tag = "admin",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 200, body = GuildPageLimitsResponse)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn get_guild_page_limits(
    State(state): State<AppState>,
    Extension(_admin): Extension<SystemAdminUser>,
    Extension(_elevated): Extension<ElevatedAdmin>,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<GuildPageLimitsResponse>, AdminError> {
    let row = queries::get_guild_page_limits(&state.db, guild_id).await?;

    let (max_pages, max_revisions) = row.ok_or(AdminError::NotFound("Guild not found".into()))?;

    Ok(Json(GuildPageLimitsResponse {
        guild_id,
        max_pages,
        max_revisions,
        instance_default_pages: state.config.max_pages_per_guild,
        instance_default_revisions: state.config.max_revisions_per_page,
    }))
}

/// Set per-guild page limits.
///
/// PATCH /api/admin/guilds/:id/page-limits
#[utoipa::path(
    patch,
    path = "/api/admin/guilds/{id}/page-limits",
    tag = "admin",
    params(("id" = Uuid, Path, description = "Guild ID")),
    request_body = SetGuildPageLimitsRequest,
    responses((status = 200, body = GuildPageLimitsResponse)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn set_guild_page_limits(
    State(state): State<AppState>,
    Extension(_admin): Extension<SystemAdminUser>,
    Extension(_elevated): Extension<ElevatedAdmin>,
    Path(guild_id): Path<Uuid>,
    Json(body): Json<SetGuildPageLimitsRequest>,
) -> Result<Json<GuildPageLimitsResponse>, AdminError> {
    // Validate bounds
    const MAX_ALLOWED_PAGES: i32 = 1000;
    const MAX_ALLOWED_REVISIONS: i32 = 500;

    if let Some(Some(max_pages)) = body.max_pages {
        if !(1..=MAX_ALLOWED_PAGES).contains(&max_pages) {
            return Err(AdminError::Validation(format!(
                "max_pages must be between 1 and {MAX_ALLOWED_PAGES}"
            )));
        }
    }
    if let Some(Some(max_revisions)) = body.max_revisions {
        if !(5..=MAX_ALLOWED_REVISIONS).contains(&max_revisions) {
            return Err(AdminError::Validation(format!(
                "max_revisions must be between 5 and {MAX_ALLOWED_REVISIONS}"
            )));
        }
    }

    let max_pages_present = body.max_pages.is_some();
    let max_pages_value = body.max_pages.flatten();
    let max_revisions_present = body.max_revisions.is_some();
    let max_revisions_value = body.max_revisions.flatten();

    let row = queries::set_guild_page_limits(
        &state.db,
        guild_id,
        max_pages_present,
        max_pages_value,
        max_revisions_present,
        max_revisions_value,
    )
    .await?;

    let (max_pages, max_revisions) = row.ok_or(AdminError::NotFound("Guild not found".into()))?;

    Ok(Json(GuildPageLimitsResponse {
        guild_id,
        max_pages,
        max_revisions,
        instance_default_pages: state.config.max_pages_per_guild,
        instance_default_revisions: state.config.max_revisions_per_page,
    }))
}
