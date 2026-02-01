//! Authentication Commands

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tauri::{command, State};
use tracing::{debug, error, info};

use crate::{AppState, User, UserStatus};

/// Login request from frontend.
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub server_url: String,
    pub username: String,
    pub password: String,
}

/// Register request from frontend.
#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub server_url: String,
    pub username: String,
    pub email: Option<String>,
    pub password: String,
    pub display_name: Option<String>,
}

/// Token response from server.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
    token_type: String,
}

/// User response from server /auth/me endpoint.
#[derive(Debug, Deserialize)]
struct UserResponse {
    id: String,
    username: String,
    display_name: String,
    email: Option<String>,
    avatar_url: Option<String>,
    status: String,
    mfa_enabled: bool,
}

impl From<UserResponse> for User {
    fn from(r: UserResponse) -> Self {
        Self {
            id: r.id,
            username: r.username,
            display_name: r.display_name,
            avatar_url: r.avatar_url,
            email: r.email,
            mfa_enabled: r.mfa_enabled,
            status: match r.status.as_str() {
                "online" => UserStatus::Online,
                "away" => UserStatus::Away,
                "busy" => UserStatus::Busy,
                _ => UserStatus::Offline,
            },
        }
    }
}

/// Login with username and password.
#[command]
pub async fn login(state: State<'_, AppState>, request: LoginRequest) -> Result<User, String> {
    info!("Attempting login for user: {}", request.username);

    let server_url = request.server_url.trim_end_matches('/');

    // Send login request to server
    let response = state
        .http
        .post(format!("{server_url}/auth/login"))
        .json(&serde_json::json!({
            "username": request.username,
            "password": request.password
        }))
        .send()
        .await
        .map_err(|e| {
            error!("Login request failed: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Login failed with status {}: {}", status, body);
        return Err(if status.as_u16() == 401 {
            "Invalid username or password".to_string()
        } else {
            format!("Login failed: {status}")
        });
    }

    let tokens: TokenResponse = response.json().await.map_err(|e| {
        error!("Failed to parse token response: {}", e);
        format!("Invalid response from server: {e}")
    })?;

    debug!("Login successful, fetching user info");

    // Fetch user info with the new token
    let user_response = state
        .http
        .get(format!("{server_url}/auth/me"))
        .header("Authorization", format!("Bearer {}", tokens.access_token))
        .send()
        .await
        .map_err(|e| format!("Failed to fetch user info: {e}"))?;

    if !user_response.status().is_success() {
        return Err("Failed to fetch user info".to_string());
    }

    let user_data: UserResponse = user_response
        .json()
        .await
        .map_err(|e| format!("Invalid user response: {e}"))?;

    let user: User = user_data.into();

    // Store auth state
    {
        let mut auth = state.auth.write().await;
        auth.access_token = Some(tokens.access_token.clone());
        auth.refresh_token = Some(tokens.refresh_token.clone());
        auth.server_url = Some(server_url.to_string());
        auth.user = Some(user.clone());
    }

    // Store refresh token securely in keyring
    if let Err(e) = store_refresh_token(server_url, &tokens.refresh_token) {
        error!("Failed to store refresh token: {}", e);
        // Continue anyway - user is still logged in for this session
    }

    info!("User {} logged in successfully", user.username);
    Ok(user)
}

/// Register a new user.
#[command]
pub async fn register(
    state: State<'_, AppState>,
    request: RegisterRequest,
) -> Result<User, String> {
    info!("Attempting registration for user: {}", request.username);

    let server_url = request.server_url.trim_end_matches('/');

    // Send register request to server
    let response = state
        .http
        .post(format!("{server_url}/auth/register"))
        .json(&serde_json::json!({
            "username": request.username,
            "email": request.email,
            "password": request.password,
            "display_name": request.display_name
        }))
        .send()
        .await
        .map_err(|e| {
            error!("Registration request failed: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Registration failed with status {}: {}", status, body);
        return Err(if status.as_u16() == 409 {
            "Username or email already exists".to_string()
        } else if status.as_u16() == 400 {
            // Try to extract validation error
            if let Ok(error) = serde_json::from_str::<serde_json::Value>(&body) {
                error["error"]
                    .as_str()
                    .unwrap_or("Invalid input")
                    .to_string()
            } else {
                "Invalid input".to_string()
            }
        } else {
            format!("Registration failed: {status}")
        });
    }

    let tokens: TokenResponse = response.json().await.map_err(|e| {
        error!("Failed to parse token response: {}", e);
        format!("Invalid response from server: {e}")
    })?;

    debug!("Registration successful, fetching user info");

    // Fetch user info with the new token
    let user_response = state
        .http
        .get(format!("{server_url}/auth/me"))
        .header("Authorization", format!("Bearer {}", tokens.access_token))
        .send()
        .await
        .map_err(|e| format!("Failed to fetch user info: {e}"))?;

    if !user_response.status().is_success() {
        return Err("Failed to fetch user info".to_string());
    }

    let user_data: UserResponse = user_response
        .json()
        .await
        .map_err(|e| format!("Invalid user response: {e}"))?;

    let user: User = user_data.into();

    // Store auth state
    {
        let mut auth = state.auth.write().await;
        auth.access_token = Some(tokens.access_token.clone());
        auth.refresh_token = Some(tokens.refresh_token.clone());
        auth.server_url = Some(server_url.to_string());
        auth.user = Some(user.clone());
    }

    // Store refresh token securely
    if let Err(e) = store_refresh_token(server_url, &tokens.refresh_token) {
        error!("Failed to store refresh token: {}", e);
    }

    info!("User {} registered successfully", user.username);
    Ok(user)
}

/// Logout and clear credentials.
#[command]
pub async fn logout(state: State<'_, AppState>) -> Result<(), String> {
    info!("Logging out");

    let (server_url, refresh_token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.refresh_token.clone())
    };

    // Try to invalidate token on server (best effort)
    if let (Some(url), Some(token)) = (server_url.as_ref(), refresh_token.as_ref()) {
        let _ = state
            .http
            .post(format!("{url}/auth/logout"))
            .json(&serde_json::json!({ "refresh_token": token }))
            .send()
            .await;
    }

    // Clear auth state
    {
        let mut auth = state.auth.write().await;
        auth.access_token = None;
        auth.refresh_token = None;
        auth.user = None;
        // Keep server_url for potential re-login
    }

    // Clear stored credentials
    if let Some(url) = server_url {
        let _ = clear_refresh_token(&url);
    }

    info!("Logged out successfully");
    Ok(())
}

/// Get the current authenticated user.
#[command]
pub async fn get_current_user(state: State<'_, AppState>) -> Result<Option<User>, String> {
    // Check if we have a user in memory
    {
        let auth = state.auth.read().await;
        if auth.user.is_some() {
            return Ok(auth.user.clone());
        }
    }

    // Try to restore session from stored credentials
    // For now, return None - session restoration will be implemented with keyring
    Ok(None)
}

/// OIDC login response returned to the frontend.
#[derive(Debug, Serialize)]
pub struct OidcLoginResult {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
}

/// Start OIDC login flow for Tauri desktop.
///
/// 1. Binds a temporary TCP listener on localhost
/// 2. Requests the authorize URL from the server (with localhost redirect_uri)
/// 3. Opens the authorize URL in the default browser
/// 4. Waits for the OIDC callback on the localhost listener
/// 5. Extracts tokens from the callback query params and returns them
#[command]
pub async fn oidc_authorize(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    server_url: String,
    provider_slug: String,
) -> Result<OidcLoginResult, String> {
    use tokio::net::TcpListener;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
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
    let mut authorize_parsed = Url::parse(server_url)
        .map_err(|e| format!("Invalid server URL: {e}"))?;
    // Use Url path segment mutation to safely encode the provider slug
    authorize_parsed
        .path_segments_mut()
        .map_err(|_| "Invalid server URL: cannot be a base")?
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
            let path = first_line
                .split_whitespace()
                .nth(1)
                .unwrap_or("/");

            // Check if this is the actual callback (not favicon, preflight, etc.)
            if !path.starts_with("/callback") {
                let response = "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n";
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.flush().await;
                continue;
            }

            // Parse query parameters
            let full_url = format!("http://localhost{path}");
            let parsed = Url::parse(&full_url).map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
            })?;

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
    let params = tokio::time::timeout(std::time::Duration::from_secs(60), accept_future)
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

    let access_token = params
        .get("access_token")
        .ok_or("No access_token in OIDC callback")?
        .clone();
    let refresh_token = params
        .get("refresh_token")
        .ok_or("No refresh_token in OIDC callback")?
        .clone();
    let expires_in: u64 = params
        .get("expires_in")
        .and_then(|s| s.parse().ok())
        .unwrap_or(900);

    // Fetch user info with the new token
    let user_response = state
        .http
        .get(format!("{server_url}/auth/me"))
        .header("Authorization", format!("Bearer {access_token}"))
        .send()
        .await
        .map_err(|e| format!("Failed to fetch user info: {e}"))?;

    if !user_response.status().is_success() {
        return Err("Failed to fetch user info after OIDC login".to_string());
    }

    let user_data: UserResponse = user_response
        .json()
        .await
        .map_err(|e| format!("Invalid user response: {e}"))?;

    let user: User = user_data.into();

    // Store auth state
    {
        let mut auth = state.auth.write().await;
        auth.access_token = Some(access_token.clone());
        auth.refresh_token = Some(refresh_token.clone());
        auth.server_url = Some(server_url.to_string());
        auth.user = Some(user.clone());
    }

    // Store refresh token securely
    if let Err(e) = store_refresh_token(server_url, &refresh_token) {
        error!("Failed to store OIDC refresh token: {}", e);
    }

    info!("OIDC login successful for user: {}", user.username);
    Ok(OidcLoginResult {
        access_token,
        refresh_token,
        expires_in,
    })
}

// Keyring helpers

const KEYRING_SERVICE: &str = "voicechat";

fn keyring_user(server_url: &str) -> String {
    // Use server URL as keyring username to support multiple servers
    format!("refresh_token:{server_url}")
}

fn store_refresh_token(server_url: &str, token: &str) -> Result<(), keyring::Error> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &keyring_user(server_url))?;
    entry.set_password(token)
}

#[allow(dead_code)]
fn get_refresh_token(server_url: &str) -> Result<String, keyring::Error> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &keyring_user(server_url))?;
    entry.get_password()
}

fn clear_refresh_token(server_url: &str) -> Result<(), keyring::Error> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &keyring_user(server_url))?;
    entry.delete_password()
}
