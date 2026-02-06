//! E2EE Key Management Commands

use std::collections::HashMap;
use std::path::PathBuf;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{command, Manager, State};
use tracing::{error, info};
use uuid::Uuid;
use vc_crypto::olm::EncryptedMessage;
use vc_crypto::{EncryptedBackup, RecoveryKey};

use crate::crypto::{ClaimedPrekey, CryptoManager, PrekeyForUpload, PrekeyInfo};
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

// =============================================================================
// E2EE Commands
// =============================================================================

/// E2EE initialization status.
#[derive(Debug, Serialize)]
pub struct E2EEStatus {
    /// Whether E2EE is initialized.
    pub initialized: bool,
    /// Device ID if initialized.
    pub device_id: Option<String>,
    /// Whether identity keys are available.
    pub has_identity_keys: bool,
}

/// Response from E2EE initialization.
#[derive(Debug, Serialize)]
pub struct InitE2EEResponse {
    /// This device's ID.
    pub device_id: String,
    /// Ed25519 identity key (base64).
    pub identity_key_ed25519: String,
    /// Curve25519 identity key (base64).
    pub identity_key_curve25519: String,
    /// One-time prekeys for upload to server.
    pub prekeys: Vec<PrekeyData>,
}

/// Prekey data for upload to server.
#[derive(Debug, Serialize)]
pub struct PrekeyData {
    /// Key ID (base64).
    pub key_id: String,
    /// Public key (base64).
    pub public_key: String,
}

impl From<PrekeyForUpload> for PrekeyData {
    fn from(p: PrekeyForUpload) -> Self {
        Self {
            key_id: p.key_id,
            public_key: p.public_key,
        }
    }
}

/// Input for a claimed prekey from the server.
#[derive(Debug, Deserialize)]
pub struct ClaimedPrekeyInput {
    /// Recipient's user ID.
    pub user_id: String,
    /// Recipient's device ID.
    pub device_id: String,
    /// Ed25519 identity key (base64).
    pub identity_key_ed25519: String,
    /// Curve25519 identity key (base64).
    pub identity_key_curve25519: String,
    /// One-time prekey (if available).
    pub one_time_prekey: Option<PrekeyInput>,
}

/// One-time prekey input.
#[derive(Debug, Deserialize)]
pub struct PrekeyInput {
    /// Key ID (base64).
    pub key_id: String,
    /// Public key (base64).
    pub public_key: String,
}

/// Encrypted message output for the frontend.
#[derive(Debug, Serialize)]
pub struct EncryptedMessageOutput {
    /// Message type: 0 = prekey, 1 = normal.
    pub message_type: u8,
    /// Base64-encoded ciphertext.
    pub ciphertext: String,
}

impl From<EncryptedMessage> for EncryptedMessageOutput {
    fn from(m: EncryptedMessage) -> Self {
        Self {
            message_type: m.message_type,
            ciphertext: m.ciphertext,
        }
    }
}

/// E2EE content output for the frontend.
#[derive(Debug, Serialize)]
pub struct E2EEContentOutput {
    /// Sender's Curve25519 public key (base64).
    pub sender_key: String,
    /// Encrypted content for each recipient: `user_id` -> `device_id` -> ciphertext.
    pub recipients: HashMap<String, HashMap<String, EncryptedMessageOutput>>,
}

/// Derive a 32-byte encryption key from a string using SHA-256.
fn derive_encryption_key(input: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&result);
    key
}

/// Get the E2EE data directory for the current user.
fn get_e2ee_data_dir(app_handle: &tauri::AppHandle, user_id: &str) -> Result<PathBuf, String> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {e}"))?;

    let e2ee_dir = app_data_dir.join("e2ee").join(user_id);

    // Create directory if it doesn't exist
    std::fs::create_dir_all(&e2ee_dir)
        .map_err(|e| format!("Failed to create E2EE directory: {e}"))?;

    Ok(e2ee_dir)
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
/// then call `create_backup` to actually store the encrypted backup.
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

    let data = String::from_utf8(decrypted).map_err(|_| "Backup data is not valid UTF-8")?;

    info!("Backup restored successfully");
    Ok(data)
}

// =============================================================================
// E2EE Commands
// =============================================================================

