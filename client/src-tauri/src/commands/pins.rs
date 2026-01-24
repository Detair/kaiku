//! Pins Tauri Commands
//!
//! CRUD operations for user pins.

use serde::{Deserialize, Serialize};
use tauri::{command, State};
use tracing::{debug, error};

use crate::AppState;

/// A pin item.
#[derive(Debug, Serialize, Deserialize)]
pub struct Pin {
    pub id: String,
    pub pin_type: String,
    pub content: String,
    pub title: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: String,
    pub position: i32,
}

/// Request to create a new pin.
#[derive(Debug, Serialize, Deserialize)]
pub struct CreatePinRequest {
    pub pin_type: String,
    pub content: String,
    pub title: Option<String>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

/// Request to update an existing pin.
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdatePinRequest {
    pub content: Option<String>,
    pub title: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

/// Fetch all pins for the current user.
#[command]
pub async fn fetch_pins(state: State<'_, AppState>) -> Result<Vec<Pin>, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Fetching pins from server");

    let response = state
        .http
        .get(format!("{server_url}/api/me/pins"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to fetch pins: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to fetch pins: {}", status);
        return Err(format!("Failed to fetch pins: {status}"));
    }

    let pins: Vec<Pin> = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Fetched {} pins", pins.len());
    Ok(pins)
}

/// Create a new pin.
#[command]
pub async fn create_pin(
    state: State<'_, AppState>,
    request: CreatePinRequest,
) -> Result<Pin, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Creating pin: type={}", request.pin_type);

    let response = state
        .http
        .post(format!("{server_url}/api/me/pins"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&request)
        .send()
        .await
        .map_err(|e| {
            error!("Failed to create pin: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to create pin: {} - {}", status, body);
        return Err(format!("Failed to create pin: {status}"));
    }

    let pin: Pin = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Pin created: id={}", pin.id);
    Ok(pin)
}

/// Update an existing pin.
#[command]
pub async fn update_pin(
    state: State<'_, AppState>,
    pin_id: String,
    request: UpdatePinRequest,
) -> Result<Pin, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Updating pin: id={}", pin_id);

    let response = state
        .http
        .put(format!("{server_url}/api/me/pins/{pin_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&request)
        .send()
        .await
        .map_err(|e| {
            error!("Failed to update pin: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to update pin: {} - {}", status, body);
        return Err(format!("Failed to update pin: {status}"));
    }

    let pin: Pin = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Pin updated: id={}", pin.id);
    Ok(pin)
}

/// Delete a pin.
#[command]
pub async fn delete_pin(state: State<'_, AppState>, pin_id: String) -> Result<(), String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Deleting pin: id={}", pin_id);

    let response = state
        .http
        .delete(format!("{server_url}/api/me/pins/{pin_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to delete pin: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to delete pin: {}", status);
        return Err(format!("Failed to delete pin: {status}"));
    }

    debug!("Pin deleted: id={}", pin_id);
    Ok(())
}

/// Reorder pins by providing the new order of pin IDs.
#[command]
pub async fn reorder_pins(state: State<'_, AppState>, pin_ids: Vec<String>) -> Result<(), String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Reordering {} pins", pin_ids.len());

    let response = state
        .http
        .put(format!("{server_url}/api/me/pins/reorder"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "pin_ids": pin_ids }))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to reorder pins: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to reorder pins: {}", status);
        return Err(format!("Failed to reorder pins: {status}"));
    }

    debug!("Pins reordered successfully");
    Ok(())
}
