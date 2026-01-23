//! Preferences Sync Commands
//!
//! Tauri commands for syncing user preferences with the server.

use tauri::{command, State};
use tracing::{debug, error};

use crate::AppState;

/// Fetch user preferences from the server.
///
/// Returns the user's synced preferences as JSON.
#[command]
pub async fn fetch_preferences(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Fetching preferences from server");

    let response = state
        .http
        .get(format!("{server_url}/api/me/preferences"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to fetch preferences: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to fetch preferences: {}", status);
        return Err(format!("Failed to fetch preferences: {status}"));
    }

    let preferences: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Preferences fetched successfully");
    Ok(preferences)
}

/// Update user preferences on the server.
///
/// Sends the provided preferences to the server for storage.
#[command]
pub async fn update_preferences(
    state: State<'_, AppState>,
    preferences: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Updating preferences on server");

    let response = state
        .http
        .put(format!("{server_url}/api/me/preferences"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "preferences": preferences }))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to update preferences: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to update preferences: {} - {}", status, body);
        return Err(format!("Failed to update preferences: {status}"));
    }

    let result: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Preferences updated successfully");
    Ok(result)
}