/// Get E2EE initialization status.
///
/// Returns information about whether E2EE is initialized for the current user.
#[command]
pub async fn get_e2ee_status(state: State<'_, AppState>) -> Result<E2EEStatus, String> {
    let crypto = state.crypto.lock().await;

    match crypto.as_ref() {
        Some(manager) => {
            // Check if we can get identity keys
            let has_identity_keys = manager.get_identity_keys().is_ok();

            Ok(E2EEStatus {
                initialized: true,
                device_id: Some(manager.device_id().to_string()),
                has_identity_keys,
            })
        }
        None => Ok(E2EEStatus {
            initialized: false,
            device_id: None,
            has_identity_keys: false,
        }),
    }
}

/// Initialize E2EE for the current user.
///
/// Creates a new Olm account if one doesn't exist, or loads an existing one.
/// Returns identity keys and prekeys for upload to the server.
///
/// # Arguments
///
/// * `encryption_key` - A string to derive the encryption key from (e.g., recovery key)
#[command]
pub async fn init_e2ee(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    encryption_key: String,
) -> Result<InitE2EEResponse, String> {
    info!("Initializing E2EE");

    // Get user_id from auth state
    let auth = state.auth.read().await;
    let user = auth.user.as_ref().ok_or("Not authenticated")?;
    let user_id_str = user.id.clone();
    drop(auth);

    let user_id =
        Uuid::parse_str(&user_id_str).map_err(|e| format!("Invalid user ID format: {e}"))?;

    // Get data directory
    let data_dir = get_e2ee_data_dir(&app_handle, &user_id_str)?;

    // Derive encryption key from input
    let key = derive_encryption_key(&encryption_key);

    // Initialize crypto manager
    let manager =
        CryptoManager::init(data_dir, user_id, key).map_err(|e| format!("Init failed: {e}"))?;

    // Get identity keys
    let identity = manager
        .get_identity_keys()
        .map_err(|e| format!("Failed to get identity keys: {e}"))?;

    // Get unpublished prekeys
    let prekeys: Vec<PrekeyData> = manager
        .get_unpublished_keys()
        .map_err(|e| format!("Failed to get prekeys: {e}"))?
        .into_iter()
        .map(PrekeyData::from)
        .collect();

    let device_id = manager.device_id().to_string();

    // Store manager in state
    let mut crypto = state.crypto.lock().await;
    *crypto = Some(manager);

    info!(
        device_id = %device_id,
        prekey_count = prekeys.len(),
        "E2EE initialized successfully"
    );

    Ok(InitE2EEResponse {
        device_id,
        identity_key_ed25519: identity.ed25519,
        identity_key_curve25519: identity.curve25519,
        prekeys,
    })
}

/// Encrypt a message for multiple recipients.
///
/// # Arguments
///
/// * `plaintext` - The message to encrypt
/// * `recipients` - List of recipients with their claimed prekeys
#[command]
pub async fn encrypt_message(
    state: State<'_, AppState>,
    plaintext: String,
    recipients: Vec<ClaimedPrekeyInput>,
) -> Result<E2EEContentOutput, String> {
    let crypto = state.crypto.lock().await;
    let manager = crypto.as_ref().ok_or("E2EE not initialized")?;

    // Get our sender key
    let sender_key = manager
        .our_curve25519_key()
        .map_err(|e| format!("Failed to get sender key: {e}"))?;

    let mut result_recipients: HashMap<String, HashMap<String, EncryptedMessageOutput>> =
        HashMap::new();

    for recipient in recipients {
        let user_id = Uuid::parse_str(&recipient.user_id)
            .map_err(|e| format!("Invalid recipient user ID: {e}"))?;

        let device_id = Uuid::parse_str(&recipient.device_id)
            .map_err(|e| format!("Invalid recipient device ID: {e}"))?;

        // Convert input to ClaimedPrekey
        let claimed = ClaimedPrekey {
            device_id,
            identity_key_ed25519: recipient.identity_key_ed25519,
            identity_key_curve25519: recipient.identity_key_curve25519,
            one_time_prekey: recipient.one_time_prekey.map(|p| PrekeyInfo {
                key_id: p.key_id,
                public_key: p.public_key,
            }),
        };

        // Encrypt for this device
        let ciphertext = manager
            .encrypt_for_device(user_id, &claimed, &plaintext)
            .map_err(|e| format!("Encryption failed for {}: {e}", recipient.user_id))?;

        // Add to result map
        let user_devices = result_recipients.entry(recipient.user_id).or_default();
        user_devices.insert(recipient.device_id, ciphertext.into());
    }

    Ok(E2EEContentOutput {
        sender_key,
        recipients: result_recipients,
    })
}

