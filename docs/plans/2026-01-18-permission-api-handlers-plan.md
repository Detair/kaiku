# Permission API Handlers Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement API handlers for system admin and guild role management with permission checking, elevation flow, and audit logging.

**Architecture:** New `admin` module for system admin routes with middleware. Permission helper functions in `permissions/helpers.rs`. Guild role handlers in `guild/roles.rs`. All destructive admin actions require elevation.

**Tech Stack:** Rust/Axum, sqlx, tower middleware, serde

---

## Task 1: Permission Helpers

**Files:**
- Create: `server/src/permissions/helpers.rs`
- Modify: `server/src/permissions/mod.rs`
- Modify: `server/src/permissions/resolver.rs`

### Step 1: Add new error variants to PermissionError

Modify `server/src/permissions/resolver.rs` - add variants after `CannotModerateOwner`:

```rust
/// Permission check errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionError {
    /// User lacks required permission.
    MissingPermission(GuildPermissions),

    /// Role hierarchy violation.
    RoleHierarchy {
        actor_position: i32,
        target_position: i32,
    },

    /// Attempted to grant permissions not held.
    CannotEscalate(GuildPermissions),

    /// Attempted to moderate guild owner.
    CannotModerateOwner,

    /// User is not a member of the guild.
    NotGuildMember,

    /// Action requires elevated session.
    ElevationRequired,

    /// User is not a system admin.
    NotSystemAdmin,
}
```

Update the `Display` impl to handle new variants:

```rust
impl std::fmt::Display for PermissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingPermission(p) => write!(f, "Missing permission: {p:?}"),
            Self::RoleHierarchy {
                actor_position,
                target_position,
            } => write!(
                f,
                "Cannot modify role at position {target_position} (your position: {actor_position})"
            ),
            Self::CannotEscalate(p) => {
                write!(f, "Cannot grant permissions you don't have: {p:?}")
            }
            Self::CannotModerateOwner => write!(f, "Cannot moderate guild owner"),
            Self::NotGuildMember => write!(f, "Not a member of this guild"),
            Self::ElevationRequired => write!(f, "This action requires an elevated session"),
            Self::NotSystemAdmin => write!(f, "System admin privileges required"),
        }
    }
}
```

### Step 2: Create helpers.rs with MemberPermissionContext

Create `server/src/permissions/helpers.rs`:

```rust
//! Permission helper functions for API handlers.

use sqlx::PgPool;
use uuid::Uuid;

use super::guild::GuildPermissions;
use super::models::GuildRole;
use super::resolver::{compute_guild_permissions, PermissionError};

/// Context for permission checks within a guild.
#[derive(Debug, Clone)]
pub struct MemberPermissionContext {
    /// Guild owner ID.
    pub guild_owner_id: Uuid,
    /// @everyone role permissions.
    pub everyone_permissions: GuildPermissions,
    /// User's assigned roles (not including @everyone).
    pub member_roles: Vec<GuildRole>,
    /// Pre-computed effective permissions.
    pub computed_permissions: GuildPermissions,
    /// Highest role position (lowest number = highest rank).
    pub highest_role_position: i32,
    /// Whether user is the guild owner.
    pub is_owner: bool,
}

/// Load permission context for a guild member.
///
/// Returns `None` if user is not a member of the guild.
pub async fn get_member_permission_context(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<Option<MemberPermissionContext>, sqlx::Error> {
    // First check membership and get guild owner
    let membership = sqlx::query!(
        r#"SELECT g.owner_id
           FROM guild_members gm
           JOIN guilds g ON g.id = gm.guild_id
           WHERE gm.guild_id = $1 AND gm.user_id = $2"#,
        guild_id,
        user_id
    )
    .fetch_optional(pool)
    .await?;

    let Some(membership) = membership else {
        return Ok(None);
    };

    let guild_owner_id = membership.owner_id;
    let is_owner = guild_owner_id == user_id;

    // Get @everyone role
    let everyone_role = sqlx::query!(
        r#"SELECT permissions FROM guild_roles
           WHERE guild_id = $1 AND is_default = true"#,
        guild_id
    )
    .fetch_optional(pool)
    .await?;

    let everyone_permissions = everyone_role
        .map(|r| GuildPermissions::from_bits_truncate(r.permissions as u64))
        .unwrap_or_default();

    // Get user's assigned roles
    let member_roles: Vec<GuildRole> = sqlx::query_as!(
        GuildRole,
        r#"SELECT r.id, r.guild_id, r.name, r.color, r.permissions,
                  r.position, r.is_default, r.created_at
           FROM guild_roles r
           JOIN guild_member_roles mr ON r.id = mr.role_id
           WHERE mr.guild_id = $1 AND mr.user_id = $2 AND r.is_default = false
           ORDER BY r.position ASC"#,
        guild_id,
        user_id
    )
    .fetch_all(pool)
    .await?;

    // Compute permissions
    let computed_permissions = compute_guild_permissions(
        user_id,
        guild_owner_id,
        everyone_permissions,
        &member_roles,
        None,
    );

    // Get highest role position (lowest number)
    let highest_role_position = member_roles
        .iter()
        .map(|r| r.position)
        .min()
        .unwrap_or(i32::MAX);

    Ok(Some(MemberPermissionContext {
        guild_owner_id,
        everyone_permissions,
        member_roles,
        computed_permissions,
        highest_role_position,
        is_owner,
    }))
}

/// Require that user has a specific guild permission.
///
/// Returns the permission context if successful, or an error if:
/// - User is not a guild member
/// - User lacks the required permission
pub async fn require_guild_permission(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
    required: GuildPermissions,
) -> Result<MemberPermissionContext, PermissionError> {
    let ctx = get_member_permission_context(pool, guild_id, user_id)
        .await
        .map_err(|_| PermissionError::NotGuildMember)?
        .ok_or(PermissionError::NotGuildMember)?;

    if !ctx.computed_permissions.contains(required) {
        return Err(PermissionError::MissingPermission(required));
    }

    Ok(ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_member_permission_context_debug() {
        // Just verify the struct can be constructed and debug-printed
        let ctx = MemberPermissionContext {
            guild_owner_id: Uuid::nil(),
            everyone_permissions: GuildPermissions::empty(),
            member_roles: vec![],
            computed_permissions: GuildPermissions::SEND_MESSAGES,
            highest_role_position: 999,
            is_owner: false,
        };
        assert!(!ctx.is_owner);
        assert!(ctx.computed_permissions.contains(GuildPermissions::SEND_MESSAGES));
    }
}
```

### Step 3: Update mod.rs to export helpers

Modify `server/src/permissions/mod.rs`:

```rust
//! Permission system types and utilities.
//!
//! Two-tier permission model:
//! - System permissions: Platform-level admin actions
//! - Guild permissions: Per-guild role-based access control

pub mod guild;
pub mod helpers;
pub mod models;
pub mod queries;
pub mod resolver;
pub mod system;

pub use guild::GuildPermissions;
pub use helpers::{get_member_permission_context, require_guild_permission, MemberPermissionContext};
pub use models::*;
pub use queries::*;
pub use resolver::{
    can_manage_role, can_moderate_member, compute_guild_permissions, PermissionError,
};
pub use system::SystemPermission;
```

### Step 4: Build and test

```bash
cd /home/detair/GIT/canis/.worktrees/permission-system/server
DATABASE_URL="postgres://voicechat:voicechat_dev@localhost:5433/voicechat" cargo build --lib
DATABASE_URL="postgres://voicechat:voicechat_dev@localhost:5433/voicechat" cargo test permissions --lib
```

Expected: Build succeeds, all permission tests pass.

### Step 5: Commit

