# Admin Dashboard UI Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build the AdminQuickModal and AdminDashboard UI for system administrators to manage users, guilds, and view audit logs.

**Architecture:** Three-layer approach: (1) Tauri commands for API communication, (2) admin store for state management, (3) UI components for AdminQuickModal and AdminDashboard. Elevation status tracked in store with countdown timer.

**Tech Stack:** Solid.js, Tauri commands, existing admin API endpoints (`/api/admin/*`)

---

## Prerequisites

- Server admin API already implemented (`server/src/admin/`)
- Existing patterns in `client/src-tauri/src/commands/roles.rs` for Tauri commands
- Existing patterns in `client/src/stores/permissions.ts` for store structure

---

## Task 1: Add Admin Types to Client

**Files:**
- Modify: `client/src/lib/types.ts`

**Step 1: Add admin types**

Add these types at the end of `types.ts`:

```typescript
// Admin Types

export interface AdminStats {
  user_count: number;
  guild_count: number;
  banned_count: number;
}

export interface AdminStatus {
  is_admin: boolean;
  is_elevated: boolean;
  elevation_expires_at: string | null;
}

export interface UserSummary {
  id: string;
  username: string;
  display_name: string;
  email: string | null;
  created_at: string;
  is_banned: boolean;
}

export interface GuildSummary {
  id: string;
  name: string;
  owner_id: string;
  member_count: number;
  created_at: string;
  suspended_at: string | null;
}

export interface AuditLogEntry {
  id: string;
  actor_id: string;
  actor_username: string | null;
  action: string;
  target_type: string | null;
  target_id: string | null;
  details: Record<string, unknown> | null;
  ip_address: string | null;
  created_at: string;
}

export interface PaginatedResponse<T> {
  items: T[];
  total: number;
  limit: number;
  offset: number;
}

export interface ElevateResponse {
  elevated: boolean;
  expires_at: string;
  session_id: string;
}
```

**Step 2: Commit**

```bash
git add client/src/lib/types.ts
git commit -m "feat(admin): add admin types to client"
```

---

## Task 2: Add Admin Tauri Commands

**Files:**
- Create: `client/src-tauri/src/commands/admin.rs`
- Modify: `client/src-tauri/src/commands/mod.rs`
- Modify: `client/src-tauri/src/lib.rs`

**Step 1: Create admin commands file**

Create `client/src-tauri/src/commands/admin.rs`:

```rust
//! Admin Management Commands
//!
//! Commands for system admin functionality: status check, user/guild management,
//! audit log viewing, and session elevation.

use serde::{Deserialize, Serialize};
use tauri::{command, State};
use tracing::{debug, error};

use crate::AppState;

// ============================================================================
// Types
// ============================================================================

/// Admin status response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminStatus {
    pub is_admin: bool,
    pub is_elevated: bool,
    pub elevation_expires_at: Option<String>,
}

/// Admin stats response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminStats {
    pub user_count: i64,
    pub guild_count: i64,
    pub banned_count: i64,
}

/// User summary from admin API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSummary {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub email: Option<String>,
    pub created_at: String,
    pub is_banned: bool,
}

/// Guild summary from admin API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuildSummary {
    pub id: String,
    pub name: String,
    pub owner_id: String,
    pub member_count: i64,
    pub created_at: String,
    pub suspended_at: Option<String>,
}

/// Audit log entry from admin API.
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

/// Paginated response wrapper.
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

/// Ban/unban response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BanResponse {
    pub banned: bool,
    pub user_id: String,
}

/// Suspend/unsuspend response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuspendResponse {
    pub suspended: bool,
    pub guild_id: String,
}

// ============================================================================
// Commands
// ============================================================================

/// Check if current user is a system admin and their elevation status.
#[command]
pub async fn check_admin_status(state: State<'_, AppState>) -> Result<AdminStatus, String> {
    let client = state.client.lock().await;
    let client = client.as_ref().ok_or("Not authenticated")?;

    // Try to hit the admin health endpoint - if it succeeds, user is admin
    let response = client
        .get(&format!("{}/api/admin/health", client.server_url))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if response.status() == 403 {
        return Ok(AdminStatus {
            is_admin: false,
            is_elevated: false,
            elevation_expires_at: None,
        });
    }

    if !response.status().is_success() {
        return Err(format!("Admin check failed: {}", response.status()));
    }

    // User is admin, now check elevation status
    // We'll use a simple heuristic - try to call an elevated endpoint
    // For now, return is_admin: true, is_elevated: false
    // The elevation status will be tracked client-side after elevation
    debug!("User is system admin");
    Ok(AdminStatus {
        is_admin: true,
        is_elevated: false,
        elevation_expires_at: None,
    })
}

/// Get admin statistics (user count, guild count, banned count).
#[command]
pub async fn get_admin_stats(state: State<'_, AppState>) -> Result<AdminStats, String> {
    let client = state.client.lock().await;
    let client = client.as_ref().ok_or("Not authenticated")?;

    // Get user count
    let users_response: PaginatedResponse<UserSummary> = client
        .get(&format!("{}/api/admin/users?limit=1", client.server_url))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    // Get guild count
    let guilds_response: PaginatedResponse<GuildSummary> = client
        .get(&format!("{}/api/admin/guilds?limit=1", client.server_url))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    // Count banned users from the user list (this is approximate)
    // For accurate count, we'd need a dedicated endpoint
    let banned_count = 0; // Placeholder - would need dedicated endpoint

    Ok(AdminStats {
        user_count: users_response.total,
        guild_count: guilds_response.total,
        banned_count,
    })
}

/// List users with pagination.
#[command]
pub async fn admin_list_users(
    state: State<'_, AppState>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<PaginatedResponse<UserSummary>, String> {
    let client = state.client.lock().await;
    let client = client.as_ref().ok_or("Not authenticated")?;

    let limit = limit.unwrap_or(50);
    let offset = offset.unwrap_or(0);

    let response = client
        .get(&format!(
            "{}/api/admin/users?limit={}&offset={}",
            client.server_url, limit, offset
        ))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        error!("Failed to list users: {} - {}", status, text);
        return Err(format!("Failed to list users: {}", status));
    }

    response.json().await.map_err(|e| e.to_string())
}

/// List guilds with pagination.
#[command]
pub async fn admin_list_guilds(
    state: State<'_, AppState>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<PaginatedResponse<GuildSummary>, String> {
    let client = state.client.lock().await;
    let client = client.as_ref().ok_or("Not authenticated")?;

    let limit = limit.unwrap_or(50);
    let offset = offset.unwrap_or(0);

    let response = client
        .get(&format!(
            "{}/api/admin/guilds?limit={}&offset={}",
            client.server_url, limit, offset
        ))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        error!("Failed to list guilds: {} - {}", status, text);
        return Err(format!("Failed to list guilds: {}", status));
    }

    response.json().await.map_err(|e| e.to_string())
}

/// Get audit log with pagination and optional action filter.
#[command]
pub async fn admin_get_audit_log(
    state: State<'_, AppState>,
    limit: Option<i64>,
    offset: Option<i64>,
    action_filter: Option<String>,
) -> Result<PaginatedResponse<AuditLogEntry>, String> {
    let client = state.client.lock().await;
    let client = client.as_ref().ok_or("Not authenticated")?;

    let limit = limit.unwrap_or(50);
    let offset = offset.unwrap_or(0);

    let mut url = format!(
        "{}/api/admin/audit-log?limit={}&offset={}",
        client.server_url, limit, offset
    );
    if let Some(action) = action_filter {
        url.push_str(&format!("&action={}", action));
    }

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        error!("Failed to get audit log: {} - {}", status, text);
        return Err(format!("Failed to get audit log: {}", status));
    }

    response.json().await.map_err(|e| e.to_string())
}

/// Elevate admin session with MFA code.
#[command]
pub async fn admin_elevate(
    state: State<'_, AppState>,
    mfa_code: String,
    reason: Option<String>,
) -> Result<ElevateResponse, String> {
    let client = state.client.lock().await;
    let client = client.as_ref().ok_or("Not authenticated")?;

    #[derive(Serialize)]
    struct ElevateRequest {
        mfa_code: String,
        reason: Option<String>,
    }

    let response = client
        .post(&format!("{}/api/admin/elevate", client.server_url))
        .json(&ElevateRequest { mfa_code, reason })
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        error!("Failed to elevate session: {} - {}", status, text);
        return Err(format!("Failed to elevate: {}", text));
    }

    response.json().await.map_err(|e| e.to_string())
}

/// De-elevate admin session.
#[command]
pub async fn admin_de_elevate(state: State<'_, AppState>) -> Result<(), String> {
    let client = state.client.lock().await;
    let client = client.as_ref().ok_or("Not authenticated")?;

    let response = client
        .delete(&format!("{}/api/admin/elevate", client.server_url))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        error!("Failed to de-elevate session: {} - {}", status, text);
        return Err(format!("Failed to de-elevate: {}", status));
    }

    Ok(())
}

/// Ban a user (requires elevation).
#[command]
pub async fn admin_ban_user(
    state: State<'_, AppState>,
    user_id: String,
    reason: String,
    expires_at: Option<String>,
) -> Result<BanResponse, String> {
    let client = state.client.lock().await;
    let client = client.as_ref().ok_or("Not authenticated")?;

    #[derive(Serialize)]
    struct BanRequest {
        reason: String,
        expires_at: Option<String>,
    }

    let response = client
        .post(&format!(
            "{}/api/admin/users/{}/ban",
            client.server_url, user_id
        ))
        .json(&BanRequest { reason, expires_at })
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        error!("Failed to ban user: {} - {}", status, text);
        return Err(format!("Failed to ban user: {}", text));
    }

    response.json().await.map_err(|e| e.to_string())
}

/// Unban a user (requires elevation).
#[command]
pub async fn admin_unban_user(
    state: State<'_, AppState>,
    user_id: String,
) -> Result<BanResponse, String> {
    let client = state.client.lock().await;
    let client = client.as_ref().ok_or("Not authenticated")?;

    let response = client
        .delete(&format!(
            "{}/api/admin/users/{}/ban",
            client.server_url, user_id
        ))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        error!("Failed to unban user: {} - {}", status, text);
        return Err(format!("Failed to unban user: {}", text));
    }

    response.json().await.map_err(|e| e.to_string())
}

/// Suspend a guild (requires elevation).
#[command]
pub async fn admin_suspend_guild(
    state: State<'_, AppState>,
    guild_id: String,
    reason: String,
) -> Result<SuspendResponse, String> {
    let client = state.client.lock().await;
    let client = client.as_ref().ok_or("Not authenticated")?;

    #[derive(Serialize)]
    struct SuspendRequest {
        reason: String,
    }

    let response = client
        .post(&format!(
            "{}/api/admin/guilds/{}/suspend",
            client.server_url, guild_id
        ))
        .json(&SuspendRequest { reason })
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        error!("Failed to suspend guild: {} - {}", status, text);
        return Err(format!("Failed to suspend guild: {}", text));
    }

    response.json().await.map_err(|e| e.to_string())
}

/// Unsuspend a guild (requires elevation).
#[command]
pub async fn admin_unsuspend_guild(
    state: State<'_, AppState>,
    guild_id: String,
) -> Result<SuspendResponse, String> {
    let client = state.client.lock().await;
    let client = client.as_ref().ok_or("Not authenticated")?;

    let response = client
        .delete(&format!(
            "{}/api/admin/guilds/{}/suspend",
            client.server_url, guild_id
        ))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        error!("Failed to unsuspend guild: {} - {}", status, text);
        return Err(format!("Failed to unsuspend guild: {}", text));
    }

    response.json().await.map_err(|e| e.to_string())
}
```

