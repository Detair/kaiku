//! Favorites Tauri Commands
//!
//! CRUD operations for cross-server channel favorites.

use serde::{Deserialize, Serialize};
use tauri::{command, State};
use tracing::{debug, error};

use crate::AppState;

/// A favorite channel.
#[derive(Debug, Serialize, Deserialize)]
pub struct FavoriteChannel {
    pub channel_id: String,
    pub channel_name: String,
    pub channel_type: String,
    pub guild_id: String,
    pub guild_name: String,
    pub guild_icon: Option<String>,
    pub guild_position: i32,
    pub channel_position: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FavoritesResponse {
    pub favorites: Vec<FavoriteChannel>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Favorite {
    pub channel_id: String,
    pub guild_id: String,
    pub guild_position: i32,
    pub channel_position: i32,
    pub created_at: String,
}

/// Fetch all favorites for the current user.
#[command]
pub async fn fetch_favorites(state: State<'_, AppState>) -> Result<Vec<FavoriteChannel>, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Fetching favorites from server");

    let response = state
        .http
        .get(format!("{server_url}/api/me/favorites"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to fetch favorites: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to fetch favorites: {}", status);
        return Err(format!("Failed to fetch favorites: {status}"));
    }

    let data: FavoritesResponse = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Fetched {} favorites", data.favorites.len());
    Ok(data.favorites)
}

/// Add a channel to favorites.
#[command]
pub async fn add_favorite(
    state: State<'_, AppState>,
    channel_id: String,
) -> Result<Favorite, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Adding favorite: channel_id={}", channel_id);

    let response = state
        .http
        .post(format!("{server_url}/api/me/favorites/{channel_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to add favorite: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to add favorite: {} - {}", status, body);
        return Err(format!("Failed to add favorite: {status}"));
    }

    let favorite: Favorite = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Favorite added: channel_id={}", favorite.channel_id);
    Ok(favorite)
}

/// Remove a channel from favorites.
#[command]
pub async fn remove_favorite(
    state: State<'_, AppState>,
    channel_id: String,
) -> Result<(), String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Removing favorite: channel_id={}", channel_id);

    let response = state
        .http
        .delete(format!("{server_url}/api/me/favorites/{channel_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to remove favorite: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to remove favorite: {}", status);
        return Err(format!("Failed to remove favorite: {status}"));
    }

    debug!("Favorite removed: channel_id={}", channel_id);
    Ok(())
}

/// Reorder channels within a guild.
#[command]
pub async fn reorder_favorite_channels(
    state: State<'_, AppState>,
    guild_id: String,
    channel_ids: Vec<String>,
) -> Result<(), String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!(
        "Reordering {} favorite channels in guild {}",
        channel_ids.len(),
        guild_id
    );

    let response = state
        .http
        .put(format!("{server_url}/api/me/favorites/reorder"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "guild_id": guild_id, "channel_ids": channel_ids }))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to reorder favorites: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to reorder favorites: {}", status);
        return Err(format!("Failed to reorder favorites: {status}"));
    }

    debug!("Favorites reordered successfully");
    Ok(())
}

/// Reorder guild groups.
#[command]
pub async fn reorder_favorite_guilds(
    state: State<'_, AppState>,
    guild_ids: Vec<String>,
) -> Result<(), String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Reordering {} favorite guilds", guild_ids.len());

    let response = state
        .http
        .put(format!("{server_url}/api/me/favorites/reorder-guilds"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "guild_ids": guild_ids }))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to reorder favorite guilds: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to reorder favorite guilds: {}", status);
        return Err(format!("Failed to reorder favorite guilds: {status}"));
    }

    debug!("Favorite guilds reordered successfully");
    Ok(())
}
