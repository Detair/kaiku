//! Chat Commands

use serde::{Deserialize, Serialize};
use tauri::{command, State};
use tracing::{debug, error};

use crate::{AppState, UserStatus};

/// Channel type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChannelType {
    Text,
    Voice,
    Dm,
}

/// Channel from server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub channel_type: ChannelType,
    pub category_id: Option<String>,
    pub topic: Option<String>,
    pub user_limit: Option<u32>,
    pub position: i32,
    pub created_at: String,
}

/// User profile for message author.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub status: UserStatus,
}

/// File attachment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub filename: String,
    pub mime_type: String,
    pub size: u64,
    pub url: String,
}

/// Message from server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub channel_id: String,
    pub author: UserProfile,
    pub content: String,
    pub encrypted: bool,
    pub attachments: Vec<Attachment>,
    pub reply_to: Option<String>,
    pub parent_id: Option<String>,
    #[serde(default)]
    pub thread_reply_count: i32,
    pub thread_last_reply_at: Option<String>,
    pub edited_at: Option<String>,
    pub created_at: String,
}

/// Get all channels.
#[command]
pub async fn get_channels(state: State<'_, AppState>) -> Result<Vec<Channel>, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Fetching channels from {}", server_url);

    let response = state
        .http
        .get(format!("{server_url}/api/channels"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to fetch channels: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to fetch channels: {}", status);
        return Err(format!("Failed to fetch channels: {status}"));
    }

    let channels: Vec<Channel> = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Fetched {} channels", channels.len());
    Ok(channels)
}

/// Get messages for a channel.
#[command]
pub async fn get_messages(
    state: State<'_, AppState>,
    channel_id: String,
    before: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<Message>, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Fetching messages for channel {}", channel_id);

    let mut url = format!("{server_url}/api/messages/channel/{channel_id}");

    // Add query params
    let mut params = vec![];
    if let Some(before_id) = before {
        params.push(format!("before={before_id}"));
    }
    if let Some(lim) = limit {
        params.push(format!("limit={lim}"));
    }
    if !params.is_empty() {
        url = format!("{}?{}", url, params.join("&"));
    }

    let response = state
        .http
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to fetch messages: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to fetch messages: {}", status);
        return Err(format!("Failed to fetch messages: {status}"));
    }

    let messages: Vec<Message> = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Fetched {} messages", messages.len());
    Ok(messages)
}

/// Send a message to a channel.
#[command]
pub async fn send_message(
    state: State<'_, AppState>,
    channel_id: String,
    content: String,
) -> Result<Message, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Sending message to channel {}", channel_id);

    let response = state
        .http
        .post(format!("{server_url}/api/messages/channel/{channel_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({
            "content": content,
            "encrypted": false
        }))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to send message: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to send message: {} - {}", status, body);
        return Err(format!("Failed to send message: {status}"));
    }

    let message: Message = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Message sent: {}", message.id);
    Ok(message)
}

/// Get thread replies for a parent message.
#[command]
pub async fn get_thread_replies(
    state: State<'_, AppState>,
    parent_id: String,
    after: Option<String>,
    limit: Option<u32>,
) -> Result<serde_json::Value, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Fetching thread replies for parent {}", parent_id);

    let mut url = format!("{server_url}/api/messages/{parent_id}/thread");

    let mut params = vec![];
    if let Some(after_id) = after {
        params.push(format!("after={after_id}"));
    }
    if let Some(lim) = limit {
        params.push(format!("limit={lim}"));
    }
    if !params.is_empty() {
        url = format!("{}?{}", url, params.join("&"));
    }

    let response = state
        .http
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to fetch thread replies: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to fetch thread replies: {}", status);
        return Err(format!("Failed to fetch thread replies: {status}"));
    }

    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Fetched thread replies for parent {}", parent_id);
    Ok(body)
}

/// Send a reply in a thread.
#[command]
pub async fn send_thread_reply(
    state: State<'_, AppState>,
    parent_id: String,
    channel_id: String,
    content: String,
    encrypted: Option<bool>,
    nonce: Option<String>,
) -> Result<serde_json::Value, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Sending thread reply to parent {} in channel {}", parent_id, channel_id);

    let mut body = serde_json::json!({
        "content": content,
        "encrypted": encrypted.unwrap_or(false),
        "parent_id": parent_id,
    });

    if let Some(n) = nonce {
        body["nonce"] = serde_json::Value::String(n);
    }

    let response = state
        .http
        .post(format!("{server_url}/api/messages/channel/{channel_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            error!("Failed to send thread reply: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let err_body = response.text().await.unwrap_or_default();
        error!("Failed to send thread reply: {} - {}", status, err_body);
        return Err(format!("Failed to send thread reply: {status}"));
    }

    let message: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Thread reply sent to parent {}", parent_id);
    Ok(message)
}

/// Mark a thread as read.
#[command]
pub async fn mark_thread_read(
    state: State<'_, AppState>,
    parent_id: String,
) -> Result<(), String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Marking thread {} as read", parent_id);

    let response = state
        .http
        .post(format!("{server_url}/api/messages/{parent_id}/thread/read"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to mark thread read: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to mark thread read: {}", status);
        return Err(format!("Failed to mark thread read: {status}"));
    }

    debug!("Thread {} marked as read", parent_id);
    Ok(())
}