**Step 2: Update mod.rs to export admin module**

Add to `client/src-tauri/src/commands/mod.rs`:

```rust
pub mod admin;
```

**Step 3: Register admin commands in lib.rs**

Find the `invoke_handler` section in `client/src-tauri/src/lib.rs` and add the admin commands:

```rust
// Add these to the existing invoke_handler list:
commands::admin::check_admin_status,
commands::admin::get_admin_stats,
commands::admin::admin_list_users,
commands::admin::admin_list_guilds,
commands::admin::admin_get_audit_log,
commands::admin::admin_elevate,
commands::admin::admin_de_elevate,
commands::admin::admin_ban_user,
commands::admin::admin_unban_user,
commands::admin::admin_suspend_guild,
commands::admin::admin_unsuspend_guild,
```

**Step 4: Commit**

```bash
git add client/src-tauri/src/commands/admin.rs client/src-tauri/src/commands/mod.rs client/src-tauri/src/lib.rs
git commit -m "feat(admin): add Tauri commands for admin API"
```

---

## Task 3: Add Admin TypeScript API Functions

**Files:**
- Modify: `client/src/lib/tauri.ts`

**Step 1: Add admin API functions**

Add these functions to `tauri.ts`:

```typescript
// ============================================================================
// Admin API
// ============================================================================

/**
 * Admin status response.
 */
export interface AdminStatus {
  is_admin: boolean;
  is_elevated: boolean;
  elevation_expires_at: string | null;
}

/**
 * Admin stats response.
 */
export interface AdminStats {
  user_count: number;
  guild_count: number;
  banned_count: number;
}

/**
 * User summary for admin listing.
 */
export interface UserSummary {
  id: string;
  username: string;
  display_name: string;
  email: string | null;
  created_at: string;
  is_banned: boolean;
}

/**
 * Guild summary for admin listing.
 */
export interface GuildSummary {
  id: string;
  name: string;
  owner_id: string;
  member_count: number;
  created_at: string;
  suspended_at: string | null;
}

/**
 * Audit log entry.
 */
export interface AuditLogEntry {
  id: string;
  actor_id: string;
  actor_username: string | null;
  action: string;
  target_type: string | null;
  target_id: string | null;
  details: Record<string, unknown> | null;
  ip_address: string | null;
  created_at: string;
}

/**
 * Paginated response wrapper.
 */
export interface PaginatedResponse<T> {
  items: T[];
  total: number;
  limit: number;
  offset: number;
}

/**
 * Elevate response.
 */
export interface ElevateResponse {
  elevated: boolean;
  expires_at: string;
  session_id: string;
}

/**
 * Check if current user is a system admin.
 */
export async function checkAdminStatus(): Promise<AdminStatus> {
  return invoke<AdminStatus>("check_admin_status");
}

/**
 * Get admin statistics.
 */
export async function getAdminStats(): Promise<AdminStats> {
  return invoke<AdminStats>("get_admin_stats");
}

/**
 * List users (admin only).
 */
export async function adminListUsers(
  limit?: number,
  offset?: number
): Promise<PaginatedResponse<UserSummary>> {
  return invoke<PaginatedResponse<UserSummary>>("admin_list_users", {
    limit,
    offset,
  });
}

/**
 * List guilds (admin only).
 */
export async function adminListGuilds(
  limit?: number,
  offset?: number
): Promise<PaginatedResponse<GuildSummary>> {
  return invoke<PaginatedResponse<GuildSummary>>("admin_list_guilds", {
    limit,
    offset,
  });
}

/**
 * Get audit log (admin only).
 */
export async function adminGetAuditLog(
  limit?: number,
  offset?: number,
  actionFilter?: string
): Promise<PaginatedResponse<AuditLogEntry>> {
  return invoke<PaginatedResponse<AuditLogEntry>>("admin_get_audit_log", {
    limit,
    offset,
    action_filter: actionFilter,
  });
}

/**
 * Elevate admin session with MFA code.
 */
export async function adminElevate(
  mfaCode: string,
  reason?: string
): Promise<ElevateResponse> {
  return invoke<ElevateResponse>("admin_elevate", {
    mfa_code: mfaCode,
    reason,
  });
}

/**
 * De-elevate admin session.
 */
export async function adminDeElevate(): Promise<void> {
  return invoke<void>("admin_de_elevate");
}

/**
 * Ban a user (requires elevation).
 */
export async function adminBanUser(
  userId: string,
  reason: string,
  expiresAt?: string
): Promise<{ banned: boolean; user_id: string }> {
  return invoke("admin_ban_user", {
    user_id: userId,
    reason,
    expires_at: expiresAt,
  });
}

/**
 * Unban a user (requires elevation).
 */
export async function adminUnbanUser(
  userId: string
): Promise<{ banned: boolean; user_id: string }> {
  return invoke("admin_unban_user", { user_id: userId });
}

/**
 * Suspend a guild (requires elevation).
 */
export async function adminSuspendGuild(
  guildId: string,
  reason: string
): Promise<{ suspended: boolean; guild_id: string }> {
  return invoke("admin_suspend_guild", { guild_id: guildId, reason });
}

/**
 * Unsuspend a guild (requires elevation).
 */
export async function adminUnsuspendGuild(
  guildId: string
): Promise<{ suspended: boolean; guild_id: string }> {
  return invoke("admin_unsuspend_guild", { guild_id: guildId });
}
```

**Step 2: Commit**

```bash
git add client/src/lib/tauri.ts
git commit -m "feat(admin): add TypeScript API functions for admin commands"
```

---

## Task 4: Create Admin Store

**Files:**
- Create: `client/src/stores/admin.ts`

**Step 1: Create admin store**

Create `client/src/stores/admin.ts`:

