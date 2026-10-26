//! Authentication Commands
//!
//! Thin wrappers around [`crate::session::SessionManager`], which owns the
//! tokens. No command returns the refresh token to the webview.

use std::time::Duration;

use serde::Deserialize;
use tauri::{command, State};
use tokio::net::TcpListener;
use tracing::{debug, info};
use url::Url;

use super::oidc_loopback::{self, LinkCallback, LoginCallback};
use crate::session::{AuthInfo, AuthResult, RegisterRequest, SessionError, SessionRestoreResult};
use crate::AppState;

/// Login request from frontend.
#[derive(Deserialize)]
pub struct LoginRequest {
    pub server_url: String,
    pub username: String,
    pub password: String,
    pub mfa_code: Option<String>,
}

impl std::fmt::Debug for LoginRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginRequest")
            .field("server_url", &self.server_url)
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .field("mfa_code", &self.mfa_code.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

/// Login with username and password.
#[command]
pub async fn login(
    state: State<'_, AppState>,
    request: LoginRequest,
) -> Result<AuthResult, String> {
    info!("Attempting login for user: {}", request.username);
    state
        .session
        .login(
            &request.server_url,
            &request.username,
            &request.password,
            request.mfa_code,
        )
        .await
        .map_err(|e| e.to_string())
}

/// Register a new user.
#[command]
pub async fn register(
    state: State<'_, AppState>,
    request: RegisterRequest,
) -> Result<AuthResult, String> {
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

/// How an SSO start authenticates to the server.
#[derive(Clone, Copy)]
enum Start<'a> {
    /// Public desktop sign-in: `X-Kaiku-Client: desktop`, JSON `{"url"}` only.
    DesktopLogin,
    /// Signed-in start (identity link) with a bearer token.
    Bearer(&'a str),
}

/// Header the server requires on desktop sign-in starts.
const DESKTOP_CLIENT_HEADER: (&str, &str) = ("x-kaiku-client", "desktop");

/// Ask the server for the provider URL. A desktop sign-in gets `{"url"}`; a
/// bearer start may also answer with a redirect we don't follow. A refused
/// start that names a server code (e.g. `OIDC_CLIENT_OUTDATED`) fails with
/// that bare code, so the webview can show a matching message.
async fn provider_url(authorize_url: &Url, start: Start<'_>, what: &str) -> Result<String, String> {
    let no_redirect_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;
    let mut request = no_redirect_client.get(authorize_url.as_str());
    request = match start {
        Start::DesktopLogin => request.header(DESKTOP_CLIENT_HEADER.0, DESKTOP_CLIENT_HEADER.1),
        Start::Bearer(token) => request.bearer_auth(token),
    };
    let response = request
        .send()
        .await
        .map_err(|e| format!("Failed to get authorize URL: {e}"))?;

    if response.status().is_redirection() && matches!(start, Start::Bearer(_)) {
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
    #[allow(deprecated)] // tauri-plugin-shell::open is deprecated in favor of tauri-plugin-opener
    {
        use tauri_plugin_shell::ShellExt;
        app_handle
            .shell()
            .open(url, None)
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

const LOGIN_OK_PAGE: &str = "<html><body><h2>Signed in</h2><p>You can close this window and return to the app.</p></body></html>";
const LOGIN_FAILED_PAGE: &str =
    "<html><body><h2>Login failed</h2><p>An error occurred. Please try again.</p></body></html>";
const LINK_OK_PAGE: &str = "<html><body><h2>Account linked!</h2><p>You can close this window and return to the app.</p></body></html>";
const LINK_FAILED_PAGE: &str = "<html><body><h2>Linking failed</h2><p>You can close this window and return to the app.</p></body></html>";

/// Start OIDC login flow for Tauri desktop.
///
/// 1. Binds a loopback listener and creates a per-flow value and a verifier
/// 2. Requests the provider URL from the server with the loopback return address (carrying the
///    per-flow value) and the verifier's S256 challenge
/// 3. Opens the provider URL in the default browser
/// 4. Waits for the server's hand-back on the loopback: only a one-time code, accepted only on
///    `/callback` with the per-flow value
/// 5. Redeems the code with the verifier; the tokens go to the session manager (they never reach
///    the webview or the browser)
#[command]
pub async fn oidc_authorize(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    server_url: String,
    provider_slug: String,
) -> Result<AuthResult, String> {
    info!("Starting OIDC flow for provider: {}", provider_slug);

    let server_url = server_url.trim_end_matches('/');
    let (listener, flow, redirect_uri) = bind_loopback().await?;
    let verifier = oidc_loopback::new_secret();
    let challenge = oidc_loopback::s256_challenge(&verifier);
    debug!("OIDC loopback listener bound");

    let authorize_url = authorize_endpoint(
        server_url,
        &["auth", "oidc", "authorize", &provider_slug],
        &redirect_uri,
        &[("client_challenge", &challenge)],
    )?;
    let auth_url = provider_url(&authorize_url, Start::DesktopLogin, "OIDC authorize").await?;

    open_browser(&app_handle, &auth_url)?;
    info!("Opened browser for OIDC login, waiting for callback...");

    let callback = oidc_loopback::wait_for_callback(
        &listener,
        &flow,
        oidc_loopback::CALLBACK_TIMEOUT,
        oidc_loopback::READ_DEADLINE,
        oidc_loopback::parse_login_callback,
        |result| match result {
            LoginCallback::Code(_) => LOGIN_OK_PAGE,
            LoginCallback::Error(_) => LOGIN_FAILED_PAGE,
        },
    )
    .await
    .map_err(|_| "OIDC login timed out (60s). Please try again.".to_string())?;
    drop(listener);

    let code = match callback {
        LoginCallback::Code(code) => code,
        // Bare server code (`OIDC_PROVIDER_ERROR`, ...), mapped by the webview.
        LoginCallback::Error(error) => return Err(error),
    };

    let result = state
        .session
        .exchange_oidc_code(server_url, &code, &verifier)
        .await
        .map_err(|e| e.to_string())?;
    info!("OIDC login successful");
    Ok(result)
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
    let auth_url = provider_url(
        &authorize_url,
        Start::Bearer(&access_token),
        "Link authorize",
    )
    .await?;

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

#[cfg(test)]
mod tests {
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    #[tokio::test]
    async fn desktop_start_sends_the_client_header_and_reads_json() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/auth/oidc/authorize/g"))
            .and(header("x-kaiku-client", "desktop"))
            .and(query_param("client_challenge", "ch"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"url": "https://idp/auth"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let url = authorize_endpoint(
            &server.uri(),
            &["auth", "oidc", "authorize", "g"],
            "http://127.0.0.1:1/callback?flow=f",
            &[("client_challenge", "ch")],
        )
        .unwrap();
        assert_eq!(
            provider_url(&url, Start::DesktopLogin, "OIDC authorize").await,
            Ok("https://idp/auth".to_string())
        );
    }

    #[tokio::test]
    async fn desktop_start_does_not_follow_or_accept_a_redirect() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(307).insert_header("location", "https://idp/auth"))
            .mount(&server)
            .await;
        let url = Url::parse(&format!("{}/auth/oidc/authorize/g", server.uri())).unwrap();
        assert!(provider_url(&url, Start::DesktopLogin, "OIDC authorize")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn refused_start_reports_the_bare_server_code() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "OIDC_CLIENT_OUTDATED",
                "message": "Update the app to sign in with this provider"
            })))
            .mount(&server)
            .await;
        let url = Url::parse(&format!("{}/auth/oidc/authorize/g", server.uri())).unwrap();
        assert_eq!(
            provider_url(&url, Start::DesktopLogin, "OIDC authorize").await,
            Err("OIDC_CLIENT_OUTDATED".to_string())
        );
    }
}
