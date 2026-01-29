//! Server Settings API
//!
//! Public endpoint for retrieving server configuration that clients need.

use axum::{extract::State, Json};
use serde::Serialize;

use crate::api::AppState;

/// Public server settings response.
#[derive(Debug, Serialize)]
pub struct ServerSettingsResponse {
    /// Whether E2EE setup is required before using the app.
    pub require_e2ee_setup: bool,
    /// Whether OIDC login is available.
    pub oidc_enabled: bool,
}

/// Get server settings (public endpoint).
///
/// GET /api/settings
pub async fn get_server_settings(State(state): State<AppState>) -> Json<ServerSettingsResponse> {
    Json(ServerSettingsResponse {
        require_e2ee_setup: state.config.require_e2ee_setup,
        oidc_enabled: state.config.has_oidc(),
    })
}

/// File upload size limits response.
#[derive(Debug, Serialize)]
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
pub async fn get_upload_limits(State(state): State<AppState>) -> Json<UploadLimitsResponse> {
    Json(UploadLimitsResponse {
        max_avatar_size: state.config.max_avatar_size,
        max_emoji_size: state.config.max_emoji_size,
        max_upload_size: state.config.max_upload_size,
    })
}
