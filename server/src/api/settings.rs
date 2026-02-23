//! Server Settings API
//!
//! Public endpoint for retrieving server configuration that clients need.

use axum::extract::State;
use axum::Json;
use serde::Serialize;

use crate::api::AppState;
use crate::db::{get_auth_methods_allowed, AuthMethodsConfig, PublicOidcProvider};

/// Public server settings response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ServerSettingsResponse {
    /// Whether E2EE setup is required before using the app.
    pub require_e2ee_setup: bool,
    /// Whether OIDC login is available.
    pub oidc_enabled: bool,
    /// List of available OIDC providers (public info only).
    pub oidc_providers: Vec<PublicOidcProvider>,
    /// Which auth methods are enabled.
    pub auth_methods: AuthMethodsConfig,
    /// Registration policy: "open", "`invite_only`", or "closed".
    pub registration_policy: String,
}

/// Get server settings (public endpoint).
///
/// GET /api/settings
#[utoipa::path(
    get,
    path = "/api/settings",
    tag = "settings",
    responses(
        (status = 200, description = "Server settings"),
    ),
)]
pub async fn get_server_settings(State(state): State<AppState>) -> Json<ServerSettingsResponse> {
    let auth_methods = get_auth_methods_allowed(&state.db)
        .await
        .unwrap_or_default();

    let oidc_providers = if auth_methods.oidc {
        if let Some(ref manager) = state.oidc_manager {
            manager.list_public().await
        } else {
            vec![]
        }
    } else {
        vec![]
    };

    let registration_policy = crate::db::get_config_value(&state.db, "registration_policy")
        .await
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| "open".to_string());

    Json(ServerSettingsResponse {
        require_e2ee_setup: state.config.require_e2ee_setup,
        oidc_enabled: auth_methods.oidc && !oidc_providers.is_empty(),
        oidc_providers,
        auth_methods,
        registration_policy,
    })
}

/// File upload size limits response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UploadLimitsResponse {
    /// Maximum avatar size in bytes (user profiles and DM groups).
    pub max_avatar_size: usize,
    /// Maximum emoji size in bytes (guild custom emojis).
    pub max_emoji_size: usize,
    /// Maximum attachment size in bytes (message attachments).
    pub max_upload_size: usize,
}

/// Get file upload size limits (public endpoint).
///
/// Returns the configured maximum file sizes for different upload types.
/// Clients should validate file sizes against these limits before attempting upload.
///
/// GET /api/config/upload-limits
#[utoipa::path(
    get,
    path = "/api/config/upload-limits",
    tag = "settings",
    responses(
        (status = 200, description = "Upload limits"),
    ),
)]
pub async fn get_upload_limits(State(state): State<AppState>) -> Json<UploadLimitsResponse> {
    Json(UploadLimitsResponse {
        max_avatar_size: state.config.max_avatar_size,
        max_emoji_size: state.config.max_emoji_size,
        max_upload_size: state.config.max_upload_size,
    })
}
