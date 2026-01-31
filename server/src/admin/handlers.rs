//! Admin API handlers for non-elevated actions.
//!
//! These handlers require system admin privileges but not elevated sessions:
//! - List users
//! - List guilds
//! - View audit log
//! - Elevate/de-elevate session

#![allow(clippy::used_underscore_binding)]

use axum::{
    extract::{ConnectInfo, Path, Query, State},
    Extension, Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt::Write;
use tracing::warn;
use std::net::SocketAddr;
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::mfa_crypto::decrypt_mfa_secret;
use crate::db::find_user_by_id;
use crate::permissions::models::AuditLogEntry;
use crate::permissions::queries::{create_elevated_session, write_audit_log};
use sqlx::PgPool;

use axum::http::header;
use axum::response::IntoResponse;

use super::types::{
    AdminError, BulkActionFailure, BulkBanRequest, BulkBanResponse, BulkSuspendRequest,
    BulkSuspendResponse, CreateAnnouncementRequest, ElevateRequest, ElevateResponse,
    ElevatedAdmin, GlobalBanRequest, SuspendGuildRequest, SystemAdminUser,
};
use crate::ws::{broadcast_admin_event, ServerEvent};

// ============================================================================
// Query Parameters
// ============================================================================

/// Pagination query parameters with optional search.
#[derive(Debug, Deserialize)]
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
#[derive(Debug, Deserialize)]
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
#[derive(Debug, Serialize)]
pub struct PaginatedResponse<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

/// User summary for admin listing.
#[derive(Debug, Serialize)]
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
#[derive(Debug, Serialize)]
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
#[derive(Debug, Serialize)]
pub struct AuditLogEntryResponse {
    pub id: Uuid,
    pub actor_id: Uuid,
    pub actor_username: Option<String>,
    pub action: String,
    pub target_type: Option<String>,
    pub target_id: Option<Uuid>,
    pub details: Option<serde_json::Value>,
    pub ip_address: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// De-elevate response.
#[derive(Debug, Serialize)]
pub struct DeElevateResponse {
    pub elevated: bool,
}

/// User guild membership info for detail view.
#[derive(Debug, Serialize)]
pub struct UserGuildMembership {
    pub guild_id: Uuid,
    pub guild_name: String,
    pub guild_icon_url: Option<String>,
    pub joined_at: DateTime<Utc>,
    pub is_owner: bool,
}

/// Detailed user information response.
#[derive(Debug, Serialize)]
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
#[derive(Debug, Serialize)]
pub struct GuildMemberInfo {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub joined_at: DateTime<Utc>,
}

/// Guild owner info for detail view.
#[derive(Debug, Serialize)]
pub struct GuildOwnerInfo {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
}

/// Detailed guild information response.
#[derive(Debug, Serialize)]
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
        sqlx::query_as!(
            ElevatedSessionRecord,
            r#"SELECT id, user_id, elevated_at, expires_at, reason
               FROM elevated_sessions
               WHERE user_id = $1 AND expires_at > NOW()
               ORDER BY elevated_at DESC
               LIMIT 1"#,
            auth.id
        )
        .fetch_optional(&state.db)
        .await?
    } else {
        None
    };

    Ok(Json(super::types::AdminStatusResponse {
        is_admin,
        is_elevated: elevated.is_some(),
        elevation_expires_at: elevated.map(|e| e.expires_at),
    }))
}

/// Elevated session record for querying.
struct ElevatedSessionRecord {
    #[allow(dead_code)]
    id: Uuid,
    #[allow(dead_code)]
    user_id: Uuid,
    #[allow(dead_code)]
    elevated_at: chrono::DateTime<chrono::Utc>,
    expires_at: chrono::DateTime<chrono::Utc>,
    #[allow(dead_code)]
    reason: Option<String>,
}

