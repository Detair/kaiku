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

/// Response from server when downloading backup.
#[derive(Debug, Deserialize)]
struct BackupResponse {
    salt: String,
    nonce: String,
    ciphertext: String,
    version: i32,
    #[allow(dead_code)]
    created_at: String,
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

/// Download and decrypt a backup using the recovery key.
///
/// Returns the decrypted backup data as a JSON string.
#[command]
pub async fn restore_backup(
    state: State<'_, AppState>,
    recovery_key: String,
) -> Result<String, String> {
    info!("Restoring backup from server");

    // Parse recovery key (handles both formatted and raw Base58)
    let key = RecoveryKey::from_formatted_string(&recovery_key)
        .map_err(|e| format!("Invalid recovery key: {e}"))?;

    // Download from server
    let auth = state.auth.read().await;
    let server_url = auth.server_url.as_ref().ok_or("Not connected")?;
    let token = auth.access_token.as_ref().ok_or("Not authenticated")?;

    let response = state
        .http
        .get(format!("{server_url}/api/keys/backup"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("Download failed: {e}"))?;

    if response.status().as_u16() == 404 {
        return Err("No backup found".to_string());
    }

    if !response.status().is_success() {
        return Err(format!("Server error: {}", response.status()));
    }

    let backup_resp: BackupResponse = response
        .json()
        .await
        .map_err(|e| format!("Parse error: {e}"))?;

    // Decode base64
    let salt = STANDARD
        .decode(&backup_resp.salt)
        .map_err(|_| "Invalid salt encoding")?;
    let nonce = STANDARD
        .decode(&backup_resp.nonce)
        .map_err(|_| "Invalid nonce encoding")?;
    let ciphertext = STANDARD
        .decode(&backup_resp.ciphertext)
        .map_err(|_| "Invalid ciphertext encoding")?;

    // Reconstruct encrypted backup
    let encrypted = EncryptedBackup {
        salt: salt.try_into().map_err(|_| "Invalid salt length")?,
        nonce: nonce.try_into().map_err(|_| "Invalid nonce length")?,
        ciphertext,
        #[allow(clippy::cast_sign_loss)]
        version: backup_resp.version as u32,
    };

    // Decrypt
    let decrypted = encrypted
        .decrypt(&key)
        .map_err(|e| format!("Decryption failed: {e}"))?;

    let data =
        String::from_utf8(decrypted).map_err(|_| "Backup data is not valid UTF-8")?;

    info!("Backup restored successfully");
    Ok(data)
}

#[cfg(test)]
mod tests {
    use vc_crypto::RecoveryKey;

    #[test]
    fn test_recovery_key_chunks() {
        // Generate a key and verify chunking (same logic as generate_recovery_key command)
        let key = RecoveryKey::generate();
        let formatted = key.to_formatted_string();

        // Get full key without spaces (same as command does)
        let full_key: String = formatted.chars().filter(|c| !c.is_whitespace()).collect();

        let chunks: Vec<String> = full_key
            .chars()
            .collect::<Vec<_>>()
            .chunks(4)
            .map(|c| c.iter().collect::<String>())
            .collect();

        // Each chunk should be 4 chars (except possibly the last)
        for (i, chunk) in chunks.iter().enumerate() {
            if i < chunks.len() - 1 {
                assert_eq!(chunk.len(), 4, "Chunk {} should be 4 chars", i);
            } else {
                assert!(chunk.len() <= 4, "Last chunk should be <= 4 chars");
            }
        }

        // Joining chunks should equal original
        let rejoined: String = chunks.join("");
        assert_eq!(rejoined, full_key);
    }

    #[test]
    fn test_recovery_key_roundtrip() {
        // Test that a key can be serialized and parsed back
        let key = RecoveryKey::generate();
        let formatted = key.to_formatted_string();

        let parsed = RecoveryKey::from_formatted_string(&formatted)
            .expect("Should parse formatted key");

        // The keys should be equivalent (same formatted output)
        assert_eq!(key.to_formatted_string(), parsed.to_formatted_string());
    }

    #[test]
    fn test_recovery_key_display_format() {
        // Verify the display format used by the UI
        let key = RecoveryKey::generate();
        let formatted = key.to_formatted_string();

        // Should contain spaces separating groups
        assert!(formatted.contains(' '), "Formatted key should contain spaces");

        // Each group should be 4 chars (except possibly the last)
        let groups: Vec<&str> = formatted.split_whitespace().collect();
        assert!(groups.len() >= 10, "Should have at least 10 groups");

        for (i, group) in groups.iter().enumerate() {
            if i < groups.len() - 1 {
                assert_eq!(group.len(), 4, "Group {} should be 4 chars", i);
            } else {
                assert!(
                    !group.is_empty() && group.len() <= 4,
                    "Last group should be 1-4 chars"
                );
            }
        }
    }

    #[test]
    fn test_recovery_key_uniqueness() {
        // Verify that generated keys are unique
        let key1 = RecoveryKey::generate();
        let key2 = RecoveryKey::generate();

        assert_ne!(
            key1.to_formatted_string(),
            key2.to_formatted_string(),
            "Generated keys should be unique"
        );
    }
}
