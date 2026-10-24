//! WebSocket Commands
//!
//! Commands for managing the WebSocket connection.

use tauri::{command, AppHandle, State};
use tracing::{debug, info};

use crate::network::{ClientEvent, ConnectionStatus, WebSocketManager, WsTimings};
use crate::AppState;

/// Connect to the WebSocket server with the current session.
///
/// Resolves once the first attempt has an outcome. On failure the Rust loop
/// keeps retrying (`ws:reconnecting` … `ws:connected`) and the error only
/// reports that the first attempt did not succeed.
#[command]
pub async fn ws_connect(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let manager = WebSocketManager::connect(app, state.session.clone(), WsTimings::default())?;
    info!("WebSocket connection loop started");

    let first = manager.first_outcome();
    let mut ws = state.websocket.write().await;
    if let Some(old) = ws.replace(manager) {
        old.disconnect();
    }
    drop(ws);
    first.await
}

/// Disconnect from the WebSocket server.
#[command]
pub async fn ws_disconnect(state: State<'_, AppState>) -> Result<(), String> {
    let mut ws = state.websocket.write().await;
    if let Some(manager) = ws.as_ref() {
        manager.disconnect();
    }
    *ws = None;
    Ok(())
}

/// Skip the reconnect backoff and try now (network back, window visible, "Retry now").
#[command]
pub async fn ws_reconnect_now(state: State<'_, AppState>) -> Result<(), String> {
    let ws = state.websocket.read().await;
    ws.as_ref()
        .map(WebSocketManager::reconnect_now)
        .ok_or_else(|| "WebSocket not started".to_string())
}

/// Get the current WebSocket connection status.
#[command]
pub async fn ws_status(state: State<'_, AppState>) -> Result<ConnectionStatus, String> {
    let ws = state.websocket.read().await;
    Ok(ws
        .as_ref()
        .map_or(ConnectionStatus::Disconnected, WebSocketManager::status))
}

/// Send a JSON message from the webview: an object with a string `type`.
#[command]
pub async fn ws_send(state: State<'_, AppState>, message: String) -> Result<(), String> {
    let ws = state.websocket.read().await;
    match &*ws {
        Some(manager) => manager.send_raw(&message),
        None => Err("WebSocket not connected".to_string()),
    }
}

/// Subscribe to a channel.
#[command]
pub async fn ws_subscribe(state: State<'_, AppState>, channel_id: String) -> Result<(), String> {
    debug!("Subscribing to channel {}", channel_id);
    send_event(&state, ClientEvent::Subscribe { channel_id }).await
}

/// Unsubscribe from a channel.
#[command]
pub async fn ws_unsubscribe(state: State<'_, AppState>, channel_id: String) -> Result<(), String> {
    debug!("Unsubscribing from channel {}", channel_id);
    send_event(&state, ClientEvent::Unsubscribe { channel_id }).await
}

/// Send typing indicator.
#[command]
pub async fn ws_typing(state: State<'_, AppState>, channel_id: String) -> Result<(), String> {
    send_event(&state, ClientEvent::Typing { channel_id }).await
}

/// Stop typing indicator.
#[command]
pub async fn ws_stop_typing(state: State<'_, AppState>, channel_id: String) -> Result<(), String> {
    send_event(&state, ClientEvent::StopTyping { channel_id }).await
}

/// Send a ping.
#[command]
pub async fn ws_ping(state: State<'_, AppState>) -> Result<(), String> {
    send_event(&state, ClientEvent::Ping).await
}

/// Send activity update to server via WebSocket.
#[command]
pub async fn ws_send_activity(
    state: State<'_, AppState>,
    activity: Option<serde_json::Value>,
) -> Result<(), String> {
    debug!("Sending activity update: {:?}", activity);
    send_event(&state, ClientEvent::SetActivity { activity }).await
}

/// Send custom status update to server via WebSocket.
#[command]
pub async fn ws_send_custom_status(
    state: State<'_, AppState>,
    custom_status: Option<serde_json::Value>,
) -> Result<(), String> {
    debug!("Sending custom status update: {:?}", custom_status);
    send_event(&state, ClientEvent::SetCustomStatus { custom_status }).await
}

/// Helper to send an event.
async fn send_event(state: &State<'_, AppState>, event: ClientEvent) -> Result<(), String> {
    let ws = state.websocket.read().await;
    match &*ws {
        Some(manager) => manager.send(event).await,
        None => Err("WebSocket not connected".to_string()),
    }
}
