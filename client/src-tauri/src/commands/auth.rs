//! Authentication Commands

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
        User {
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
        .post(format!("{}/auth/login", server_url))
        .json(&serde_json::json!({
            "username": request.username,
            "password": request.password
        }))
        .send()
        .await
        .map_err(|e| {
            error!("Login request failed: {}", e);
            format!("Connection failed: {}", e)
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Login failed with status {}: {}", status, body);
        return Err(if status.as_u16() == 401 {
            "Invalid username or password".to_string()
        } else {
            format!("Login failed: {}", status)
        });
    }

    let tokens: TokenResponse = response.json().await.map_err(|e| {
        error!("Failed to parse token response: {}", e);
        format!("Invalid response from server: {}", e)
    })?;

    debug!("Login successful, fetching user info");

    // Fetch user info with the new token
    let user_response = state
        .http
        .get(format!("{}/auth/me", server_url))
        .header("Authorization", format!("Bearer {}", tokens.access_token))
        .send()
        .await
        .map_err(|e| format!("Failed to fetch user info: {}", e))?;

    if !user_response.status().is_success() {
        return Err("Failed to fetch user info".to_string());
    }

    let user_data: UserResponse = user_response
        .json()
        .await
        .map_err(|e| format!("Invalid user response: {}", e))?;

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
        .post(format!("{}/auth/register", server_url))
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
            format!("Connection failed: {}", e)
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
            format!("Registration failed: {}", status)
        });
    }

    let tokens: TokenResponse = response.json().await.map_err(|e| {
        error!("Failed to parse token response: {}", e);
        format!("Invalid response from server: {}", e)
    })?;

    debug!("Registration successful, fetching user info");

    // Fetch user info with the new token
    let user_response = state
        .http
        .get(format!("{}/auth/me", server_url))
        .header("Authorization", format!("Bearer {}", tokens.access_token))
        .send()
        .await
        .map_err(|e| format!("Failed to fetch user info: {}", e))?;

    if !user_response.status().is_success() {
        return Err("Failed to fetch user info".to_string());
    }

    let user_data: UserResponse = user_response
        .json()
        .await
        .map_err(|e| format!("Invalid user response: {}", e))?;

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
            .post(format!("{}/auth/logout", url))
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

// Keyring helpers

const KEYRING_SERVICE: &str = "voicechat";

fn keyring_user(server_url: &str) -> String {
    // Use server URL as keyring username to support multiple servers
    format!("refresh_token:{}", server_url)
}

fn store_refresh_token(server_url: &str, token: &str) -> Result<(), keyring::Error> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &keyring_user(server_url))?;
    entry.set_password(token)
}

fn get_refresh_token(server_url: &str) -> Result<String, keyring::Error> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &keyring_user(server_url))?;
    entry.get_password()
}

fn clear_refresh_token(server_url: &str) -> Result<(), keyring::Error> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &keyring_user(server_url))?;
    entry.delete_credential()
}
