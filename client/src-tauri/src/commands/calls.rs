//! DM Call Commands
//!
//! Tauri commands for DM voice call signaling: start, join, decline, leave.
//! These commands handle the call lifecycle via HTTP, while voice.rs handles
//! the actual WebRTC connection.

use serde::{Deserialize, Serialize};
use tauri::{command, State};
use tracing::{debug, error, info};

use crate::AppState;

// ============================================================================
// Types
// ============================================================================

/// Call capabilities (for future video/screen share support).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallCapabilities {
    pub audio: bool,
    pub video: bool,
    pub screenshare: bool,
}

/// Call state response from server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallStateResponse {
    pub channel_id: String,
    #[serde(flatten)]
    pub state: CallStateInfo,
    pub capabilities: Option<Vec<String>>,
}

/// Call state information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CallStateInfo {
    Ringing {
        started_by: String,
        started_at: String,
        declined_by: Vec<String>,
        target_users: Vec<String>,
    },
    Active {
        started_at: String,
        participants: Vec<String>,
    },
    Ended {
        reason: String,
        duration_secs: Option<u32>,
        ended_at: String,
    },
}

// ============================================================================
// Call Commands
// ============================================================================

/// Start a voice call in a DM channel.
///
/// The initiator starts the call and joins immediately.
/// Other participants receive an incoming call notification.
#[command]
pub async fn start_dm_call(
    channel_id: String,
    state: State<'_, AppState>,
) -> Result<CallStateResponse, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    info!("Starting call in DM: {}", channel_id);

    let response = state
        .http
        .post(format!("{server_url}/api/dm/{channel_id}/call/start"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to start call: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to start call: {} - {}", status, body);
        return Err(format!("Failed to start call: {status}"));
    }

    let call_state: CallStateResponse = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Call started: {:?}", call_state);
    Ok(call_state)
}

/// Join an active call in a DM channel.
///
/// Called when accepting an incoming call.
#[command]
pub async fn join_dm_call(
    channel_id: String,
    state: State<'_, AppState>,
) -> Result<CallStateResponse, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    info!("Joining call in DM: {}", channel_id);

    let response = state
        .http
        .post(format!("{server_url}/api/dm/{channel_id}/call/join"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to join call: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to join call: {} - {}", status, body);
        return Err(format!("Failed to join call: {status}"));
    }

    let call_state: CallStateResponse = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Joined call: {:?}", call_state);
    Ok(call_state)
}

/// Decline an incoming call in a DM channel.
///
/// The user won't join the call and others will be notified.
#[command]
pub async fn decline_dm_call(
    channel_id: String,
    state: State<'_, AppState>,
) -> Result<CallStateResponse, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    info!("Declining call in DM: {}", channel_id);

    let response = state
        .http
        .post(format!("{server_url}/api/dm/{channel_id}/call/decline"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to decline call: {}", e);
            format!("Connection failed: {e}")
        })?;

    // 404 means call already ended, which is fine
    if response.status().as_u16() == 404 {
        return Ok(CallStateResponse {
            channel_id,
            state: CallStateInfo::Ended {
                reason: "no_answer".to_string(),
                duration_secs: None,
                ended_at: chrono::Utc::now().to_rfc3339(),
            },
            capabilities: None,
        });
    }

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to decline call: {} - {}", status, body);
        return Err(format!("Failed to decline call: {status}"));
    }

    let call_state: CallStateResponse = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Declined call: {:?}", call_state);
    Ok(call_state)
}

/// Leave an active call in a DM channel.
///
/// If this is the last participant, the call ends.
#[command]
pub async fn leave_dm_call(
    channel_id: String,
    state: State<'_, AppState>,
) -> Result<CallStateResponse, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    info!("Leaving call in DM: {}", channel_id);

    let response = state
        .http
        .post(format!("{server_url}/api/dm/{channel_id}/call/leave"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to leave call: {}", e);
            format!("Connection failed: {e}")
        })?;

    // 404 means call already ended, which is fine
    if response.status().as_u16() == 404 {
        return Ok(CallStateResponse {
            channel_id,
            state: CallStateInfo::Ended {
                reason: "last_left".to_string(),
                duration_secs: None,
                ended_at: chrono::Utc::now().to_rfc3339(),
            },
            capabilities: None,
        });
    }

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to leave call: {} - {}", status, body);
        return Err(format!("Failed to leave call: {status}"));
    }

    let call_state: CallStateResponse = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Left call: {:?}", call_state);
    Ok(call_state)
}

/// Get current call state for a DM channel.
#[command]
pub async fn get_dm_call(
    channel_id: String,
    state: State<'_, AppState>,
) -> Result<Option<CallStateResponse>, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Getting call state for DM: {}", channel_id);

    let response = state
        .http
        .get(format!("{server_url}/api/dm/{channel_id}/call"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to get call state: {}", e);
            format!("Connection failed: {e}")
        })?;

    // 404 means no active call
    if response.status().as_u16() == 404 {
        return Ok(None);
    }

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to get call state: {} - {}", status, body);
        return Err(format!("Failed to get call state: {status}"));
    }

    let call_state: CallStateResponse = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    Ok(Some(call_state))
}
