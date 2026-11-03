//! Pre-session commands (C27): account requests and registration tickets.
//!
//! The request secret and the ticket arrive in response bodies and stay in
//! Rust ([`crate::session::pre_session`]); the webview only sees the answers
//! without them and calls an allowlisted set of routes through
//! [`pre_session_request`].

use serde_json::Value;
use tauri::{command, State};
use tracing::info;

use crate::session::PreSessionKind;
use crate::AppState;

/// Start a recovery or bootstrap request; `request_secret` is kept here.
#[command]
pub async fn account_request_start(
    state: State<'_, AppState>,
    server_url: String,
    body: Value,
) -> Result<Value, String> {
    info!("Starting an account request");
    state
        .session
        .account_request_start(&server_url, body)
        .await
        .map_err(|e| e.to_string())
}

/// The OIDC factor of an account request (C27, C29 step 8) → `{oidc_code}`.
///
/// TODO(LP rebase): run the request OIDC start
/// (`/auth/account-requests/oidc/authorize/{provider_id}`) with LP's
/// `loopback_post::bind()` + `wait_for_post` and
/// `open_browser(app, url, settings.passkey_browser)`, keep the verifier with
/// `SessionManager::keep_request_oidc_verifier` and return the hand-off code.
/// Until LP's listener exists this refuses without contacting the server.
#[command]
pub async fn request_oidc_factor(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    server_url: String,
    provider_id: String,
) -> Result<Value, String> {
    let _ = (&state, &app);
    info!(provider = %provider_id, server = %server_url, "Request OIDC factor is not available before LP");
    Err("OIDC_UNAVAILABLE".to_string())
}

/// A request-client or ticket call with its secret header (allowlisted;
/// anything else is `PATH_NOT_ALLOWED` without a request).
#[command]
pub async fn pre_session_request(
    state: State<'_, AppState>,
    kind: PreSessionKind,
    method: String,
    path: String,
    body: Option<Value>,
) -> Result<Value, String> {
    state
        .session
        .pre_session_request(kind, &method, &path, body)
        .await
        .map_err(|e| e.to_string())
}