```typescript
/**
 * Admin Store
 *
 * Manages system admin state: status, elevation, users, guilds, audit log.
 */

import { createStore } from "solid-js/store";
import * as tauri from "@/lib/tauri";

// State interface
interface AdminState {
  // Status
  isAdmin: boolean;
  isElevated: boolean;
  elevationExpiresAt: Date | null;
  isStatusLoading: boolean;

  // Stats
  stats: {
    userCount: number;
    guildCount: number;
    bannedCount: number;
  } | null;

  // Users panel
  users: tauri.UserSummary[];
  usersTotal: number;
  usersPage: number;
  usersLoading: boolean;
  selectedUserId: string | null;

  // Guilds panel
  guilds: tauri.GuildSummary[];
  guildsTotal: number;
  guildsPage: number;
  guildsLoading: boolean;
  selectedGuildId: string | null;

  // Audit log panel
  auditLog: tauri.AuditLogEntry[];
  auditLogTotal: number;
  auditLogPage: number;
  auditLogLoading: boolean;
  auditLogFilter: string | null;

  // Errors
  error: string | null;
}

const PAGE_SIZE = 20;

// Create the store
const [adminState, setAdminState] = createStore<AdminState>({
  isAdmin: false,
  isElevated: false,
  elevationExpiresAt: null,
  isStatusLoading: false,
  stats: null,
  users: [],
  usersTotal: 0,
  usersPage: 0,
  usersLoading: false,
  selectedUserId: null,
  guilds: [],
  guildsTotal: 0,
  guildsPage: 0,
  guildsLoading: false,
  selectedGuildId: null,
  auditLog: [],
  auditLogTotal: 0,
  auditLogPage: 0,
  auditLogLoading: false,
  auditLogFilter: null,
  error: null,
});

// Elevation timer
let elevationTimer: ReturnType<typeof setInterval> | null = null;

function startElevationTimer() {
  if (elevationTimer) clearInterval(elevationTimer);

  elevationTimer = setInterval(() => {
    const expiresAt = adminState.elevationExpiresAt;
    if (!expiresAt) {
      if (elevationTimer) clearInterval(elevationTimer);
      return;
    }

    if (new Date() >= expiresAt) {
      setAdminState({
        isElevated: false,
        elevationExpiresAt: null,
      });
      if (elevationTimer) clearInterval(elevationTimer);
    }
  }, 1000);
}

// Actions

/**
 * Check admin status.
 */
export async function checkAdminStatus(): Promise<void> {
  setAdminState({ isStatusLoading: true, error: null });

  try {
    const status = await tauri.checkAdminStatus();
    setAdminState({
      isAdmin: status.is_admin,
      isElevated: status.is_elevated,
      elevationExpiresAt: status.elevation_expires_at
        ? new Date(status.elevation_expires_at)
        : null,
      isStatusLoading: false,
    });

    if (status.is_elevated) {
      startElevationTimer();
    }
  } catch (err) {
    console.error("Failed to check admin status:", err);
    setAdminState({
      isAdmin: false,
      isStatusLoading: false,
    });
  }
}

/**
 * Load admin stats.
 */
export async function loadAdminStats(): Promise<void> {
  try {
    const stats = await tauri.getAdminStats();
    setAdminState({
      stats: {
        userCount: stats.user_count,
        guildCount: stats.guild_count,
        bannedCount: stats.banned_count,
      },
    });
  } catch (err) {
    console.error("Failed to load admin stats:", err);
  }
}

/**
 * Elevate session with MFA code.
 */
export async function elevateSession(
  mfaCode: string,
  reason?: string
): Promise<void> {
  setAdminState({ error: null });

  try {
    const response = await tauri.adminElevate(mfaCode, reason);
    setAdminState({
      isElevated: true,
      elevationExpiresAt: new Date(response.expires_at),
    });
    startElevationTimer();
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    setAdminState({ error });
    throw err;
  }
}

/**
 * De-elevate session.
 */
export async function deElevateSession(): Promise<void> {
  try {
    await tauri.adminDeElevate();
    setAdminState({
      isElevated: false,
      elevationExpiresAt: null,
    });
    if (elevationTimer) clearInterval(elevationTimer);
  } catch (err) {
    console.error("Failed to de-elevate:", err);
  }
}

/**
 * Get remaining elevation time as formatted string.
 */
export function getElevationTimeRemaining(): string {
  const expiresAt = adminState.elevationExpiresAt;
  if (!expiresAt) return "";

  const now = new Date();
  const diff = expiresAt.getTime() - now.getTime();
  if (diff <= 0) return "0:00";

  const minutes = Math.floor(diff / 60000);
  const seconds = Math.floor((diff % 60000) / 1000);
  return `${minutes}:${seconds.toString().padStart(2, "0")}`;
}

/**
 * Load users with pagination.
 */
export async function loadUsers(page = 0): Promise<void> {
  setAdminState({ usersLoading: true, error: null });

  try {
    const response = await tauri.adminListUsers(PAGE_SIZE, page * PAGE_SIZE);
    setAdminState({
      users: response.items,
      usersTotal: response.total,
      usersPage: page,
      usersLoading: false,
    });
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    setAdminState({ usersLoading: false, error });
  }
}

/**
 * Load guilds with pagination.
 */
export async function loadGuilds(page = 0): Promise<void> {
  setAdminState({ guildsLoading: true, error: null });

  try {
    const response = await tauri.adminListGuilds(PAGE_SIZE, page * PAGE_SIZE);
    setAdminState({
      guilds: response.items,
      guildsTotal: response.total,
      guildsPage: page,
      guildsLoading: false,
    });
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    setAdminState({ guildsLoading: false, error });
  }
}

/**
 * Load audit log with pagination and optional filter.
 */
export async function loadAuditLog(
  page = 0,
  actionFilter?: string
): Promise<void> {
  setAdminState({ auditLogLoading: true, error: null });

  try {
    const response = await tauri.adminGetAuditLog(
      PAGE_SIZE,
      page * PAGE_SIZE,
      actionFilter
    );
    setAdminState({
      auditLog: response.items,
      auditLogTotal: response.total,
      auditLogPage: page,
      auditLogFilter: actionFilter ?? null,
      auditLogLoading: false,
    });
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    setAdminState({ auditLogLoading: false, error });
  }
}

/**
 * Ban a user.
 */
export async function banUser(
  userId: string,
  reason: string
): Promise<void> {
  setAdminState({ error: null });

  try {
    await tauri.adminBanUser(userId, reason);
    // Refresh users list
    await loadUsers(adminState.usersPage);
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    setAdminState({ error });
    throw err;
  }
}

/**
 * Unban a user.
 */
export async function unbanUser(userId: string): Promise<void> {
  setAdminState({ error: null });

  try {
    await tauri.adminUnbanUser(userId);
    // Refresh users list
    await loadUsers(adminState.usersPage);
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    setAdminState({ error });
    throw err;
  }
}

/**
 * Suspend a guild.
 */
export async function suspendGuild(
  guildId: string,
  reason: string
): Promise<void> {
  setAdminState({ error: null });

  try {
    await tauri.adminSuspendGuild(guildId, reason);
    // Refresh guilds list
    await loadGuilds(adminState.guildsPage);
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    setAdminState({ error });
    throw err;
  }
}

/**
 * Unsuspend a guild.
 */
export async function unsuspendGuild(guildId: string): Promise<void> {
  setAdminState({ error: null });

  try {
    await tauri.adminUnsuspendGuild(guildId);
    // Refresh guilds list
    await loadGuilds(adminState.guildsPage);
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    setAdminState({ error });
    throw err;
  }
}

/**
 * Select a user.
 */
export function selectUser(userId: string | null): void {
  setAdminState({ selectedUserId: userId });
}

/**
 * Select a guild.
 */
export function selectGuild(guildId: string | null): void {
  setAdminState({ selectedGuildId: guildId });
}

/**
 * Clear admin error.
 */
export function clearError(): void {
  setAdminState({ error: null });
}

/**
 * Reset admin state (for logout).
 */
export function resetAdminState(): void {
  if (elevationTimer) clearInterval(elevationTimer);
  setAdminState({
    isAdmin: false,
    isElevated: false,
    elevationExpiresAt: null,
    isStatusLoading: false,
    stats: null,
    users: [],
    usersTotal: 0,
    usersPage: 0,
    usersLoading: false,
    selectedUserId: null,
    guilds: [],
    guildsTotal: 0,
    guildsPage: 0,
    guildsLoading: false,
    selectedGuildId: null,
    auditLog: [],
    auditLogTotal: 0,
    auditLogPage: 0,
    auditLogLoading: false,
    auditLogFilter: null,
    error: null,
  });
}

// Export state
export { adminState };
```

**Step 2: Commit**

```bash
git add client/src/stores/admin.ts
git commit -m "feat(admin): add admin store for state management"
```

---

## Task 5: Create AdminQuickModal Component

**Files:**
- Create: `client/src/components/admin/AdminQuickModal.tsx`
- Create: `client/src/components/admin/index.ts`

**Step 1: Create AdminQuickModal**

Create `client/src/components/admin/AdminQuickModal.tsx`:

