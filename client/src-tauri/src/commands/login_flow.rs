//! Login-attempt commands (C27, SM1): thin wrappers around
//! [`crate::session::SessionManager`]. The webview names the attempt id only;
//! the verifier (collection secret) and the tokens never leave Rust.

use serde_json::Value;
use tauri::{command, State};
use tracing::info;

use crate::session::AuthResult;
use crate::AppState;

/// Start "Sign in with your phone" → `LoginAttempt` JSON (with the first QR).
#[command]
pub async fn login_attempt_create(
    state: State<'_, AppState>,
    server_url: String,
) -> Result<Value, String> {
    info!("Starting a phone sign-in");
    state
        .session
        .create_qr_attempt(&server_url)
        .await
        .map_err(|e| e.to_string())
}

/// Long-poll an attempt (`wait` 0..25 s) → `LoginAttempt` JSON.
#[command]
pub async fn login_attempt_poll(
    state: State<'_, AppState>,
    attempt_id: String,
    wait: u8,
) -> Result<Value, String> {
    state
        .session
        .poll_attempt(&attempt_id, wait)
        .await
        .map_err(|e| e.to_string())
}

/// Collect an approved attempt: the session (probation) starts.
#[command]
pub async fn login_attempt_collect(
    state: State<'_, AppState>,
    attempt_id: String,
) -> Result<AuthResult, String> {
    state
        .session
        .collect_attempt(&attempt_id)
        .await
        .map_err(|e| e.to_string())
}

/// The TOTP step of a password or OIDC attempt.
#[command]
pub async fn login_attempt_totp(
    state: State<'_, AppState>,
    attempt_id: String,
    code: String,
) -> Result<AuthResult, String> {
    state
        .session
        .attempt_totp(&attempt_id, &code)
        .await
        .map_err(|e| e.to_string())
}

/// Give an attempt up (the user left the sign-in screen).
#[command]
pub async fn login_attempt_cancel(
    state: State<'_, AppState>,
    attempt_id: String,
) -> Result<(), String> {
    state
        .session
        .cancel_attempt(&attempt_id)
        .await
        .map_err(|e| e.to_string())
}