/// Get admin statistics.
///
/// `GET /api/admin/stats`
#[tracing::instrument(skip(state))]
pub async fn get_admin_stats(
    State(state): State<AppState>,
    Extension(_admin): Extension<SystemAdminUser>,
) -> Result<Json<super::types::AdminStatsResponse>, AdminError> {
    // Get user count
    let user_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await?;

    // Get guild count
    let guild_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM guilds")
        .fetch_one(&state.db)
        .await?;

    // Get banned count
    let banned_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM global_bans WHERE expires_at IS NULL OR expires_at > NOW()")
        .fetch_one(&state.db)
        .await?;

    Ok(Json(super::types::AdminStatsResponse {
        user_count: user_count.0,
        guild_count: guild_count.0,
        banned_count: banned_count.0,
    }))
}

/// List all users with pagination and optional search.
///
/// `GET /api/admin/users`
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
    let search_pattern = params.search.as_ref().map(|s| format!("%{}%", s.to_lowercase()));

    // Get total count (with or without search filter)
    let total: (i64,) = if let Some(ref pattern) = search_pattern {
        sqlx::query_as(
            r"SELECT COUNT(*) FROM users u
              WHERE LOWER(u.username) LIKE $1
                 OR LOWER(u.display_name) LIKE $1
                 OR LOWER(COALESCE(u.email, '')) LIKE $1"
        )
        .bind(pattern)
        .fetch_one(&state.db)
        .await?
    } else {
        sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(&state.db)
            .await?
    };

    // Get users with ban status (with or without search filter)
    let users = if let Some(ref pattern) = search_pattern {
        sqlx::query_as::<_, (Uuid, String, String, Option<String>, Option<String>, DateTime<Utc>, bool)>(
            r"
            SELECT
                u.id,
                u.username,
                u.display_name,
                u.email,
                u.avatar_url,
                u.created_at,
                EXISTS(SELECT 1 FROM global_bans gb WHERE gb.user_id = u.id AND (gb.expires_at IS NULL OR gb.expires_at > NOW())) as is_banned
            FROM users u
            WHERE LOWER(u.username) LIKE $3
               OR LOWER(u.display_name) LIKE $3
               OR LOWER(COALESCE(u.email, '')) LIKE $3
            ORDER BY u.created_at DESC
            LIMIT $1 OFFSET $2
            ",
        )
        .bind(limit)
        .bind(offset)
        .bind(pattern)
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as::<_, (Uuid, String, String, Option<String>, Option<String>, DateTime<Utc>, bool)>(
            r"
            SELECT
                u.id,
                u.username,
                u.display_name,
                u.email,
                u.avatar_url,
                u.created_at,
                EXISTS(SELECT 1 FROM global_bans gb WHERE gb.user_id = u.id AND (gb.expires_at IS NULL OR gb.expires_at > NOW())) as is_banned
            FROM users u
            ORDER BY u.created_at DESC
            LIMIT $1 OFFSET $2
            ",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&state.db)
        .await?
    };

    let items: Vec<UserSummary> = users
        .into_iter()
        .map(|(id, username, display_name, email, avatar_url, created_at, is_banned)| UserSummary {
            id,
            username,
            display_name,
            email,
            avatar_url,
            created_at,
            is_banned,
        })
        .collect();

    Ok(Json(PaginatedResponse {
        items,
        total: total.0,
        limit,
        offset,
    }))
}

