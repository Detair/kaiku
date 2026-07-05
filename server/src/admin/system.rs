//! System administration handlers (status, stats, elevation, announcements,
//! auth settings, OIDC providers).

#![allow(clippy::used_underscore_binding)]

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Path, State};
use axum::{Extension, Json};
use chrono::{DateTime, Utc};
use fred::interfaces::ClientLike;
use serde::{Deserialize, Serialize};
use tracing::warn;
use uuid::Uuid;

use super::error::AdminError;
use super::queries;
use super::types::{
    AdminStatsResponse, AdminStatusResponse, CreateAnnouncementRequest, ElevateRequest,
    ElevateResponse, ElevatedAdmin, SystemAdminUser,
};
use crate::api::AppState;
use crate::permissions::queries::{create_elevated_session, write_audit_log};

/// De-elevate response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DeElevateResponse {
    pub elevated: bool,
}

/// Announcement response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AnnouncementResponse {
    pub id: Uuid,
    pub title: String,
    pub created: bool,
}

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
) -> Result<Json<AdminStatusResponse>, AdminError> {
    use crate::permissions::queries::get_system_admin;

    // Check if user is a system admin
    let is_admin = get_system_admin(&state.db, auth.id).await?.is_some();

    // Check for active elevated session
    let elevated = if is_admin {
        queries::find_latest_elevated_session_expiry(&state.db, auth.id).await?
    } else {
        None
    };

    Ok(Json(AdminStatusResponse {
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
) -> Result<Json<AdminStatsResponse>, AdminError> {
    let user_count = queries::count_users(&state.db).await?;
    let guild_count = queries::count_guilds(&state.db).await?;
    let banned_count = queries::count_active_bans(&state.db).await?;

    Ok(Json(AdminStatsResponse {
        user_count,
        guild_count,
        banned_count,
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
// Auth Settings & OIDC Provider Management (Elevated)
// ============================================================================

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
        crate::db::set_auth_methods_allowed(&state.db, methods, Some(admin.user_id)).await?;
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
            Some(admin.user_id),
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
            created_by: Some(admin.user_id),
        },
    )
    .await?;

    // Reload providers in the manager
    if let Err(e) = oidc_manager.load_providers(&state.db).await {
        warn!(error = %e, "Failed to reload OIDC providers after creation");
    }

    Ok(Json(row.into()))
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
// Diagnostics (operator supportability pack — Phase 8)
// ============================================================================

/// Filesystem usage for the server's root mount, if measurable.
#[derive(Debug, Serialize)]
pub struct DiskDiagnostics {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub used_percent: f64,
}

/// Database connectivity and pool pressure.
#[derive(Debug, Serialize)]
pub struct DatabaseDiagnostics {
    pub ok: bool,
    pub pool_size: u32,
    pub pool_idle: usize,
}

/// One-call infrastructure snapshot for operator triage.
#[derive(Debug, Serialize)]
pub struct DiagnosticsResponse {
    /// "ok" when both stores respond, "degraded" otherwise.
    pub status: &'static str,
    pub version: &'static str,
    pub uptime_seconds: u64,
    pub database: DatabaseDiagnostics,
    pub valkey_ok: bool,
    /// `None` when the host has no usable `df` (e.g. minimal containers).
    pub disk: Option<DiskDiagnostics>,
    /// Active voice sessions in the last 5 minutes (telemetry-derived).
    pub voice_active_sessions: Option<i64>,
    /// Active WebSocket connections in the last 5 minutes (telemetry-derived).
    pub ws_active_connections: Option<i64>,
    /// Server error events in the last 5 minutes (telemetry-derived).
    pub errors_last_5m: Option<i64>,
}

/// Best-effort disk usage via `df` (no unsafe statvfs — the workspace
/// forbids unsafe code). Returns `None` if `df` is unavailable or output
/// is unparseable; diagnostics must degrade, not fail.
async fn disk_usage() -> Option<DiskDiagnostics> {
    let output = tokio::process::Command::new("df")
        .args(["-B1", "--output=size,avail", "/"])
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let line = text.lines().nth(1)?;
    let mut parts = line.split_whitespace();
    let total_bytes: u64 = parts.next()?.parse().ok()?;
    let available_bytes: u64 = parts.next()?.parse().ok()?;
    if total_bytes == 0 {
        return None;
    }
    #[allow(clippy::cast_precision_loss)]
    let used_percent =
        ((total_bytes - available_bytes) as f64 / total_bytes as f64 * 1000.0).round() / 10.0;
    Some(DiskDiagnostics {
        total_bytes,
        available_bytes,
        used_percent,
    })
}

/// `GET /api/admin/diagnostics`
///
/// Operator triage snapshot: store connectivity, DB pool pressure, disk,
/// and recent activity/error counts in a single call. Complements
/// `/api/admin/observability/summary` (metrics-flavored) with the
/// infrastructure-flavored view the ops runbooks reference.
///
/// Telemetry-derived fields are `null` when observability is disabled —
/// connectivity checks always run.
#[tracing::instrument(skip(state, _admin))]
pub async fn get_diagnostics(
    Extension(_admin): Extension<SystemAdminUser>,
    State(state): State<AppState>,
) -> Json<DiagnosticsResponse> {
    let now = Utc::now();
    let five_min_ago = now - chrono::Duration::minutes(5);

    let db_ok = sqlx::query("SELECT 1").fetch_one(&state.db).await.is_ok();
    let valkey_ok = state.redis.ping::<String>(None).await.is_ok();
    let disk = disk_usage().await;

    // Telemetry-derived counts are best-effort: a failure (e.g. telemetry
    // tables absent) must not break the triage endpoint.
    let voice_active_sessions = queries::summary_active_voice_sessions(&state.db, five_min_ago)
        .await
        .ok()
        .flatten();
    let ws_active_connections = queries::summary_active_ws_connections(&state.db, five_min_ago)
        .await
        .ok()
        .flatten();
    let errors_last_5m = queries::summary_recent_error_count(&state.db, five_min_ago)
        .await
        .ok();

    Json(DiagnosticsResponse {
        status: if db_ok && valkey_ok { "ok" } else { "degraded" },
        version: env!("CARGO_PKG_VERSION"),
        uptime_seconds: super::observability::server_uptime_seconds(),
        database: DatabaseDiagnostics {
            ok: db_ok,
            pool_size: state.db.size(),
            pool_idle: state.db.num_idle(),
        },
        valkey_ok,
        disk,
        voice_active_sessions,
        ws_active_connections,
        errors_last_5m,
    })
}
