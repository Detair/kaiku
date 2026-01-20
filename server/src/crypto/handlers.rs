//! E2EE Key Management HTTP Handlers
//!
//! Handlers for uploading identity keys, prekeys, retrieving user keys, and key backups.

use axum::{extract::Path, extract::State, http::StatusCode, Json};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::{AuthError, AuthUser};

// ============================================================================
// Request/Response Types
// ============================================================================

/// Request to upload device keys.
#[derive(Debug, Deserialize)]
pub struct UploadKeysRequest {
    /// Optional human-readable device name (e.g., "Desktop", "Phone").
    pub device_name: Option<String>,
    /// Ed25519 signing key (base64-encoded public key).
    pub identity_key_ed25519: String,
    /// Curve25519 key exchange key (base64-encoded public key).
    pub identity_key_curve25519: String,
    /// One-time prekeys to upload.
    pub one_time_prekeys: Vec<PrekeyUpload>,
}

/// A single prekey to upload.
#[derive(Debug, Deserialize)]
pub struct PrekeyUpload {
    /// Unique identifier for this prekey (usually a counter or UUID).
    pub key_id: String,
    /// Curve25519 public key (base64-encoded).
    pub public_key: String,
}

/// Response after uploading keys.
#[derive(Debug, Serialize)]
pub struct UploadKeysResponse {
    /// The device ID (new or existing).
    pub device_id: Uuid,
    /// Number of prekeys that were actually uploaded (excludes duplicates).
    pub prekeys_uploaded: usize,
    /// Number of prekeys that were skipped due to validation errors.
    pub prekeys_skipped: usize,
}

/// Response containing a user's device keys.
#[derive(Debug, Serialize)]
pub struct UserKeysResponse {
    /// List of devices with their public keys.
    pub devices: Vec<DeviceKeys>,
}

/// Request to claim a prekey from a specific device.
#[derive(Debug, Deserialize)]
pub struct ClaimPrekeyRequest {
    /// The device ID to claim a prekey from.
    pub device_id: Uuid,
}

/// Response after claiming a prekey.
#[derive(Debug, Serialize)]
pub struct ClaimPrekeyResponse {
    /// The device ID the prekey was claimed from.
    pub device_id: Uuid,
    /// Ed25519 signing key (base64-encoded).
    pub identity_key_ed25519: String,
    /// Curve25519 key exchange key (base64-encoded).
    pub identity_key_curve25519: String,
    /// The claimed one-time prekey (if available).
    pub one_time_prekey: Option<ClaimedPrekey>,
}

/// A claimed one-time prekey.
#[derive(Debug, Serialize, FromRow)]
pub struct ClaimedPrekey {
    /// Unique identifier for this prekey.
    pub key_id: String,
    /// Curve25519 public key (base64-encoded).
    pub public_key: String,
}

/// Public keys for a single device.
#[derive(Debug, Serialize, FromRow)]
pub struct DeviceKeys {
    /// Device ID.
    pub device_id: Uuid,
    /// Device name (if set).
    pub device_name: Option<String>,
    /// Ed25519 signing key (base64-encoded).
    pub identity_key_ed25519: String,
    /// Curve25519 key exchange key (base64-encoded).
    pub identity_key_curve25519: String,
}

// ============================================================================
// Constants
// ============================================================================

/// Maximum number of prekeys that can be uploaded in a single request.
/// Prevents denial-of-service attacks by limiting the amount of work per request.
const MAX_PREKEYS_PER_UPLOAD: usize = 100;

// ============================================================================
// Handlers
// ============================================================================