/// List all guilds with pagination and optional search.
///
/// `GET /api/admin/guilds`
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
    let search_pattern = params.search.as_ref().map(|s| format!("%{}%", s.to_lowercase()));

    // Get total count (with or without search filter)
    let total: (i64,) = if let Some(ref pattern) = search_pattern {
        sqlx::query_as("SELECT COUNT(*) FROM guilds g WHERE LOWER(g.name) LIKE $1")
            .bind(pattern)
            .fetch_one(&state.db)
            .await?
    } else {
        sqlx::query_as("SELECT COUNT(*) FROM guilds")
            .fetch_one(&state.db)
            .await?
    };

    // Get guilds with member count (with or without search filter)
    let guilds = if let Some(ref pattern) = search_pattern {
        sqlx::query_as::<_, (Uuid, String, Uuid, Option<String>, i64, DateTime<Utc>, Option<DateTime<Utc>>)>(
            r"
            SELECT
                g.id,
                g.name,
                g.owner_id,
                g.icon_url,
                COALESCE((SELECT COUNT(*) FROM guild_members gm WHERE gm.guild_id = g.id), 0) as member_count,
                g.created_at,
                g.suspended_at
            FROM guilds g
            WHERE LOWER(g.name) LIKE $3
            ORDER BY g.created_at DESC
            LIMIT $1 OFFSET $2
            ",
        )
        .bind(limit)
        .bind(offset)
        .bind(pattern)
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as::<_, (Uuid, String, Uuid, Option<String>, i64, DateTime<Utc>, Option<DateTime<Utc>>)>(
            r"
            SELECT
                g.id,
                g.name,
                g.owner_id,
                g.icon_url,
                COALESCE((SELECT COUNT(*) FROM guild_members gm WHERE gm.guild_id = g.id), 0) as member_count,
                g.created_at,
                g.suspended_at
            FROM guilds g
            ORDER BY g.created_at DESC
            LIMIT $1 OFFSET $2
            ",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&state.db)
        .await?
    };

    let items: Vec<GuildSummary> = guilds
        .into_iter()
        .map(|(id, name, owner_id, icon_url, member_count, created_at, suspended_at)| GuildSummary {
            id,
            name,
            owner_id,
            icon_url,
            member_count,
            created_at,
            suspended_at,
        })
        .collect();

    Ok(Json(PaginatedResponse {
        items,
        total: total.0,
        limit,
        offset,
    }))
}

/// Helper function to query audit log with dynamic filters.
async fn get_audit_log_filtered(
    pool: &PgPool,
    limit: i64,
    offset: i64,
    action_filter: Option<&str>,
    exact_action_match: bool,
    from_date: Option<DateTime<Utc>>,
    to_date: Option<DateTime<Utc>>,
) -> Result<(Vec<AuditLogEntry>, (i64,)), AdminError> {
    // Build WHERE clauses dynamically
    let mut conditions: Vec<String> = Vec::new();
    let mut param_idx = 1;

    // Action filter
    if action_filter.is_some() {
        if exact_action_match {
            conditions.push(format!("action = ${param_idx}"));
        } else {
            conditions.push(format!("action LIKE ${param_idx}"));
        }
        param_idx += 1;
    }

    // Date range filters
    if from_date.is_some() {
        conditions.push(format!("created_at >= ${param_idx}"));
        param_idx += 1;
    }
    if to_date.is_some() {
        conditions.push(format!("created_at <= ${param_idx}"));
        param_idx += 1;
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    // Build count query
    let count_sql = format!("SELECT COUNT(*) FROM system_audit_log {where_clause}");

    // Build main query
    let main_sql = format!(
        r"
        SELECT
            id,
            actor_id,
            action,
            target_type,
            target_id,
            details,
            host(ip_address) as ip_address,
            created_at
        FROM system_audit_log
        {where_clause}
        ORDER BY created_at DESC
        LIMIT ${param_idx} OFFSET ${}
        ",
        param_idx + 1
    );

    // Execute queries with dynamic parameter binding
    // We need to handle this with raw query building since we have optional params
    let action_pattern = action_filter.map(|a| {
        if exact_action_match {
            a.to_string()
        } else {
            format!("{a}%")
        }
    });

    // Get total count
    let total: (i64,) = {
        let mut query = sqlx::query_as::<_, (i64,)>(&count_sql);
        if let Some(ref pattern) = action_pattern {
            query = query.bind(pattern);
        }
        if let Some(from) = from_date {
            query = query.bind(from);
        }
        if let Some(to) = to_date {
            query = query.bind(to);
        }
        query.fetch_one(pool).await?
    };

    // Get entries
    let entries: Vec<AuditLogEntry> = {
        let mut query = sqlx::query_as::<_, AuditLogEntry>(&main_sql);
        if let Some(ref pattern) = action_pattern {
            query = query.bind(pattern);
        }
        if let Some(from) = from_date {
            query = query.bind(from);
        }
        if let Some(to) = to_date {
            query = query.bind(to);
        }
        query = query.bind(limit).bind(offset);
        query.fetch_all(pool).await?
    };

    Ok((entries, total))
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
    let (entries, total) = get_audit_log_filtered(
        &state.db,
        limit,
        offset,
        action_filter,
        params.action_type.is_some(), // exact match if action_type is provided
        params.from_date,
        params.to_date,
    )
    .await?;

    // Collect unique actor IDs for username lookup (deduplicated)
    let actor_ids: Vec<Uuid> = entries
        .iter()
        .map(|e| e.actor_id)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    // Fetch usernames for actors
    let usernames: std::collections::HashMap<Uuid, String> = if actor_ids.is_empty() {
        std::collections::HashMap::new()
    } else {
        sqlx::query_as::<_, (Uuid, String)>("SELECT id, username FROM users WHERE id = ANY($1)")
            .bind(&actor_ids)
            .fetch_all(&state.db)
            .await?
            .into_iter()
            .collect()
    };

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
        total: total.0,
        limit,
        offset,
    }))
}