```typescript
/**
 * AdminQuickModal - Quick admin access modal
 *
 * Shows elevation status, quick stats, and link to full dashboard.
 * Only shown to system admins.
 */

import { Component, Show, onMount, createSignal, createEffect } from "solid-js";
import { X, Shield, ShieldAlert, Users, Building2, Ban, ExternalLink } from "lucide-solid";
import { useNavigate } from "@solidjs/router";
import {
  adminState,
  checkAdminStatus,
  loadAdminStats,
  elevateSession,
  deElevateSession,
  getElevationTimeRemaining,
} from "@/stores/admin";

interface AdminQuickModalProps {
  onClose: () => void;
}

const AdminQuickModal: Component<AdminQuickModalProps> = (props) => {
  const navigate = useNavigate();
  const [mfaCode, setMfaCode] = createSignal("");
  const [elevating, setElevating] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [timeRemaining, setTimeRemaining] = createSignal("");

  onMount(async () => {
    await checkAdminStatus();
    if (adminState.isAdmin) {
      await loadAdminStats();
    }
  });

  // Update time remaining every second
  createEffect(() => {
    if (adminState.isElevated) {
      const interval = setInterval(() => {
        setTimeRemaining(getElevationTimeRemaining());
      }, 1000);
      return () => clearInterval(interval);
    }
  });

  const handleElevate = async () => {
    if (!mfaCode() || mfaCode().length !== 6) {
      setError("Please enter a 6-digit MFA code");
      return;
    }

    setElevating(true);
    setError(null);

    try {
      await elevateSession(mfaCode());
      setMfaCode("");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to elevate");
    } finally {
      setElevating(false);
    }
  };

  const handleDeElevate = async () => {
    await deElevateSession();
  };

  const openDashboard = () => {
    props.onClose();
    navigate("/admin");
  };

  return (
    <div class="fixed inset-0 z-50 flex items-center justify-center">
      {/* Backdrop */}
      <div
        class="absolute inset-0 bg-black/60"
        onClick={props.onClose}
      />

      {/* Modal */}
      <div
        class="relative w-full max-w-md mx-4 rounded-xl border border-white/10 shadow-2xl overflow-hidden"
        style="background-color: var(--color-surface-layer1)"
      >
        {/* Header */}
        <div class="flex items-center justify-between px-5 py-4 border-b border-white/10">
          <div class="flex items-center gap-2">
            <Shield class="w-5 h-5 text-accent-primary" />
            <h2 class="text-lg font-semibold text-text-primary">Admin Panel</h2>
          </div>
          <button
            onClick={props.onClose}
            class="p-1.5 text-text-secondary hover:text-text-primary hover:bg-white/10 rounded-lg transition-colors"
          >
            <X class="w-5 h-5" />
          </button>
        </div>

        {/* Content */}
        <div class="p-5 space-y-5">
          {/* Loading state */}
          <Show when={adminState.isStatusLoading}>
            <div class="text-center py-4 text-text-secondary">
              Checking admin status...
            </div>
          </Show>

          {/* Not admin */}
          <Show when={!adminState.isStatusLoading && !adminState.isAdmin}>
            <div class="text-center py-4 text-text-secondary">
              You do not have system admin privileges.
            </div>
          </Show>

          {/* Admin content */}
          <Show when={!adminState.isStatusLoading && adminState.isAdmin}>
            {/* Elevation Status */}
            <div class="space-y-3">
              <h3 class="text-sm font-medium text-text-secondary uppercase tracking-wider">
                Session Status
              </h3>
              <div
                class="p-4 rounded-lg border"
                classList={{
                  "border-accent-success/30 bg-accent-success/5": adminState.isElevated,
                  "border-white/10 bg-white/5": !adminState.isElevated,
                }}
              >
                <Show when={!adminState.isElevated}>
                  <div class="flex items-center gap-3">
                    <Shield class="w-6 h-6 text-text-secondary" />
                    <div class="flex-1">
                      <div class="font-medium text-text-primary">Not Elevated</div>
                      <div class="text-sm text-text-secondary">
                        Destructive actions require elevation
                      </div>
                    </div>
                  </div>
                  <div class="mt-4 space-y-3">
                    <input
                      type="text"
                      inputMode="numeric"
                      pattern="[0-9]*"
                      maxLength={6}
                      placeholder="Enter MFA code"
                      value={mfaCode()}
                      onInput={(e) => setMfaCode(e.currentTarget.value.replace(/\D/g, ""))}
                      onKeyDown={(e) => e.key === "Enter" && handleElevate()}
                      class="w-full px-3 py-2 rounded-lg border border-white/10 bg-black/20 text-text-primary placeholder-text-secondary/50 focus:border-accent-primary focus:outline-none text-center text-lg tracking-widest"
                    />
                    <Show when={error()}>
                      <div class="text-sm text-accent-danger">{error()}</div>
                    </Show>
                    <button
                      onClick={handleElevate}
                      disabled={elevating() || mfaCode().length !== 6}
                      class="w-full py-2 px-4 rounded-lg bg-accent-primary text-white font-medium hover:bg-accent-primary/90 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
                    >
                      {elevating() ? "Elevating..." : "Elevate Session"}
                    </button>
                  </div>
                </Show>

                <Show when={adminState.isElevated}>
                  <div class="flex items-center gap-3">
                    <ShieldAlert class="w-6 h-6 text-accent-success" />
                    <div class="flex-1">
                      <div class="font-medium text-text-primary">Elevated</div>
                      <div class="text-sm text-text-secondary">
                        Expires in {timeRemaining()}
                      </div>
                    </div>
                    <button
                      onClick={handleDeElevate}
                      class="px-3 py-1.5 rounded-lg border border-white/10 text-sm text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors"
                    >
                      De-elevate
                    </button>
                  </div>
                </Show>
              </div>
            </div>

            {/* Quick Stats */}
            <div class="space-y-3">
              <h3 class="text-sm font-medium text-text-secondary uppercase tracking-wider">
                Quick Stats
              </h3>
              <div class="grid grid-cols-3 gap-3">
                <div class="p-3 rounded-lg bg-white/5 text-center">
                  <Users class="w-5 h-5 mx-auto mb-1 text-text-secondary" />
                  <div class="text-xl font-bold text-text-primary">
                    {adminState.stats?.userCount ?? "—"}
                  </div>
                  <div class="text-xs text-text-secondary">Users</div>
                </div>
                <div class="p-3 rounded-lg bg-white/5 text-center">
                  <Building2 class="w-5 h-5 mx-auto mb-1 text-text-secondary" />
                  <div class="text-xl font-bold text-text-primary">
                    {adminState.stats?.guildCount ?? "—"}
                  </div>
                  <div class="text-xs text-text-secondary">Guilds</div>
                </div>
                <div class="p-3 rounded-lg bg-white/5 text-center">
                  <Ban class="w-5 h-5 mx-auto mb-1 text-text-secondary" />
                  <div class="text-xl font-bold text-text-primary">
                    {adminState.stats?.bannedCount ?? "—"}
                  </div>
                  <div class="text-xs text-text-secondary">Banned</div>
                </div>
              </div>
            </div>

            {/* Dashboard Link */}
            <button
              onClick={openDashboard}
              class="w-full flex items-center justify-center gap-2 py-3 px-4 rounded-lg border border-white/10 text-text-primary hover:bg-white/5 transition-colors"
            >
              <ExternalLink class="w-4 h-4" />
              Open Full Dashboard
            </button>
            <p class="text-xs text-text-secondary text-center">
              Opens detailed admin view with user management, guild oversight, and audit logs.
            </p>
          </Show>
        </div>
      </div>
    </div>
  );
};

export default AdminQuickModal;
```

**Step 2: Create index.ts**

Create `client/src/components/admin/index.ts`:

```typescript
export { default as AdminQuickModal } from "./AdminQuickModal";
```

**Step 3: Commit**

```bash
git add client/src/components/admin/
git commit -m "feat(admin): add AdminQuickModal component"
```

---

## Task 6: Add Admin Button to UserPanel

**Files:**
- Modify: `client/src/components/layout/UserPanel.tsx`

**Step 1: Update UserPanel to show admin button**

Update `UserPanel.tsx` to add an admin shield button for system admins:

```typescript
/**
 * UserPanel - User Info at Bottom of Sidebar
 *
 * Shows current user's avatar, name, username, and settings button.
 * Shows admin button for system admins.
 * Fixed to the bottom of the sidebar with mt-auto.
 */

import { Component, Show, createSignal, onMount } from "solid-js";
import { Settings, Shield } from "lucide-solid";
import { authState } from "@/stores/auth";
import { adminState, checkAdminStatus } from "@/stores/admin";
import Avatar from "@/components/ui/Avatar";
import { SettingsModal } from "@/components/settings";
import { AdminQuickModal } from "@/components/admin";

const UserPanel: Component = () => {
  const user = () => authState.user;
  const [showSettings, setShowSettings] = createSignal(false);
  const [showAdmin, setShowAdmin] = createSignal(false);

  onMount(async () => {
    // Check admin status when component mounts
    await checkAdminStatus();
  });

  return (
    <>
      <div class="mt-auto p-3 bg-surface-base/50 border-t border-white/5">
        <div class="flex items-center gap-3">
          {/* User info */}
          <Show when={user()}>
            <div class="flex items-center gap-2.5 flex-1 min-w-0">
              <Avatar
                src={user()!.avatar_url}
                alt={user()!.display_name}
                size="sm"
                status={user()!.status}
                showStatus
              />
              <div class="flex-1 min-w-0">
                <div class="text-sm font-semibold text-text-primary truncate">
                  {user()!.display_name}
                </div>
                <div class="text-xs text-text-secondary truncate">
                  @{user()!.username}
                </div>
              </div>
            </div>
          </Show>

          {/* Action buttons */}
          <div class="flex items-center gap-1">
            {/* Admin button - only shown to system admins */}
            <Show when={adminState.isAdmin}>
              <button
                class="p-1.5 hover:bg-white/10 rounded-lg transition-all duration-200"
                classList={{
                  "text-accent-success": adminState.isElevated,
                  "text-text-secondary hover:text-accent-primary": !adminState.isElevated,
                }}
                title={adminState.isElevated ? "Admin Panel (Elevated)" : "Admin Panel"}
                onClick={() => setShowAdmin(true)}
              >
                <Shield class="w-4 h-4" />
              </button>
            </Show>

            {/* Settings button */}
            <button
              class="p-1.5 text-text-secondary hover:text-accent-primary hover:bg-white/10 rounded-lg transition-all duration-200"
              title="User Settings"
              onClick={() => setShowSettings(true)}
            >
              <Settings class="w-4 h-4" />
            </button>
          </div>
        </div>
      </div>

      {/* Settings Modal */}
      <Show when={showSettings()}>
        <SettingsModal onClose={() => setShowSettings(false)} />
      </Show>

      {/* Admin Quick Modal */}
      <Show when={showAdmin()}>
        <AdminQuickModal onClose={() => setShowAdmin(false)} />
      </Show>
    </>
  );
};

export default UserPanel;
```

**Step 2: Commit**

```bash
git add client/src/components/layout/UserPanel.tsx
git commit -m "feat(admin): add admin button to UserPanel for system admins"
```

---

## Task 7: Create AdminSidebar Component

**Files:**
- Create: `client/src/components/admin/AdminSidebar.tsx`

**Step 1: Create AdminSidebar**

Create `client/src/components/admin/AdminSidebar.tsx`:

```typescript
/**
 * AdminSidebar - Navigation sidebar for admin dashboard
 */

import { Component } from "solid-js";
import { LayoutDashboard, Users, Building2, ScrollText } from "lucide-solid";

export type AdminPanel = "overview" | "users" | "guilds" | "audit-log";

interface AdminSidebarProps {
  activePanel: AdminPanel;
  onSelectPanel: (panel: AdminPanel) => void;
}

const AdminSidebar: Component<AdminSidebarProps> = (props) => {
  const items: { id: AdminPanel; label: string; icon: typeof LayoutDashboard }[] = [
    { id: "overview", label: "Overview", icon: LayoutDashboard },
    { id: "users", label: "Users", icon: Users },
    { id: "guilds", label: "Guilds", icon: Building2 },
    { id: "audit-log", label: "Audit Log", icon: ScrollText },
  ];

  return (
    <div class="w-48 flex-shrink-0 border-r border-white/10 p-3 space-y-1">
      {items.map((item) => (
        <button
          onClick={() => props.onSelectPanel(item.id)}
          class="w-full flex items-center gap-3 px-3 py-2 rounded-lg text-sm font-medium transition-colors"
          classList={{
            "bg-accent-primary/10 text-accent-primary": props.activePanel === item.id,
            "text-text-secondary hover:text-text-primary hover:bg-white/5": props.activePanel !== item.id,
          }}
        >
          <item.icon class="w-4 h-4" />
          {item.label}
        </button>
      ))}
    </div>
  );
};

export default AdminSidebar;
```

**Step 2: Update index.ts**

Add to `client/src/components/admin/index.ts`:

```typescript
export { default as AdminQuickModal } from "./AdminQuickModal";
export { default as AdminSidebar } from "./AdminSidebar";
export type { AdminPanel } from "./AdminSidebar";
```

