//! Authentication Commands
//!
//! Thin wrappers around [`crate::session::SessionManager`], which owns the
//! tokens. No command returns the refresh token to the webview.

use std::time::Duration;

use serde::Deserialize;
use tauri::{command, State};
use tokio::net::TcpListener;
use tracing::info;
use url::Url;

use super::oidc_loopback::{self, LinkCallback};
use crate::session::{AuthInfo, RegisterRequest, SessionError, SessionRestoreResult};
use crate::AppState;

/// Login request from frontend (C27: no `mfa_code`; the TOTP step is its own call).
#[derive(Deserialize)]
pub struct LoginRequest {
    pub server_url: String,
    pub username: String,
    pub password: String,
}

impl std::fmt::Debug for LoginRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginRequest")
            .field("server_url", &self.server_url)
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .finish()
    }
}

/// Username + password (F3). Returns the `LoginStep` JSON (never a session);
/// the attempt's verifier stays in Rust.
#[command]
pub async fn login(
    state: State<'_, AppState>,
    request: LoginRequest,
) -> Result<serde_json::Value, String> {
    info!("Attempting login for user: {}", request.username);
    state
        .session
        .login(&request.server_url, &request.username, &request.password)
        .await
        .map_err(|e| e.to_string())
}

/// Register a new user (F8): `{user_id, username, ticket_expires_at}`, no
/// session; the registration ticket stays in Rust.
#[command]
pub async fn register(
    state: State<'_, AppState>,
    request: RegisterRequest,
) -> Result<serde_json::Value, String> {
    info!("Attempting registration for user: {}", request.username);
    state
        .session
        .register(request)
        .await
        .map_err(|e| e.to_string())
}

/// Logout and clear credentials.
#[command]
pub async fn logout(state: State<'_, AppState>) -> Result<(), String> {
    info!("Logging out");
    let result = state.session.logout().await.map_err(|e| e.to_string());
    // Clear crypto state — drops Megolm sessions, Olm account, closes SQLite
    *state.crypto.lock().await = None;
    result
}

/// Get the current user, restoring the session from the keyring if needed.
#[command]
pub async fn get_current_user(state: State<'_, AppState>) -> Result<SessionRestoreResult, String> {
    Ok(state.session.restore().await)
}

/// Base URL and access token for webview-side requests (REST, uploads).
#[command]
pub async fn get_auth_info(state: State<'_, AppState>) -> Result<Option<AuthInfo>, String> {
    Ok(state.session.auth_info())
}

/// Refresh the access token now. `None` means the session has ended.
#[command]
pub async fn refresh_session(state: State<'_, AppState>) -> Result<Option<AuthInfo>, String> {
    match state.session.refresh().await {
        Ok(info) => Ok(Some(info)),
        Err(SessionError::Expired) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Session-management requests that need the refresh token (allowlisted).
#[command]
pub async fn session_request(
    state: State<'_, AppState>,
    method: String,
    path: String,
    body: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    state
        .session
        .session_request(&method, &path, body)
        .await
        .map_err(|e| e.to_string())
}

/// Ask the server for the provider URL of a signed-in start (identity link,
/// with the bearer); it may answer `{"url"}` or a redirect we don't follow.
/// A refused start that names a server code fails with that bare code, so
/// the webview can show a matching message. (The desktop sign-in starts in
/// `oidc_delivery`.)
async fn provider_url(authorize_url: &Url, token: &str, what: &str) -> Result<String, String> {
    let no_redirect_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;
    let response = no_redirect_client
        .get(authorize_url.as_str())
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("Failed to get authorize URL: {e}"))?;

    if response.status().is_redirection() {
        Ok(response
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .ok_or("Server did not return a redirect URL")?
            .to_string())
    } else if response.status().is_success() {
        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse authorize response: {e}"))?;
        Ok(body["url"]
            .as_str()
            .ok_or("No authorize URL in response")?
            .to_string())
    } else {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let code = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string));
        Err(code.unwrap_or_else(|| format!("{what} failed ({status})")))
    }
}

/// Open `url` in the system browser.
fn open_browser(app_handle: &tauri::AppHandle, url: &str) -> Result<(), String> {
    open_browser_with(app_handle, url, super::settings::PasskeyBrowser::System)
}

/// The `tauri-plugin-shell` program for the passkey browser setting
/// (`None` = the system default).
#[allow(deprecated)] // tauri-plugin-shell::open is deprecated in favor of tauri-plugin-opener
pub const fn browser_program(
    browser: super::settings::PasskeyBrowser,
) -> Option<tauri_plugin_shell::open::Program> {
    use tauri_plugin_shell::open::Program;

    use super::settings::PasskeyBrowser;
    match browser.validated() {
        PasskeyBrowser::System => None,
        PasskeyBrowser::Chromium => Some(Program::Chromium),
        PasskeyBrowser::Chrome => Some(Program::Chrome),
        PasskeyBrowser::Firefox => Some(Program::Firefox),
        PasskeyBrowser::Safari => Some(Program::Safari),
    }
}

