/**
 * Admin Store
 *
 * Manages admin dashboard state including user/guild management,
 * session elevation, and audit log.
 */

import { createStore } from "solid-js/store";
import type {
  AdminStats,
  UserSummary,
  GuildSummary,
  AuditLogEntry,
  PaginatedResponse,
} from "@/lib/types";
import * as tauri from "@/lib/tauri";

// ============================================================================
// Types
// ============================================================================

/**
 * Pagination state for lists
 */
interface PaginationState {
  page: number;
  pageSize: number;
  total: number;
}

/**
 * Admin store state
 */
interface AdminStoreState {
  // Admin status
  isAdmin: boolean;
  isElevated: boolean;
  elevationExpiresAt: string | null;

  // Statistics
  stats: AdminStats | null;

  // Users list
  users: UserSummary[];
  usersPagination: PaginationState;
  selectedUserId: string | null;

  // Guilds list
  guilds: GuildSummary[];
  guildsPagination: PaginationState;
  selectedGuildId: string | null;

  // Audit log
  auditLog: AuditLogEntry[];
  auditLogPagination: PaginationState;
  auditLogFilter: string | null;

  // Loading states
  isStatusLoading: boolean;
  isStatsLoading: boolean;
  isUsersLoading: boolean;
  isGuildsLoading: boolean;
  isAuditLogLoading: boolean;
  isElevating: boolean;

  // Error state
  error: string | null;
}

// Default page size for lists
const DEFAULT_PAGE_SIZE = 20;

// Create the store
const [adminState, setAdminState] = createStore<AdminStoreState>({
  // Admin status
  isAdmin: false,
  isElevated: false,
  elevationExpiresAt: null,

  // Statistics
  stats: null,

  // Users list
  users: [],
  usersPagination: { page: 1, pageSize: DEFAULT_PAGE_SIZE, total: 0 },
  selectedUserId: null,

  // Guilds list
  guilds: [],
  guildsPagination: { page: 1, pageSize: DEFAULT_PAGE_SIZE, total: 0 },
  selectedGuildId: null,

  // Audit log
  auditLog: [],
  auditLogPagination: { page: 1, pageSize: DEFAULT_PAGE_SIZE, total: 0 },
  auditLogFilter: null,

  // Loading states
  isStatusLoading: false,
  isStatsLoading: false,
  isUsersLoading: false,
  isGuildsLoading: false,
  isAuditLogLoading: false,
  isElevating: false,

  // Error state
  error: null,
});

// ============================================================================
// Elevation Timer
// ============================================================================

let elevationTimer: ReturnType<typeof setInterval> | null = null;

/**
 * Start the elevation timer that auto-clears elevation when expired
 */
function startElevationTimer(): void {
  // Clear any existing timer
  stopElevationTimer();

  // Check every 10 seconds if elevation has expired
  elevationTimer = setInterval(() => {
    if (adminState.elevationExpiresAt) {
      const expiresAt = new Date(adminState.elevationExpiresAt).getTime();
      const now = Date.now();

      if (now >= expiresAt) {
        console.log("[Admin] Elevation expired, clearing state");
        setAdminState({
          isElevated: false,
          elevationExpiresAt: null,
        });
        stopElevationTimer();
      }
    }
  }, 10000);
}

/**
 * Stop the elevation timer
 */
function stopElevationTimer(): void {
  if (elevationTimer) {
    clearInterval(elevationTimer);
    elevationTimer = null;
  }
}

// ============================================================================
// Admin Status Functions
// ============================================================================

/**
 * Check admin status for the current user
 */
export async function checkAdminStatus(): Promise<void> {
  setAdminState({ isStatusLoading: true, error: null });

  try {
    const status = await tauri.checkAdminStatus();
    setAdminState({
      isAdmin: status.is_admin,
      isElevated: status.is_elevated,
      elevationExpiresAt: status.elevation_expires_at,
      isStatusLoading: false,
    });

    // Start elevation timer if elevated
    if (status.is_elevated && status.elevation_expires_at) {
      startElevationTimer();
    }
  } catch (err) {
    console.error("[Admin] Failed to check admin status:", err);
    setAdminState({
      error: err instanceof Error ? err.message : "Failed to check admin status",
      isStatusLoading: false,
    });
  }
}

/**
 * Load admin statistics
 */