**Step 3: Commit**

```bash
git add client/src/components/admin/
git commit -m "feat(admin): add AdminSidebar navigation component"
```

---

## Task 8: Create UsersPanel Component

**Files:**
- Create: `client/src/components/admin/UsersPanel.tsx`

**Step 1: Create UsersPanel**

Create `client/src/components/admin/UsersPanel.tsx`:

```typescript
/**
 * UsersPanel - User management panel for admin dashboard
 */

import { Component, Show, For, onMount, createSignal } from "solid-js";
import { Search, Ban, CheckCircle, ChevronLeft, ChevronRight } from "lucide-solid";
import {
  adminState,
  loadUsers,
  selectUser,
  banUser,
  unbanUser,
} from "@/stores/admin";
import Avatar from "@/components/ui/Avatar";

const PAGE_SIZE = 20;

const UsersPanel: Component = () => {
  const [searchQuery, setSearchQuery] = createSignal("");
  const [banReason, setBanReason] = createSignal("");
  const [showBanDialog, setShowBanDialog] = createSignal(false);
  const [actionLoading, setActionLoading] = createSignal(false);

  onMount(() => {
    loadUsers(0);
  });

  const totalPages = () => Math.ceil(adminState.usersTotal / PAGE_SIZE);

  const selectedUser = () =>
    adminState.users.find((u) => u.id === adminState.selectedUserId);

  const handleBan = async () => {
    if (!adminState.selectedUserId || !banReason()) return;

    setActionLoading(true);
    try {
      await banUser(adminState.selectedUserId, banReason());
      setShowBanDialog(false);
      setBanReason("");
    } catch (err) {
      console.error("Failed to ban user:", err);
    } finally {
      setActionLoading(false);
    }
  };

  const handleUnban = async () => {
    if (!adminState.selectedUserId) return;

    setActionLoading(true);
    try {
      await unbanUser(adminState.selectedUserId);
    } catch (err) {
      console.error("Failed to unban user:", err);
    } finally {
      setActionLoading(false);
    }
  };

  const formatDate = (dateStr: string) => {
    return new Date(dateStr).toLocaleDateString("en-US", {
      month: "short",
      day: "numeric",
      year: "numeric",
    });
  };

  return (
    <div class="flex-1 flex flex-col">
      {/* Header */}
      <div class="px-6 py-4 border-b border-white/10">
        <h2 class="text-xl font-semibold text-text-primary mb-4">Users</h2>
        <div class="relative">
          <Search class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-text-secondary" />
          <input
            type="text"
            placeholder="Search users..."
            value={searchQuery()}
            onInput={(e) => setSearchQuery(e.currentTarget.value)}
            class="w-full pl-10 pr-4 py-2 rounded-lg border border-white/10 bg-black/20 text-text-primary placeholder-text-secondary/50 focus:border-accent-primary focus:outline-none"
          />
        </div>
      </div>

      {/* Content */}
      <div class="flex-1 flex overflow-hidden">
        {/* User list */}
        <div class="flex-1 overflow-y-auto">
          <Show when={adminState.usersLoading}>
            <div class="p-6 text-center text-text-secondary">Loading users...</div>
          </Show>

          <Show when={!adminState.usersLoading}>
            {/* Table header */}
            <div class="grid grid-cols-4 gap-4 px-6 py-3 text-xs font-medium text-text-secondary uppercase tracking-wider border-b border-white/5">
              <div>Username</div>
              <div>Email</div>
              <div>Joined</div>
              <div>Status</div>
            </div>

            {/* Table rows */}
            <For each={adminState.users}>
              {(user) => (
                <div
                  class="grid grid-cols-4 gap-4 px-6 py-3 text-sm cursor-pointer transition-colors"
                  classList={{
                    "bg-accent-primary/10": adminState.selectedUserId === user.id,
                    "hover:bg-white/5": adminState.selectedUserId !== user.id,
                  }}
                  onClick={() => selectUser(user.id)}
                >
                  <div class="flex items-center gap-2 min-w-0">
                    <Avatar
                      src={null}
                      alt={user.display_name}
                      size="xs"
                    />
                    <span class="truncate text-text-primary">{user.username}</span>
                  </div>
                  <div class="text-text-secondary truncate">
                    {user.email ?? "—"}
                  </div>
                  <div class="text-text-secondary">
                    {formatDate(user.created_at)}
                  </div>
                  <div>
                    {user.is_banned ? (
                      <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs bg-accent-danger/20 text-accent-danger">
                        <Ban class="w-3 h-3" />
                        Banned
                      </span>
                    ) : (
                      <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs bg-accent-success/20 text-accent-success">
                        <CheckCircle class="w-3 h-3" />
                        Active
                      </span>
                    )}
                  </div>
                </div>
              )}
            </For>

            {/* Pagination */}
            <div class="flex items-center justify-between px-6 py-3 border-t border-white/5">
              <span class="text-sm text-text-secondary">
                {adminState.usersTotal} total users
              </span>
              <div class="flex items-center gap-2">
                <button
                  onClick={() => loadUsers(adminState.usersPage - 1)}
                  disabled={adminState.usersPage === 0}
                  class="p-1.5 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10 disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  <ChevronLeft class="w-4 h-4" />
                </button>
                <span class="text-sm text-text-secondary">
                  {adminState.usersPage + 1} / {totalPages()}
                </span>
                <button
                  onClick={() => loadUsers(adminState.usersPage + 1)}
                  disabled={adminState.usersPage >= totalPages() - 1}
                  class="p-1.5 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10 disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  <ChevronRight class="w-4 h-4" />
                </button>
              </div>
            </div>
          </Show>
        </div>

        {/* Detail panel */}
        <Show when={selectedUser()}>
          <div class="w-80 border-l border-white/10 p-6 space-y-4">
            <div class="flex items-center gap-3">
              <Avatar
                src={null}
                alt={selectedUser()!.display_name}
                size="lg"
              />
              <div>
                <div class="font-semibold text-text-primary">
                  {selectedUser()!.display_name}
                </div>
                <div class="text-sm text-text-secondary">
                  @{selectedUser()!.username}
                </div>
              </div>
            </div>

            <div class="space-y-2 text-sm">
              <div class="flex justify-between">
                <span class="text-text-secondary">Email</span>
                <span class="text-text-primary">
                  {selectedUser()!.email ?? "Not set"}
                </span>
              </div>
              <div class="flex justify-between">
                <span class="text-text-secondary">Joined</span>
                <span class="text-text-primary">
                  {formatDate(selectedUser()!.created_at)}
                </span>
              </div>
              <div class="flex justify-between">
                <span class="text-text-secondary">Status</span>
                <span class={selectedUser()!.is_banned ? "text-accent-danger" : "text-accent-success"}>
                  {selectedUser()!.is_banned ? "Banned" : "Active"}
                </span>
              </div>
            </div>

            <div class="pt-4 border-t border-white/10">
              <Show when={!selectedUser()!.is_banned}>
                <button
                  onClick={() => setShowBanDialog(true)}
                  disabled={!adminState.isElevated || actionLoading()}
                  class="w-full py-2 px-4 rounded-lg bg-accent-danger text-white font-medium hover:bg-accent-danger/90 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
                  title={!adminState.isElevated ? "Requires elevation" : ""}
                >
                  Ban User
                </button>
                <Show when={!adminState.isElevated}>
                  <p class="mt-2 text-xs text-text-secondary text-center">
                    Requires elevation
                  </p>
                </Show>
              </Show>

              <Show when={selectedUser()!.is_banned}>
                <button
                  onClick={handleUnban}
                  disabled={!adminState.isElevated || actionLoading()}
                  class="w-full py-2 px-4 rounded-lg bg-accent-success text-white font-medium hover:bg-accent-success/90 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
                  title={!adminState.isElevated ? "Requires elevation" : ""}
                >
                  {actionLoading() ? "Unbanning..." : "Unban User"}
                </button>
                <Show when={!adminState.isElevated}>
                  <p class="mt-2 text-xs text-text-secondary text-center">
                    Requires elevation
                  </p>
                </Show>
              </Show>
            </div>
          </div>
        </Show>
      </div>

      {/* Ban dialog */}
      <Show when={showBanDialog()}>
        <div class="fixed inset-0 z-50 flex items-center justify-center">
          <div class="absolute inset-0 bg-black/60" onClick={() => setShowBanDialog(false)} />
          <div
            class="relative w-full max-w-md mx-4 p-6 rounded-xl border border-white/10"
            style="background-color: var(--color-surface-layer1)"
          >
            <h3 class="text-lg font-semibold text-text-primary mb-4">
              Ban User: {selectedUser()?.username}
            </h3>
            <textarea
              placeholder="Reason for ban..."
              value={banReason()}
              onInput={(e) => setBanReason(e.currentTarget.value)}
              class="w-full h-24 px-3 py-2 rounded-lg border border-white/10 bg-black/20 text-text-primary placeholder-text-secondary/50 focus:border-accent-primary focus:outline-none resize-none"
            />
            <div class="flex justify-end gap-3 mt-4">
              <button
                onClick={() => setShowBanDialog(false)}
                class="px-4 py-2 rounded-lg border border-white/10 text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={handleBan}
                disabled={!banReason() || actionLoading()}
                class="px-4 py-2 rounded-lg bg-accent-danger text-white font-medium hover:bg-accent-danger/90 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
              >
                {actionLoading() ? "Banning..." : "Confirm Ban"}
              </button>
            </div>
          </div>
        </div>
      </Show>
    </div>
  );
};

export default UsersPanel;
```

**Step 2: Update index.ts**

Add to `client/src/components/admin/index.ts`:

```typescript
export { default as UsersPanel } from "./UsersPanel";
```

**Step 3: Commit**

```bash
git add client/src/components/admin/
git commit -m "feat(admin): add UsersPanel component for user management"
```

---