```bash
git add server/src/permissions/
git commit -m "feat(permissions): add permission helpers for API handlers

- MemberPermissionContext struct with pre-computed permissions
- get_member_permission_context for loading guild member context
- require_guild_permission helper for handlers
- Extended PermissionError with NotGuildMember, ElevationRequired, NotSystemAdmin

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 2: Admin Module Structure and Middleware

**Files:**
- Create: `server/src/admin/mod.rs`
- Create: `server/src/admin/middleware.rs`
- Create: `server/src/admin/types.rs`
- Modify: `server/src/lib.rs`

### Step 1: Create admin types

Create `server/src/admin/types.rs`:

```rust
//! Admin module types.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::permissions::PermissionError;

/// Authenticated system admin user.
#[derive(Debug, Clone)]
pub struct SystemAdminUser {
    /// User ID.
    pub user_id: Uuid,
    /// Username.
    pub username: String,
    /// When admin was granted.
    pub granted_at: DateTime<Utc>,
}

/// Elevated admin session.
#[derive(Debug, Clone)]
pub struct ElevatedAdmin {
    /// User ID.
    pub user_id: Uuid,
    /// When elevation started.
    pub elevated_at: DateTime<Utc>,
    /// When elevation expires.
    pub expires_at: DateTime<Utc>,
    /// Reason for elevation.
    pub reason: Option<String>,
}

/// Admin API error type.
#[derive(Debug)]
pub enum AdminError {
    /// Not a system admin.
    NotAdmin,
    /// Elevation required for this action.
    ElevationRequired,
    /// MFA required to elevate.
    MfaRequired,
    /// Invalid MFA code.
    InvalidMfaCode,
    /// Resource not found.
    NotFound(String),
    /// Validation error.
    Validation(String),
    /// Database error.
    Database(sqlx::Error),
    /// Permission error.
    Permission(PermissionError),
}

impl IntoResponse for AdminError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Self::NotAdmin => (
                StatusCode::FORBIDDEN,
                serde_json::json!({"error": "not_admin", "message": "System admin privileges required"}),
            ),
            Self::ElevationRequired => (
                StatusCode::FORBIDDEN,
                serde_json::json!({"error": "elevation_required", "message": "This action requires an elevated session"}),
            ),
            Self::MfaRequired => (
                StatusCode::BAD_REQUEST,
                serde_json::json!({"error": "mfa_required", "message": "MFA must be enabled to elevate session"}),
            ),
            Self::InvalidMfaCode => (
                StatusCode::UNAUTHORIZED,
                serde_json::json!({"error": "invalid_mfa_code", "message": "Invalid MFA code"}),
            ),
            Self::NotFound(what) => (
                StatusCode::NOT_FOUND,
                serde_json::json!({"error": "not_found", "message": format!("{} not found", what)}),
            ),
            Self::Validation(msg) => (
                StatusCode::BAD_REQUEST,
                serde_json::json!({"error": "validation", "message": msg}),
            ),
            Self::Database(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"error": "database", "message": "Database error"}),
            ),
            Self::Permission(e) => (
                StatusCode::FORBIDDEN,
                serde_json::json!({"error": "permission", "message": e.to_string()}),
            ),
        };
        (status, Json(body)).into_response()
    }
}

impl From<sqlx::Error> for AdminError {
    fn from(err: sqlx::Error) -> Self {
        Self::Database(err)
    }
}

impl From<PermissionError> for AdminError {
    fn from(err: PermissionError) -> Self {
        Self::Permission(err)
    }
}

/// Request to elevate session.
#[derive(Debug, Deserialize)]
pub struct ElevateRequest {
    /// TOTP code.
    pub mfa_code: String,
    /// Optional reason for elevation.
    pub reason: Option<String>,
}

/// Response after successful elevation.
#[derive(Debug, Serialize)]
pub struct ElevateResponse {
    /// Whether elevation was successful.
    pub elevated: bool,
    /// When elevation expires.
    pub expires_at: DateTime<Utc>,
    /// Elevated session ID.
    pub session_id: Uuid,
}

/// Request to ban a user globally.
#[derive(Debug, Deserialize)]
pub struct GlobalBanRequest {
    /// Reason for ban.
    pub reason: String,
    /// Optional expiry time.
    pub expires_at: Option<DateTime<Utc>>,
}

/// Request to suspend a guild.
#[derive(Debug, Deserialize)]
pub struct SuspendGuildRequest {
    /// Reason for suspension.
    pub reason: String,
}

/// Request to create an announcement.
#[derive(Debug, Deserialize)]
pub struct CreateAnnouncementRequest {
    /// Announcement title.
    pub title: String,
    /// Announcement content.
    pub content: String,
    /// Severity level.
    #[serde(default = "default_severity")]
    pub severity: String,
    /// When to start showing.
    pub starts_at: Option<DateTime<Utc>>,
    /// When to stop showing.
    pub ends_at: Option<DateTime<Utc>>,
}

fn default_severity() -> String {
    "info".to_string()
}
```

### Step 2: Create admin middleware

Create `server/src/admin/middleware.rs`:

```rust
//! Admin authentication and authorization middleware.

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};

use crate::api::AppState;
use crate::auth::AuthUser;
use crate::permissions::queries::{get_elevated_session, is_system_admin, get_system_admin};

use super::types::{AdminError, ElevatedAdmin, SystemAdminUser};

