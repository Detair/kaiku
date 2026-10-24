//! Authentication Commands
//!
//! Thin wrappers around [`crate::session::SessionManager`], which owns the
//! tokens. No command returns the refresh token to the webview.

use std::collections::HashMap;

use serde::Deserialize;
use tauri::{command, State};
use tracing::{debug, info};

use crate::session::{
    AuthInfo, AuthResult, RegisterRequest, SessionError, SessionRestoreResult, TokenResponse,
};
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

/// Start OIDC login flow for Tauri desktop.
///
/// 1. Binds a temporary TCP listener on localhost
/// 2. Requests the authorize URL from the server (with localhost `redirect_uri`)
/// 3. Opens the authorize URL in the default browser
/// 4. Waits for the OIDC callback on the localhost listener
/// 5. Hands the tokens from the callback to the session manager (they never reach the webview)
#[command]
pub async fn oidc_authorize(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    server_url: String,
    provider_slug: String,
) -> Result<AuthResult, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use url::Url;

    info!("Starting OIDC flow for provider: {}", provider_slug);

    let server_url = server_url.trim_end_matches('/');

    // 1. Bind a temporary TCP listener on localhost
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("Failed to bind localhost listener: {e}"))?;

    let local_addr = listener
        .local_addr()
        .map_err(|e| format!("Failed to get local address: {e}"))?;

    let redirect_uri = format!("http://127.0.0.1:{}/callback", local_addr.port());
    debug!("OIDC redirect URI: {}", redirect_uri);

    // 2. Get the authorize URL from the server
    let mut authorize_parsed =
        Url::parse(server_url).map_err(|e| format!("Invalid server URL: {e}"))?;
    // Use Url path segment mutation to safely encode the provider slug
    authorize_parsed
        .path_segments_mut()
        .map_err(|()| "Invalid server URL: cannot be a base")?
        .extend(&["auth", "oidc", "authorize", &provider_slug]);
    authorize_parsed
        .query_pairs_mut()
        .append_pair("redirect_uri", &redirect_uri);
    let authorize_url = authorize_parsed.to_string();

    // The server returns a 302 redirect to the OIDC provider.
    // Use a no-redirect client to capture the Location header.
    let no_redirect_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let redirect_response = no_redirect_client
        .get(&authorize_url)
        .send()
        .await
        .map_err(|e| format!("Failed to get authorize URL: {e}"))?;

    let auth_url = if redirect_response.status().is_redirection() {
        redirect_response
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .ok_or("Server did not return a redirect URL")?
            .to_string()
    } else if redirect_response.status().is_success() {
        // Some implementations return the URL in JSON body
        let body: serde_json::Value = redirect_response
            .json()
            .await
            .map_err(|e| format!("Failed to parse authorize response: {e}"))?;
        body["url"]
            .as_str()
            .ok_or("No authorize URL in response")?
            .to_string()
    } else {
        let status = redirect_response.status();
        let body = redirect_response.text().await.unwrap_or_default();
        return Err(format!("OIDC authorize failed ({status}): {body}"));
    };

    // 3. Open the authorize URL in the default browser
    #[allow(deprecated)] // tauri-plugin-shell::open is deprecated in favor of tauri-plugin-opener
    {
        use tauri_plugin_shell::ShellExt;
        app_handle
            .shell()
            .open(&auth_url, None)
            .map_err(|e| format!("Failed to open browser: {e}"))?;
    }

    info!("Opened browser for OIDC login, waiting for callback...");

    // 4. Wait for the OIDC callback (with timeout)
    // Accept in a loop to handle favicon requests and other non-callback connections
    let accept_future = async {
        loop {
            let (mut stream, _addr) = listener.accept().await?;

            // Read the HTTP request
            let mut buf = vec![0u8; 8192];
            let n = stream.read(&mut buf).await?;
            let request = String::from_utf8_lossy(&buf[..n]);

            // Extract the request path from the first line (GET /callback?... HTTP/1.1)
            let first_line = request.lines().next().unwrap_or("");
            let path = first_line.split_whitespace().nth(1).unwrap_or("/");

            // Check if this is the actual callback (not favicon, preflight, etc.)
            if !path.starts_with("/callback") {
                let response = "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n";
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.flush().await;
                continue;
            }

            // Parse query parameters
            let full_url = format!("http://localhost{path}");
            let parsed = Url::parse(&full_url)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;

            let params: HashMap<String, String> = parsed
                .query_pairs()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();

            // Send a response to the browser
            let html = if params.contains_key("access_token") {
                "<html><body><h2>Login successful!</h2><p>You can close this window and return to the app.</p><script>window.close()</script></body></html>"
            } else if params.contains_key("error") {
                "<html><body><h2>Login failed</h2><p>An error occurred. Please try again.</p></body></html>"
            } else {
                "<html><body><h2>Processing...</h2></body></html>"
            };

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                html.len(),
                html
            );
            stream.write_all(response.as_bytes()).await?;
            stream.flush().await?;

            return Ok::<HashMap<String, String>, std::io::Error>(params);
        }
    };

    // 60 second timeout for the user to complete the OIDC flow
    let params = tokio::time::timeout(std::time::Duration::from_mins(1), accept_future)
        .await
        .map_err(|_| "OIDC login timed out (60s). Please try again.".to_string())?
        .map_err(|e| format!("Failed to receive OIDC callback: {e}"))?;

    // 5. Extract tokens from callback params
    if let Some(error) = params.get("error") {
        let desc = params
            .get("error_description")
            .map(|s| s.as_str())
            .unwrap_or("Unknown error");
        return Err(format!("OIDC login failed: {error} — {desc}"));
    }

    let token = |key: &str| {
        params
            .get(key)
            .cloned()
            .ok_or_else(|| format!("No {key} in OIDC callback"))
    };
    let flag = |key: &str| {
        params
            .get(key)
            .and_then(|s| s.parse().ok())
            .unwrap_or(false)
    };
    let tokens = TokenResponse {
        access_token: token("access_token")?,
        refresh_token: token("refresh_token")?,
        expires_in: params
            .get("expires_in")
            .and_then(|s| s.parse().ok())
            .unwrap_or(900),
        setup_required: flag("setup_required"),
        password_change_required: flag("password_change_required"),
    };

    let result = state
        .session
        .adopt_tokens(server_url, tokens)
        .await
        .map_err(|e| e.to_string())?;
    info!("OIDC login successful");
    Ok(result)
}