## Task 9: Create GuildsPanel Component

**Files:**
- Create: `client/src/components/admin/GuildsPanel.tsx`

**Step 1: Create GuildsPanel**

Create `client/src/components/admin/GuildsPanel.tsx`:

```typescript
/**
 * GuildsPanel - Guild management panel for admin dashboard
 */

import { Component, Show, For, onMount, createSignal } from "solid-js";
import { Search, Building2, Ban, CheckCircle, ChevronLeft, ChevronRight, Users } from "lucide-solid";
import {
  adminState,
  loadGuilds,
  selectGuild,
  suspendGuild,
  unsuspendGuild,
} from "@/stores/admin";

const PAGE_SIZE = 20;

const GuildsPanel: Component = () => {
  const [searchQuery, setSearchQuery] = createSignal("");
  const [suspendReason, setSuspendReason] = createSignal("");
  const [showSuspendDialog, setShowSuspendDialog] = createSignal(false);
  const [actionLoading, setActionLoading] = createSignal(false);

  onMount(() => {
    loadGuilds(0);
  });

  const totalPages = () => Math.ceil(adminState.guildsTotal / PAGE_SIZE);

  const selectedGuild = () =>
    adminState.guilds.find((g) => g.id === adminState.selectedGuildId);

  const handleSuspend = async () => {
    if (!adminState.selectedGuildId || !suspendReason()) return;

    setActionLoading(true);
    try {
      await suspendGuild(adminState.selectedGuildId, suspendReason());
      setShowSuspendDialog(false);
      setSuspendReason("");
    } catch (err) {
      console.error("Failed to suspend guild:", err);
    } finally {
      setActionLoading(false);
    }
  };

  const handleUnsuspend = async () => {
    if (!adminState.selectedGuildId) return;

    setActionLoading(true);
    try {
      await unsuspendGuild(adminState.selectedGuildId);
    } catch (err) {
      console.error("Failed to unsuspend guild:", err);
    } finally {
      setActionLoading(false);
    }
  };

  const formatDate = (dateStr: string) => {
    return new Date(dateStr).toLocaleDateString("en-US", {
      month: "short",
      day: "numeric",
      year: "numeric",
    });
  };

  return (
    <div class="flex-1 flex flex-col">
      {/* Header */}
      <div class="px-6 py-4 border-b border-white/10">
        <h2 class="text-xl font-semibold text-text-primary mb-4">Guilds</h2>
        <div class="relative">
          <Search class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-text-secondary" />
          <input
            type="text"
            placeholder="Search guilds..."
            value={searchQuery()}
            onInput={(e) => setSearchQuery(e.currentTarget.value)}
            class="w-full pl-10 pr-4 py-2 rounded-lg border border-white/10 bg-black/20 text-text-primary placeholder-text-secondary/50 focus:border-accent-primary focus:outline-none"
          />
        </div>
      </div>

      {/* Content */}
      <div class="flex-1 flex overflow-hidden">
        {/* Guild list */}
        <div class="flex-1 overflow-y-auto">
          <Show when={adminState.guildsLoading}>
            <div class="p-6 text-center text-text-secondary">Loading guilds...</div>
          </Show>

          <Show when={!adminState.guildsLoading}>
            {/* Table header */}
            <div class="grid grid-cols-4 gap-4 px-6 py-3 text-xs font-medium text-text-secondary uppercase tracking-wider border-b border-white/5">
              <div>Name</div>
              <div>Members</div>
              <div>Created</div>
              <div>Status</div>
            </div>

            {/* Table rows */}
            <For each={adminState.guilds}>
              {(guild) => (
                <div
                  class="grid grid-cols-4 gap-4 px-6 py-3 text-sm cursor-pointer transition-colors"
                  classList={{
                    "bg-accent-primary/10": adminState.selectedGuildId === guild.id,
                    "hover:bg-white/5": adminState.selectedGuildId !== guild.id,
                  }}
                  onClick={() => selectGuild(guild.id)}
                >
                  <div class="flex items-center gap-2 min-w-0">
                    <div class="w-8 h-8 rounded-lg bg-accent-primary/20 flex items-center justify-center">
                      <Building2 class="w-4 h-4 text-accent-primary" />
                    </div>
                    <span class="truncate text-text-primary">{guild.name}</span>
                  </div>
                  <div class="flex items-center gap-1 text-text-secondary">
                    <Users class="w-4 h-4" />
                    {guild.member_count}
                  </div>
                  <div class="text-text-secondary">
                    {formatDate(guild.created_at)}
                  </div>
                  <div>
                    {guild.suspended_at ? (
                      <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs bg-accent-danger/20 text-accent-danger">
                        <Ban class="w-3 h-3" />
                        Suspended
                      </span>
                    ) : (
                      <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs bg-accent-success/20 text-accent-success">
                        <CheckCircle class="w-3 h-3" />
                        Active
                      </span>
                    )}
                  </div>
                </div>
              )}
            </For>

            {/* Pagination */}
            <div class="flex items-center justify-between px-6 py-3 border-t border-white/5">
              <span class="text-sm text-text-secondary">
                {adminState.guildsTotal} total guilds
              </span>
              <div class="flex items-center gap-2">
                <button
                  onClick={() => loadGuilds(adminState.guildsPage - 1)}
                  disabled={adminState.guildsPage === 0}
                  class="p-1.5 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10 disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  <ChevronLeft class="w-4 h-4" />
                </button>
                <span class="text-sm text-text-secondary">
                  {adminState.guildsPage + 1} / {totalPages()}
                </span>
                <button
                  onClick={() => loadGuilds(adminState.guildsPage + 1)}
                  disabled={adminState.guildsPage >= totalPages() - 1}
                  class="p-1.5 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10 disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  <ChevronRight class="w-4 h-4" />
                </button>
              </div>
            </div>
          </Show>
        </div>

        {/* Detail panel */}
        <Show when={selectedGuild()}>
          <div class="w-80 border-l border-white/10 p-6 space-y-4">
            <div class="flex items-center gap-3">
              <div class="w-12 h-12 rounded-lg bg-accent-primary/20 flex items-center justify-center">
                <Building2 class="w-6 h-6 text-accent-primary" />
              </div>
              <div>
                <div class="font-semibold text-text-primary">
                  {selectedGuild()!.name}
                </div>
                <div class="text-sm text-text-secondary">
                  {selectedGuild()!.member_count} members
                </div>
              </div>
            </div>

            <div class="space-y-2 text-sm">
              <div class="flex justify-between">
                <span class="text-text-secondary">Owner ID</span>
                <span class="text-text-primary font-mono text-xs">
                  {selectedGuild()!.owner_id.slice(0, 8)}...
                </span>
              </div>
              <div class="flex justify-between">
                <span class="text-text-secondary">Created</span>
                <span class="text-text-primary">
                  {formatDate(selectedGuild()!.created_at)}
                </span>
              </div>
              <div class="flex justify-between">
                <span class="text-text-secondary">Status</span>
                <span class={selectedGuild()!.suspended_at ? "text-accent-danger" : "text-accent-success"}>
                  {selectedGuild()!.suspended_at ? "Suspended" : "Active"}
                </span>
              </div>
              <Show when={selectedGuild()!.suspended_at}>
                <div class="flex justify-between">
                  <span class="text-text-secondary">Suspended</span>
                  <span class="text-text-primary">
                    {formatDate(selectedGuild()!.suspended_at!)}
                  </span>
                </div>
              </Show>
            </div>

            <div class="pt-4 border-t border-white/10">
              <Show when={!selectedGuild()!.suspended_at}>
                <button
                  onClick={() => setShowSuspendDialog(true)}
                  disabled={!adminState.isElevated || actionLoading()}
                  class="w-full py-2 px-4 rounded-lg bg-accent-danger text-white font-medium hover:bg-accent-danger/90 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
                  title={!adminState.isElevated ? "Requires elevation" : ""}
                >
                  Suspend Guild
                </button>
                <Show when={!adminState.isElevated}>
                  <p class="mt-2 text-xs text-text-secondary text-center">
                    Requires elevation
                  </p>
                </Show>
              </Show>

              <Show when={selectedGuild()!.suspended_at}>
                <button
                  onClick={handleUnsuspend}
                  disabled={!adminState.isElevated || actionLoading()}
                  class="w-full py-2 px-4 rounded-lg bg-accent-success text-white font-medium hover:bg-accent-success/90 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
                  title={!adminState.isElevated ? "Requires elevation" : ""}
                >
                  {actionLoading() ? "Unsuspending..." : "Unsuspend Guild"}
                </button>
                <Show when={!adminState.isElevated}>
                  <p class="mt-2 text-xs text-text-secondary text-center">
                    Requires elevation
                  </p>
                </Show>
              </Show>
            </div>
          </div>
        </Show>
      </div>

      {/* Suspend dialog */}
      <Show when={showSuspendDialog()}>
        <div class="fixed inset-0 z-50 flex items-center justify-center">
          <div class="absolute inset-0 bg-black/60" onClick={() => setShowSuspendDialog(false)} />
          <div
            class="relative w-full max-w-md mx-4 p-6 rounded-xl border border-white/10"
            style="background-color: var(--color-surface-layer1)"
          >
            <h3 class="text-lg font-semibold text-text-primary mb-4">
              Suspend Guild: {selectedGuild()?.name}
            </h3>
            <textarea
              placeholder="Reason for suspension..."
              value={suspendReason()}
              onInput={(e) => setSuspendReason(e.currentTarget.value)}
              class="w-full h-24 px-3 py-2 rounded-lg border border-white/10 bg-black/20 text-text-primary placeholder-text-secondary/50 focus:border-accent-primary focus:outline-none resize-none"
            />
            <div class="flex justify-end gap-3 mt-4">
              <button
                onClick={() => setShowSuspendDialog(false)}
                class="px-4 py-2 rounded-lg border border-white/10 text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={handleSuspend}
                disabled={!suspendReason() || actionLoading()}
                class="px-4 py-2 rounded-lg bg-accent-danger text-white font-medium hover:bg-accent-danger/90 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
              >
                {actionLoading() ? "Suspending..." : "Confirm Suspend"}
              </button>
            </div>
          </div>
        </div>
      </Show>
    </div>
  );
};

export default GuildsPanel;
```

