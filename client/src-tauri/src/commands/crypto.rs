//! E2EE Key Management Commands

use std::collections::HashMap;
use std::path::PathBuf;

use argon2::{Argon2, Params};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{command, Manager, State};
use tracing::{error, info, warn};
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

/// Maximum length for encryption key / passphrase input (1 KB).
/// Argon2id processes the entire input, so unbounded strings cause CPU stalls.
const MAX_ENCRYPTION_KEY_LEN: usize = 1_024;

/// Maximum length for plaintext messages (100 KB, consistent with pages.rs).
const MAX_PLAINTEXT_LEN: usize = 102_400;

/// Maximum length for backup data (10 MB).
const MAX_BACKUP_DATA_LEN: usize = 10 * 1024 * 1024;

/// Maximum length for base64-encoded ciphertext (200 KB).
/// Base64 expands data ~33%, so this covers ~150 KB of raw ciphertext.
const MAX_CIPHERTEXT_LEN: usize = 204_800;

/// Maximum length for base64-encoded sender key (256 bytes).
const MAX_SENDER_KEY_LEN: usize = 256;

/// Maximum length for recovery key input (256 bytes).
/// A Base58-encoded recovery key is ~44 chars plus optional whitespace.
const MAX_RECOVERY_KEY_LEN: usize = 256;

/// Maximum number of recipients for a single encrypt operation.
const MAX_RECIPIENTS: usize = 1_000;

/// Maximum number of prekeys to generate in one call.
const MAX_PREKEY_COUNT: usize = 100;

/// Salt file name stored alongside the E2EE database.
const SALT_FILE: &str = "kdf_salt";

/// Derive a 32-byte encryption key from a string using Argon2id with a random salt.
///
/// The salt is stored in `{data_dir}/kdf_salt` and generated on first use.
/// For existing installations without a salt file, falls back to SHA-256 for
/// backward compatibility. Note: legacy installs remain on SHA-256 until a
/// future migration re-encrypts the database under an Argon2id-derived key.
fn derive_encryption_key(input: &str, data_dir: &std::path::Path) -> Result<[u8; 32], String> {
    let salt_path = data_dir.join(SALT_FILE);
    let db_path = data_dir.join("keys.db");

    if let Ok(salt_bytes) = std::fs::read(&salt_path) {
        if salt_bytes.len() == 16 {
            // Salt exists — use Argon2id
            let mut salt = [0u8; 16];
            salt.copy_from_slice(&salt_bytes);
            return derive_with_argon2id(input, &salt);
        }
    }

    if db_path.exists() {
        // Existing database without salt file — use legacy SHA-256 for backward compat
        warn!("Using legacy SHA-256 key derivation for existing key store (no salt file)");
        return Ok(derive_with_sha256(input));
    }

    // New installation — generate salt and use Argon2id
    let mut salt = [0u8; 16];
    getrandom::getrandom(&mut salt).map_err(|e| format!("Failed to generate KDF salt: {e}"))?;
    if let Err(e) = std::fs::write(&salt_path, salt) {
        warn!("Failed to write KDF salt file, falling back to SHA-256: {e}");
        return Ok(derive_with_sha256(input));
    }
    info!("Generated new Argon2id KDF salt for E2EE key store");
    derive_with_argon2id(input, &salt)
}

/// Derive key using Argon2id (secure KDF).
fn derive_with_argon2id(input: &str, salt: &[u8; 16]) -> Result<[u8; 32], String> {
    // Parameters: 32 MiB memory, 2 iterations, 1 parallelism
    let params =
        Params::new(32768, 2, 1, Some(32)).map_err(|e| format!("Invalid Argon2 params: {e}"))?;
    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);

    let mut output = [0u8; 32];
    argon2
        .hash_password_into(input.as_bytes(), salt, &mut output)
        .map_err(|e| format!("Argon2id key derivation failed: {e}"))?;
    Ok(output)
}

