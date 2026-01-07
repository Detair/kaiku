//! WebSocket Commands
//!
//! Commands for managing the WebSocket connection.

use tauri::{command, AppHandle, State};
use tracing::{debug, info};

use crate::network::{ClientEvent, ConnectionStatus, WebSocketManager};
use crate::AppState;

/// Connect to the WebSocket server.
#[command]
pub async fn ws_connect(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    info!("Connecting WebSocket to {}", server_url);

    let manager = WebSocketManager::connect(app, server_url, token).await?;

    // Store the manager
    let mut ws = state.websocket.write().await;
    *ws = Some(manager);

    Ok(())
}

/// Disconnect from the WebSocket server.
#[command]
pub async fn ws_disconnect(state: State<'_, AppState>) -> Result<(), String> {
    let mut ws = state.websocket.write().await;
    if let Some(ref mut manager) = *ws {
        manager.disconnect().await;
    }
    *ws = None;
    Ok(())
}

/// Get the current WebSocket connection status.
#[command]
pub async fn ws_status(state: State<'_, AppState>) -> Result<ConnectionStatus, String> {
    let ws = state.websocket.read().await;
    match &*ws {
        Some(manager) => Ok(manager.status().await),
        None => Ok(ConnectionStatus::Disconnected),
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

/// Helper to send an event.
async fn send_event(state: &State<'_, AppState>, event: ClientEvent) -> Result<(), String> {
    let ws = state.websocket.read().await;
    match &*ws {
        Some(manager) => manager.send(event).await,
        None => Err("WebSocket not connected".to_string()),
    }
}