/// Middleware that requires the user to be a system admin.
///
/// Extracts `AuthUser` from request extensions (set by `require_auth`),
/// verifies admin status, and injects `SystemAdminUser` into extensions.
pub async fn require_system_admin(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, AdminError> {
    // Get authenticated user from extensions
    let auth = request
        .extensions()
        .get::<AuthUser>()
        .cloned()
        .ok_or(AdminError::NotAdmin)?;

    // Check if user is a system admin
    let admin = get_system_admin(&state.db, auth.id)
        .await?
        .ok_or(AdminError::NotAdmin)?;

    // Inject SystemAdminUser into extensions
    let admin_user = SystemAdminUser {
        user_id: auth.id,
        username: auth.username,
        granted_at: admin.granted_at,
    };
    request.extensions_mut().insert(admin_user);

    Ok(next.run(request).await)
}

/// Middleware that requires an elevated admin session.
///
/// Must be applied AFTER `require_system_admin`.
/// Checks for valid, non-expired elevated session.
pub async fn require_elevated(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, AdminError> {
    // Get admin user from extensions (set by require_system_admin)
    let admin = request
        .extensions()
        .get::<SystemAdminUser>()
        .cloned()
        .ok_or(AdminError::NotAdmin)?;

    // Get session ID from JWT claims (stored in extensions by require_auth)
    // For now, we'll use a simplified approach - check if user has any active elevated session
    let elevated = sqlx::query!(
        r#"SELECT id, user_id, elevated_at, expires_at, reason
           FROM elevated_sessions
           WHERE user_id = $1 AND expires_at > NOW()
           ORDER BY elevated_at DESC
           LIMIT 1"#,
        admin.user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AdminError::ElevationRequired)?;

    // Inject ElevatedAdmin into extensions
    let elevated_admin = ElevatedAdmin {
        user_id: elevated.user_id,
        elevated_at: elevated.elevated_at,
        expires_at: elevated.expires_at,
        reason: elevated.reason,
    };
    request.extensions_mut().insert(elevated_admin);

    Ok(next.run(request).await)
}
```

### Step 3: Create admin mod.rs

Create `server/src/admin/mod.rs`:

```rust
//! System Admin Module
//!
//! Platform-level administration for system admins.

pub mod middleware;
pub mod types;

use axum::{routing::{delete, get, post}, Router};

use crate::api::AppState;

pub use middleware::{require_elevated, require_system_admin};
pub use types::{AdminError, ElevatedAdmin, SystemAdminUser};

/// Create the admin router.
///
/// All routes require system admin privileges.
/// Destructive routes additionally require elevation.
pub fn router() -> Router<AppState> {
    Router::new()
        // Placeholder - handlers will be added in next task
        .route("/health", get(|| async { "admin ok" }))
}
```

### Step 4: Register admin module in lib.rs

Modify `server/src/lib.rs`:

```rust
//! `VoiceChat` Server
//!
//! Self-hosted voice and text chat platform for gaming communities.
//! Optimized for low latency (<50ms), high quality, and maximum security.

pub mod admin;
pub mod api;
pub mod auth;
pub mod chat;
pub mod config;
pub mod db;
pub mod guild;
pub mod permissions;
pub mod ratelimit;
pub mod social;
pub mod voice;
pub mod ws;

#[cfg(test)]
mod redis_tests;
```

### Step 5: Build

```bash
cd /home/detair/GIT/canis/.worktrees/permission-system/server
DATABASE_URL="postgres://voicechat:voicechat_dev@localhost:5433/voicechat" cargo build --lib
```

Expected: Build succeeds.

### Step 6: Commit

```bash
git add server/src/admin/ server/src/lib.rs
git commit -m "feat(admin): add admin module structure and middleware

- SystemAdminUser and ElevatedAdmin types
- require_system_admin middleware
- require_elevated middleware for destructive actions
- AdminError with detailed JSON responses
- Request types for elevation, bans, suspensions

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 3: Admin Handlers (Non-Elevated)

**Files:**
- Create: `server/src/admin/handlers.rs`
- Modify: `server/src/admin/mod.rs`

### Step 1: Create admin handlers

Create `server/src/admin/handlers.rs`:

```rust
//! System admin API handlers.

use axum::{
    extract::{Path, Query, State},
    Extension, Json,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::AuthUser;
use crate::permissions::{queries as perm_queries, SystemPermission};

use super::types::{
    AdminError, ElevateRequest, ElevateResponse, SystemAdminUser,
};

// ============================================================================
// Query Parameters
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    50
}

#[derive(Debug, Deserialize)]
pub struct AuditLogParams {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
    pub action: Option<String>,
}

// ============================================================================
// Response Types
// ============================================================================

#[derive(Debug, Serialize)]
pub struct UserListItem {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub email: Option<String>,
    pub created_at: chrono::DateTime<Utc>,
    pub is_banned: bool,
}

#[derive(Debug, Serialize)]
pub struct GuildListItem {
    pub id: Uuid,
    pub name: String,
    pub owner_id: Uuid,
    pub member_count: i64,
    pub created_at: chrono::DateTime<Utc>,
    pub suspended_at: Option<chrono::DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct PaginatedResponse<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Serialize)]
pub struct AuditLogEntry {
    pub id: Uuid,
    pub actor_id: Uuid,
    pub actor_username: Option<String>,
    pub action: String,
    pub target_type: Option<String>,
    pub target_id: Option<Uuid>,
    pub details: Option<serde_json::Value>,
    pub ip_address: Option<String>,
    pub created_at: chrono::DateTime<Utc>,
}

// ============================================================================
// Handlers
// ============================================================================

/// GET /api/admin/users
/// List all users (paginated)
#[tracing::instrument(skip(state))]
pub async fn list_users(
    State(state): State<AppState>,
    Extension(_admin): Extension<SystemAdminUser>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<PaginatedResponse<UserListItem>>, AdminError> {
    let users = sqlx::query!(
        r#"SELECT u.id, u.username, u.display_name, u.email, u.created_at,
                  (gb.user_id IS NOT NULL) as "is_banned!"
           FROM users u
           LEFT JOIN global_bans gb ON u.id = gb.user_id
           ORDER BY u.created_at DESC
           LIMIT $1 OFFSET $2"#,
        params.limit,
        params.offset
    )
    .fetch_all(&state.db)
    .await?;

    let total = sqlx::query_scalar!("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await?
        .unwrap_or(0);

    let items = users
        .into_iter()
        .map(|u| UserListItem {
            id: u.id,
            username: u.username,
            display_name: u.display_name,
            email: u.email,
            created_at: u.created_at,
            is_banned: u.is_banned,
        })
        .collect();

    Ok(Json(PaginatedResponse {
        items,
        total,
        limit: params.limit,
        offset: params.offset,
    }))
}

/// GET /api/admin/guilds
/// List all guilds (paginated)
#[tracing::instrument(skip(state))]
pub async fn list_guilds(
    State(state): State<AppState>,
    Extension(_admin): Extension<SystemAdminUser>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<PaginatedResponse<GuildListItem>>, AdminError> {
    let guilds = sqlx::query!(
        r#"SELECT g.id, g.name, g.owner_id, g.created_at, g.suspended_at,
                  (SELECT COUNT(*) FROM guild_members WHERE guild_id = g.id) as "member_count!"
           FROM guilds g
           ORDER BY g.created_at DESC
           LIMIT $1 OFFSET $2"#,
        params.limit,
        params.offset
    )
    .fetch_all(&state.db)
    .await?;

    let total = sqlx::query_scalar!("SELECT COUNT(*) FROM guilds")
        .fetch_one(&state.db)
        .await?
        .unwrap_or(0);

    let items = guilds
        .into_iter()
        .map(|g| GuildListItem {
            id: g.id,
            name: g.name,
            owner_id: g.owner_id,
            member_count: g.member_count,
            created_at: g.created_at,
            suspended_at: g.suspended_at,
        })
        .collect();

    Ok(Json(PaginatedResponse {
        items,
        total,
        limit: params.limit,
        offset: params.offset,
    }))
}

/// GET /api/admin/audit-log
/// View system audit log
#[tracing::instrument(skip(state))]
pub async fn get_audit_log(
    State(state): State<AppState>,
    Extension(_admin): Extension<SystemAdminUser>,
    Query(params): Query<AuditLogParams>,
) -> Result<Json<PaginatedResponse<AuditLogEntry>>, AdminError> {
    let entries = sqlx::query!(
        r#"SELECT al.id, al.actor_id, u.username as actor_username,
                  al.action, al.target_type, al.target_id, al.details,
                  al.ip_address::text as ip_address, al.created_at
           FROM system_audit_log al
           LEFT JOIN users u ON al.actor_id = u.id
           WHERE ($3::text IS NULL OR al.action LIKE $3 || '%')
           ORDER BY al.created_at DESC
           LIMIT $1 OFFSET $2"#,
        params.limit,
        params.offset,
        params.action
    )
    .fetch_all(&state.db)
    .await?;

    let total = sqlx::query_scalar!(
        r#"SELECT COUNT(*) FROM system_audit_log
           WHERE ($1::text IS NULL OR action LIKE $1 || '%')"#,
        params.action
    )
    .fetch_one(&state.db)
    .await?
    .unwrap_or(0);

    let items = entries
        .into_iter()
        .map(|e| AuditLogEntry {
            id: e.id,
            actor_id: e.actor_id,
            actor_username: e.actor_username,
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
        limit: params.limit,
        offset: params.offset,
    }))
}

/// POST /api/admin/elevate
/// Elevate admin session (requires MFA)
#[tracing::instrument(skip(state, body))]
pub async fn elevate_session(
    State(state): State<AppState>,
    Extension(admin): Extension<SystemAdminUser>,
    auth: AuthUser,
    Json(body): Json<ElevateRequest>,
) -> Result<Json<ElevateResponse>, AdminError> {
    // Check if user has MFA enabled
    let user = sqlx::query!(
        "SELECT mfa_secret FROM users WHERE id = $1",
        admin.user_id
    )
    .fetch_one(&state.db)
    .await?;

    let mfa_secret = user.mfa_secret.ok_or(AdminError::MfaRequired)?;

    // Verify MFA code
    let totp = totp_rs::TOTP::new(
        totp_rs::Algorithm::SHA1,
        6,
        1,
        30,
        totp_rs::Secret::Encoded(mfa_secret).to_bytes().map_err(|_| AdminError::InvalidMfaCode)?,
    )
    .map_err(|_| AdminError::InvalidMfaCode)?;

    if !totp.check_current(&body.mfa_code).map_err(|_| AdminError::InvalidMfaCode)? {
        return Err(AdminError::InvalidMfaCode);
    }

    // Get session ID from auth context
    // For now we'll create a new elevated session without linking to specific session
    let session_id = Uuid::now_v7();
    let duration_minutes = 15i64; // TODO: make configurable from system_settings
    let expires_at = Utc::now() + Duration::minutes(duration_minutes);

    // Get IP address (would come from request in real impl)
    let ip_address = "0.0.0.0"; // Placeholder

    // Create elevated session
    let elevated = perm_queries::create_elevated_session(
        &state.db,
        admin.user_id,
        session_id,
        ip_address,
        duration_minutes,
        body.reason.as_deref(),
    )
    .await?;

    // Log the elevation
    perm_queries::write_audit_log(
        &state.db,
        admin.user_id,
        SystemPermission::UseBreakGlass.action_name(),
        Some("user"),
        Some(admin.user_id),
        Some(serde_json::json!({"reason": body.reason})),
        Some(ip_address),
    )
    .await?;

    Ok(Json(ElevateResponse {
        elevated: true,
        expires_at: elevated.expires_at,
        session_id: elevated.id,
    }))
}

/// DELETE /api/admin/elevate
/// De-elevate admin session
#[tracing::instrument(skip(state))]
pub async fn de_elevate_session(
    State(state): State<AppState>,
    Extension(admin): Extension<SystemAdminUser>,
) -> Result<Json<serde_json::Value>, AdminError> {
    // Delete all elevated sessions for this user
    sqlx::query!(
        "DELETE FROM elevated_sessions WHERE user_id = $1",
        admin.user_id
    )
    .execute(&state.db)
    .await?;

    // Log the de-elevation
    perm_queries::write_audit_log(
        &state.db,
        admin.user_id,
        "system.session.de_elevate",
        Some("user"),
        Some(admin.user_id),
        None,
        None,
    )
    .await?;

    Ok(Json(serde_json::json!({"elevated": false})))
}
```

### Step 2: Update admin mod.rs with routes

Modify `server/src/admin/mod.rs`:

```rust
//! System Admin Module
//!
//! Platform-level administration for system admins.

pub mod handlers;
pub mod middleware;
pub mod types;

use axum::{
    routing::{delete, get, post},
    Router,
};

use crate::api::AppState;

pub use middleware::{require_elevated, require_system_admin};
pub use types::{AdminError, ElevatedAdmin, SystemAdminUser};

/// Create the admin router.
///
/// All routes require system admin privileges (applied by parent).
/// Destructive routes additionally require elevation.
pub fn router() -> Router<AppState> {
    Router::new()
        // Non-elevated routes
        .route("/users", get(handlers::list_users))
        .route("/guilds", get(handlers::list_guilds))
        .route("/audit-log", get(handlers::get_audit_log))
        .route("/elevate", post(handlers::elevate_session).delete(handlers::de_elevate_session))
}
```

### Step 3: Build

```bash
cd /home/detair/GIT/canis/.worktrees/permission-system/server
DATABASE_URL="postgres://voicechat:voicechat_dev@localhost:5433/voicechat" cargo build --lib
```

Expected: Build succeeds.

### Step 4: Commit

```bash
git add server/src/admin/
git commit -m "feat(admin): add non-elevated admin handlers

- GET /api/admin/users - list all users
- GET /api/admin/guilds - list all guilds
- GET /api/admin/audit-log - view audit log
- POST /api/admin/elevate - elevate session with MFA
- DELETE /api/admin/elevate - de-elevate session

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 4: Admin Handlers (Elevated - Destructive)

**Files:**
- Modify: `server/src/admin/handlers.rs`
- Modify: `server/src/admin/mod.rs`

### Step 1: Add elevated handlers

Add to `server/src/admin/handlers.rs`:

```rust
// ============================================================================
// Elevated Handlers (Destructive Actions)
// ============================================================================

/// POST /api/admin/users/:id/ban
/// Global ban a user (requires elevation)
#[tracing::instrument(skip(state))]
pub async fn ban_user(
    State(state): State<AppState>,
    Extension(admin): Extension<SystemAdminUser>,
    Extension(_elevated): Extension<ElevatedAdmin>,
    Path(user_id): Path<Uuid>,
    Json(body): Json<super::types::GlobalBanRequest>,
) -> Result<Json<serde_json::Value>, AdminError> {
    // Check user exists
    let user_exists = sqlx::query_scalar!("SELECT id FROM users WHERE id = $1", user_id)
        .fetch_optional(&state.db)
        .await?
        .is_some();

    if !user_exists {
        return Err(AdminError::NotFound("User".to_string()));
    }

    // Cannot ban yourself
    if user_id == admin.user_id {
        return Err(AdminError::Validation("Cannot ban yourself".to_string()));
    }

    // Create or update ban
    sqlx::query!(
        r#"INSERT INTO global_bans (user_id, banned_by, reason, expires_at)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT (user_id) DO UPDATE SET
               banned_by = $2,
               reason = $3,
               expires_at = $4,
               created_at = NOW()"#,
        user_id,
        admin.user_id,
        body.reason,
        body.expires_at
    )
    .execute(&state.db)
    .await?;

    // Log the action
    perm_queries::write_audit_log(
        &state.db,
        admin.user_id,
        SystemPermission::GlobalBanUser.action_name(),
        Some("user"),
        Some(user_id),
        Some(serde_json::json!({"reason": body.reason, "expires_at": body.expires_at})),
        None,
    )
    .await?;

    Ok(Json(serde_json::json!({"banned": true, "user_id": user_id})))
}

/// DELETE /api/admin/users/:id/ban
/// Remove global ban (requires elevation)
#[tracing::instrument(skip(state))]
pub async fn unban_user(
    State(state): State<AppState>,
    Extension(admin): Extension<SystemAdminUser>,
    Extension(_elevated): Extension<ElevatedAdmin>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AdminError> {
    let deleted = sqlx::query!("DELETE FROM global_bans WHERE user_id = $1", user_id)
        .execute(&state.db)
        .await?
        .rows_affected();

    if deleted == 0 {
        return Err(AdminError::NotFound("Ban".to_string()));
    }

    // Log the action
    perm_queries::write_audit_log(
        &state.db,
        admin.user_id,
        "system.users.unban",
        Some("user"),
        Some(user_id),
        None,
        None,
    )
    .await?;

    Ok(Json(serde_json::json!({"banned": false, "user_id": user_id})))
}

/// POST /api/admin/guilds/:id/suspend
/// Suspend a guild (requires elevation)
#[tracing::instrument(skip(state))]
pub async fn suspend_guild(
    State(state): State<AppState>,
    Extension(admin): Extension<SystemAdminUser>,
    Extension(_elevated): Extension<ElevatedAdmin>,
    Path(guild_id): Path<Uuid>,
    Json(body): Json<super::types::SuspendGuildRequest>,
) -> Result<Json<serde_json::Value>, AdminError> {
    let updated = sqlx::query!(
        r#"UPDATE guilds SET
               suspended_at = NOW(),
               suspended_by = $2,
               suspension_reason = $3
           WHERE id = $1 AND suspended_at IS NULL"#,
        guild_id,
        admin.user_id,
        body.reason
    )
    .execute(&state.db)
    .await?
    .rows_affected();

    if updated == 0 {
        // Check if guild exists
        let exists = sqlx::query_scalar!("SELECT id FROM guilds WHERE id = $1", guild_id)
            .fetch_optional(&state.db)
            .await?
            .is_some();

        if !exists {
            return Err(AdminError::NotFound("Guild".to_string()));
        }
        return Err(AdminError::Validation("Guild is already suspended".to_string()));
    }

    // Log the action
    perm_queries::write_audit_log(
        &state.db,
        admin.user_id,
        SystemPermission::SuspendGuild.action_name(),
        Some("guild"),
        Some(guild_id),
        Some(serde_json::json!({"reason": body.reason})),
        None,
    )
    .await?;

    Ok(Json(serde_json::json!({"suspended": true, "guild_id": guild_id})))
}

/// DELETE /api/admin/guilds/:id/suspend
/// Unsuspend a guild (requires elevation)
#[tracing::instrument(skip(state))]
pub async fn unsuspend_guild(
    State(state): State<AppState>,
    Extension(admin): Extension<SystemAdminUser>,
    Extension(_elevated): Extension<ElevatedAdmin>,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AdminError> {
    let updated = sqlx::query!(
        r#"UPDATE guilds SET
               suspended_at = NULL,
               suspended_by = NULL,
               suspension_reason = NULL
           WHERE id = $1 AND suspended_at IS NOT NULL"#,
        guild_id
    )
    .execute(&state.db)
    .await?
    .rows_affected();

    if updated == 0 {
        return Err(AdminError::NotFound("Suspended guild".to_string()));
    }

    // Log the action
    perm_queries::write_audit_log(
        &state.db,
        admin.user_id,
        "system.guilds.unsuspend",
        Some("guild"),
        Some(guild_id),
        None,
        None,
    )
    .await?;

    Ok(Json(serde_json::json!({"suspended": false, "guild_id": guild_id})))
}

/// POST /api/admin/announcements
/// Create system announcement (requires elevation)
#[tracing::instrument(skip(state))]
pub async fn create_announcement(
    State(state): State<AppState>,
    Extension(admin): Extension<SystemAdminUser>,
    Extension(_elevated): Extension<ElevatedAdmin>,
    Json(body): Json<super::types::CreateAnnouncementRequest>,
) -> Result<Json<serde_json::Value>, AdminError> {
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

    sqlx::query!(
        r#"INSERT INTO system_announcements (id, author_id, title, content, severity, starts_at, ends_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
        announcement_id,
        admin.user_id,
        body.title,
        body.content,
        body.severity,
        starts_at,
        body.ends_at
    )
    .execute(&state.db)
    .await?;

    // Log the action
    perm_queries::write_audit_log(
        &state.db,
        admin.user_id,
        SystemPermission::SendAnnouncement.action_name(),
        Some("announcement"),
        Some(announcement_id),
        Some(serde_json::json!({"title": body.title, "severity": body.severity})),
        None,
    )
    .await?;

    Ok(Json(serde_json::json!({
        "id": announcement_id,
        "title": body.title,
        "created": true
    })))
}
```

### Step 2: Add import for ElevatedAdmin

At the top of `server/src/admin/handlers.rs`, update the import:

```rust
use super::types::{
    AdminError, ElevateRequest, ElevateResponse, ElevatedAdmin, SystemAdminUser,
};
```

### Step 3: Update router with elevated routes

Modify `server/src/admin/mod.rs`:

```rust
//! System Admin Module
//!
//! Platform-level administration for system admins.

pub mod handlers;
pub mod middleware;
pub mod types;

use axum::{
    middleware::from_fn_with_state,
    routing::{delete, get, post},
    Router,
};

use crate::api::AppState;

pub use middleware::{require_elevated, require_system_admin};
pub use types::{AdminError, ElevatedAdmin, SystemAdminUser};

/// Create the admin router.
///
/// All routes require system admin privileges (applied by parent).
/// Destructive routes additionally require elevation.
pub fn router(state: AppState) -> Router<AppState> {
    // Routes that require elevation
    let elevated_routes = Router::new()
        .route("/users/:id/ban", post(handlers::ban_user).delete(handlers::unban_user))
        .route("/guilds/:id/suspend", post(handlers::suspend_guild).delete(handlers::unsuspend_guild))
        .route("/announcements", post(handlers::create_announcement))
        .layer(from_fn_with_state(state, require_elevated));

    // Non-elevated routes
    Router::new()
        .route("/users", get(handlers::list_users))
        .route("/guilds", get(handlers::list_guilds))
        .route("/audit-log", get(handlers::get_audit_log))
        .route("/elevate", post(handlers::elevate_session).delete(handlers::de_elevate_session))
        .merge(elevated_routes)
}
```

### Step 4: Update api/mod.rs to integrate admin routes

Modify `server/src/api/mod.rs` - add admin import and routes:

Add to imports:

```rust
use crate::admin;
```

In `create_router` function, add admin routes before the final Router::new():

```rust
    // Admin routes with admin middleware
    let admin_routes = admin::router(state.clone())
        .layer(from_fn_with_state(state.clone(), admin::require_system_admin))
        .layer(from_fn_with_state(state.clone(), auth::require_auth));

    Router::new()
        // Health check
        .route("/health", get(health_check))
        // Auth routes (pass state for middleware)
        .nest("/auth", auth::router(state.clone()))
        // Admin routes
        .nest("/api/admin", admin_routes)
        // Protected chat and voice routes
        .merge(protected_routes)
        // ... rest unchanged
```

### Step 5: Build

```bash
cd /home/detair/GIT/canis/.worktrees/permission-system/server
DATABASE_URL="postgres://voicechat:voicechat_dev@localhost:5433/voicechat" cargo build --lib
```

Expected: Build succeeds.

### Step 6: Commit

```bash
git add server/src/admin/ server/src/api/mod.rs
git commit -m "feat(admin): add elevated admin handlers and router integration

- POST /api/admin/users/:id/ban - global ban user
- DELETE /api/admin/users/:id/ban - remove ban
- POST /api/admin/guilds/:id/suspend - suspend guild
- DELETE /api/admin/guilds/:id/suspend - unsuspend guild
- POST /api/admin/announcements - create announcement
- Integrated admin routes into main router

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 5: Guild Role Handlers

**Files:**
- Create: `server/src/guild/roles.rs`
- Modify: `server/src/guild/mod.rs`
- Modify: `server/src/guild/types.rs`

### Step 1: Add role types

Add to `server/src/guild/types.rs`:

```rust
/// Request to create a guild role.
#[derive(Debug, Deserialize, Validate)]
pub struct CreateRoleRequest {
    #[validate(length(min = 1, max = 64))]
    pub name: String,
    pub color: Option<String>,
    pub permissions: Option<u64>,
}

/// Request to update a guild role.
#[derive(Debug, Deserialize)]
pub struct UpdateRoleRequest {
    pub name: Option<String>,
    pub color: Option<String>,
    pub permissions: Option<u64>,
    pub position: Option<i32>,
}

/// Guild role response.
#[derive(Debug, Serialize)]
pub struct RoleResponse {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub name: String,
    pub color: Option<String>,
    pub permissions: u64,
    pub position: i32,
    pub is_default: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
```

Also add the import at the top if not present:

```rust
use uuid::Uuid;
```

### Step 2: Create role handlers

Create `server/src/guild/roles.rs`:

```rust
//! Guild role management handlers.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use uuid::Uuid;
use validator::Validate;

use crate::api::AppState;
use crate::auth::AuthUser;
use crate::permissions::{
    can_manage_role, require_guild_permission, GuildPermissions, PermissionError,
};

use super::types::{CreateRoleRequest, RoleResponse, UpdateRoleRequest};

// ============================================================================
// Error Type
// ============================================================================

#[derive(Debug)]
pub enum RoleError {
    NotFound,
    NotMember,
    Permission(PermissionError),
    Validation(String),
    Database(sqlx::Error),
}

impl IntoResponse for RoleError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                serde_json::json!({"error": "not_found", "message": "Role not found"}),
            ),
            Self::NotMember => (
                StatusCode::FORBIDDEN,
                serde_json::json!({"error": "not_member", "message": "Not a member of this guild"}),
            ),
            Self::Permission(e) => {
                let body = match &e {
                    PermissionError::MissingPermission(p) => serde_json::json!({
                        "error": "missing_permission",
                        "required": format!("{:?}", p),
                        "message": e.to_string()
                    }),
                    PermissionError::RoleHierarchy { actor_position, target_position } => serde_json::json!({
                        "error": "role_hierarchy",
                        "your_position": actor_position,
                        "target_position": target_position,
                        "message": e.to_string()
                    }),
                    PermissionError::CannotEscalate(p) => serde_json::json!({
                        "error": "cannot_escalate",
                        "attempted": format!("{:?}", p),
                        "message": e.to_string()
                    }),
                    _ => serde_json::json!({
                        "error": "permission",
                        "message": e.to_string()
                    }),
                };
                (StatusCode::FORBIDDEN, body)
            }
            Self::Validation(msg) => (
                StatusCode::BAD_REQUEST,
                serde_json::json!({"error": "validation", "message": msg}),
            ),
            Self::Database(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"error": "database", "message": "Database error"}),
            ),
        };
        (status, Json(body)).into_response()
    }
}

impl From<sqlx::Error> for RoleError {
    fn from(err: sqlx::Error) -> Self {
        Self::Database(err)
    }
}

impl From<PermissionError> for RoleError {
    fn from(err: PermissionError) -> Self {
        match err {
            PermissionError::NotGuildMember => Self::NotMember,
            other => Self::Permission(other),
        }
    }
}

// ============================================================================
// Handlers
// ============================================================================

/// GET /api/guilds/:guild_id/roles
/// List all roles in a guild
#[tracing::instrument(skip(state))]
pub async fn list_roles(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<Vec<RoleResponse>>, RoleError> {
    // Just need to be a member to view roles
    let _ctx = require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        GuildPermissions::empty(), // No specific permission required
    )
    .await?;

    let roles = sqlx::query!(
        r#"SELECT id, guild_id, name, color, permissions, position, is_default, created_at
           FROM guild_roles
           WHERE guild_id = $1
           ORDER BY position ASC"#,
        guild_id
    )
    .fetch_all(&state.db)
    .await?;

    let response: Vec<RoleResponse> = roles
        .into_iter()
        .map(|r| RoleResponse {
            id: r.id,
            guild_id: r.guild_id,
            name: r.name,
            color: r.color,
            permissions: r.permissions as u64,
            position: r.position,
            is_default: r.is_default,
            created_at: r.created_at,
        })
        .collect();

    Ok(Json(response))
}

/// POST /api/guilds/:guild_id/roles
/// Create a new role
#[tracing::instrument(skip(state))]
pub async fn create_role(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(body): Json<CreateRoleRequest>,
) -> Result<Json<RoleResponse>, RoleError> {
    body.validate()
        .map_err(|e| RoleError::Validation(e.to_string()))?;

    let ctx = require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        GuildPermissions::MANAGE_ROLES,
    )
    .await?;

    // Check if trying to grant permissions we don't have
    let new_perms = GuildPermissions::from_bits_truncate(body.permissions.unwrap_or(0));
    can_manage_role(
        ctx.computed_permissions,
        ctx.highest_role_position,
        i32::MAX, // New role, no position yet
        Some(new_perms),
    )?;

    // Get next position (higher number = lower rank)
    let max_position: i32 = sqlx::query_scalar!(
        "SELECT COALESCE(MAX(position), 0) FROM guild_roles WHERE guild_id = $1",
        guild_id
    )
    .fetch_one(&state.db)
    .await?
    .unwrap_or(0);

    let role_id = Uuid::now_v7();
    let position = max_position + 1;

    let role = sqlx::query!(
        r#"INSERT INTO guild_roles (id, guild_id, name, color, permissions, position)
           VALUES ($1, $2, $3, $4, $5, $6)
           RETURNING id, guild_id, name, color, permissions, position, is_default, created_at"#,
        role_id,
        guild_id,
        body.name,
        body.color,
        new_perms.bits() as i64,
        position
    )
    .fetch_one(&state.db)
    .await?;

    Ok(Json(RoleResponse {
        id: role.id,
        guild_id: role.guild_id,
        name: role.name,
        color: role.color,
        permissions: role.permissions as u64,
        position: role.position,
        is_default: role.is_default,
        created_at: role.created_at,
    }))
}

/// PATCH /api/guilds/:guild_id/roles/:role_id
/// Update a role
#[tracing::instrument(skip(state))]
pub async fn update_role(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((guild_id, role_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateRoleRequest>,
) -> Result<Json<RoleResponse>, RoleError> {
    let ctx = require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        GuildPermissions::MANAGE_ROLES,
    )
    .await?;

    // Get current role
    let current_role = sqlx::query!(
        "SELECT position, permissions, is_default FROM guild_roles WHERE id = $1 AND guild_id = $2",
        role_id,
        guild_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(RoleError::NotFound)?;

    // Cannot edit @everyone role name
    if current_role.is_default && body.name.is_some() {
        return Err(RoleError::Validation("Cannot rename @everyone role".to_string()));
    }

    // Check hierarchy - cannot edit roles at or above our position
    let new_perms = body.permissions.map(GuildPermissions::from_bits_truncate);
    can_manage_role(
        ctx.computed_permissions,
        ctx.highest_role_position,
        current_role.position,
        new_perms,
    )?;

    let role = sqlx::query!(
        r#"UPDATE guild_roles SET
               name = COALESCE($3, name),
               color = COALESCE($4, color),
               permissions = COALESCE($5, permissions),
               position = COALESCE($6, position)
           WHERE id = $1 AND guild_id = $2
           RETURNING id, guild_id, name, color, permissions, position, is_default, created_at"#,
        role_id,
        guild_id,
        body.name,
        body.color,
        body.permissions.map(|p| p as i64),
        body.position
    )
    .fetch_one(&state.db)
    .await?;

    Ok(Json(RoleResponse {
        id: role.id,
        guild_id: role.guild_id,
        name: role.name,
        color: role.color,
        permissions: role.permissions as u64,
        position: role.position,
        is_default: role.is_default,
        created_at: role.created_at,
    }))
}

/// DELETE /api/guilds/:guild_id/roles/:role_id
/// Delete a role
#[tracing::instrument(skip(state))]
pub async fn delete_role(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((guild_id, role_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, RoleError> {
    let ctx = require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        GuildPermissions::MANAGE_ROLES,
    )
    .await?;

    // Get role to check position and if it's default
    let role = sqlx::query!(
        "SELECT position, is_default FROM guild_roles WHERE id = $1 AND guild_id = $2",
        role_id,
        guild_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(RoleError::NotFound)?;

    if role.is_default {
        return Err(RoleError::Validation("Cannot delete @everyone role".to_string()));
    }

    // Check hierarchy
    can_manage_role(
        ctx.computed_permissions,
        ctx.highest_role_position,
        role.position,
        None,
    )?;

    sqlx::query!("DELETE FROM guild_roles WHERE id = $1", role_id)
        .execute(&state.db)
        .await?;

    Ok(Json(serde_json::json!({"deleted": true, "role_id": role_id})))
}

/// POST /api/guilds/:guild_id/members/:user_id/roles/:role_id
/// Assign a role to a member
#[tracing::instrument(skip(state))]
pub async fn assign_role(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((guild_id, user_id, role_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, RoleError> {
    let ctx = require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        GuildPermissions::MANAGE_ROLES,
    )
    .await?;

    // Get role to check position
    let role = sqlx::query!(
        "SELECT position, is_default FROM guild_roles WHERE id = $1 AND guild_id = $2",
        role_id,
        guild_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(RoleError::NotFound)?;

    if role.is_default {
        return Err(RoleError::Validation("Cannot assign @everyone role".to_string()));
    }

    // Check hierarchy
    can_manage_role(
        ctx.computed_permissions,
        ctx.highest_role_position,
        role.position,
        None,
    )?;

    // Check target is a member
    let is_member = sqlx::query_scalar!(
        "SELECT 1 FROM guild_members WHERE guild_id = $1 AND user_id = $2",
        guild_id,
        user_id
    )
    .fetch_optional(&state.db)
    .await?
    .is_some();

    if !is_member {
        return Err(RoleError::Validation("User is not a member of this guild".to_string()));
    }

    // Assign role (ignore if already assigned)
    sqlx::query!(
        r#"INSERT INTO guild_member_roles (guild_id, user_id, role_id, assigned_by)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT (guild_id, user_id, role_id) DO NOTHING"#,
        guild_id,
        user_id,
        role_id,
        auth.id
    )
    .execute(&state.db)
    .await?;

    Ok(Json(serde_json::json!({"assigned": true, "user_id": user_id, "role_id": role_id})))
}

/// DELETE /api/guilds/:guild_id/members/:user_id/roles/:role_id
/// Remove a role from a member
#[tracing::instrument(skip(state))]
pub async fn remove_role(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((guild_id, user_id, role_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, RoleError> {
    let ctx = require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        GuildPermissions::MANAGE_ROLES,
    )
    .await?;

    // Get role to check position
    let role = sqlx::query!("SELECT position FROM guild_roles WHERE id = $1 AND guild_id = $2", role_id, guild_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(RoleError::NotFound)?;

    // Check hierarchy
    can_manage_role(
        ctx.computed_permissions,
        ctx.highest_role_position,
        role.position,
        None,
    )?;

    let deleted = sqlx::query!(
        "DELETE FROM guild_member_roles WHERE guild_id = $1 AND user_id = $2 AND role_id = $3",
        guild_id,
        user_id,
        role_id
    )
    .execute(&state.db)
    .await?
    .rows_affected();

    if deleted == 0 {
        return Err(RoleError::NotFound);
    }

    Ok(Json(serde_json::json!({"removed": true, "user_id": user_id, "role_id": role_id})))
}
```

### Step 3: Update guild mod.rs

Modify `server/src/guild/mod.rs`:

```rust
//! Guild (Server) Management Module
//!
//! Handles guild creation, membership, invites, and management.

pub mod handlers;
pub mod invites;
pub mod roles;
pub mod types;

use axum::{
    routing::{delete, get, patch, post},
    Router,
};

use crate::api::AppState;

/// Create the guild router with all endpoints
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(handlers::list_guilds).post(handlers::create_guild))
        .route(
            "/:id",
            get(handlers::get_guild)
                .patch(handlers::update_guild)
                .delete(handlers::delete_guild),
        )
        .route("/:id/join", post(handlers::join_guild))
        .route("/:id/leave", post(handlers::leave_guild))
        .route("/:id/members", get(handlers::list_members))
        .route("/:id/members/:user_id", delete(handlers::kick_member))
        .route("/:id/channels", get(handlers::list_channels))
        // Role routes
        .route("/:id/roles", get(roles::list_roles).post(roles::create_role))
        .route("/:id/roles/:role_id", patch(roles::update_role).delete(roles::delete_role))
        .route("/:id/members/:user_id/roles/:role_id", post(roles::assign_role).delete(roles::remove_role))
        // Invite routes
        .route(
            "/:id/invites",
            get(invites::list_invites).post(invites::create_invite),
        )
        .route("/:id/invites/:code", delete(invites::delete_invite))
}

/// Create the invite join router (separate for public access pattern)
pub fn invite_router() -> Router<AppState> {
    Router::new().route("/:code/join", post(invites::join_via_invite))
}
```

### Step 4: Build

```bash
cd /home/detair/GIT/canis/.worktrees/permission-system/server
DATABASE_URL="postgres://voicechat:voicechat_dev@localhost:5433/voicechat" cargo build --lib
```

Expected: Build succeeds.

### Step 5: Commit

```bash
git add server/src/guild/
git commit -m "feat(guild): add role management handlers

- GET /api/guilds/:id/roles - list roles
- POST /api/guilds/:id/roles - create role
- PATCH /api/guilds/:id/roles/:id - update role
- DELETE /api/guilds/:id/roles/:id - delete role
- POST /api/guilds/:id/members/:id/roles/:id - assign role
- DELETE /api/guilds/:id/members/:id/roles/:id - remove role
- Role hierarchy and escalation prevention

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 6: Channel Override Handlers

**Files:**
- Create: `server/src/chat/overrides.rs`
- Modify: `server/src/chat/mod.rs`

### Step 1: Create override handlers

Create `server/src/chat/overrides.rs`:

```rust
//! Channel permission override handlers.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::AuthUser;
use crate::permissions::{
    require_guild_permission, GuildPermissions, PermissionError,
};

// ============================================================================
// Types
// ============================================================================

#[derive(Debug)]
pub enum OverrideError {
    ChannelNotFound,
    RoleNotFound,
    NotMember,
    Permission(PermissionError),
    Database(sqlx::Error),
}

impl IntoResponse for OverrideError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Self::ChannelNotFound => (
                StatusCode::NOT_FOUND,
                serde_json::json!({"error": "not_found", "message": "Channel not found"}),
            ),
            Self::RoleNotFound => (
                StatusCode::NOT_FOUND,
                serde_json::json!({"error": "not_found", "message": "Role not found"}),
            ),
            Self::NotMember => (
                StatusCode::FORBIDDEN,
                serde_json::json!({"error": "not_member", "message": "Not a member of this guild"}),
            ),
            Self::Permission(e) => (
                StatusCode::FORBIDDEN,
                serde_json::json!({"error": "permission", "message": e.to_string()}),
            ),
            Self::Database(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"error": "database", "message": "Database error"}),
            ),
        };
        (status, Json(body)).into_response()
    }
}