/// Upload identity keys and prekeys for a device.
///
/// Creates a new device if the identity key is new, or updates the existing
/// device's `last_seen_at` timestamp. Prekeys are uploaded with `ON CONFLICT
/// DO NOTHING` to avoid duplicate key errors.
///
/// POST /api/keys/upload
#[tracing::instrument(skip(state, req), fields(user_id = %auth_user.id))]
pub async fn upload_keys(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(req): Json<UploadKeysRequest>,
) -> Result<Json<UploadKeysResponse>, AuthError> {
    let user_id = auth_user.id;

    // Validate input lengths to prevent abuse
    if let Some(ref name) = req.device_name {
        if name.len() > 128 {
            return Err(AuthError::Validation(
                "Device name must be 128 characters or less".to_string(),
            ));
        }
    }
    if req.identity_key_ed25519.len() > 64 {
        return Err(AuthError::Validation(
            "identity_key_ed25519 must be 64 characters or less".to_string(),
        ));
    }
    if req.identity_key_curve25519.len() > 64 {
        return Err(AuthError::Validation(
            "identity_key_curve25519 must be 64 characters or less".to_string(),
        ));
    }
    if req.one_time_prekeys.len() > MAX_PREKEYS_PER_UPLOAD {
        return Err(AuthError::Validation(format!(
            "Cannot upload more than {MAX_PREKEYS_PER_UPLOAD} prekeys at once"
        )));
    }

    // Insert or update device
    let device_id: Uuid = sqlx::query_scalar(
        "
        INSERT INTO user_devices (user_id, device_name, identity_key_ed25519, identity_key_curve25519)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (user_id, identity_key_curve25519)
        DO UPDATE SET last_seen_at = NOW(), device_name = COALESCE(EXCLUDED.device_name, user_devices.device_name)
        RETURNING id
        ",
    )
    .bind(user_id)
    .bind(&req.device_name)
    .bind(&req.identity_key_ed25519)
    .bind(&req.identity_key_curve25519)
    .fetch_one(&state.db)
    .await
    .map_err(AuthError::Database)?;

    // Insert prekeys (skip duplicates and invalid keys)
    let mut prekeys_uploaded = 0;
    let mut prekeys_skipped = 0;
    for prekey in &req.one_time_prekeys {
        // Validate prekey lengths
        if prekey.key_id.len() > 64 || prekey.public_key.len() > 64 {
            prekeys_skipped += 1;
            continue;
        }

        let result = sqlx::query(
            "
            INSERT INTO prekeys (device_id, key_id, public_key)
            VALUES ($1, $2, $3)
            ON CONFLICT (device_id, key_id) DO NOTHING
            ",
        )
        .bind(device_id)
        .bind(&prekey.key_id)
        .bind(&prekey.public_key)
        .execute(&state.db)
        .await
        .map_err(AuthError::Database)?;

        if result.rows_affected() > 0 {
            prekeys_uploaded += 1;
        }
    }

    tracing::info!(
        user_id = %user_id,
        device_id = %device_id,
        prekeys_uploaded = prekeys_uploaded,
        prekeys_skipped = prekeys_skipped,
        "Keys uploaded"
    );

    Ok(Json(UploadKeysResponse {
        device_id,
        prekeys_uploaded,
        prekeys_skipped,
    }))
}

