//! Admin API handlers for non-elevated actions.
//!
//! These handlers require system admin privileges but not elevated sessions:
//! - List users
//! - List guilds
//! - View audit log
//! - Elevate/de-elevate session

#![allow(clippy::used_underscore_binding)]

use std::collections::HashSet;
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
use super::queries::{self, AuditLogFilter};
use super::types::{
    AdminStatsResponse, AdminStatusResponse, BulkActionFailure, BulkBanRequest, BulkBanResponse,
    BulkSuspendRequest, BulkSuspendResponse, CreateAnnouncementRequest, ElevateRequest,
    ElevateResponse, ElevatedAdmin, GlobalBanRequest, SuspendGuildRequest, SystemAdminUser,
};
use crate::api::AppState;
use crate::permissions::queries::{create_elevated_session, write_audit_log};
use crate::ws::{broadcast_admin_event, ServerEvent};

// ============================================================================
// Query Parameters
// ============================================================================

/// Pagination query parameters with optional search.
#[derive(Debug, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
pub struct PaginationParams {
    /// Maximum number of items to return.
    #[serde(default = "default_limit")]
    pub limit: i64,
    /// Number of items to skip.
    #[serde(default)]
    pub offset: i64,
    /// Search query (searches username, `display_name`, email for users; name for guilds).
    pub search: Option<String>,
}

#[allow(clippy::missing_const_for_fn)]
fn default_limit() -> i64 {
    50
}

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

// ============================================================================
// Response Types
// ============================================================================

/// Generic paginated response wrapper.
#[derive(Debug, Serialize, ToSchema)]
pub struct PaginatedResponse<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

/// User summary for admin listing.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UserSummary {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub is_banned: bool,
}

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

/// De-elevate response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DeElevateResponse {
    pub elevated: bool,
}

/// User guild membership info for detail view.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UserGuildMembership {
    pub guild_id: Uuid,
    pub guild_name: String,
    pub guild_icon_url: Option<String>,
    pub joined_at: DateTime<Utc>,
    pub is_owner: bool,
}

/// Detailed user information response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UserDetailsResponse {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub is_banned: bool,
    pub last_login: Option<DateTime<Utc>>,
    pub guild_count: i64,
    pub guilds: Vec<UserGuildMembership>,
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

// ============================================================================
// Handlers
// ============================================================================

