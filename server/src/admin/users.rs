//! Admin user management handlers (list, details, ban, delete, export, bulk).

#![allow(clippy::used_underscore_binding)]

use std::fmt::Write;
use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::{Extension, Json};
use chrono::{DateTime, Utc};
use serde::Serialize;
use tracing::warn;
use uuid::Uuid;

use super::error::AdminError;
use super::queries;
use super::shared::{escape_csv, DeleteResponse, PaginatedResponse, PaginationParams};
use super::types::{
    BulkActionFailure, BulkBanRequest, BulkBanResponse, ElevatedAdmin, GlobalBanRequest,
    SystemAdminUser,
};
use crate::api::AppState;
use crate::permissions::queries::write_audit_log;
use crate::ws::{broadcast_admin_event, ServerEvent};

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

/// Global ban response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct BanResponse {
    pub banned: bool,
    pub user_id: Uuid,
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