/// Legacy key derivation using SHA-256 (for backward compatibility only).
fn derive_with_sha256(input: &str) -> [u8; 32] {
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
    if recovery_key.len() > MAX_RECOVERY_KEY_LEN {
        return Err(format!(
            "Recovery key exceeds maximum length of {MAX_RECOVERY_KEY_LEN} bytes"
        ));
    }
    if backup_data.len() > MAX_BACKUP_DATA_LEN {
        return Err(format!(
            "Backup data exceeds maximum size of {} MB",
            MAX_BACKUP_DATA_LEN / (1024 * 1024)
        ));
    }

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
    if recovery_key.len() > MAX_RECOVERY_KEY_LEN {
        return Err(format!(
            "Recovery key exceeds maximum length of {MAX_RECOVERY_KEY_LEN} bytes"
        ));
    }

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
    if encryption_key.len() > MAX_ENCRYPTION_KEY_LEN {
        return Err(format!(
            "Encryption key exceeds maximum length of {MAX_ENCRYPTION_KEY_LEN} bytes"
        ));
    }

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

    // Derive encryption key from input using Argon2id (or SHA-256 for legacy stores)
    let key = derive_encryption_key(&encryption_key, &data_dir)?;

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
    if plaintext.len() > MAX_PLAINTEXT_LEN {
        return Err(format!(
            "Plaintext exceeds maximum size of {} KB",
            MAX_PLAINTEXT_LEN / 1024
        ));
    }
    if recipients.len() > MAX_RECIPIENTS {
        return Err(format!("Too many recipients (max {MAX_RECIPIENTS})"));
    }

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
    if sender_key.len() > MAX_SENDER_KEY_LEN {
        return Err(format!(
            "Sender key exceeds maximum length of {MAX_SENDER_KEY_LEN} bytes"
        ));
    }
    if ciphertext.len() > MAX_CIPHERTEXT_LEN {
        return Err(format!(
            "Ciphertext exceeds maximum size of {} KB",
            MAX_CIPHERTEXT_LEN / 1024
        ));
    }

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
    if count > MAX_PREKEY_COUNT {
        return Err(format!(
            "Prekey count exceeds maximum of {MAX_PREKEY_COUNT}"
        ));
    }

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

// =============================================================================
// Megolm Group E2EE Commands
// =============================================================================

/// Create a new Megolm outbound session for a group/channel.
/// Returns the exportable session key (base64) that should be shared with other members
/// via 1:1 Olm encrypted messages.
#[command]
pub async fn create_megolm_session(
    state: State<'_, AppState>,
    room_id: String,
) -> Result<String, String> {
    let crypto = state.crypto.lock().await;
    let manager = crypto.as_ref().ok_or("E2EE not initialized")?;

    manager
        .create_outbound_group_session(&room_id)
        .map_err(|e| format!("Failed to create Megolm session: {e}"))
}

/// Encrypt a message for a group using Megolm.
#[command]
pub async fn encrypt_group_message(
    state: State<'_, AppState>,
    room_id: String,
    plaintext: String,
) -> Result<String, String> {
    if plaintext.len() > MAX_PLAINTEXT_LEN {
        return Err(format!(
            "Plaintext exceeds maximum size of {} KB",
            MAX_PLAINTEXT_LEN / 1024
        ));
    }

    let crypto = state.crypto.lock().await;
    let manager = crypto.as_ref().ok_or("E2EE not initialized")?;

    manager
        .encrypt_group_message(&room_id, &plaintext)
        .map_err(|e| format!("Failed to encrypt group message: {e}"))
}

/// Store an inbound Megolm session key received from another user.
#[command]
pub async fn add_inbound_group_session(
    state: State<'_, AppState>,
    room_id: String,
    sender_key: String,
    session_key: String,
) -> Result<(), String> {
    let crypto = state.crypto.lock().await;
    let manager = crypto.as_ref().ok_or("E2EE not initialized")?;

    manager
        .add_inbound_group_session(&room_id, &sender_key, &session_key)
        .map_err(|e| format!("Failed to add inbound group session: {e}"))
}

/// Decrypt a Megolm group message.
#[command]
pub async fn decrypt_group_message(
    state: State<'_, AppState>,
    room_id: String,
    sender_key: String,
    ciphertext: String,
) -> Result<String, String> {
    if ciphertext.len() > MAX_CIPHERTEXT_LEN {
        return Err(format!(
            "Ciphertext exceeds maximum size of {} KB",
            MAX_CIPHERTEXT_LEN / 1024
        ));
    }

    let crypto = state.crypto.lock().await;
    let manager = crypto.as_ref().ok_or("E2EE not initialized")?;

    manager
        .decrypt_group_message(&room_id, &sender_key, &ciphertext)
        .map_err(|e| format!("Failed to decrypt group message: {e}"))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use vc_crypto::RecoveryKey;

    use super::{derive_encryption_key, derive_with_argon2id, derive_with_sha256};

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

    #[test]
    fn test_argon2id_deterministic() {
        let salt = [42u8; 16];
        let key1 = derive_with_argon2id("test_password", &salt).unwrap();
        let key2 = derive_with_argon2id("test_password", &salt).unwrap();
        assert_eq!(key1, key2);
        assert_ne!(key1, [0u8; 32]);
    }

    #[test]
    fn test_argon2id_different_salts() {
        let key1 = derive_with_argon2id("same_password", &[1u8; 16]).unwrap();
        let key2 = derive_with_argon2id("same_password", &[2u8; 16]).unwrap();
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_argon2id_different_inputs() {
        let salt = [0u8; 16];
        let key1 = derive_with_argon2id("password_a", &salt).unwrap();
        let key2 = derive_with_argon2id("password_b", &salt).unwrap();
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_sha256_deterministic() {
        let key1 = derive_with_sha256("test_input");
        let key2 = derive_with_sha256("test_input");
        assert_eq!(key1, key2);
        assert_eq!(key1.len(), 32);
    }

    #[test]
    fn test_derive_new_installation_uses_argon2id() {
        let dir = tempdir().unwrap();
        let salt_path = dir.path().join("kdf_salt");

        // No keys.db, no salt file → should create salt and use Argon2id
        let key = derive_encryption_key("my_key", dir.path()).unwrap();
        assert_eq!(key.len(), 32);

        // Salt file should now exist
        assert!(salt_path.exists());
        let salt = std::fs::read(&salt_path).unwrap();
        assert_eq!(salt.len(), 16);

        // Same input should produce same key (deterministic with stored salt)
        let key2 = derive_encryption_key("my_key", dir.path()).unwrap();
        assert_eq!(key, key2);
    }

    #[test]
    fn test_derive_legacy_falls_back_to_sha256() {
        let dir = tempdir().unwrap();
        // Create a fake keys.db but no salt file → legacy path
        std::fs::write(dir.path().join("keys.db"), b"fake").unwrap();

        let key = derive_encryption_key("my_key", dir.path()).unwrap();
        let expected = derive_with_sha256("my_key");
        assert_eq!(key, expected);

        // Salt file should NOT have been created
        assert!(!dir.path().join("kdf_salt").exists());
    }

    #[test]
    fn test_derive_corrupted_salt_with_existing_db() {
        let dir = tempdir().unwrap();
        // Write a corrupted salt file (wrong length)
        std::fs::write(dir.path().join("kdf_salt"), b"too_short").unwrap();
        // Create existing DB
        std::fs::write(dir.path().join("keys.db"), b"fake").unwrap();

        // Should fall through to SHA-256 legacy path
        let key = derive_encryption_key("my_key", dir.path()).unwrap();
        let expected = derive_with_sha256("my_key");
        assert_eq!(key, expected);
    }

    #[test]
    fn test_derive_corrupted_salt_without_db() {
        let dir = tempdir().unwrap();
        // Write a corrupted salt file (wrong length), no DB
        std::fs::write(dir.path().join("kdf_salt"), b"short").unwrap();

        // Should generate new salt and overwrite the corrupted one
        let key = derive_encryption_key("my_key", dir.path()).unwrap();
        assert_eq!(key.len(), 32);

        // Salt file should now be correct (16 bytes)
        let salt = std::fs::read(dir.path().join("kdf_salt")).unwrap();
        assert_eq!(salt.len(), 16);
    }
}
