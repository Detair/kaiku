//! Role Management Commands
//!
//! Commands for managing guild roles, member role assignments, and channel overrides.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::{command, State};
use tracing::{debug, error};

use crate::AppState;

// ============================================================================
// Types
// ============================================================================

/// Guild role from server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuildRole {
    pub id: String,
    pub guild_id: String,
    pub name: String,
    pub color: Option<String>,
    pub permissions: u64,
    pub position: i32,
    pub is_default: bool,
    pub created_at: String,
}

/// Channel permission override.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelOverride {
    pub id: String,
    pub channel_id: String,
    pub role_id: String,
    pub allow_permissions: u64,
    pub deny_permissions: u64,
}

/// Request to create a role.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRoleRequest {
    pub name: String,
    pub color: Option<String>,
    pub permissions: Option<u64>,
}

/// Request to update a role.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateRoleRequest {
    pub name: Option<String>,
    pub color: Option<String>,
    pub permissions: Option<u64>,
    pub position: Option<i32>,
}

/// Request to set channel override.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetChannelOverrideRequest {
    pub allow: Option<u64>,
    pub deny: Option<u64>,
}

/// Response for role assignment/removal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleAssignmentResponse {
    pub assigned: Option<bool>,
    pub removed: Option<bool>,
    pub user_id: String,
    pub role_id: String,
}

/// Response for role deletion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteRoleResponse {
    pub deleted: bool,
    pub role_id: String,
}

// ============================================================================
// Role Commands
// ============================================================================

/// Get all roles for a guild.
#[command]
pub async fn get_guild_roles(
    state: State<'_, AppState>,
    guild_id: String,
) -> Result<Vec<GuildRole>, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Fetching roles for guild {}", guild_id);

    let response = state
        .http
        .get(format!("{server_url}/api/guilds/{guild_id}/roles"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to fetch roles: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to fetch roles: {} - {}", status, body);
        return Err(format!("Failed to fetch roles: {status}"));
    }

    let roles: Vec<GuildRole> = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Fetched {} roles for guild {}", roles.len(), guild_id);
    Ok(roles)
}

