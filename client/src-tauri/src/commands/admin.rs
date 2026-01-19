//! Admin Commands
//!
//! Tauri commands for system administration: user management, guild management,
//! audit log viewing, and admin session elevation.

use serde::{Deserialize, Serialize};
use tauri::{command, State};
use tracing::{debug, error};
use url::form_urlencoded;

use crate::AppState;

// ============================================================================
// Types
// ============================================================================

/// Admin status response from health check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminStatus {
    pub is_admin: bool,
    pub is_elevated: bool,
    pub elevation_expires_at: Option<String>,
}

/// Admin statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminStats {
    pub user_count: i64,
    pub guild_count: i64,
    pub banned_count: i64,
}

/// User summary for admin listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSummary {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub email: Option<String>,
    pub created_at: String,
    pub is_banned: bool,
}

/// Guild summary for admin listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuildSummary {
    pub id: String,
    pub name: String,
    pub owner_id: String,
    pub member_count: i64,
    pub created_at: String,
    pub suspended_at: Option<String>,
}

/// Audit log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogEntry {
    pub id: String,
    pub actor_id: String,
    pub actor_username: Option<String>,
    pub action: String,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub details: Option<serde_json::Value>,
    pub ip_address: Option<String>,
    pub created_at: String,
}

/// Generic paginated response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedResponse<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

/// Elevate session response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElevateResponse {
    pub elevated: bool,
    pub expires_at: String,
    pub session_id: String,
}

/// De-elevate session response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeElevateResponse {
    pub elevated: bool,
}

/// Ban user response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BanResponse {
    pub banned: bool,
    pub user_id: String,
}

/// Suspend guild response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuspendResponse {
    pub suspended: bool,
    pub guild_id: String,
}

// ============================================================================
// Admin Status Commands
// ============================================================================

/// Check if current user has admin access.
///
/// Returns admin status by hitting the admin health endpoint.
#[command]
pub async fn check_admin_status(state: State<'_, AppState>) -> Result<AdminStatus, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Checking admin status");

    let response = state
        .http
        .get(format!("{server_url}/api/admin/health"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to check admin status: {}", e);
            format!("Connection failed: {e}")
        })?;

    // 403 means not admin, which is a valid response
    if response.status().as_u16() == 403 {
        return Ok(AdminStatus {
            is_admin: false,
            is_elevated: false,
            elevation_expires_at: None,
        });
    }

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to check admin status: {} - {}", status, body);
        return Err(format!("Failed to check admin status: {status}"));
    }

    // If we got a 200, user is admin
    // TODO: Parse elevation status from response headers or separate endpoint
    debug!("User has admin privileges");
    Ok(AdminStatus {
        is_admin: true,
        is_elevated: false,
        elevation_expires_at: None,
    })
}

