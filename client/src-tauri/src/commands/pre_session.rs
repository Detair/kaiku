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
/// The provider sign-in runs through LP's loopback POST delivery
/// ([`super::oidc_delivery`]) in the browser chosen under "Browser for
/// passkey sign-in"; the verifier stays in Rust until `account_request_start`
/// adds it to the factor (single use, two minutes).
#[command]
pub async fn request_oidc_factor(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    server_url: String,
    provider_id: String,
) -> Result<Value, String> {
    info!(provider = %provider_id, "Starting a request OIDC factor");
    let browser = super::settings::load_settings(&app)
        .await
        .map(|s| s.passkey_browser)
        .unwrap_or_default();
    super::oidc_delivery::request_factor(
        &state.session,
        server_url.trim_end_matches('/'),
        &provider_id,
        |url| super::auth::open_browser_with(&app, url, browser),
        super::oidc_delivery::OIDC_LISTEN_MAX,
    )
    .await
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