/// Create a new role in a guild.
#[command]
pub async fn create_guild_role(
    state: State<'_, AppState>,
    guild_id: String,
    request: CreateRoleRequest,
) -> Result<GuildRole, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Creating role '{}' in guild {}", request.name, guild_id);

    let response = state
        .http
        .post(format!("{server_url}/api/guilds/{guild_id}/roles"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&request)
        .send()
        .await
        .map_err(|e| {
            error!("Failed to create role: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to create role: {} - {}", status, body);
        return Err(format!("Failed to create role: {status}"));
    }

    let role: GuildRole = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Created role: {} ({})", role.name, role.id);
    Ok(role)
}

/// Update an existing role.
#[command]
pub async fn update_guild_role(
    state: State<'_, AppState>,
    guild_id: String,
    role_id: String,
    request: UpdateRoleRequest,
) -> Result<GuildRole, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Updating role {} in guild {}", role_id, guild_id);

    let response = state
        .http
        .patch(format!(
            "{server_url}/api/guilds/{guild_id}/roles/{role_id}"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .json(&request)
        .send()
        .await
        .map_err(|e| {
            error!("Failed to update role: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to update role: {} - {}", status, body);
        return Err(format!("Failed to update role: {status}"));
    }

    let role: GuildRole = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Updated role: {} ({})", role.name, role.id);
    Ok(role)
}

/// Delete a role from a guild.
#[command]
pub async fn delete_guild_role(
    state: State<'_, AppState>,
    guild_id: String,
    role_id: String,
) -> Result<DeleteRoleResponse, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Deleting role {} from guild {}", role_id, guild_id);

    let response = state
        .http
        .delete(format!(
            "{server_url}/api/guilds/{guild_id}/roles/{role_id}"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to delete role: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to delete role: {} - {}", status, body);
        return Err(format!("Failed to delete role: {status}"));
    }

    let result: DeleteRoleResponse = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Deleted role: {}", role_id);
    Ok(result)
}

// ============================================================================
// Member Role Commands
// ============================================================================

/// Get all member role assignments for a guild.
/// Returns a map of `user_id` -> list of `role_ids`.
#[command]
pub async fn get_guild_member_roles(
    state: State<'_, AppState>,
    guild_id: String,
) -> Result<HashMap<String, Vec<String>>, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Fetching member roles for guild {}", guild_id);

    let response = state
        .http
        .get(format!("{server_url}/api/guilds/{guild_id}/member-roles"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to fetch member roles: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to fetch member roles: {} - {}", status, body);
        return Err(format!("Failed to fetch member roles: {status}"));
    }

    let member_roles: HashMap<String, Vec<String>> = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!(
        "Fetched member roles for {} users in guild {}",
        member_roles.len(),
        guild_id
    );
    Ok(member_roles)
}

/// Assign a role to a guild member.
#[command]
pub async fn assign_member_role(
    state: State<'_, AppState>,
    guild_id: String,
    user_id: String,
    role_id: String,
) -> Result<RoleAssignmentResponse, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!(
        "Assigning role {} to user {} in guild {}",
        role_id, user_id, guild_id
    );

    let response = state
        .http
        .post(format!(
            "{server_url}/api/guilds/{guild_id}/members/{user_id}/roles/{role_id}"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to assign role: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to assign role: {} - {}", status, body);
        return Err(format!("Failed to assign role: {status}"));
    }

    let result: RoleAssignmentResponse = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Assigned role {} to user {}", role_id, user_id);
    Ok(result)
}

/// Remove a role from a guild member.
#[command]
pub async fn remove_member_role(
    state: State<'_, AppState>,
    guild_id: String,
    user_id: String,
    role_id: String,
) -> Result<RoleAssignmentResponse, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!(
        "Removing role {} from user {} in guild {}",
        role_id, user_id, guild_id
    );

    let response = state
        .http
        .delete(format!(
            "{server_url}/api/guilds/{guild_id}/members/{user_id}/roles/{role_id}"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to remove role: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to remove role: {} - {}", status, body);
        return Err(format!("Failed to remove role: {status}"));
    }

    let result: RoleAssignmentResponse = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Removed role {} from user {}", role_id, user_id);
    Ok(result)
}

// ============================================================================
// Channel Override Commands
// ============================================================================

/// Get permission overrides for a channel.
#[command]
pub async fn get_channel_overrides(
    state: State<'_, AppState>,
    channel_id: String,
) -> Result<Vec<ChannelOverride>, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Fetching overrides for channel {}", channel_id);

    let response = state
        .http
        .get(format!("{server_url}/api/channels/{channel_id}/overrides"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to fetch channel overrides: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to fetch channel overrides: {} - {}", status, body);
        return Err(format!("Failed to fetch channel overrides: {status}"));
    }

    let overrides: Vec<ChannelOverride> = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!(
        "Fetched {} overrides for channel {}",
        overrides.len(),
        channel_id
    );
    Ok(overrides)
}

/// Set a permission override for a role in a channel.
#[command]
pub async fn set_channel_override(
    state: State<'_, AppState>,
    channel_id: String,
    role_id: String,
    request: SetChannelOverrideRequest,
) -> Result<ChannelOverride, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!(
        "Setting override for role {} in channel {}",
        role_id, channel_id
    );

    let response = state
        .http
        .put(format!(
            "{server_url}/api/channels/{channel_id}/overrides/{role_id}"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .json(&request)
        .send()
        .await
        .map_err(|e| {
            error!("Failed to set channel override: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to set channel override: {} - {}", status, body);
        return Err(format!("Failed to set channel override: {status}"));
    }

    let override_result: ChannelOverride = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!(
        "Set override for role {} in channel {}",
        role_id, channel_id
    );
    Ok(override_result)
}

/// Delete a permission override for a role in a channel.
#[command]
pub async fn delete_channel_override(
    state: State<'_, AppState>,
    channel_id: String,
    role_id: String,
) -> Result<(), String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!(
        "Deleting override for role {} in channel {}",
        role_id, channel_id
    );

    let response = state
        .http
        .delete(format!(
            "{server_url}/api/channels/{channel_id}/overrides/{role_id}"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to delete channel override: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to delete channel override: {} - {}", status, body);
        return Err(format!("Failed to delete channel override: {status}"));
    }

    debug!(
        "Deleted override for role {} in channel {}",
        role_id, channel_id
    );
    Ok(())
}
