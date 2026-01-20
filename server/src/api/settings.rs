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