export async function loadAdminStats(): Promise<void> {
  setAdminState({ isStatsLoading: true, error: null });

  try {
    const stats = await tauri.getAdminStats();
    setAdminState({
      stats,
      isStatsLoading: false,
    });
  } catch (err) {
    console.error("[Admin] Failed to load admin stats:", err);
    setAdminState({
      error: err instanceof Error ? err.message : "Failed to load admin stats",
      isStatsLoading: false,
    });
  }
}

/**
 * Elevate admin session with MFA code
 */
export async function elevateSession(
  mfaCode: string,
  reason?: string
): Promise<boolean> {
  setAdminState({ isElevating: true, error: null });

  try {
    const response = await tauri.adminElevate(mfaCode, reason);
    setAdminState({
      isElevated: response.elevated,
      elevationExpiresAt: response.expires_at,
      isElevating: false,
    });

    // Start the elevation timer
    if (response.elevated) {
      startElevationTimer();
    }

    return response.elevated;
  } catch (err) {
    console.error("[Admin] Failed to elevate session:", err);
    setAdminState({
      error: err instanceof Error ? err.message : "Failed to elevate session",
      isElevating: false,
    });
    return false;
  }
}

/**
 * De-elevate admin session
 */
export async function deElevateSession(): Promise<void> {
  try {
    await tauri.adminDeElevate();
    setAdminState({
      isElevated: false,
      elevationExpiresAt: null,
    });
    stopElevationTimer();
  } catch (err) {
    console.error("[Admin] Failed to de-elevate session:", err);
    setAdminState({
      error: err instanceof Error ? err.message : "Failed to de-elevate session",
    });
  }
}

/**
 * Get formatted time remaining for elevation
 */
export function getElevationTimeRemaining(): string {
  if (!adminState.elevationExpiresAt) {
    return "Not elevated";
  }

  const expiresAt = new Date(adminState.elevationExpiresAt).getTime();
  const now = Date.now();
  const remainingMs = expiresAt - now;

  if (remainingMs <= 0) {
    return "Expired";
  }

  const minutes = Math.floor(remainingMs / 60000);
  const seconds = Math.floor((remainingMs % 60000) / 1000);

  if (minutes > 0) {
    return `${minutes}m ${seconds}s`;
  }
  return `${seconds}s`;
}

// ============================================================================
// Users Functions
// ============================================================================

/**
 * Load users list with pagination
 */
export async function loadUsers(page: number = 1): Promise<void> {
  setAdminState({ isUsersLoading: true, error: null });

  try {
    const offset = (page - 1) * adminState.usersPagination.pageSize;
    const response: PaginatedResponse<UserSummary> = await tauri.adminListUsers(
      adminState.usersPagination.pageSize,
      offset
    );

    setAdminState({
      users: response.items,
      usersPagination: {
        page,
        pageSize: response.limit,
        total: response.total,
      },
      isUsersLoading: false,
    });
  } catch (err) {
    console.error("[Admin] Failed to load users:", err);
    setAdminState({
      error: err instanceof Error ? err.message : "Failed to load users",
      isUsersLoading: false,
    });
  }
}

/**
 * Ban a user
 */
export async function banUser(userId: string, reason: string): Promise<boolean> {
  try {
    const result = await tauri.adminBanUser(userId, reason);

    // Update user in local state
    if (result.banned) {
      setAdminState("users", (users) =>
        users.map((u) => (u.id === userId ? { ...u, is_banned: true } : u))
      );

      // Update stats
      if (adminState.stats) {
        setAdminState("stats", {
          ...adminState.stats,
          banned_count: adminState.stats.banned_count + 1,
        });
      }
    }

    return result.banned;
  } catch (err) {
    console.error("[Admin] Failed to ban user:", err);
    setAdminState({
      error: err instanceof Error ? err.message : "Failed to ban user",
    });
    return false;
  }
}

/**
 * Unban a user
 */
export async function unbanUser(userId: string): Promise<boolean> {
  try {
    const result = await tauri.adminUnbanUser(userId);

    // Update user in local state
    if (!result.banned) {
      setAdminState("users", (users) =>
        users.map((u) => (u.id === userId ? { ...u, is_banned: false } : u))
      );

      // Update stats
      if (adminState.stats && adminState.stats.banned_count > 0) {
        setAdminState("stats", {
          ...adminState.stats,
          banned_count: adminState.stats.banned_count - 1,
        });
      }
    }

    return !result.banned;
  } catch (err) {
    console.error("[Admin] Failed to unban user:", err);
    setAdminState({
      error: err instanceof Error ? err.message : "Failed to unban user",
    });
    return false;
  }
}

/**
 * Select a user in the list
 */
export function selectUser(userId: string | null): void {
  setAdminState({ selectedUserId: userId });
}