/// Open `url` in the system browser, or in the browser chosen under
/// "Browser for passkey sign-in" (DC14).
pub fn open_browser_with(
    app_handle: &tauri::AppHandle,
    url: &str,
    browser: super::settings::PasskeyBrowser,
) -> Result<(), String> {
    #[allow(deprecated)] // tauri-plugin-shell::open is deprecated in favor of tauri-plugin-opener
    {
        use tauri_plugin_shell::ShellExt;
        app_handle
            .shell()
            .open(url, browser_program(browser))
            .map_err(|e| format!("Failed to open browser: {e}"))
    }
}

/// Bind the loopback listener and build its return address with a fresh
/// per-flow value; returns `(listener, flow, redirect_uri)`.
async fn bind_loopback() -> Result<(TcpListener, String, String), String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("Failed to bind localhost listener: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("Failed to get local address: {e}"))?
        .port();
    let flow = oidc_loopback::new_secret();
    let redirect_uri = oidc_loopback::return_address(port, &flow);
    Ok((listener, flow, redirect_uri))
}

/// `{server}/auth/.../{slug}?redirect_uri=...` with `extra` query pairs.
fn authorize_endpoint(
    server_url: &str,
    segments: &[&str],
    redirect_uri: &str,
    extra: &[(&str, &str)],
) -> Result<Url, String> {
    let mut url = Url::parse(server_url).map_err(|e| format!("Invalid server URL: {e}"))?;
    // Path segment mutation encodes the provider slug safely.
    url.path_segments_mut()
        .map_err(|()| "Invalid server URL: cannot be a base")?
        .extend(segments);
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("redirect_uri", redirect_uri);
        for (k, v) in extra {
            query.append_pair(k, v);
        }
    }
    Ok(url)
}

const LINK_OK_PAGE: &str = "<html><body><h2>Account linked!</h2><p>You can close this window and return to the app.</p></body></html>";
const LINK_FAILED_PAGE: &str = "<html><body><h2>Linking failed</h2><p>You can close this window and return to the app.</p></body></html>";

/// Desktop SSO sign-in (F3 variant, C27, C29 step 8).
///
/// LP's loopback listener is bound first, the start sends `loopback_port`,
/// `callback_path` and `client_challenge` (no `redirect_uri`), the provider
/// opens in the browser chosen under "Browser for passkey sign-in", and the
/// server's hand-off page posts a one-time code to the listener. The code is
/// redeemed with the verifier; the answer is the `LoginStep` (confirmed like a
/// password), the verifier stays in Rust as the attempt's collection secret.
/// See [`super::oidc_delivery`].
#[command]
pub async fn oidc_authorize(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    server_url: String,
    provider_slug: String,
) -> Result<serde_json::Value, String> {
    info!("Starting OIDC flow for provider: {}", provider_slug);
    let browser = super::settings::load_settings(&app_handle)
        .await
        .map(|s| s.passkey_browser)
        .unwrap_or_default();
    let step = super::oidc_delivery::sign_in(
        &state.session,
        server_url.trim_end_matches('/'),
        &provider_slug,
        |url| open_browser_with(&app_handle, url, browser),
        super::oidc_delivery::OIDC_LISTEN_MAX,
    )
    .await?;
    info!("OIDC sign-in needs its confirmation step");
    Ok(step)
}

/// Link an additional external (OIDC) identity to the signed-in account (desktop).
///
/// Mirrors `oidc_authorize`, but: (1) it sends the user's Bearer token to the
/// authenticated link-authorize endpoint, and (2) the localhost callback carries
/// `linked` / `link_error` instead of a code. Returns `Ok(())` on success; an
/// `Err(message)` (e.g. already-linked) is surfaced to the user.
#[command]
pub async fn oidc_link_identity(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    server_url: String,
    provider_slug: String,
) -> Result<(), String> {
    info!(
        "Starting OIDC identity link for provider: {}",
        provider_slug
    );

    let server_url = server_url.trim_end_matches('/');

    // The link-authorize endpoint requires authentication.
    let access_token = {
        let auth = state.auth.read().await;
        auth.access_token.clone()
    }
    .ok_or("Not signed in")?;

    let (listener, flow, redirect_uri) = bind_loopback().await?;
    let authorize_url = authorize_endpoint(
        server_url,
        &["auth", "me", "identities", "authorize", &provider_slug],
        &redirect_uri,
        &[],
    )?;
    let auth_url = provider_url(&authorize_url, &access_token, "Link authorize").await?;

    open_browser(&app_handle, &auth_url)?;
    info!("Opened browser for OIDC link, waiting for callback...");

    let callback = oidc_loopback::wait_for_callback(
        &listener,
        &flow,
        oidc_loopback::CALLBACK_TIMEOUT,
        oidc_loopback::READ_DEADLINE,
        oidc_loopback::parse_link_callback,
        |result| match result {
            LinkCallback::Linked => LINK_OK_PAGE,
            LinkCallback::Error(_) => LINK_FAILED_PAGE,
        },
    )
    .await
    .map_err(|_| "Linking timed out (60s). Please try again.".to_string())?;

    match callback {
        LinkCallback::Linked => {
            info!("OIDC identity linked: {}", provider_slug);
            Ok(())
        }
        // Bare server code (`IDENTITY_ALREADY_LINKED`, ...), mapped by the webview.
        LinkCallback::Error(code) => Err(code),
    }
}