/// Get admin status for the current user.
///
/// `GET /api/admin/status`
///
/// This endpoint does NOT require admin privileges - it checks if the user IS an admin.
#[utoipa::path(
    get,
    path = "/api/admin/status",
    tag = "admin",
    responses((status = 200, body = AdminStatusResponse)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn get_admin_status(
    State(state): State<AppState>,
    Extension(auth): Extension<crate::auth::AuthUser>,
) -> Result<Json<super::types::AdminStatusResponse>, AdminError> {
    use crate::permissions::queries::get_system_admin;

    // Check if user is a system admin
    let is_admin = get_system_admin(&state.db, auth.id).await?.is_some();

    // Check for active elevated session
    let elevated = if is_admin {
        queries::find_latest_elevated_session_expiry(&state.db, auth.id).await?
    } else {
        None
    };

    Ok(Json(super::types::AdminStatusResponse {
        is_admin,
        is_elevated: elevated.is_some(),
        elevation_expires_at: elevated.map(|e| e.expires_at),
    }))
}

/// Get admin statistics.
///
/// `GET /api/admin/stats`
#[utoipa::path(
    get,
    path = "/api/admin/stats",
    tag = "admin",
    responses((status = 200, body = AdminStatsResponse)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn get_admin_stats(
    State(state): State<AppState>,
    Extension(_admin): Extension<SystemAdminUser>,
) -> Result<Json<super::types::AdminStatsResponse>, AdminError> {
    let user_count = queries::count_users(&state.db).await?;
    let guild_count = queries::count_guilds(&state.db).await?;
    let banned_count = queries::count_active_bans(&state.db).await?;

    Ok(Json(super::types::AdminStatsResponse {
        user_count,
        guild_count,
        banned_count,
    }))
}

/// List all users with pagination and optional search.
///
/// `GET /api/admin/users`
#[utoipa::path(
    get,
    path = "/api/admin/users",
    tag = "admin",
    params(PaginationParams),
    responses((status = 200, body = PaginatedResponse<UserSummary>)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn list_users(
    State(state): State<AppState>,
    Extension(_admin): Extension<SystemAdminUser>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<PaginatedResponse<UserSummary>>, AdminError> {
    // Clamp limit to reasonable bounds
    let limit = params.limit.clamp(1, 100);
    let offset = params.offset.max(0);

    // Prepare search pattern if provided
    let search_pattern = params
        .search
        .as_ref()
        .map(|s| format!("%{}%", s.to_lowercase()));

    let total = queries::count_users_filtered(&state.db, search_pattern.as_deref()).await?;
    let users =
        queries::list_users_filtered(&state.db, limit, offset, search_pattern.as_deref()).await?;

    let items: Vec<UserSummary> = users
        .into_iter()
        .map(
            |(id, username, display_name, email, avatar_url, created_at, is_banned)| UserSummary {
                id,
                username,
                display_name,
                email,
                avatar_url,
                created_at,
                is_banned,
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

/// Elevate admin session.
///
/// `POST /api/admin/elevate`
///
/// Confirms elevation of the current admin session. MFA verification will be
/// added in a future iteration.
#[utoipa::path(
    post,
    path = "/api/admin/elevate",
    tag = "admin",
    request_body = ElevateRequest,
    responses((status = 200, body = ElevateResponse)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state, body))]
pub async fn elevate_session(
    State(state): State<AppState>,
    Extension(admin): Extension<SystemAdminUser>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<ElevateRequest>,
) -> Result<Json<ElevateResponse>, AdminError> {
    // NOTE: MFA verification for admin elevation is deferred — elevation
    // currently relies on password-only re-authentication.

    // Find an existing login session id — elevated_sessions.session_id
    // references sessions.id, so this must resolve for the user.
    let session_id = queries::find_latest_active_session_id(&state.db, admin.user_id)
        .await?
        .ok_or_else(|| AdminError::Validation("No active session found".to_string()))?;

    // Create elevated session (15 minutes)
    let ip_address = addr.ip().to_string();
    let elevated = create_elevated_session(
        &state.db,
        admin.user_id,
        session_id,
        &ip_address,
        15, // 15 minutes
        body.reason.as_deref(),
    )
    .await?;

    // Cache elevated status in Redis (TTL = 15 minutes = 900 seconds)
    super::cache_elevated_status(&state.redis, admin.user_id, true, 900).await;

    // Log the elevation
    write_audit_log(
        &state.db,
        admin.user_id,
        "admin.session.elevated",
        Some("user"),
        Some(admin.user_id),
        Some(serde_json::json!({
            "reason": body.reason,
            "session_id": session_id,
        })),
        Some(&ip_address),
    )
    .await?;

    Ok(Json(ElevateResponse {
        elevated: true,
        expires_at: elevated.expires_at,
        session_id: elevated.id,
    }))
}

/// De-elevate admin session.
///
/// `DELETE /api/admin/elevate`
#[utoipa::path(
    delete,
    path = "/api/admin/elevate",
    tag = "admin",
    responses((status = 200, body = DeElevateResponse)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn de_elevate_session(
    State(state): State<AppState>,
    Extension(admin): Extension<SystemAdminUser>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Result<Json<DeElevateResponse>, AdminError> {
    let ip_address = addr.ip().to_string();

    // Delete all elevated sessions for this user
    let removed = queries::delete_elevated_sessions(&state.db, admin.user_id).await?;

    // Clear elevated status cache
    super::cache_elevated_status(&state.redis, admin.user_id, false, 1).await;

    // Log the de-elevation if any sessions were deleted
    if removed > 0 {
        write_audit_log(
            &state.db,
            admin.user_id,
            "admin.session.de_elevated",
            Some("user"),
            Some(admin.user_id),
            Some(serde_json::json!({
                "sessions_removed": removed,
            })),
            Some(&ip_address),
        )
        .await?;
    }

    Ok(Json(DeElevateResponse { elevated: false }))
}

// ============================================================================
// Elevated Handlers (Destructive Actions)
// ============================================================================

/// Global ban response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct BanResponse {
    pub banned: bool,
    pub user_id: Uuid,
}

/// Guild suspend response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SuspendResponse {
    pub suspended: bool,
    pub guild_id: Uuid,
}

/// Delete response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DeleteResponse {
    pub deleted: bool,
    pub id: Uuid,
}

/// Announcement response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AnnouncementResponse {
    pub id: Uuid,
    pub title: String,
    pub created: bool,
}

/// Global ban a user.
///
/// `POST /api/admin/users/:id/ban`
#[utoipa::path(
    post,
    path = "/api/admin/users/{id}/ban",
    tag = "admin",
    params(("id" = Uuid, Path, description = "User ID")),
    request_body = GlobalBanRequest,
    responses((status = 200, description = "User banned", body = BanResponse)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn ban_user(
    State(state): State<AppState>,
    Extension(admin): Extension<SystemAdminUser>,
    Extension(_elevated): Extension<ElevatedAdmin>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(user_id): Path<Uuid>,
    Json(body): Json<GlobalBanRequest>,
) -> Result<Json<BanResponse>, AdminError> {
    // Check user exists and get username
    let username = queries::find_user(&state.db, user_id)
        .await?
        .map(|(_, name)| name)
        .ok_or_else(|| AdminError::NotFound("User".to_string()))?;

    // Cannot ban yourself
    if user_id == admin.user_id {
        return Err(AdminError::Validation("Cannot ban yourself".to_string()));
    }

    // Create or update ban
    queries::upsert_global_ban(
        &state.db,
        user_id,
        admin.user_id,
        &body.reason,
        body.expires_at,
    )
    .await?;

    // Log the action
    let ip_address = addr.ip().to_string();
    write_audit_log(
        &state.db,
        admin.user_id,
        "admin.users.ban",
        Some("user"),
        Some(user_id),
        Some(serde_json::json!({"reason": body.reason, "expires_at": body.expires_at})),
        Some(&ip_address),
    )
    .await?;

    // Broadcast admin event
    if let Err(e) = broadcast_admin_event(
        &state.redis,
        &ServerEvent::AdminUserBanned {
            user_id,
            username: username.clone(),
        },
    )
    .await
    {
        warn!(user_id = %user_id, error = %e, "Failed to broadcast user ban event");
    }

    Ok(Json(BanResponse {
        banned: true,
        user_id,
    }))
}

/// Remove global ban from a user.
///
/// `DELETE /api/admin/users/:id/ban`
#[utoipa::path(
    delete,
    path = "/api/admin/users/{id}/ban",
    tag = "admin",
    params(("id" = Uuid, Path, description = "User ID")),
    responses(
        (status = 200, description = "User unbanned", body = BanResponse),
        (status = 404, description = "User or ban not found"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state))]
pub async fn unban_user(
    State(state): State<AppState>,
    Extension(admin): Extension<SystemAdminUser>,
    Extension(_elevated): Extension<ElevatedAdmin>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<BanResponse>, AdminError> {
    // Get username for the event
    let username = queries::get_username(&state.db, user_id)
        .await?
        .unwrap_or_else(|| "Unknown".to_string());

    let deleted = queries::delete_global_ban(&state.db, user_id).await?;

    if deleted == 0 {
        return Err(AdminError::NotFound("Ban".to_string()));
    }

    // Log the action
    let ip_address = addr.ip().to_string();
    write_audit_log(
        &state.db,
        admin.user_id,
        "admin.users.unban",
        Some("user"),
        Some(user_id),
        None,
        Some(&ip_address),
    )
    .await?;

    // Broadcast admin event
    if let Err(e) = broadcast_admin_event(
        &state.redis,
        &ServerEvent::AdminUserUnbanned {
            user_id,
            username: username.clone(),
        },
    )
    .await
    {
        warn!(user_id = %user_id, error = %e, "Failed to broadcast user unban event");
    }

    Ok(Json(BanResponse {
        banned: false,
        user_id,
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

/// Create a system announcement.
///
/// `POST /api/admin/announcements`
#[utoipa::path(
    post,
    path = "/api/admin/announcements",
    tag = "admin",
    request_body = CreateAnnouncementRequest,
    responses((status = 200, description = "Announcement created", body = AnnouncementResponse)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn create_announcement(
    State(state): State<AppState>,
    Extension(admin): Extension<SystemAdminUser>,
    Extension(_elevated): Extension<ElevatedAdmin>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<CreateAnnouncementRequest>,
) -> Result<Json<AnnouncementResponse>, AdminError> {
    // Validate severity
    let valid_severities = ["info", "warning", "critical", "maintenance"];
    if !valid_severities.contains(&body.severity.as_str()) {
        return Err(AdminError::Validation(format!(
            "Invalid severity. Must be one of: {}",
            valid_severities.join(", ")
        )));
    }

    let announcement_id = Uuid::now_v7();
    let starts_at = body.starts_at.unwrap_or_else(Utc::now);

    queries::insert_announcement(
        &state.db,
        announcement_id,
        admin.user_id,
        &body.title,
        &body.content,
        &body.severity,
        starts_at,
        body.ends_at,
    )
    .await?;

    // Log the action
    let ip_address = addr.ip().to_string();
    write_audit_log(
        &state.db,
        admin.user_id,
        "admin.announcements.create",
        Some("announcement"),
        Some(announcement_id),
        Some(serde_json::json!({"title": body.title, "severity": body.severity})),
        Some(&ip_address),
    )
    .await?;

    Ok(Json(AnnouncementResponse {
        id: announcement_id,
        title: body.title,
        created: true,
    }))
}

// ============================================================================
// Detail View Handlers
// ============================================================================

/// Get detailed user information.
///
/// `GET /api/admin/users/:id/details`
#[utoipa::path(
    get,
    path = "/api/admin/users/{id}/details",
    tag = "admin",
    params(("id" = Uuid, Path, description = "User ID")),
    responses((status = 200, body = UserDetailsResponse)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn get_user_details(
    State(state): State<AppState>,
    Extension(_admin): Extension<SystemAdminUser>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<UserDetailsResponse>, AdminError> {
    // Get basic user info
    let user = queries::get_user_details(&state.db, user_id)
        .await?
        .ok_or_else(|| AdminError::NotFound("User not found".to_string()))?;

    // Get last login from sessions table
    let last_login = queries::get_user_last_login(&state.db, user_id).await?;

    // Get guild memberships
    let guild_memberships = queries::list_user_guild_memberships(&state.db, user_id).await?;

    let guilds: Vec<UserGuildMembership> = guild_memberships
        .into_iter()
        .map(|row| UserGuildMembership {
            guild_id: row.guild_id,
            guild_name: row.guild_name,
            guild_icon_url: row.guild_icon_url,
            joined_at: row.joined_at,
            is_owner: row.is_owner,
        })
        .collect();

    Ok(Json(UserDetailsResponse {
        id: user.id,
        username: user.username,
        display_name: user.display_name,
        email: user.email,
        avatar_url: user.avatar_url,
        created_at: user.created_at,
        is_banned: user.is_banned,
        last_login,
        guild_count: guilds.len() as i64,
        guilds,
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

// ============================================================================
// Export Handlers
// ============================================================================

/// Export users to CSV.
///
/// `GET /api/admin/users/export`
#[utoipa::path(
    get,
    path = "/api/admin/users/export",
    tag = "admin",
    responses((status = 200, description = "CSV file download")),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn export_users_csv(
    State(state): State<AppState>,
    Extension(_admin): Extension<SystemAdminUser>,
    Query(params): Query<PaginationParams>,
) -> Result<impl IntoResponse, AdminError> {
    // Build search condition (use empty string to match all if no search)
    let search_pattern = params
        .search
        .as_ref()
        .map(|s| format!("%{}%", s.to_lowercase()));

    // Query all matching users (no pagination for export)
    let users = queries::export_users(&state.db, search_pattern.as_deref()).await?;

    // Build CSV content
    let mut csv = String::from("id,username,display_name,email,created_at,is_banned\n");
    for user in users {
        writeln!(
            csv,
            "{},{},{},{},{},{}",
            user.id,
            escape_csv(&user.username),
            escape_csv(&user.display_name),
            escape_csv(&user.email.unwrap_or_default()),
            user.created_at.format("%Y-%m-%d %H:%M:%S"),
            user.is_banned
        )
        .expect("write to String is infallible");
    }

    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"users_export.csv\"",
            ),
        ],
        csv,
    ))
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

/// Escape a string for CSV (handles commas and quotes).
fn escape_csv(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

// ============================================================================
// Bulk Action Handlers
// ============================================================================

/// Ban multiple users at once.
///
/// `POST /api/admin/users/bulk-ban`
#[utoipa::path(
    post,
    path = "/api/admin/users/bulk-ban",
    tag = "admin",
    request_body = BulkBanRequest,
    responses((status = 200, body = BulkBanResponse)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn bulk_ban_users(
    State(state): State<AppState>,
    Extension(admin): Extension<ElevatedAdmin>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<BulkBanRequest>,
) -> Result<Json<BulkBanResponse>, AdminError> {
    // Validate request
    if body.user_ids.is_empty() {
        return Err(AdminError::Validation("No user IDs provided".to_string()));
    }
    if body.user_ids.len() > 100 {
        return Err(AdminError::Validation(
            "Cannot ban more than 100 users at once".to_string(),
        ));
    }
    if body.reason.trim().is_empty() {
        return Err(AdminError::Validation("Reason is required".to_string()));
    }

    let mut banned_count = 0;
    let mut already_banned = 0;
    let mut failed: Vec<BulkActionFailure> = Vec::new();
    let ip_address = addr.ip().to_string();

    for user_id in &body.user_ids {
        // Check if user exists
        if !queries::user_exists(&state.db, *user_id).await? {
            failed.push(BulkActionFailure {
                id: *user_id,
                reason: "User not found".to_string(),
            });
            continue;
        }

        // Check if already banned
        if queries::is_user_banned(&state.db, *user_id).await? {
            already_banned += 1;
            continue;
        }

        // Ban the user
        match queries::insert_global_ban(
            &state.db,
            *user_id,
            admin.user_id,
            &body.reason,
            body.expires_at,
        )
        .await
        {
            Ok(()) => {
                banned_count += 1;
            }
            Err(e) => {
                failed.push(BulkActionFailure {
                    id: *user_id,
                    reason: format!("Database error: {e}"),
                });
            }
        }
    }

    // Log the bulk action
    write_audit_log(
        &state.db,
        admin.user_id,
        "admin.users.bulk_ban",
        Some("user"),
        None,
        Some(serde_json::json!({
            "user_count": body.user_ids.len(),
            "banned_count": banned_count,
            "already_banned": already_banned,
            "failed_count": failed.len(),
            "reason": body.reason
        })),
        Some(&ip_address),
    )
    .await?;

    Ok(Json(BulkBanResponse {
        banned_count,
        already_banned,
        failed,
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

// ============================================================================
// Auth Settings & OIDC Provider Management (Elevated)
// ============================================================================

/// Auth settings response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AuthSettingsResponse {
    pub auth_methods: crate::db::AuthMethodsConfig,
    pub registration_policy: String,
}

/// Auth settings update request.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateAuthSettingsRequest {
    pub auth_methods: Option<crate::db::AuthMethodsConfig>,
    pub registration_policy: Option<String>,
}

/// Get auth settings.
///
/// GET /api/admin/auth-settings
#[utoipa::path(
    get,
    path = "/api/admin/auth-settings",
    tag = "admin",
    responses((status = 200, description = "Auth settings", body = AuthSettingsResponse)),
    security(("bearer_auth" = []))
)]
pub async fn get_auth_settings(
    State(state): State<AppState>,
    Extension(_admin): Extension<SystemAdminUser>,
    Extension(_elevated): Extension<ElevatedAdmin>,
) -> Result<Json<AuthSettingsResponse>, AdminError> {
    let auth_methods = crate::db::get_auth_methods_allowed(&state.db).await?;
    let registration_policy = crate::db::get_config_value(&state.db, "registration_policy")
        .await
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| "open".to_string());

    Ok(Json(AuthSettingsResponse {
        auth_methods,
        registration_policy,
    }))
}

/// Update auth settings.
///
/// PUT /api/admin/auth-settings
#[utoipa::path(
    put,
    path = "/api/admin/auth-settings",
    tag = "admin",
    request_body = UpdateAuthSettingsRequest,
    responses((status = 200, description = "Auth settings updated", body = AuthSettingsResponse)),
    security(("bearer_auth" = []))
)]
pub async fn update_auth_settings(
    State(state): State<AppState>,
    Extension(admin): Extension<SystemAdminUser>,
    Extension(_elevated): Extension<ElevatedAdmin>,
    Json(body): Json<UpdateAuthSettingsRequest>,
) -> Result<Json<AuthSettingsResponse>, AdminError> {
    if let Some(ref methods) = body.auth_methods {
        crate::db::set_auth_methods_allowed(&state.db, methods, admin.user_id).await?;
    }

    if let Some(ref policy) = body.registration_policy {
        let valid = matches!(policy.as_str(), "open" | "invite_only" | "closed");
        if !valid {
            return Err(AdminError::Validation(
                "registration_policy must be 'open', 'invite_only', or 'closed'".into(),
            ));
        }
        crate::db::set_config_value(
            &state.db,
            "registration_policy",
            serde_json::json!(policy),
            admin.user_id,
        )
        .await?;
    }

    // Re-read current state
    let auth_methods = crate::db::get_auth_methods_allowed(&state.db).await?;
    let registration_policy = crate::db::get_config_value(&state.db, "registration_policy")
        .await
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| "open".to_string());

    Ok(Json(AuthSettingsResponse {
        auth_methods,
        registration_policy,
    }))
}

/// OIDC provider response (secrets masked).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct OidcProviderResponse {
    pub id: Uuid,
    pub slug: String,
    pub display_name: String,
    pub icon_hint: Option<String>,
    pub provider_type: String,
    pub issuer_url: Option<String>,
    pub authorization_url: Option<String>,
    pub token_url: Option<String>,
    pub userinfo_url: Option<String>,
    pub client_id: String,
    pub scopes: String,
    pub enabled: bool,
    pub position: i32,
    pub created_at: DateTime<Utc>,
}

impl From<crate::db::OidcProviderRow> for OidcProviderResponse {
    fn from(row: crate::db::OidcProviderRow) -> Self {
        Self {
            id: row.id,
            slug: row.slug,
            display_name: row.display_name,
            icon_hint: row.icon_hint,
            provider_type: row.provider_type,
            issuer_url: row.issuer_url,
            authorization_url: row.authorization_url,
            token_url: row.token_url,
            userinfo_url: row.userinfo_url,
            client_id: row.client_id,
            scopes: row.scopes,
            enabled: row.enabled,
            position: row.position,
            created_at: row.created_at,
        }
    }
}

/// List all OIDC providers (admin view with secrets masked).
///
/// GET /api/admin/oidc-providers
#[utoipa::path(
    get,
    path = "/api/admin/oidc-providers",
    tag = "admin",
    responses((status = 200, description = "OIDC providers list", body = Vec<OidcProviderResponse>)),
    security(("bearer_auth" = []))
)]
pub async fn list_oidc_providers(
    State(state): State<AppState>,
    Extension(_admin): Extension<SystemAdminUser>,
    Extension(_elevated): Extension<ElevatedAdmin>,
) -> Result<Json<Vec<OidcProviderResponse>>, AdminError> {
    let providers = crate::db::list_all_oidc_providers(&state.db).await?;
    Ok(Json(providers.into_iter().map(Into::into).collect()))
}

/// Create OIDC provider request.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateOidcProviderRequest {
    pub slug: String,
    pub display_name: String,
    pub icon_hint: Option<String>,
    pub provider_type: Option<String>,
    pub issuer_url: Option<String>,
    pub authorization_url: Option<String>,
    pub token_url: Option<String>,
    pub userinfo_url: Option<String>,
    pub client_id: String,
    pub client_secret: String,
    pub scopes: Option<String>,
}

/// Create a new OIDC provider.
///
/// POST /api/admin/oidc-providers
#[utoipa::path(
    post,
    path = "/api/admin/oidc-providers",
    tag = "admin",
    request_body = CreateOidcProviderRequest,
    responses((status = 200, description = "OIDC provider created", body = OidcProviderResponse)),
    security(("bearer_auth" = []))
)]
pub async fn create_oidc_provider(
    State(state): State<AppState>,
    Extension(admin): Extension<SystemAdminUser>,
    Extension(_elevated): Extension<ElevatedAdmin>,
    Json(body): Json<CreateOidcProviderRequest>,
) -> Result<Json<OidcProviderResponse>, AdminError> {
    let oidc_manager = state.oidc_manager.as_ref().ok_or_else(|| {
        AdminError::Internal("OIDC manager not configured (requires MFA_ENCRYPTION_KEY)".into())
    })?;

    // Apply preset defaults
    let (provider_type, issuer_url, authorization_url, token_url, userinfo_url, scopes) =
        match body.slug.as_str() {
            "github" => (
                "preset".to_string(),
                None,
                Some(crate::auth::oidc::GitHubPreset::AUTHORIZATION_URL.to_string()),
                Some(crate::auth::oidc::GitHubPreset::TOKEN_URL.to_string()),
                Some(crate::auth::oidc::GitHubPreset::USERINFO_URL.to_string()),
                body.scopes
                    .unwrap_or_else(|| crate::auth::oidc::GitHubPreset::SCOPES.to_string()),
            ),
            "google" => (
                "preset".to_string(),
                Some(crate::auth::oidc::GooglePreset::ISSUER_URL.to_string()),
                body.authorization_url,
                body.token_url,
                body.userinfo_url,
                body.scopes
                    .unwrap_or_else(|| crate::auth::oidc::GooglePreset::SCOPES.to_string()),
            ),
            _ => (
                body.provider_type.unwrap_or_else(|| "custom".to_string()),
                body.issuer_url,
                body.authorization_url,
                body.token_url,
                body.userinfo_url,
                body.scopes
                    .unwrap_or_else(|| "openid profile email".to_string()),
            ),
        };

    // Encrypt client secret
    let encrypted_secret = oidc_manager
        .encrypt_secret(&body.client_secret)
        .map_err(|e| AdminError::Internal(format!("Failed to encrypt secret: {e}")))?;

    let row = crate::db::create_oidc_provider(
        &state.db,
        crate::db::CreateOidcProviderParams {
            slug: &body.slug,
            display_name: &body.display_name,
            icon_hint: body.icon_hint.as_deref(),
            provider_type: &provider_type,
            issuer_url: issuer_url.as_deref(),
            authorization_url: authorization_url.as_deref(),
            token_url: token_url.as_deref(),
            userinfo_url: userinfo_url.as_deref(),
            client_id: &body.client_id,
            client_secret_encrypted: &encrypted_secret,
            scopes: &scopes,
            created_by: admin.user_id,
        },
    )
    .await?;

    // Reload providers in the manager
    if let Err(e) = oidc_manager.load_providers(&state.db).await {
        warn!(error = %e, "Failed to reload OIDC providers after creation");
    }

    Ok(Json(row.into()))
}

/// Update OIDC provider request.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateOidcProviderRequest {
    pub display_name: String,
    pub icon_hint: Option<String>,
    pub issuer_url: Option<String>,
    pub authorization_url: Option<String>,
    pub token_url: Option<String>,
    pub userinfo_url: Option<String>,
    pub client_id: String,
    /// If omitted, the existing secret is kept.
    pub client_secret: Option<String>,
    pub scopes: String,
    pub enabled: bool,
}

/// Update an OIDC provider.
///
/// PUT /api/admin/oidc-providers/:id
#[utoipa::path(
    put,
    path = "/api/admin/oidc-providers/{id}",
    tag = "admin",
    params(("id" = Uuid, Path, description = "Provider ID")),
    request_body = UpdateOidcProviderRequest,
    responses((status = 200, description = "OIDC provider updated", body = OidcProviderResponse)),
    security(("bearer_auth" = []))
)]
pub async fn update_oidc_provider(
    State(state): State<AppState>,
    Extension(_admin): Extension<SystemAdminUser>,
    Extension(_elevated): Extension<ElevatedAdmin>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateOidcProviderRequest>,
) -> Result<Json<OidcProviderResponse>, AdminError> {
    let oidc_manager = state
        .oidc_manager
        .as_ref()
        .ok_or_else(|| AdminError::Internal("OIDC manager not configured".into()))?;

    let encrypted_secret = if let Some(ref secret) = body.client_secret {
        Some(
            oidc_manager
                .encrypt_secret(secret)
                .map_err(|e| AdminError::Internal(format!("Failed to encrypt secret: {e}")))?,
        )
    } else {
        None
    };

    let row = crate::db::update_oidc_provider(
        &state.db,
        crate::db::UpdateOidcProviderParams {
            id,
            display_name: &body.display_name,
            icon_hint: body.icon_hint.as_deref(),
            issuer_url: body.issuer_url.as_deref(),
            authorization_url: body.authorization_url.as_deref(),
            token_url: body.token_url.as_deref(),
            userinfo_url: body.userinfo_url.as_deref(),
            client_id: &body.client_id,
            client_secret_encrypted: encrypted_secret.as_deref(),
            scopes: &body.scopes,
            enabled: body.enabled,
        },
    )
    .await?;

    // Reload providers
    if let Err(e) = oidc_manager.load_providers(&state.db).await {
        warn!(error = %e, "Failed to reload OIDC providers after update");
    }

    Ok(Json(row.into()))
}

/// Delete an OIDC provider.
///
/// DELETE /api/admin/oidc-providers/:id
#[utoipa::path(
    delete,
    path = "/api/admin/oidc-providers/{id}",
    tag = "admin",
    params(("id" = Uuid, Path, description = "Provider ID")),
    responses((status = 200, description = "OIDC provider deleted")),
    security(("bearer_auth" = []))
)]
pub async fn delete_oidc_provider(
    State(state): State<AppState>,
    Extension(_admin): Extension<SystemAdminUser>,
    Extension(_elevated): Extension<ElevatedAdmin>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AdminError> {
    let oidc_manager = state
        .oidc_manager
        .as_ref()
        .ok_or_else(|| AdminError::Internal("OIDC manager not configured".into()))?;

    crate::db::delete_oidc_provider(&state.db, id).await?;

    // Reload providers
    if let Err(e) = oidc_manager.load_providers(&state.db).await {
        warn!(error = %e, "Failed to reload OIDC providers after deletion");
    }

    Ok(Json(serde_json::json!({ "success": true })))
}

// ============================================================================
// Delete User / Guild (Elevated)
// ============================================================================

/// Permanently delete a user and all associated data.
///
/// `DELETE /api/admin/users/:id`
#[utoipa::path(
    delete,
    path = "/api/admin/users/{id}",
    tag = "admin",
    params(("id" = Uuid, Path, description = "User ID")),
    responses((status = 200, description = "User deleted", body = DeleteResponse)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn delete_user(
    State(state): State<AppState>,
    Extension(admin): Extension<SystemAdminUser>,
    Extension(_elevated): Extension<ElevatedAdmin>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<DeleteResponse>, AdminError> {
    // Check user exists and get username
    let username = queries::find_user(&state.db, user_id)
        .await?
        .map(|(_, name)| name)
        .ok_or_else(|| AdminError::NotFound("User".to_string()))?;

    // Cannot delete yourself
    if user_id == admin.user_id {
        return Err(AdminError::Validation("Cannot delete yourself".to_string()));
    }

    // Delete user (cascades to guild_members, messages, sessions, global_bans, etc.)
    let deleted = queries::delete_user(&state.db, user_id).await?;

    if deleted == 0 {
        return Err(AdminError::NotFound("User".to_string()));
    }

    // Log the action
    let ip_address = addr.ip().to_string();
    write_audit_log(
        &state.db,
        admin.user_id,
        "admin.users.delete",
        Some("user"),
        Some(user_id),
        Some(serde_json::json!({"username": username})),
        Some(&ip_address),
    )
    .await?;

    // Broadcast admin event
    if let Err(e) = broadcast_admin_event(
        &state.redis,
        &ServerEvent::AdminUserDeleted {
            user_id,
            username: username.clone(),
        },
    )
    .await
    {
        warn!(user_id = %user_id, error = %e, "Failed to broadcast user delete event");
    }

    Ok(Json(DeleteResponse {
        deleted: true,
        id: user_id,
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
