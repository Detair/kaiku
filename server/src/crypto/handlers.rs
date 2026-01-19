//! E2EE Key Management HTTP Handlers
//!
//! Handlers for uploading identity keys, prekeys, and retrieving user keys.

use axum::{extract::Path, extract::State, Json};
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
/// Prevents DoS attacks by limiting the amount of work per request.
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
            "Cannot upload more than {} prekeys at once",
            MAX_PREKEYS_PER_UPLOAD
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