/// Link an additional external (OIDC) identity to the signed-in account (desktop).
///
/// Mirrors `oidc_authorize`, but: (1) it sends the user's Bearer token to the
/// authenticated link-authorize endpoint, and (2) the localhost callback carries
/// `linked` / `link_error` instead of tokens. Returns `Ok(())` on success; an
/// `Err(message)` (e.g. already-linked) is surfaced to the user.
#[command]
pub async fn oidc_link_identity(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    server_url: String,
    provider_slug: String,
) -> Result<(), String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use url::Url;

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

    // 1. Bind a temporary TCP listener on localhost for the callback
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("Failed to bind localhost listener: {e}"))?;
    let local_addr = listener
        .local_addr()
        .map_err(|e| format!("Failed to get local address: {e}"))?;
    let redirect_uri = format!("http://127.0.0.1:{}/callback", local_addr.port());

    // 2. Ask the server (authenticated) for the provider authorization URL. The server replies with
    //    a 302 to the provider; capture the Location.
    let mut authorize_parsed =
        Url::parse(server_url).map_err(|e| format!("Invalid server URL: {e}"))?;
    authorize_parsed
        .path_segments_mut()
        .map_err(|()| "Invalid server URL: cannot be a base")?
        .extend(&["auth", "me", "identities", "authorize", &provider_slug]);
    authorize_parsed
        .query_pairs_mut()
        .append_pair("redirect_uri", &redirect_uri);
    let authorize_url = authorize_parsed.to_string();

    let no_redirect_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let redirect_response = no_redirect_client
        .get(&authorize_url)
        .header("Authorization", format!("Bearer {access_token}"))
        .send()
        .await
        .map_err(|e| format!("Failed to get authorize URL: {e}"))?;

    let auth_url = if redirect_response.status().is_redirection() {
        redirect_response
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .ok_or("Server did not return a redirect URL")?
            .to_string()
    } else {
        let status = redirect_response.status();
        let body = redirect_response.text().await.unwrap_or_default();
        return Err(format!("Link authorize failed ({status}): {body}"));
    };

    // 3. Open the authorize URL in the default browser
    #[allow(deprecated)] // tauri-plugin-shell::open is deprecated in favor of tauri-plugin-opener
    {
        use tauri_plugin_shell::ShellExt;
        app_handle
            .shell()
            .open(&auth_url, None)
            .map_err(|e| format!("Failed to open browser: {e}"))?;
    }

    info!("Opened browser for OIDC link, waiting for callback...");

    // 4. Wait for the callback (with timeout), ignoring favicon/other requests
    let accept_future = async {
        loop {
            let (mut stream, _addr) = listener.accept().await?;
            let mut buf = vec![0u8; 8192];
            let n = stream.read(&mut buf).await?;
            let request = String::from_utf8_lossy(&buf[..n]);
            let first_line = request.lines().next().unwrap_or("");
            let path = first_line.split_whitespace().nth(1).unwrap_or("/");
            if !path.starts_with("/callback") {
                let response = "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n";
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.flush().await;
                continue;
            }
            let full_url = format!("http://localhost{path}");
            let parsed = Url::parse(&full_url)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
            let params: HashMap<String, String> = parsed
                .query_pairs()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            let html = if params.contains_key("linked") {
                "<html><body><h2>Account linked!</h2><p>You can close this window and return to the app.</p><script>window.close()</script></body></html>"
            } else {
                "<html><body><h2>Linking failed</h2><p>You can close this window and return to the app.</p></body></html>"
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                html.len(),
                html
            );
            stream.write_all(response.as_bytes()).await?;
            stream.flush().await?;
            return Ok::<HashMap<String, String>, std::io::Error>(params);
        }
    };

    let params = tokio::time::timeout(std::time::Duration::from_mins(1), accept_future)
        .await
        .map_err(|_| "Linking timed out (60s). Please try again.".to_string())?
        .map_err(|e| format!("Failed to receive link callback: {e}"))?;

    // 5. Interpret the callback result
    if params.contains_key("linked") {
        info!("OIDC identity linked: {}", provider_slug);
        Ok(())
    } else if let Some(code) = params.get("link_error") {
        match code.as_str() {
            "IDENTITY_ALREADY_LINKED" => {
                Err("This account is already linked to another Kaiku account.".to_string())
            }
            other => Err(format!("Linking failed: {other}")),
        }
    } else if let Some(error) = params.get("error") {
        Err(format!("Linking failed: {error}"))
    } else {
        Err("Linking failed: no result in callback".to_string())
    }
}
