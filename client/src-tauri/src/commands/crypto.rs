//! E2EE Key Management Commands

use serde::{Deserialize, Serialize};
use tauri::{command, State};
use tracing::info;

use crate::AppState;

/// Recovery key formatted for display (4-char chunks).
#[derive(Debug, Serialize)]
pub struct RecoveryKeyDisplay {
    /// Full key in Base58 (for copy/download).
    pub full_key: String,
    /// Key split into 4-char chunks for display.
    pub chunks: Vec<String>,
}

/// Backup status from server.
#[derive(Debug, Deserialize, Serialize)]
pub struct BackupStatus {
    pub has_backup: bool,
    pub backup_created_at: Option<String>,
    pub version: Option<i32>,
}

/// Server settings.
#[derive(Debug, Deserialize, Serialize)]
pub struct ServerSettings {
    pub require_e2ee_setup: bool,
    pub oidc_enabled: bool,
}

/// Get server settings.
#[command]
pub async fn get_server_settings(state: State<'_, AppState>) -> Result<ServerSettings, String> {
    info!("Fetching server settings");

    let auth = state.auth.read().await;
    let server_url = auth.server_url.as_ref().ok_or("Not connected")?;

    let response = state
        .http
        .get(format!("{server_url}/api/settings"))
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("Server error: {}", response.status()));
    }

    response
        .json::<ServerSettings>()
        .await
        .map_err(|e| format!("Parse error: {e}"))
}

/// Get backup status for current user.
#[command]
pub async fn get_backup_status(state: State<'_, AppState>) -> Result<BackupStatus, String> {
    info!("Fetching backup status");

    let auth = state.auth.read().await;
    let server_url = auth.server_url.as_ref().ok_or("Not connected")?;
    let token = auth.access_token.as_ref().ok_or("Not authenticated")?;

    let response = state
        .http
        .get(format!("{server_url}/api/keys/backup/status"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("Server error: {}", response.status()));
    }

    response
        .json::<BackupStatus>()
        .await
        .map_err(|e| format!("Parse error: {e}"))
}