// ============================================================================
// Guilds Functions
// ============================================================================

/**
 * Load guilds list with pagination
 */
export async function loadGuilds(page: number = 1): Promise<void> {
  setAdminState({ isGuildsLoading: true, error: null });

  try {
    const offset = (page - 1) * adminState.guildsPagination.pageSize;
    const response: PaginatedResponse<GuildSummary> = await tauri.adminListGuilds(
      adminState.guildsPagination.pageSize,
      offset
    );

    setAdminState({
      guilds: response.items,
      guildsPagination: {
        page,
        pageSize: response.limit,
        total: response.total,
      },
      isGuildsLoading: false,
    });
  } catch (err) {
    console.error("[Admin] Failed to load guilds:", err);
    setAdminState({
      error: err instanceof Error ? err.message : "Failed to load guilds",
      isGuildsLoading: false,
    });
  }
}

/**
 * Suspend a guild
 */
export async function suspendGuild(
  guildId: string,
  reason: string
): Promise<boolean> {
  try {
    const result = await tauri.adminSuspendGuild(guildId, reason);

    // Update guild in local state
    if (result.suspended) {
      setAdminState("guilds", (guilds) =>
        guilds.map((g) =>
          g.id === guildId
            ? { ...g, suspended_at: new Date().toISOString() }
            : g
        )
      );
    }

    return result.suspended;
  } catch (err) {
    console.error("[Admin] Failed to suspend guild:", err);
    setAdminState({
      error: err instanceof Error ? err.message : "Failed to suspend guild",
    });
    return false;
  }
}

/**
 * Unsuspend a guild
 */
export async function unsuspendGuild(guildId: string): Promise<boolean> {
  try {
    const result = await tauri.adminUnsuspendGuild(guildId);

    // Update guild in local state
    if (!result.suspended) {
      setAdminState("guilds", (guilds) =>
        guilds.map((g) => (g.id === guildId ? { ...g, suspended_at: null } : g))
      );
    }

    return !result.suspended;
  } catch (err) {
    console.error("[Admin] Failed to unsuspend guild:", err);
    setAdminState({
      error: err instanceof Error ? err.message : "Failed to unsuspend guild",
    });
    return false;
  }
}

/**
 * Select a guild in the list
 */
export function selectGuild(guildId: string | null): void {
  setAdminState({ selectedGuildId: guildId });
}

// ============================================================================
// Audit Log Functions
// ============================================================================

/**
 * Load audit log with pagination and optional filter
 */
export async function loadAuditLog(
  page: number = 1,
  actionFilter?: string
): Promise<void> {
  setAdminState({ isAuditLogLoading: true, error: null });

  try {
    const offset = (page - 1) * adminState.auditLogPagination.pageSize;
    const response: PaginatedResponse<AuditLogEntry> =
      await tauri.adminGetAuditLog(
        adminState.auditLogPagination.pageSize,
        offset,
        actionFilter
      );

    setAdminState({
      auditLog: response.items,
      auditLogPagination: {
        page,
        pageSize: response.limit,
        total: response.total,
      },
      auditLogFilter: actionFilter || null,
      isAuditLogLoading: false,
    });
  } catch (err) {
    console.error("[Admin] Failed to load audit log:", err);
    setAdminState({
      error: err instanceof Error ? err.message : "Failed to load audit log",
      isAuditLogLoading: false,
    });
  }
}

// ============================================================================
// Utility Functions
// ============================================================================

/**
 * Clear error state
 */
export function clearError(): void {
  setAdminState({ error: null });
}

/**
 * Reset admin state (e.g., on logout)
 */
export function resetAdminState(): void {
  stopElevationTimer();

  setAdminState({
    isAdmin: false,
    isElevated: false,
    elevationExpiresAt: null,
    stats: null,
    users: [],
    usersPagination: { page: 1, pageSize: DEFAULT_PAGE_SIZE, total: 0 },
    selectedUserId: null,
    guilds: [],
    guildsPagination: { page: 1, pageSize: DEFAULT_PAGE_SIZE, total: 0 },
    selectedGuildId: null,
    auditLog: [],
    auditLogPagination: { page: 1, pageSize: DEFAULT_PAGE_SIZE, total: 0 },
    auditLogFilter: null,
    isStatusLoading: false,
    isStatsLoading: false,
    isUsersLoading: false,
    isGuildsLoading: false,
    isAuditLogLoading: false,
    isElevating: false,
    error: null,
  });
}

// Export the store for reading
export { adminState };