impl From<sqlx::Error> for OverrideError {
    fn from(err: sqlx::Error) -> Self {
        Self::Database(err)
    }
}

impl From<PermissionError> for OverrideError {
    fn from(err: PermissionError) -> Self {
        match err {
            PermissionError::NotGuildMember => Self::NotMember,
            other => Self::Permission(other),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct OverrideResponse {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub role_id: Uuid,
    pub allow: u64,
    pub deny: u64,
}

#[derive(Debug, Deserialize)]
pub struct SetOverrideRequest {
    pub allow: Option<u64>,
    pub deny: Option<u64>,
}

// ============================================================================
// Handlers
// ============================================================================

/// GET /api/channels/:channel_id/overrides
/// List all permission overrides for a channel
#[tracing::instrument(skip(state))]
pub async fn list_overrides(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<Json<Vec<OverrideResponse>>, OverrideError> {
    // Get channel and its guild
    let channel = sqlx::query!("SELECT guild_id FROM channels WHERE id = $1", channel_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(OverrideError::ChannelNotFound)?;

    let guild_id = channel.guild_id.ok_or(OverrideError::ChannelNotFound)?;

    // Check membership
    let _ctx = require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        GuildPermissions::empty(),
    )
    .await?;

    let overrides = sqlx::query!(
        r#"SELECT id, channel_id, role_id, allow_permissions, deny_permissions
           FROM channel_overrides
           WHERE channel_id = $1"#,
        channel_id
    )
    .fetch_all(&state.db)
    .await?;

    let response: Vec<OverrideResponse> = overrides
        .into_iter()
        .map(|o| OverrideResponse {
            id: o.id,
            channel_id: o.channel_id,
            role_id: o.role_id,
            allow: o.allow_permissions as u64,
            deny: o.deny_permissions as u64,
        })
        .collect();

    Ok(Json(response))
}

/// PUT /api/channels/:channel_id/overrides/:role_id
/// Set permission override for a role on a channel
#[tracing::instrument(skip(state))]
pub async fn set_override(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((channel_id, role_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<SetOverrideRequest>,
) -> Result<Json<OverrideResponse>, OverrideError> {
    // Get channel and its guild
    let channel = sqlx::query!("SELECT guild_id FROM channels WHERE id = $1", channel_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(OverrideError::ChannelNotFound)?;

    let guild_id = channel.guild_id.ok_or(OverrideError::ChannelNotFound)?;

    // Check permission
    let _ctx = require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        GuildPermissions::MANAGE_CHANNELS,
    )
    .await?;

    // Verify role belongs to this guild
    let role_exists = sqlx::query_scalar!(
        "SELECT 1 FROM guild_roles WHERE id = $1 AND guild_id = $2",
        role_id,
        guild_id
    )
    .fetch_optional(&state.db)
    .await?
    .is_some();

    if !role_exists {
        return Err(OverrideError::RoleNotFound);
    }

    let allow = body.allow.unwrap_or(0) as i64;
    let deny = body.deny.unwrap_or(0) as i64;

    let override_entry = sqlx::query!(
        r#"INSERT INTO channel_overrides (channel_id, role_id, allow_permissions, deny_permissions)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT (channel_id, role_id) DO UPDATE SET
               allow_permissions = $3,
               deny_permissions = $4
           RETURNING id, channel_id, role_id, allow_permissions, deny_permissions"#,
        channel_id,
        role_id,
        allow,
        deny
    )
    .fetch_one(&state.db)
    .await?;

    Ok(Json(OverrideResponse {
        id: override_entry.id,
        channel_id: override_entry.channel_id,
        role_id: override_entry.role_id,
        allow: override_entry.allow_permissions as u64,
        deny: override_entry.deny_permissions as u64,
    }))
}

/// DELETE /api/channels/:channel_id/overrides/:role_id
/// Remove permission override
#[tracing::instrument(skip(state))]
pub async fn delete_override(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((channel_id, role_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, OverrideError> {
    // Get channel and its guild
    let channel = sqlx::query!("SELECT guild_id FROM channels WHERE id = $1", channel_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(OverrideError::ChannelNotFound)?;

    let guild_id = channel.guild_id.ok_or(OverrideError::ChannelNotFound)?;

    // Check permission
    let _ctx = require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        GuildPermissions::MANAGE_CHANNELS,
    )
    .await?;

    let deleted = sqlx::query!(
        "DELETE FROM channel_overrides WHERE channel_id = $1 AND role_id = $2",
        channel_id,
        role_id
    )
    .execute(&state.db)
    .await?
    .rows_affected();

    if deleted == 0 {
        return Err(OverrideError::RoleNotFound);
    }

    Ok(Json(serde_json::json!({"deleted": true, "channel_id": channel_id, "role_id": role_id})))
}
```

### Step 2: Update chat mod.rs

Modify `server/src/chat/mod.rs`:

```rust
//! Chat Service
//!
//! Handles channels, messages, and file uploads.

mod channels;
mod dm;
mod messages;
pub mod overrides;
pub mod s3;
mod uploads;

use axum::{
    routing::{delete, get, patch, post, put},
    Router,
};

use crate::api::AppState;

pub use s3::S3Client;

/// Create channels router.
pub fn channels_router() -> Router<AppState> {
    Router::new()
        .route("/", get(channels::list))
        .route("/", post(channels::create))
        .route("/:id", get(channels::get))
        .route("/:id", patch(channels::update))
        .route("/:id", delete(channels::delete))
        .route("/:id/members", get(channels::list_members))
        .route("/:id/members", post(channels::add_member))
        .route("/:id/members/:user_id", delete(channels::remove_member))
        // Permission overrides
        .route("/:id/overrides", get(overrides::list_overrides))
        .route("/:id/overrides/:role_id", put(overrides::set_override).delete(overrides::delete_override))
}

/// Create messages router (protected routes).
pub fn messages_router() -> Router<AppState> {
    Router::new()
        .route(
            "/channel/:channel_id",
            get(messages::list).post(messages::create),
        )
        .route(
            "/channel/:channel_id/upload",
            post(uploads::upload_message_with_file),
        )
        .route("/:id", patch(messages::update).delete(messages::delete))
        .route("/upload", post(uploads::upload_file))
        .route("/attachments/:id", get(uploads::get_attachment))
}

/// Create public messages router (routes that handle their own auth).
/// The download route accepts auth via query parameter for browser requests.
pub fn messages_public_router() -> Router<AppState> {
    Router::new().route("/attachments/:id/download", get(uploads::download))
}

/// Create DM (Direct Message) router.
pub fn dm_router() -> Router<AppState> {
    Router::new()
        .route("/", get(dm::list_dms).post(dm::create_dm))
        .route("/:id", get(dm::get_dm))
        .route("/:id/leave", post(dm::leave_dm))
        .route("/:id/read", post(dm::mark_as_read))
}
```

### Step 3: Build

```bash
cd /home/detair/GIT/canis/.worktrees/permission-system/server
DATABASE_URL="postgres://voicechat:voicechat_dev@localhost:5433/voicechat" cargo build --lib
```

Expected: Build succeeds.

### Step 4: Commit

```bash
git add server/src/chat/
git commit -m "feat(chat): add channel permission override handlers

- GET /api/channels/:id/overrides - list overrides
- PUT /api/channels/:id/overrides/:role_id - set override
- DELETE /api/channels/:id/overrides/:role_id - remove override
- Requires MANAGE_CHANNELS permission

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 7: Final Integration and Testing

**Files:**
- Verify all builds pass
- Update sqlx prepare
- Run all tests

### Step 1: Build entire project

```bash
cd /home/detair/GIT/canis/.worktrees/permission-system/server
DATABASE_URL="postgres://voicechat:voicechat_dev@localhost:5433/voicechat" cargo build
```

Expected: Full build succeeds.

### Step 2: Regenerate sqlx metadata

```bash
cd /home/detair/GIT/canis/.worktrees/permission-system/server
rm -rf .sqlx
DATABASE_URL="postgres://voicechat:voicechat_dev@localhost:5433/voicechat" cargo sqlx prepare
```

Expected: Query data written to .sqlx

### Step 3: Run all tests

```bash
cd /home/detair/GIT/canis/.worktrees/permission-system/server
DATABASE_URL="postgres://voicechat:voicechat_dev@localhost:5433/voicechat" cargo test --lib
```

Expected: All tests pass.

### Step 4: Verify sqlx offline mode works

```bash
cd /home/detair/GIT/canis/.worktrees/permission-system/server
DATABASE_URL="postgres://voicechat:voicechat_dev@localhost:5433/voicechat" cargo sqlx prepare --check
```

Expected: Check passes.

### Step 5: Commit sqlx metadata

```bash
git add server/.sqlx/
git commit -m "chore(db): update sqlx prepared queries

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

### Step 6: Push and update PR

```bash
git push
```

---

## Summary

| Task | Description | Files |
|------|-------------|-------|
| 1 | Permission helpers | permissions/helpers.rs, resolver.rs, mod.rs |
| 2 | Admin module structure | admin/mod.rs, middleware.rs, types.rs |
| 3 | Admin handlers (non-elevated) | admin/handlers.rs |
| 4 | Admin handlers (elevated) | admin/handlers.rs, api/mod.rs |
| 5 | Guild role handlers | guild/roles.rs, types.rs, mod.rs |
| 6 | Channel override handlers | chat/overrides.rs, mod.rs |
| 7 | Integration and testing | .sqlx/ |

**Total estimated: ~1000 lines of new code, 16 API endpoints**
