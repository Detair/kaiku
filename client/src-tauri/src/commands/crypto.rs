//! E2EE Key Management Commands

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use tauri::{command, State};
use tracing::{error, info};
use vc_crypto::{EncryptedBackup, RecoveryKey};

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

/// Request to upload encrypted backup to server.
#[derive(Debug, Serialize)]
struct UploadBackupRequest {
    salt: String,
    nonce: String,
    ciphertext: String,
    version: i32,
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

/// Generate a new recovery key and return it for display.
///
/// The key is NOT stored - the UI must prompt user to save it,
/// then call create_backup to actually store the encrypted backup.
#[command]
pub async fn generate_recovery_key() -> Result<RecoveryKeyDisplay, String> {
    let key = RecoveryKey::generate();
    let formatted = key.to_formatted_string();

    // Get full key without spaces for copy/download
    let full_key: String = formatted.chars().filter(|c| !c.is_whitespace()).collect();

    // Split into 4-char chunks for display
    let chunks: Vec<String> = full_key
        .chars()
        .collect::<Vec<_>>()
        .chunks(4)
        .map(|c| c.iter().collect::<String>())
        .collect();

    info!("Generated new recovery key");

    Ok(RecoveryKeyDisplay { full_key, chunks })
}

/// Create and upload an encrypted backup of the user's keys.
///
/// Takes the recovery key (Base58, with or without spaces) and the data to backup (JSON string).
/// Encrypts locally using AES-256-GCM, then uploads to server.
#[command]
pub async fn create_backup(
    state: State<'_, AppState>,
    recovery_key: String,
    backup_data: String,
) -> Result<(), String> {
    info!("Creating encrypted backup");

    // Parse recovery key (handles both formatted and raw Base58)
    let key = RecoveryKey::from_formatted_string(&recovery_key)
        .map_err(|e| format!("Invalid recovery key: {e}"))?;

    // Encrypt the backup data locally
    let encrypted = EncryptedBackup::create(&key, backup_data.as_bytes());

    // Prepare request with base64-encoded binary fields
    let request = UploadBackupRequest {
        salt: STANDARD.encode(encrypted.salt),
        nonce: STANDARD.encode(encrypted.nonce),
        ciphertext: STANDARD.encode(&encrypted.ciphertext),
        #[allow(clippy::cast_possible_wrap)]
        version: encrypted.version as i32,
    };

    // Upload to server
    let auth = state.auth.read().await;
    let server_url = auth.server_url.as_ref().ok_or("Not connected")?;
    let token = auth.access_token.as_ref().ok_or("Not authenticated")?;

    let response = state
        .http
        .post(format!("{server_url}/api/keys/backup"))
        .bearer_auth(token)
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("Upload failed: {e}"))?;

    if !response.status().is_success() {
        let body = response.text().await.unwrap_or_default();
        error!("Backup upload failed: {}", body);
        return Err(format!("Server error: {body}"));
    }

    info!("Backup uploaded successfully");
    Ok(())
}