/// Elevate admin session with MFA verification.
///
/// `POST /api/admin/elevate`
///
/// If MFA is not enabled on the user account, elevation is allowed without MFA
/// (for development/testing purposes).
#[tracing::instrument(skip(state, body))]
pub async fn elevate_session(
    State(state): State<AppState>,
    Extension(admin): Extension<SystemAdminUser>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<ElevateRequest>,
) -> Result<Json<ElevateResponse>, AdminError> {
    // Load user to check MFA status
    let user = find_user_by_id(&state.db, admin.user_id)
        .await?
        .ok_or(AdminError::NotAdmin)?;

    // Only verify MFA if the user has it enabled
    if let Some(mfa_secret_encrypted) = user.mfa_secret {
        // Validate MFA code format (6 digits)
        if body.mfa_code.len() != 6 || !body.mfa_code.chars().all(|c| c.is_ascii_digit()) {
            return Err(AdminError::InvalidMfaCode);
        }

        // Get and validate encryption key
        let encryption_key = state
            .config
            .mfa_encryption_key
            .as_ref()
            .ok_or_else(|| AdminError::Validation("MFA encryption not configured".to_string()))?;

        let key_bytes = hex::decode(encryption_key)
            .map_err(|_| AdminError::Validation("Invalid MFA encryption key".to_string()))?;

        // Decrypt MFA secret
        let mfa_secret = decrypt_mfa_secret(&mfa_secret_encrypted, &key_bytes)
            .map_err(|_| AdminError::InvalidMfaCode)?;

        // Verify TOTP code
        let totp = totp_rs::TOTP::new(
            totp_rs::Algorithm::SHA1,
            6,
            1,
            30,
            totp_rs::Secret::Encoded(mfa_secret)
                .to_bytes()
                .map_err(|_| AdminError::InvalidMfaCode)?,
            Some("VoiceChat".to_string()),
            admin.username.clone(),
        )
        .map_err(|_| AdminError::InvalidMfaCode)?;

        if !totp
            .check_current(&body.mfa_code)
            .map_err(|_| AdminError::InvalidMfaCode)?
        {
            return Err(AdminError::InvalidMfaCode);
        }
    }
    // If MFA is not enabled, skip verification (dev/testing mode)

    // Find or create a session for this user
    // We need a valid session_id that references sessions table
    let session = sqlx::query_as::<_, (Uuid,)>(
        r"
        SELECT id FROM sessions
        WHERE user_id = $1 AND expires_at > NOW()
        ORDER BY created_at DESC
        LIMIT 1
        ",
    )
    .bind(admin.user_id)
    .fetch_optional(&state.db)
    .await?;

    let session_id = match session {
        Some((id,)) => id,
        None => {
            // No active session found - this shouldn't happen if user is authenticated
            // but we handle it gracefully by returning an error
            return Err(AdminError::Validation(
                "No active session found".to_string(),
            ));
        }
    };

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
#[tracing::instrument(skip(state))]
pub async fn de_elevate_session(
    State(state): State<AppState>,
    Extension(admin): Extension<SystemAdminUser>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Result<Json<DeElevateResponse>, AdminError> {
    let ip_address = addr.ip().to_string();

    // Delete all elevated sessions for this user
    let result = sqlx::query("DELETE FROM elevated_sessions WHERE user_id = $1")
        .bind(admin.user_id)
        .execute(&state.db)
        .await?;

    // Clear elevated status cache
    super::cache_elevated_status(&state.redis, admin.user_id, false, 1).await;

    // Log the de-elevation if any sessions were deleted
    if result.rows_affected() > 0 {
        write_audit_log(
            &state.db,
            admin.user_id,
            "admin.session.de_elevated",
            Some("user"),
            Some(admin.user_id),
            Some(serde_json::json!({
                "sessions_removed": result.rows_affected(),
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
#[derive(Debug, Serialize)]
pub struct BanResponse {
    pub banned: bool,
    pub user_id: Uuid,
}

/// Guild suspend response.
#[derive(Debug, Serialize)]
pub struct SuspendResponse {
    pub suspended: bool,
    pub guild_id: Uuid,
}

/// Announcement response.
#[derive(Debug, Serialize)]
pub struct AnnouncementResponse {
    pub id: Uuid,
    pub title: String,
    pub created: bool,
}

/// Global ban a user.
///
/// `POST /api/admin/users/:id/ban`
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
    let user = sqlx::query_as::<_, (Uuid, String)>("SELECT id, username FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?;

    let username = match user {
        Some((_, name)) => name,
        None => return Err(AdminError::NotFound("User".to_string())),
    };

    // Cannot ban yourself
    if user_id == admin.user_id {
        return Err(AdminError::Validation("Cannot ban yourself".to_string()));
    }

    // Create or update ban
    sqlx::query(
        r"
        INSERT INTO global_bans (user_id, banned_by, reason, expires_at)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (user_id) DO UPDATE SET
            banned_by = $2,
            reason = $3,
            expires_at = $4,
            created_at = NOW()
        ",
    )
    .bind(user_id)
    .bind(admin.user_id)
    .bind(&body.reason)
    .bind(body.expires_at)
    .execute(&state.db)
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
#[tracing::instrument(skip(state))]
pub async fn unban_user(
    State(state): State<AppState>,
    Extension(admin): Extension<SystemAdminUser>,
    Extension(_elevated): Extension<ElevatedAdmin>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<BanResponse>, AdminError> {
    // Get username for the event
    let username = sqlx::query_scalar::<_, String>("SELECT username FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?
        .unwrap_or_else(|| "Unknown".to_string());

    let result = sqlx::query("DELETE FROM global_bans WHERE user_id = $1")
        .bind(user_id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
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
        &ServerEvent::AdminUserUnbanned { user_id, username: username.clone() },
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
    let guild = sqlx::query_as::<_, (Uuid, String)>("SELECT id, name FROM guilds WHERE id = $1")
        .bind(guild_id)
        .fetch_optional(&state.db)
        .await?;

    let guild_name = match guild {
        Some((_, name)) => name,
        None => return Err(AdminError::NotFound("Guild".to_string())),
    };

    let result = sqlx::query(
        r"
        UPDATE guilds SET
            suspended_at = NOW(),
            suspended_by = $2,
            suspension_reason = $3
        WHERE id = $1 AND suspended_at IS NULL
        ",
    )
    .bind(guild_id)
    .bind(admin.user_id)
    .bind(&body.reason)
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
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
#[tracing::instrument(skip(state))]
pub async fn unsuspend_guild(
    State(state): State<AppState>,
    Extension(admin): Extension<SystemAdminUser>,
    Extension(_elevated): Extension<ElevatedAdmin>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<SuspendResponse>, AdminError> {
    // Get guild name for the event
    let guild_name = sqlx::query_scalar::<_, String>("SELECT name FROM guilds WHERE id = $1")
        .bind(guild_id)
        .fetch_optional(&state.db)
        .await?
        .unwrap_or_else(|| "Unknown".to_string());

    let result = sqlx::query(
        r"
        UPDATE guilds SET
            suspended_at = NULL,
            suspended_by = NULL,
            suspension_reason = NULL
        WHERE id = $1 AND suspended_at IS NOT NULL
        ",
    )
    .bind(guild_id)
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
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

    sqlx::query(
        r"
        INSERT INTO system_announcements (id, author_id, title, content, severity, starts_at, ends_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ",
    )
    .bind(announcement_id)
    .bind(admin.user_id)
    .bind(&body.title)
    .bind(&body.content)
    .bind(&body.severity)
    .bind(starts_at)
    .bind(body.ends_at)
    .execute(&state.db)
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
#[tracing::instrument(skip(state))]
pub async fn get_user_details(
    State(state): State<AppState>,
    Extension(_admin): Extension<SystemAdminUser>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<UserDetailsResponse>, AdminError> {
    // Get basic user info
    let user = sqlx::query!(
        r#"
        SELECT id, username, display_name, email, avatar_url, created_at,
               EXISTS(SELECT 1 FROM global_bans gb WHERE gb.user_id = users.id AND (gb.expires_at IS NULL OR gb.expires_at > NOW())) as "is_banned!"
        FROM users
        WHERE id = $1
        "#,
        user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AdminError::NotFound("User not found".to_string()))?;

    // Get last login from sessions table
    let last_login: Option<DateTime<Utc>> = sqlx::query_scalar!(
        r#"
        SELECT MAX(created_at) as "last_login"
        FROM sessions
        WHERE user_id = $1
        "#,
        user_id
    )
    .fetch_one(&state.db)
    .await?;

    // Get guild memberships
    let guild_memberships = sqlx::query!(
        r#"
        SELECT g.id as guild_id, g.name as guild_name, g.icon_url as guild_icon_url,
               gm.joined_at, g.owner_id = $1 as "is_owner!"
        FROM guild_members gm
        JOIN guilds g ON gm.guild_id = g.id
        WHERE gm.user_id = $1
        ORDER BY gm.joined_at DESC
        "#,
        user_id
    )
    .fetch_all(&state.db)
    .await?;

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
#[tracing::instrument(skip(state))]
pub async fn get_guild_details(
    State(state): State<AppState>,
    Extension(_admin): Extension<SystemAdminUser>,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<GuildDetailsResponse>, AdminError> {
    // Get basic guild info with member count
    let guild = sqlx::query!(
        r#"
        SELECT g.id, g.name, g.icon_url, g.owner_id, g.created_at, g.suspended_at,
               (SELECT COUNT(*) FROM guild_members WHERE guild_id = g.id) as "member_count!"
        FROM guilds g
        WHERE g.id = $1
        "#,
        guild_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AdminError::NotFound("Guild not found".to_string()))?;

    // Get owner info
    let owner = sqlx::query!(
        r#"
        SELECT id, username, display_name, avatar_url
        FROM users
        WHERE id = $1
        "#,
        guild.owner_id
    )
    .fetch_one(&state.db)
    .await?;

    // Get top 5 members (excluding owner, most recent first)
    let top_members_rows = sqlx::query!(
        r#"
        SELECT u.id as user_id, u.username, u.display_name, u.avatar_url, gm.joined_at
        FROM guild_members gm
        JOIN users u ON gm.user_id = u.id
        WHERE gm.guild_id = $1 AND gm.user_id != $2
        ORDER BY gm.joined_at DESC
        LIMIT 5
        "#,
        guild_id,
        guild.owner_id
    )
    .fetch_all(&state.db)
    .await?;

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
    // Uses a single query with optional search filter
    let users = sqlx::query!(
        r#"
        SELECT u.id, u.username, u.display_name, u.email, u.avatar_url, u.created_at,
               EXISTS(SELECT 1 FROM global_bans gb WHERE gb.user_id = u.id AND (gb.expires_at IS NULL OR gb.expires_at > NOW())) as "is_banned!"
        FROM users u
        WHERE $1::text IS NULL
           OR LOWER(u.username) LIKE $1
           OR LOWER(u.display_name) LIKE $1
           OR LOWER(COALESCE(u.email, '')) LIKE $1
        ORDER BY u.created_at DESC
        LIMIT 10000
        "#,
        search_pattern
    )
    .fetch_all(&state.db)
    .await?;

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
        .unwrap();
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
    // Uses a single query with optional search filter
    let guilds = sqlx::query!(
        r#"
        SELECT g.id, g.name, g.owner_id, g.icon_url, g.created_at, g.suspended_at,
               (SELECT COUNT(*) FROM guild_members WHERE guild_id = g.id) as "member_count!"
        FROM guilds g
        WHERE $1::text IS NULL OR LOWER(g.name) LIKE $1
        ORDER BY g.created_at DESC
        LIMIT 10000
        "#,
        search_pattern
    )
    .fetch_all(&state.db)
    .await?;

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
        .unwrap();
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
        return Err(AdminError::Validation("Cannot ban more than 100 users at once".to_string()));
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
        let user_exists = sqlx::query_scalar!(
            "SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)",
            user_id
        )
        .fetch_one(&state.db)
        .await?
        .unwrap_or(false);

        if !user_exists {
            failed.push(BulkActionFailure {
                id: *user_id,
                reason: "User not found".to_string(),
            });
            continue;
        }

        // Check if already banned
        let is_banned = sqlx::query_scalar!(
            "SELECT EXISTS(SELECT 1 FROM global_bans WHERE user_id = $1 AND (expires_at IS NULL OR expires_at > NOW()))",
            user_id
        )
        .fetch_one(&state.db)
        .await?
        .unwrap_or(false);

        if is_banned {
            already_banned += 1;
            continue;
        }

        // Ban the user
        let ban_result = sqlx::query(
            r"
            INSERT INTO global_bans (user_id, banned_by, reason, expires_at)
            VALUES ($1, $2, $3, $4)
            ",
        )
        .bind(user_id)
        .bind(admin.user_id)
        .bind(&body.reason)
        .bind(body.expires_at)
        .execute(&state.db)
        .await;

        match ban_result {
            Ok(_) => {
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
        return Err(AdminError::Validation("Cannot suspend more than 100 guilds at once".to_string()));
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
        let guild = sqlx::query!(
            "SELECT id, suspended_at FROM guilds WHERE id = $1",
            guild_id
        )
        .fetch_optional(&state.db)
        .await?;

        match guild {
            None => {
                failed.push(BulkActionFailure {
                    id: *guild_id,
                    reason: "Guild not found".to_string(),
                });
            }
            Some(g) if g.suspended_at.is_some() => {
                already_suspended += 1;
            }
            Some(_) => {
                // Suspend the guild
                let suspend_result = sqlx::query(
                    "UPDATE guilds SET suspended_at = NOW(), suspension_reason = $1 WHERE id = $2"
                )
                .bind(&body.reason)
                .bind(guild_id)
                .execute(&state.db)
                .await;

                match suspend_result {
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
