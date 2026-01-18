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
use std::net::SocketAddr;
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::mfa_crypto::decrypt_mfa_secret;
use crate::db::find_user_by_id;
use crate::permissions::queries::{
    create_elevated_session, get_audit_log as query_audit_log, write_audit_log,
};

use super::types::{
    AdminError, CreateAnnouncementRequest, ElevateRequest, ElevateResponse, ElevatedAdmin,
    GlobalBanRequest, SuspendGuildRequest, SystemAdminUser,
};

// ============================================================================
// Query Parameters
// ============================================================================

/// Pagination query parameters.
#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    /// Maximum number of items to return.
    #[serde(default = "default_limit")]
    pub limit: i64,
    /// Number of items to skip.
    #[serde(default)]
    pub offset: i64,
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
    pub created_at: DateTime<Utc>,
    pub is_banned: bool,
}

/// Guild summary for admin listing.
#[derive(Debug, Serialize)]
pub struct GuildSummary {
    pub id: Uuid,
    pub name: String,
    pub owner_id: Uuid,
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

// ============================================================================
// Handlers
// ============================================================================

/// List all users with pagination.
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

    // Get total count
    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await?;

    // Get users with ban status
    let users = sqlx::query_as::<_, (Uuid, String, String, Option<String>, DateTime<Utc>, bool)>(
        r"
        SELECT
            u.id,
            u.username,
            u.display_name,
            u.email,
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
    .await?;

    let items: Vec<UserSummary> = users
        .into_iter()
        .map(|(id, username, display_name, email, created_at, is_banned)| UserSummary {
            id,
            username,
            display_name,
            email,
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

/// List all guilds with pagination.
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

    // Get total count
    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM guilds")
        .fetch_one(&state.db)
        .await?;

    // Get guilds with member count
    let guilds = sqlx::query_as::<_, (Uuid, String, Uuid, i64, DateTime<Utc>, Option<DateTime<Utc>>)>(
        r"
        SELECT
            g.id,
            g.name,
            g.owner_id,
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
    .await?;

    let items: Vec<GuildSummary> = guilds
        .into_iter()
        .map(|(id, name, owner_id, member_count, created_at, suspended_at)| GuildSummary {
            id,
            name,
            owner_id,
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

/// Get system audit log with pagination and optional action filter.
///
/// `GET /api/admin/audit-log`
#[tracing::instrument(skip(state))]
pub async fn get_audit_log(
    State(state): State<AppState>,
    Extension(_admin): Extension<SystemAdminUser>,
    Query(params): Query<AuditLogParams>,
) -> Result<Json<PaginatedResponse<AuditLogEntryResponse>>, AdminError> {
    // Clamp limit to reasonable bounds
    let limit = params.limit.clamp(1, 100);
    let offset = params.offset.max(0);

    // Get total count (with or without filter)
    let total: (i64,) = if let Some(ref action_filter) = params.action {
        let pattern = format!("{action_filter}%");
        sqlx::query_as("SELECT COUNT(*) FROM system_audit_log WHERE action LIKE $1")
            .bind(pattern)
            .fetch_one(&state.db)
            .await?
    } else {
        sqlx::query_as("SELECT COUNT(*) FROM system_audit_log")
            .fetch_one(&state.db)
            .await?
    };

    // Get audit log entries
    let entries = query_audit_log(&state.db, limit, offset, params.action.as_deref()).await?;

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
#[tracing::instrument(skip(state, body))]
pub async fn elevate_session(
    State(state): State<AppState>,
    Extension(admin): Extension<SystemAdminUser>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<ElevateRequest>,
) -> Result<Json<ElevateResponse>, AdminError> {
    // Validate MFA code format (6 digits)
    if body.mfa_code.len() != 6 || !body.mfa_code.chars().all(|c| c.is_ascii_digit()) {
        return Err(AdminError::InvalidMfaCode);
    }

    // Load user to check MFA status
    let user = find_user_by_id(&state.db, admin.user_id)
        .await?
        .ok_or(AdminError::NotAdmin)?;

    // Check if MFA is enabled
    let mfa_secret_encrypted = user.mfa_secret.ok_or(AdminError::MfaRequired)?;

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
    // Check user exists
    let user_exists: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?;

    if user_exists.is_none() {
        return Err(AdminError::NotFound("User".to_string()));
    }

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
        // Check if guild exists
        let exists: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM guilds WHERE id = $1")
            .bind(guild_id)
            .fetch_optional(&state.db)
            .await?;

        if exists.is_none() {
            return Err(AdminError::NotFound("Guild".to_string()));
        }
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