/// Decrypt a received message.
///
/// # Arguments
///
/// * `sender_user_id` - The sender's user ID
/// * `sender_key` - The sender's Curve25519 public key (base64)
/// * `message_type` - Message type: 0 = prekey, 1 = normal
/// * `ciphertext` - Base64-encoded ciphertext
#[command]
pub async fn decrypt_message(
    state: State<'_, AppState>,
    sender_user_id: String,
    sender_key: String,
    message_type: u8,
    ciphertext: String,
) -> Result<String, String> {
    let crypto = state.crypto.lock().await;
    let manager = crypto.as_ref().ok_or("E2EE not initialized")?;

    let sender_uuid =
        Uuid::parse_str(&sender_user_id).map_err(|e| format!("Invalid sender user ID: {e}"))?;

    // Construct EncryptedMessage
    let message = EncryptedMessage {
        message_type,
        ciphertext,
    };

    // Decrypt
    let plaintext = manager
        .decrypt_message(sender_uuid, &sender_key, &message)
        .map_err(|e| format!("Decryption failed: {e}"))?;

    Ok(plaintext)
}

/// Mark prekeys as published after successful upload to server.
#[command]
pub async fn mark_prekeys_published(state: State<'_, AppState>) -> Result<(), String> {
    let crypto = state.crypto.lock().await;
    let manager = crypto.as_ref().ok_or("E2EE not initialized")?;

    manager
        .mark_keys_published()
        .map_err(|e| format!("Failed to mark keys published: {e}"))?;

    info!("Prekeys marked as published");
    Ok(())
}

/// Generate additional prekeys for upload to server.
///
/// # Arguments
///
/// * `count` - Number of prekeys to generate
#[command]
pub async fn generate_prekeys(
    state: State<'_, AppState>,
    count: usize,
) -> Result<Vec<PrekeyData>, String> {
    let crypto = state.crypto.lock().await;
    let manager = crypto.as_ref().ok_or("E2EE not initialized")?;

    let prekeys: Vec<PrekeyData> = manager
        .generate_prekeys(count)
        .map_err(|e| format!("Failed to generate prekeys: {e}"))?
        .into_iter()
        .map(PrekeyData::from)
        .collect();

    info!(count = prekeys.len(), "Generated new prekeys");
    Ok(prekeys)
}

/// Check if we need to upload more prekeys.
#[command]
pub async fn needs_prekey_upload(state: State<'_, AppState>) -> Result<bool, String> {
    let crypto = state.crypto.lock().await;
    let manager = crypto.as_ref().ok_or("E2EE not initialized")?;

    manager
        .needs_key_upload()
        .map_err(|e| format!("Failed to check key upload status: {e}"))
}

/// Get our Curve25519 public key (base64).
///
/// This is needed for looking up our ciphertext in encrypted messages.
#[command]
pub async fn get_our_curve25519_key(state: State<'_, AppState>) -> Result<String, String> {
    let crypto = state.crypto.lock().await;
    let manager = crypto.as_ref().ok_or("E2EE not initialized")?;

    manager
        .our_curve25519_key()
        .map_err(|e| format!("Failed to get Curve25519 key: {e}"))
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

        let parsed =
            RecoveryKey::from_formatted_string(&formatted).expect("Should parse formatted key");

        // The keys should be equivalent (same formatted output)
        assert_eq!(key.to_formatted_string(), parsed.to_formatted_string());
    }

    #[test]
    fn test_recovery_key_display_format() {
        // Verify the display format used by the UI
        let key = RecoveryKey::generate();
        let formatted = key.to_formatted_string();

        // Should contain spaces separating groups
        assert!(
            formatted.contains(' '),
            "Formatted key should contain spaces"
        );

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
