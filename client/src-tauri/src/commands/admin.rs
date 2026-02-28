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

/// Delete response (user or guild).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteResponse {
    pub deleted: bool,
    pub id: String,
}

// ============================================================================
// Admin Status Commands
// ============================================================================

/// Admin status as returned by `GET /api/admin/status`.
#[derive(Debug, Deserialize)]
struct AdminStatusJson {
    is_admin: bool,
    is_elevated: bool,
    elevation_expires_at: Option<String>,
}

/// Check if current user has admin access and whether their session is elevated.
///
/// Calls `GET /api/admin/status` which is accessible to any authenticated user
/// and returns real elevation state from the server.
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
        .get(format!("{server_url}/api/admin/status"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to check admin status: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to check admin status: {} - {}", status, body);
        return Err(format!("Failed to check admin status: {status}"));
    }

    let json: AdminStatusJson = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!(
        is_admin = json.is_admin,
        is_elevated = json.is_elevated,
        "Admin status fetched"
    );

    Ok(AdminStatus {
        is_admin: json.is_admin,
        is_elevated: json.is_elevated,
        elevation_expires_at: json.elevation_expires_at,
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

    debug!(
        "Fetched {} users (total: {})",
        users.items.len(),
        users.total
    );
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
        .delete(format!("{server_url}/api/admin/guilds/{guild_id}/suspend"))
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
// Delete User / Guild Commands
// ============================================================================

/// Permanently delete a user.
#[command]
pub async fn admin_delete_user(
    state: State<'_, AppState>,
    user_id: String,
) -> Result<DeleteResponse, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Deleting user {}", user_id);

    let response = state
        .http
        .delete(format!("{server_url}/api/admin/users/{user_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to delete user: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to delete user: {} - {}", status, body);
        return Err(format!("Failed to delete user: {status}"));
    }

    let result: DeleteResponse = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Deleted user {}", user_id);
    Ok(result)
}

/// Permanently delete a guild.
#[command]
pub async fn admin_delete_guild(
    state: State<'_, AppState>,
    guild_id: String,
) -> Result<DeleteResponse, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Deleting guild {}", guild_id);

    let response = state
        .http
        .delete(format!("{server_url}/api/admin/guilds/{guild_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to delete guild: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to delete guild: {} - {}", status, body);
        return Err(format!("Failed to delete guild: {status}"));
    }

    let result: DeleteResponse = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Deleted guild {}", guild_id);
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
        url.push_str("&action=");
        url.push_str(&encoded);
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

/// Elevate admin session.
#[command]
pub async fn admin_elevate(
    state: State<'_, AppState>,
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

// ============================================================================
// Observability Commands (Command Center)
// ============================================================================

/// Helper to read auth from state. Returns `(server_url, token)`.
async fn read_auth(state: &AppState) -> Result<(String, String), String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };
    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;
    Ok((server_url, token))
}

const VALID_RANGES: &[&str] = &["1h", "6h", "24h", "7d", "30d"];
const VALID_SORTS: &[&str] = &["latency", "errors"];
const VALID_LEVELS: &[&str] = &["ERROR", "WARN", "INFO", "DEBUG", "TRACE"];
const VALID_TRACE_STATUSES: &[&str] = &["error", "slow"];

fn validate_optional(value: Option<&str>, allowed: &[&str], name: &str) -> Result<(), String> {
    if let Some(v) = value {
        if !allowed.contains(&v) {
            return Err(format!("Invalid {name}: {v}"));
        }
    }
    Ok(())
}

/// Build query params from optional key-value pairs.
fn build_query(pairs: &[(&str, Option<String>)]) -> String {
    let mut s = String::new();
    for (key, value) in pairs {
        if let Some(v) = value {
            if !s.is_empty() {
                s.push('&');
            }
            s.push_str(&form_urlencoded::Serializer::new(String::new())
                .append_pair(key, v)
                .finish());
        }
    }
    s
}

/// Fetch observability summary (vital signs, server metadata, voice health).
#[command]
pub async fn admin_obs_summary(
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let (server_url, token) = read_auth(&state).await?;
    debug!("Fetching observability summary");

    let response = state
        .http
        .get(format!("{server_url}/api/admin/observability/summary"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| format!("Connection failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to fetch obs summary: {} - {}", status, body);
        return Err(format!("Failed to fetch obs summary: {status}"));
    }

    response
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Invalid response: {e}"))
}

/// Fetch trend time-series for the given metrics over a time range.
#[command]
pub async fn admin_obs_trends(
    state: State<'_, AppState>,
    range: String,
    metrics: Vec<String>,
) -> Result<serde_json::Value, String> {
    validate_optional(Some(range.as_str()), VALID_RANGES, "range")?;
    let (server_url, token) = read_auth(&state).await?;
    debug!("Fetching observability trends (range={}, metrics={})", range, metrics.len());

    let params = {
        let mut p = build_query(&[("range", Some(range))]);
        for m in &metrics {
            if !p.is_empty() {
                p.push('&');
            }
            p.push_str(&form_urlencoded::Serializer::new(String::new())
                .append_pair("metric", m)
                .finish());
        }
        p
    };

    let response = state
        .http
        .get(format!(
            "{server_url}/api/admin/observability/trends?{params}"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| format!("Connection failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to fetch obs trends: {} - {}", status, body);
        return Err(format!("Failed to fetch obs trends: {status}"));
    }

    response
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Invalid response: {e}"))
}

/// Fetch top routes ranked by latency or error count.
#[command]
pub async fn admin_obs_top_routes(
    state: State<'_, AppState>,
    range: String,
    sort: Option<String>,
    limit: Option<i64>,
) -> Result<serde_json::Value, String> {
    validate_optional(Some(range.as_str()), VALID_RANGES, "range")?;
    validate_optional(sort.as_deref(), VALID_SORTS, "sort")?;
    let (server_url, token) = read_auth(&state).await?;
    debug!("Fetching top routes (range={})", range);

    let params = build_query(&[
        ("range", Some(range)),
        ("sort", sort),
        ("limit", limit.map(|l| l.to_string())),
    ]);

    let response = state
        .http
        .get(format!(
            "{server_url}/api/admin/observability/top-routes?{params}"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| format!("Connection failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to fetch top routes: {} - {}", status, body);
        return Err(format!("Failed to fetch top routes: {status}"));
    }

    response
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Invalid response: {e}"))
}

/// Fetch top error categories.
#[command]
pub async fn admin_obs_top_errors(
    state: State<'_, AppState>,
    range: String,
    limit: Option<i64>,
) -> Result<serde_json::Value, String> {
    validate_optional(Some(range.as_str()), VALID_RANGES, "range")?;
    let (server_url, token) = read_auth(&state).await?;
    debug!("Fetching top errors (range={})", range);

    let params = build_query(&[
        ("range", Some(range)),
        ("limit", limit.map(|l| l.to_string())),
    ]);

    let response = state
        .http
        .get(format!(
            "{server_url}/api/admin/observability/top-errors?{params}"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| format!("Connection failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to fetch top errors: {} - {}", status, body);
        return Err(format!("Failed to fetch top errors: {status}"));
    }

    response
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Invalid response: {e}"))
}

/// Fetch paginated log events with optional filters.
#[command]
pub async fn admin_obs_logs(
    state: State<'_, AppState>,
    level: Option<String>,
    domain: Option<String>,
    search: Option<String>,
    cursor: Option<String>,
    limit: Option<i64>,
) -> Result<serde_json::Value, String> {
    validate_optional(level.as_deref(), VALID_LEVELS, "level")?;
    let (server_url, token) = read_auth(&state).await?;
    debug!("Fetching obs logs (level={:?}, domain={:?})", level, domain);

    let params = build_query(&[
        ("level", level),
        ("domain", domain),
        ("search", search),
        ("cursor", cursor),
        ("limit", limit.map(|l| l.to_string())),
    ]);

    let url = if params.is_empty() {
        format!("{server_url}/api/admin/observability/logs")
    } else {
        format!("{server_url}/api/admin/observability/logs?{params}")
    };

    let response = state
        .http
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| format!("Connection failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to fetch obs logs: {} - {}", status, body);
        return Err(format!("Failed to fetch obs logs: {status}"));
    }

    response
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Invalid response: {e}"))
}

/// Fetch paginated trace entries with optional filters.
#[command]
pub async fn admin_obs_traces(
    state: State<'_, AppState>,
    status: Option<String>,
    domain: Option<String>,
    cursor: Option<String>,
    limit: Option<i64>,
) -> Result<serde_json::Value, String> {
    validate_optional(status.as_deref(), VALID_TRACE_STATUSES, "status")?;
    let (server_url, token) = read_auth(&state).await?;
    debug!("Fetching obs traces (status={:?}, domain={:?})", status, domain);

    let params = build_query(&[
        ("status", status),
        ("domain", domain),
        ("cursor", cursor),
        ("limit", limit.map(|l| l.to_string())),
    ]);

    let url = if params.is_empty() {
        format!("{server_url}/api/admin/observability/traces")
    } else {
        format!("{server_url}/api/admin/observability/traces?{params}")
    };

    let response = state
        .http
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| format!("Connection failed: {e}"))?;

    if !response.status().is_success() {
        let status_code = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to fetch obs traces: {} - {}", status_code, body);
        return Err(format!(
            "Failed to fetch obs traces: {status_code}"
        ));
    }

    response
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Invalid response: {e}"))
}

/// Fetch external observability tool links.
#[command]
pub async fn admin_obs_links(
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let (server_url, token) = read_auth(&state).await?;
    debug!("Fetching observability links");

    let response = state
        .http
        .get(format!("{server_url}/api/admin/observability/links"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| format!("Connection failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to fetch obs links: {} - {}", status, body);
        return Err(format!("Failed to fetch obs links: {status}"));
    }

    response
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Invalid response: {e}"))
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
