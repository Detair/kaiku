//! E2EE Key Management
//!
//! Handles device identity keys, one-time prekeys, and key backups
//! for end-to-end encrypted messaging using the Olm/Megolm protocol.

pub mod handlers;

use axum::{
    routing::{get, post},
    Router,
};

use crate::api::AppState;

/// Create E2EE key management router.
///
/// Routes:
/// - POST /upload - Upload identity keys and prekeys for a device
/// - GET /backup - Download encrypted key backup
/// - POST /backup - Upload encrypted key backup
/// - GET /backup/status - Check backup existence and metadata
/// - GET /devices - Get current user's devices
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/upload", post(handlers::upload_keys))
        .route("/backup", get(handlers::get_backup).post(handlers::upload_backup))
        .route("/backup/status", get(handlers::get_backup_status))
        .route("/devices", get(handlers::get_own_devices))
}

/// Create user keys router for fetching other users' keys.
///
/// Routes:
/// - GET / - Get a user's device keys
/// - POST /claim - Claim a prekey from a user's device
pub fn user_keys_router() -> Router<AppState> {
    Router::new()
        .route("/", get(handlers::get_user_keys))
        .route("/claim", post(handlers::claim_prekey))
}