/// Get admin statistics (user count, guild count, banned count).
#[command]
pub async fn get_admin_stats(state: State<'_, AppState>) -> Result<AdminStats, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Fetching admin stats");

    // Get user count from users endpoint
    let users_response = state
        .http
        .get(format!("{server_url}/api/admin/users?limit=1"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to fetch user stats: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !users_response.status().is_success() {
        let status = users_response.status();
        let body = users_response.text().await.unwrap_or_default();
        error!("Failed to fetch user stats: {} - {}", status, body);
        return Err(format!("Failed to fetch admin stats: {status}"));
    }

    let users_data: PaginatedResponse<UserSummary> = users_response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    // Get guild count from guilds endpoint
    let guilds_response = state
        .http
        .get(format!("{server_url}/api/admin/guilds?limit=1"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to fetch guild stats: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !guilds_response.status().is_success() {
        let status = guilds_response.status();
        let body = guilds_response.text().await.unwrap_or_default();
        error!("Failed to fetch guild stats: {} - {}", status, body);
        return Err(format!("Failed to fetch admin stats: {status}"));
    }

    let guilds_data: PaginatedResponse<GuildSummary> = guilds_response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    Ok(AdminStats {
        user_count: users_data.total,
        guild_count: guilds_data.total,
        // NOTE: Accurate banned count would require a dedicated stats endpoint
        // or fetching all users. Set to 0 for now.
        banned_count: 0,
    })
}

// ============================================================================
// User Management Commands
// ============================================================================

/// List all users with pagination.
#[command]
pub async fn admin_list_users(
    state: State<'_, AppState>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<PaginatedResponse<UserSummary>, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    let limit = limit.unwrap_or(50);
    let offset = offset.unwrap_or(0);

    debug!("Fetching users (limit={}, offset={})", limit, offset);

    let response = state
        .http
        .get(format!(
            "{server_url}/api/admin/users?limit={limit}&offset={offset}"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to fetch users: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to fetch users: {} - {}", status, body);
        return Err(format!("Failed to fetch users: {status}"));
    }

    let users: PaginatedResponse<UserSummary> = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Fetched {} users (total: {})", users.items.len(), users.total);
    Ok(users)
}

/// Ban a user globally.
#[command]
pub async fn admin_ban_user(
    state: State<'_, AppState>,
    user_id: String,
    reason: String,
    expires_at: Option<String>,
) -> Result<BanResponse, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Banning user {}", user_id);

    let body = serde_json::json!({
        "reason": reason,
        "expires_at": expires_at,
    });

    let response = state
        .http
        .post(format!("{server_url}/api/admin/users/{user_id}/ban"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            error!("Failed to ban user: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to ban user: {} - {}", status, body);
        return Err(format!("Failed to ban user: {status}"));
    }

    let result: BanResponse = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Banned user {}", user_id);
    Ok(result)
}

/// Remove global ban from a user.
#[command]
pub async fn admin_unban_user(
    state: State<'_, AppState>,
    user_id: String,
) -> Result<BanResponse, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Unbanning user {}", user_id);

    let response = state
        .http
        .delete(format!("{server_url}/api/admin/users/{user_id}/ban"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to unban user: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to unban user: {} - {}", status, body);
        return Err(format!("Failed to unban user: {status}"));
    }

    let result: BanResponse = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Unbanned user {}", user_id);
    Ok(result)
}

// ============================================================================
// Guild Management Commands
// ============================================================================

/// List all guilds with pagination.
#[command]
pub async fn admin_list_guilds(
    state: State<'_, AppState>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<PaginatedResponse<GuildSummary>, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    let limit = limit.unwrap_or(50);
    let offset = offset.unwrap_or(0);

    debug!("Fetching guilds (limit={}, offset={})", limit, offset);

    let response = state
        .http
        .get(format!(
            "{server_url}/api/admin/guilds?limit={limit}&offset={offset}"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to fetch guilds: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to fetch guilds: {} - {}", status, body);
        return Err(format!("Failed to fetch guilds: {status}"));
    }

    let guilds: PaginatedResponse<GuildSummary> = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!(
        "Fetched {} guilds (total: {})",
        guilds.items.len(),
        guilds.total
    );
    Ok(guilds)
}

/// Suspend a guild.
#[command]
pub async fn admin_suspend_guild(
    state: State<'_, AppState>,
    guild_id: String,
    reason: String,
) -> Result<SuspendResponse, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Suspending guild {}", guild_id);

    let body = serde_json::json!({
        "reason": reason,
    });

    let response = state
        .http
        .post(format!("{server_url}/api/admin/guilds/{guild_id}/suspend"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            error!("Failed to suspend guild: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to suspend guild: {} - {}", status, body);
        return Err(format!("Failed to suspend guild: {status}"));
    }

    let result: SuspendResponse = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Suspended guild {}", guild_id);
    Ok(result)
}

/// Unsuspend a guild.
#[command]
pub async fn admin_unsuspend_guild(
    state: State<'_, AppState>,
    guild_id: String,
) -> Result<SuspendResponse, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Unsuspending guild {}", guild_id);

    let response = state
        .http
        .delete(format!(
            "{server_url}/api/admin/guilds/{guild_id}/suspend"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to unsuspend guild: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to unsuspend guild: {} - {}", status, body);
        return Err(format!("Failed to unsuspend guild: {status}"));
    }

    let result: SuspendResponse = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Unsuspended guild {}", guild_id);
    Ok(result)
}

// ============================================================================
// Audit Log Commands
// ============================================================================

/// Get system audit log with pagination and optional action filter.
#[command]
pub async fn admin_get_audit_log(
    state: State<'_, AppState>,
    limit: Option<i64>,
    offset: Option<i64>,
    action_filter: Option<String>,
) -> Result<PaginatedResponse<AuditLogEntry>, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    let limit = limit.unwrap_or(50);
    let offset = offset.unwrap_or(0);

    debug!(
        "Fetching audit log (limit={}, offset={}, action={:?})",
        limit, offset, action_filter
    );

    let mut url = format!("{server_url}/api/admin/audit-log?limit={limit}&offset={offset}");
    if let Some(ref action) = action_filter {
        let encoded: String = form_urlencoded::byte_serialize(action.as_bytes()).collect();
        url.push_str(&format!("&action={encoded}"));
    }

    let response = state
        .http
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to fetch audit log: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to fetch audit log: {} - {}", status, body);
        return Err(format!("Failed to fetch audit log: {status}"));
    }

    let entries: PaginatedResponse<AuditLogEntry> = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!(
        "Fetched {} audit log entries (total: {})",
        entries.items.len(),
        entries.total
    );
    Ok(entries)
}

// ============================================================================
// Session Elevation Commands
// ============================================================================

/// Elevate admin session with MFA verification.
#[command]
pub async fn admin_elevate(
    state: State<'_, AppState>,
    mfa_code: String,
    reason: Option<String>,
) -> Result<ElevateResponse, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Elevating admin session");

    let body = serde_json::json!({
        "mfa_code": mfa_code,
        "reason": reason,
    });

    let response = state
        .http
        .post(format!("{server_url}/api/admin/elevate"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            error!("Failed to elevate session: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to elevate session: {} - {}", status, body);

        // Parse error message for better UX
        if let Ok(error_obj) = serde_json::from_str::<serde_json::Value>(&body) {
            if let Some(message) = error_obj.get("message").and_then(|m| m.as_str()) {
                return Err(message.to_string());
            }
        }

        return Err(format!("Failed to elevate session: {status}"));
    }

    let result: ElevateResponse = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Session elevated until {}", result.expires_at);
    Ok(result)
}

/// De-elevate admin session.
#[command]
pub async fn admin_de_elevate(state: State<'_, AppState>) -> Result<DeElevateResponse, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("De-elevating admin session");

    let response = state
        .http
        .delete(format!("{server_url}/api/admin/elevate"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to de-elevate session: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to de-elevate session: {} - {}", status, body);
        return Err(format!("Failed to de-elevate session: {status}"));
    }

    let result: DeElevateResponse = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Session de-elevated");
    Ok(result)
}