/// Get a user's device keys for encryption.
///
/// Returns all devices and their public identity keys for a given user.
/// This is used when establishing encrypted sessions with another user.
///
/// GET /api/users/:id/keys
#[tracing::instrument(skip(state, auth_user), fields(target_user_id = %user_id))]
pub async fn get_user_keys(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(user_id): Path<Uuid>,
) -> Result<Json<UserKeysResponse>, AuthError> {
    // auth_user is required for authentication but not used in this handler
    let _ = auth_user;

    let devices: Vec<DeviceKeys> = sqlx::query_as(
        "
        SELECT
            id as device_id,
            device_name,
            identity_key_ed25519,
            identity_key_curve25519
        FROM user_devices
        WHERE user_id = $1
        ORDER BY last_seen_at DESC
        ",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .map_err(AuthError::Database)?;

    Ok(Json(UserKeysResponse { devices }))
}

/// Get the current user's own device keys.
///
/// Returns all devices registered for the authenticated user.
///
/// GET /api/keys/devices
#[tracing::instrument(skip(state), fields(user_id = %auth_user.id))]
pub async fn get_own_devices(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Result<Json<UserKeysResponse>, AuthError> {
    let devices: Vec<DeviceKeys> = sqlx::query_as(
        "
        SELECT
            id as device_id,
            device_name,
            identity_key_ed25519,
            identity_key_curve25519
        FROM user_devices
        WHERE user_id = $1
        ORDER BY last_seen_at DESC
        ",
    )
    .bind(auth_user.id)
    .fetch_all(&state.db)
    .await
    .map_err(AuthError::Database)?;

    Ok(Json(UserKeysResponse { devices }))
}

/// Claim a prekey for a specific device (atomic).
///
/// Atomically claims one prekey from the specified device using `FOR UPDATE SKIP LOCKED`
/// to ensure concurrent requests don't claim the same prekey. Returns the device's
/// identity keys along with the claimed prekey.
///
/// POST /api/users/:id/keys/claim
#[tracing::instrument(skip(state), fields(claimer_id = %auth_user.id, target_user_id = %target_user_id))]
pub async fn claim_prekey(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(target_user_id): Path<Uuid>,
    Json(req): Json<ClaimPrekeyRequest>,
) -> Result<Json<ClaimPrekeyResponse>, AuthError> {
    let claimer_id = auth_user.id;

    // Get device info and verify it belongs to the target user
    let device: DeviceIdentityKeys = sqlx::query_as(
        "
        SELECT identity_key_ed25519, identity_key_curve25519
        FROM user_devices
        WHERE id = $1 AND user_id = $2
        ",
    )
    .bind(req.device_id)
    .bind(target_user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(AuthError::Database)?
    .ok_or_else(|| AuthError::Validation("Device not found".to_string()))?;

    // Atomically claim one prekey using FOR UPDATE SKIP LOCKED
    // This ensures concurrent requests don't claim the same prekey
    let prekey: Option<ClaimedPrekey> = sqlx::query_as(
        "
        UPDATE prekeys
        SET claimed_at = NOW(), claimed_by = $1
        WHERE id = (
            SELECT id FROM prekeys
            WHERE device_id = $2 AND claimed_at IS NULL
            ORDER BY created_at
            LIMIT 1
            FOR UPDATE SKIP LOCKED
        )
        RETURNING key_id, public_key
        ",
    )
    .bind(claimer_id)
    .bind(req.device_id)
    .fetch_optional(&state.db)
    .await
    .map_err(AuthError::Database)?;

    tracing::info!(
        claimer_id = %claimer_id,
        device_id = %req.device_id,
        prekey_claimed = prekey.is_some(),
        "Prekey claim attempt"
    );

    Ok(Json(ClaimPrekeyResponse {
        device_id: req.device_id,
        identity_key_ed25519: device.identity_key_ed25519,
        identity_key_curve25519: device.identity_key_curve25519,
        one_time_prekey: prekey,
    }))
}

/// Internal struct for fetching device identity keys.
#[derive(Debug, FromRow)]
struct DeviceIdentityKeys {
    identity_key_ed25519: String,
    identity_key_curve25519: String,
}

// ============================================================================
// Backup Request/Response Types
// ============================================================================

/// Request to upload an encrypted key backup.
#[derive(Debug, Deserialize)]
pub struct UploadBackupRequest {
    /// Salt used for key derivation (Base64-encoded, must be 16 bytes).
    pub salt: String,
    /// AES-GCM nonce (Base64-encoded, must be 12 bytes).
    pub nonce: String,
    /// Encrypted backup data (Base64-encoded, max 1MB).
    pub ciphertext: String,
    /// Backup version for future compatibility.
    pub version: i32,
}

/// Response containing an encrypted key backup.
#[derive(Debug, Serialize)]
pub struct BackupResponse {
    /// Salt used for key derivation (Base64-encoded).
    pub salt: String,
    /// AES-GCM nonce (Base64-encoded).
    pub nonce: String,
    /// Encrypted backup data (Base64-encoded).
    pub ciphertext: String,
    /// Backup version.
    pub version: i32,
    /// When the backup was created.
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// ============================================================================
// Backup Handlers
// ============================================================================

/// Upload an encrypted key backup.
///
/// Creates or updates the user's encrypted key backup. Only one backup is stored
/// per user (UPSERT pattern). The client is responsible for encrypting the backup
/// using a recovery key before uploading.
///
/// POST /api/keys/backup
#[tracing::instrument(skip(state, req), fields(user_id = %auth_user.id))]
pub async fn upload_backup(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(req): Json<UploadBackupRequest>,
) -> Result<StatusCode, AuthError> {
    let user_id = auth_user.id;

    // Decode and validate base64
    let salt = STANDARD
        .decode(&req.salt)
        .map_err(|_| AuthError::Validation("Invalid salt encoding".into()))?;
    let nonce = STANDARD
        .decode(&req.nonce)
        .map_err(|_| AuthError::Validation("Invalid nonce encoding".into()))?;
    let ciphertext = STANDARD
        .decode(&req.ciphertext)
        .map_err(|_| AuthError::Validation("Invalid ciphertext encoding".into()))?;

    // Validate sizes (match DB constraints)
    if salt.len() != 16 {
        return Err(AuthError::Validation("Salt must be 16 bytes".into()));
    }
    if nonce.len() != 12 {
        return Err(AuthError::Validation("Nonce must be 12 bytes".into()));
    }
    if ciphertext.len() > 1_048_576 {
        // 1MB max
        return Err(AuthError::Validation("Ciphertext too large".into()));
    }

    sqlx::query(
        r"
        INSERT INTO key_backups (user_id, salt, nonce, ciphertext, version)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (user_id) DO UPDATE SET
            salt = EXCLUDED.salt,
            nonce = EXCLUDED.nonce,
            ciphertext = EXCLUDED.ciphertext,
            version = EXCLUDED.version,
            created_at = NOW()
        ",
    )
    .bind(user_id)
    .bind(&salt)
    .bind(&nonce)
    .bind(&ciphertext)
    .bind(req.version)
    .execute(&state.db)
    .await
    .map_err(AuthError::Database)?;

    tracing::info!(user_id = %user_id, version = req.version, "Key backup uploaded");

    Ok(StatusCode::CREATED)
}

/// Download the user's encrypted key backup.
///
/// Returns the encrypted key backup if one exists. The client is responsible
/// for decrypting the backup using their recovery key.
///
/// GET /api/keys/backup
#[tracing::instrument(skip(state), fields(user_id = %auth_user.id))]
pub async fn get_backup(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Result<Json<BackupResponse>, AuthError> {
    let user_id = auth_user.id;

    let backup = sqlx::query_as::<_, BackupRow>(
        r"
        SELECT salt, nonce, ciphertext, version, created_at
        FROM key_backups
        WHERE user_id = $1
        ",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(AuthError::Database)?
    .ok_or_else(|| AuthError::NotFound("No backup found".into()))?;

    Ok(Json(BackupResponse {
        salt: STANDARD.encode(&backup.salt),
        nonce: STANDARD.encode(&backup.nonce),
        ciphertext: STANDARD.encode(&backup.ciphertext),
        version: backup.version,
        created_at: backup.created_at,
    }))
}

/// Internal struct for fetching backup data.
#[derive(Debug, FromRow)]
struct BackupRow {
    salt: Vec<u8>,
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
    version: i32,
    created_at: chrono::DateTime<chrono::Utc>,
}

/// Response for backup status check.
#[derive(Debug, Serialize)]
pub struct BackupStatusResponse {
    /// Whether a backup exists.
    pub has_backup: bool,
    /// When the backup was created (if exists).
    pub backup_created_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Backup version (if exists).
    pub version: Option<i32>,
}

/// Internal struct for fetching backup status.
#[derive(Debug, FromRow)]
struct BackupStatusRow {
    version: i32,
    created_at: chrono::DateTime<chrono::Utc>,
}

/// Check if user has a key backup.
///
/// Returns the backup status including existence, creation timestamp, and version.
/// This is a lightweight endpoint that doesn't return the actual backup data.
///
/// GET /api/keys/backup/status
#[tracing::instrument(skip(state), fields(user_id = %auth_user.id))]
pub async fn get_backup_status(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Result<Json<BackupStatusResponse>, AuthError> {
    let backup = sqlx::query_as::<_, BackupStatusRow>(
        "SELECT version, created_at FROM key_backups WHERE user_id = $1",
    )
    .bind(auth_user.id)
    .fetch_optional(&state.db)
    .await
    .map_err(AuthError::Database)?;

    Ok(Json(match backup {
        Some(b) => BackupStatusResponse {
            has_backup: true,
            backup_created_at: Some(b.created_at),
            version: Some(b.version),
        },
        None => BackupStatusResponse {
            has_backup: false,
            backup_created_at: None,
            version: None,
        },
    }))
}