**Step 2: Update index.ts**

Add to `client/src/components/admin/index.ts`:

```typescript
export { default as GuildsPanel } from "./GuildsPanel";
```

**Step 3: Commit**

```bash
git add client/src/components/admin/
git commit -m "feat(admin): add GuildsPanel component for guild management"
```

---

## Task 10: Create AuditLogPanel Component

**Files:**
- Create: `client/src/components/admin/AuditLogPanel.tsx`

**Step 1: Create AuditLogPanel**

Create `client/src/components/admin/AuditLogPanel.tsx`:

```typescript
/**
 * AuditLogPanel - Audit log viewer for admin dashboard
 */

import { Component, Show, For, onMount, createSignal } from "solid-js";
import { Filter, ChevronLeft, ChevronRight, User, Building2, Shield, FileText } from "lucide-solid";
import { adminState, loadAuditLog } from "@/stores/admin";

const PAGE_SIZE = 20;

const AuditLogPanel: Component = () => {
  const [filterValue, setFilterValue] = createSignal("");

  onMount(() => {
    loadAuditLog(0);
  });

  const totalPages = () => Math.ceil(adminState.auditLogTotal / PAGE_SIZE);

  const applyFilter = () => {
    loadAuditLog(0, filterValue() || undefined);
  };

  const clearFilter = () => {
    setFilterValue("");
    loadAuditLog(0);
  };

  const formatDate = (dateStr: string) => {
    const date = new Date(dateStr);
    return date.toLocaleDateString("en-US", {
      month: "short",
      day: "numeric",
      year: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  };

  const getActionIcon = (action: string) => {
    if (action.startsWith("admin.users")) return User;
    if (action.startsWith("admin.guilds")) return Building2;
    if (action.startsWith("admin.session")) return Shield;
    return FileText;
  };

  const getActionColor = (action: string) => {
    if (action.includes("ban") || action.includes("suspend")) return "text-accent-danger";
    if (action.includes("unban") || action.includes("unsuspend")) return "text-accent-success";
    if (action.includes("elevate")) return "text-accent-warning";
    return "text-text-secondary";
  };

  const formatAction = (action: string) => {
    // Convert "admin.users.ban" to "Ban User"
    return action
      .replace("admin.", "")
      .split(".")
      .reverse()
      .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
      .join(" ");
  };

  return (
    <div class="flex-1 flex flex-col">
      {/* Header */}
      <div class="px-6 py-4 border-b border-white/10">
        <h2 class="text-xl font-semibold text-text-primary mb-4">Audit Log</h2>
        <div class="flex gap-2">
          <div class="relative flex-1">
            <Filter class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-text-secondary" />
            <input
              type="text"
              placeholder="Filter by action (e.g., admin.users)"
              value={filterValue()}
              onInput={(e) => setFilterValue(e.currentTarget.value)}
              onKeyDown={(e) => e.key === "Enter" && applyFilter()}
              class="w-full pl-10 pr-4 py-2 rounded-lg border border-white/10 bg-black/20 text-text-primary placeholder-text-secondary/50 focus:border-accent-primary focus:outline-none"
            />
          </div>
          <button
            onClick={applyFilter}
            class="px-4 py-2 rounded-lg bg-accent-primary text-white font-medium hover:bg-accent-primary/90 transition-colors"
          >
            Filter
          </button>
          <Show when={adminState.auditLogFilter}>
            <button
              onClick={clearFilter}
              class="px-4 py-2 rounded-lg border border-white/10 text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors"
            >
              Clear
            </button>
          </Show>
        </div>
        <Show when={adminState.auditLogFilter}>
          <div class="mt-2 text-sm text-text-secondary">
            Filtering by: <span class="text-accent-primary">{adminState.auditLogFilter}</span>
          </div>
        </Show>
      </div>

      {/* Content */}
      <div class="flex-1 overflow-y-auto">
        <Show when={adminState.auditLogLoading}>
          <div class="p-6 text-center text-text-secondary">Loading audit log...</div>
        </Show>

        <Show when={!adminState.auditLogLoading}>
          {/* Table header */}
          <div class="grid grid-cols-5 gap-4 px-6 py-3 text-xs font-medium text-text-secondary uppercase tracking-wider border-b border-white/5">
            <div>Action</div>
            <div>Actor</div>
            <div>Target</div>
            <div>IP Address</div>
            <div>Time</div>
          </div>

          {/* Table rows */}
          <For each={adminState.auditLog}>
            {(entry) => {
              const Icon = getActionIcon(entry.action);
              return (
                <div class="grid grid-cols-5 gap-4 px-6 py-3 text-sm hover:bg-white/5 transition-colors border-b border-white/5">
                  <div class="flex items-center gap-2 min-w-0">
                    <Icon class={`w-4 h-4 ${getActionColor(entry.action)}`} />
                    <span class={`truncate ${getActionColor(entry.action)}`}>
                      {formatAction(entry.action)}
                    </span>
                  </div>
                  <div class="text-text-primary truncate">
                    {entry.actor_username ?? entry.actor_id.slice(0, 8)}
                  </div>
                  <div class="text-text-secondary truncate">
                    <Show when={entry.target_type}>
                      {entry.target_type}: {entry.target_id?.slice(0, 8)}...
                    </Show>
                    <Show when={!entry.target_type}>—</Show>
                  </div>
                  <div class="text-text-secondary font-mono text-xs">
                    {entry.ip_address ?? "—"}
                  </div>
                  <div class="text-text-secondary">
                    {formatDate(entry.created_at)}
                  </div>
                </div>
              );
            }}
          </For>

          {/* Empty state */}
          <Show when={adminState.auditLog.length === 0}>
            <div class="p-6 text-center text-text-secondary">
              No audit log entries found.
            </div>
          </Show>

          {/* Pagination */}
          <Show when={adminState.auditLog.length > 0}>
            <div class="flex items-center justify-between px-6 py-3 border-t border-white/5">
              <span class="text-sm text-text-secondary">
                {adminState.auditLogTotal} total entries
              </span>
              <div class="flex items-center gap-2">
                <button
                  onClick={() => loadAuditLog(adminState.auditLogPage - 1, adminState.auditLogFilter ?? undefined)}
                  disabled={adminState.auditLogPage === 0}
                  class="p-1.5 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10 disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  <ChevronLeft class="w-4 h-4" />
                </button>
                <span class="text-sm text-text-secondary">
                  {adminState.auditLogPage + 1} / {totalPages()}
                </span>
                <button
                  onClick={() => loadAuditLog(adminState.auditLogPage + 1, adminState.auditLogFilter ?? undefined)}
                  disabled={adminState.auditLogPage >= totalPages() - 1}
                  class="p-1.5 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10 disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  <ChevronRight class="w-4 h-4" />
                </button>
              </div>
            </div>
          </Show>
        </Show>
      </div>
    </div>
  );
};

export default AuditLogPanel;
```

**Step 2: Update index.ts**

Add to `client/src/components/admin/index.ts`:

```typescript
export { default as AuditLogPanel } from "./AuditLogPanel";
```

**Step 3: Commit**

```bash
git add client/src/components/admin/
git commit -m "feat(admin): add AuditLogPanel component for audit log viewing"
```

---

## Task 11: Create AdminDashboard Page

**Files:**
- Create: `client/src/views/AdminDashboard.tsx`

**Step 1: Create AdminDashboard**

Create `client/src/views/AdminDashboard.tsx`:

```typescript
/**
 * AdminDashboard - Full admin dashboard page at /admin
 */

import { Component, Show, createSignal, onMount, createEffect } from "solid-js";
import { useNavigate } from "@solidjs/router";
import { ArrowLeft, Shield, ShieldAlert, Users, Building2, Ban } from "lucide-solid";
import {
  adminState,
  checkAdminStatus,
  loadAdminStats,
  getElevationTimeRemaining,
} from "@/stores/admin";
import {
  AdminSidebar,
  UsersPanel,
  GuildsPanel,
  AuditLogPanel,
  type AdminPanel,
} from "@/components/admin";

const AdminDashboard: Component = () => {
  const navigate = useNavigate();
  const [activePanel, setActivePanel] = createSignal<AdminPanel>("overview");
  const [timeRemaining, setTimeRemaining] = createSignal("");

  onMount(async () => {
    await checkAdminStatus();
    if (adminState.isAdmin) {
      await loadAdminStats();
    }
  });

  // Update time remaining every second
  createEffect(() => {
    if (adminState.isElevated) {
      const interval = setInterval(() => {
        setTimeRemaining(getElevationTimeRemaining());
      }, 1000);
      return () => clearInterval(interval);
    }
  });

  // Redirect non-admins
  createEffect(() => {
    if (!adminState.isStatusLoading && !adminState.isAdmin) {
      navigate("/");
    }
  });

  return (
    <div class="h-screen flex flex-col bg-background-tertiary">
      {/* Header */}
      <div class="flex items-center justify-between px-6 py-4 border-b border-white/10 bg-surface-base">
        <div class="flex items-center gap-4">
          <button
            onClick={() => navigate("/")}
            class="flex items-center gap-2 text-text-secondary hover:text-text-primary transition-colors"
          >
            <ArrowLeft class="w-4 h-4" />
            Back to App
          </button>
        </div>
        <div class="flex items-center gap-4">
          <h1 class="text-lg font-semibold text-text-primary">Admin Dashboard</h1>
          <Show when={adminState.isElevated}>
            <div class="flex items-center gap-2 px-3 py-1.5 rounded-full bg-accent-success/20 text-accent-success text-sm">
              <ShieldAlert class="w-4 h-4" />
              Elevated ({timeRemaining()})
            </div>
          </Show>
          <Show when={!adminState.isElevated}>
            <div class="flex items-center gap-2 px-3 py-1.5 rounded-full bg-white/10 text-text-secondary text-sm">
              <Shield class="w-4 h-4" />
              Not Elevated
            </div>
          </Show>
        </div>
      </div>

      {/* Main content */}
      <div class="flex-1 flex overflow-hidden">
        {/* Loading state */}
        <Show when={adminState.isStatusLoading}>
          <div class="flex-1 flex items-center justify-center text-text-secondary">
            Loading admin status...
          </div>
        </Show>

        {/* Admin content */}
        <Show when={!adminState.isStatusLoading && adminState.isAdmin}>
          {/* Sidebar */}
          <AdminSidebar activePanel={activePanel()} onSelectPanel={setActivePanel} />

          {/* Panel content */}
          <div class="flex-1 flex flex-col overflow-hidden" style="background-color: var(--color-surface-layer1)">
            {/* Overview panel */}
            <Show when={activePanel() === "overview"}>
              <div class="p-6 space-y-6">
                <h2 class="text-xl font-semibold text-text-primary">Overview</h2>

                {/* Quick Stats */}
                <div class="grid grid-cols-3 gap-4">
                  <div class="p-6 rounded-xl border border-white/10 bg-white/5">
                    <div class="flex items-center gap-3 mb-2">
                      <Users class="w-5 h-5 text-accent-primary" />
                      <span class="text-text-secondary">Total Users</span>
                    </div>
                    <div class="text-3xl font-bold text-text-primary">
                      {adminState.stats?.userCount ?? "—"}
                    </div>
                  </div>
                  <div class="p-6 rounded-xl border border-white/10 bg-white/5">
                    <div class="flex items-center gap-3 mb-2">
                      <Building2 class="w-5 h-5 text-accent-primary" />
                      <span class="text-text-secondary">Total Guilds</span>
                    </div>
                    <div class="text-3xl font-bold text-text-primary">
                      {adminState.stats?.guildCount ?? "—"}
                    </div>
                  </div>
                  <div class="p-6 rounded-xl border border-white/10 bg-white/5">
                    <div class="flex items-center gap-3 mb-2">
                      <Ban class="w-5 h-5 text-accent-danger" />
                      <span class="text-text-secondary">Banned Users</span>
                    </div>
                    <div class="text-3xl font-bold text-text-primary">
                      {adminState.stats?.bannedCount ?? "—"}
                    </div>
                  </div>
                </div>

                {/* Quick actions */}
                <div class="space-y-3">
                  <h3 class="text-sm font-medium text-text-secondary uppercase tracking-wider">
                    Quick Actions
                  </h3>
                  <div class="flex gap-3">
                    <button
                      onClick={() => setActivePanel("users")}
                      class="flex items-center gap-2 px-4 py-2 rounded-lg border border-white/10 text-text-primary hover:bg-white/5 transition-colors"
                    >
                      <Users class="w-4 h-4" />
                      Manage Users
                    </button>
                    <button
                      onClick={() => setActivePanel("guilds")}
                      class="flex items-center gap-2 px-4 py-2 rounded-lg border border-white/10 text-text-primary hover:bg-white/5 transition-colors"
                    >
                      <Building2 class="w-4 h-4" />
                      Manage Guilds
                    </button>
                    <button
                      onClick={() => setActivePanel("audit-log")}
                      class="flex items-center gap-2 px-4 py-2 rounded-lg border border-white/10 text-text-primary hover:bg-white/5 transition-colors"
                    >
                      View Audit Log
                    </button>
                  </div>
                </div>

                {/* Elevation notice */}
                <Show when={!adminState.isElevated}>
                  <div class="p-4 rounded-lg border border-accent-warning/30 bg-accent-warning/5">
                    <div class="flex items-center gap-3">
                      <Shield class="w-5 h-5 text-accent-warning" />
                      <div>
                        <div class="font-medium text-text-primary">Session Not Elevated</div>
                        <div class="text-sm text-text-secondary">
                          Destructive actions (ban users, suspend guilds) require an elevated session.
                          Use the Admin Panel in the sidebar to elevate.
                        </div>
                      </div>
                    </div>
                  </div>
                </Show>
              </div>
            </Show>

            {/* Users panel */}
            <Show when={activePanel() === "users"}>
              <UsersPanel />
            </Show>

            {/* Guilds panel */}
            <Show when={activePanel() === "guilds"}>
              <GuildsPanel />
            </Show>

            {/* Audit Log panel */}
            <Show when={activePanel() === "audit-log"}>
              <AuditLogPanel />
            </Show>
          </div>
        </Show>
      </div>
    </div>
  );
};

export default AdminDashboard;
```

**Step 2: Commit**

```bash
git add client/src/views/AdminDashboard.tsx
git commit -m "feat(admin): add AdminDashboard page"
```

---

## Task 12: Add Admin Route to App.tsx

**Files:**
- Modify: `client/src/App.tsx`

**Step 1: Import and add admin route**

Update `client/src/App.tsx` to add the `/admin` route:

```typescript
import { Component, ParentProps, JSX, onMount } from "solid-js";
import { Route } from "@solidjs/router";

// Views
import Login from "./views/Login";
import Register from "./views/Register";
import Main from "./views/Main";
import ThemeDemo from "./pages/ThemeDemo";
import InviteJoin from "./views/InviteJoin";
import PageViewRoute from "./views/PageViewRoute";
import AdminDashboard from "./views/AdminDashboard";

// Components
import AuthGuard from "./components/auth/AuthGuard";
import { AcceptanceManager } from "./components/pages";

// Theme
import { initTheme } from "./stores/theme";

// Layout wrapper
const Layout: Component<ParentProps> = (props) => {
  onMount(async () => {
    await initTheme();
  });

  return (
    <div class="h-screen bg-background-tertiary text-text-primary">
      {props.children}
    </div>
  );
};

// Protected route wrapper
const ProtectedMain: Component = () => (
  <AuthGuard>
    <AcceptanceManager />
    <Main />
  </AuthGuard>
);

// Protected invite wrapper (needs auth check but shows loading state)
const ProtectedInvite: Component = () => (
  <AuthGuard>
    <InviteJoin />
  </AuthGuard>
);

// Protected page view wrapper
const ProtectedPageView: Component = () => (
  <AuthGuard>
    <PageViewRoute />
  </AuthGuard>
);

// Protected admin wrapper
const ProtectedAdmin: Component = () => (
  <AuthGuard>
    <AdminDashboard />
  </AuthGuard>
);

// Wrapped components for routes
const LoginPage = () => <Layout><Login /></Layout>;
const RegisterPage = () => <Layout><Register /></Layout>;
const MainPage = () => <Layout><ProtectedMain /></Layout>;
const ThemeDemoPage = () => <Layout><ThemeDemo /></Layout>;
const InvitePage = () => <Layout><ProtectedInvite /></Layout>;
const PagePage = () => <Layout><ProtectedPageView /></Layout>;
const AdminPage = () => <Layout><ProtectedAdmin /></Layout>;

// Export routes as JSX Route elements
export const AppRoutes = (): JSX.Element => (
  <>
    <Route path="/demo" component={ThemeDemoPage} />
    <Route path="/login" component={LoginPage} />
    <Route path="/register" component={RegisterPage} />
    <Route path="/invite/:code" component={InvitePage} />
    <Route path="/pages/:slug" component={PagePage} />
    <Route path="/guilds/:guildId/pages/:slug" component={PagePage} />
    <Route path="/admin" component={AdminPage} />
    <Route path="/*" component={MainPage} />
  </>
);

export default AppRoutes;
```

**Step 2: Commit**

```bash
git add client/src/App.tsx
git commit -m "feat(admin): add /admin route to App.tsx"
```

---

## Task 13: Update Admin Index Exports

**Files:**
- Modify: `client/src/components/admin/index.ts`

**Step 1: Ensure all exports are present**

Update `client/src/components/admin/index.ts` to export everything:

```typescript
export { default as AdminQuickModal } from "./AdminQuickModal";
export { default as AdminSidebar } from "./AdminSidebar";
export { default as UsersPanel } from "./UsersPanel";
export { default as GuildsPanel } from "./GuildsPanel";
export { default as AuditLogPanel } from "./AuditLogPanel";
export type { AdminPanel } from "./AdminSidebar";
```

**Step 2: Commit**

```bash
git add client/src/components/admin/index.ts
git commit -m "chore(admin): update admin component exports"
```

---

## Task 14: Update CHANGELOG

**Files:**
- Modify: `CHANGELOG.md`

**Step 1: Add changelog entry**

Add entry under `[Unreleased]` → `### Added`:

```markdown
- Admin Dashboard with user management, guild oversight, and audit log viewing
- AdminQuickModal for quick admin access with elevation status and stats
- Session elevation system with MFA verification and 15-minute expiry
- Ban/unban users and suspend/unsuspend guilds (requires elevation)
```

**Step 2: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs: add Admin Dashboard to CHANGELOG"
```

---

## Summary

**Total tasks:** 14

**New files created:**
- `client/src-tauri/src/commands/admin.rs`
- `client/src/stores/admin.ts`
- `client/src/components/admin/AdminQuickModal.tsx`
- `client/src/components/admin/AdminSidebar.tsx`
- `client/src/components/admin/UsersPanel.tsx`
- `client/src/components/admin/GuildsPanel.tsx`
- `client/src/components/admin/AuditLogPanel.tsx`
- `client/src/components/admin/index.ts`
- `client/src/views/AdminDashboard.tsx`

**Files modified:**
- `client/src/lib/types.ts`
- `client/src/lib/tauri.ts`
- `client/src-tauri/src/commands/mod.rs`
- `client/src-tauri/src/lib.rs`
- `client/src/components/layout/UserPanel.tsx`
- `client/src/App.tsx`
- `CHANGELOG.md`
