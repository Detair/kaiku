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
  UserDetailsResponse,
  GuildDetailsResponse,
  BulkBanResponse,
  BulkSuspendResponse,
  ObservabilitySummary,
  TrendsResponse,
  TopRoutesResponse,
  TopErrorsResponse,
  ObsLogEvent,
  ObsTraceEntry,
  ObsLinksResponse,
  ObsTimeRange,
} from "@/lib/types";
import * as tauri from "@/lib/tauri";
import type { AuditLogFilters } from "@/lib/tauri";

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
 * Audit log filter state
 */
interface AuditLogFilterState {
  action: string | null;
  actionType: string | null;
  fromDate: string | null;
  toDate: string | null;
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
  usersSearch: string;
  selectedUserId: string | null;
  selectedUserDetails: UserDetailsResponse | null;
  selectedUserIds: Set<string>;

  // Guilds list
  guilds: GuildSummary[];
  guildsPagination: PaginationState;
  guildsSearch: string;
  selectedGuildId: string | null;
  selectedGuildDetails: GuildDetailsResponse | null;
  selectedGuildIds: Set<string>;

  // Audit log
  auditLog: AuditLogEntry[];
  auditLogPagination: PaginationState;
  auditLogFilter: string | null;
  auditLogFilters: AuditLogFilterState;

  // Observability (Command Center)
  obsSummary: ObservabilitySummary | null;
  obsTrends: TrendsResponse | null;
  obsTopRoutes: TopRoutesResponse | null;
  obsTopErrors: TopErrorsResponse | null;
  obsLogs: ObsLogEvent[];
  obsLogsCursor: string | null;
  obsLogsHasMore: boolean;
  obsTraces: ObsTraceEntry[];
  obsTracesCursor: string | null;
  obsTracesHasMore: boolean;
  obsLinks: ObsLinksResponse | null;
  obsTimeRange: ObsTimeRange;
  obsLastRefresh: number | null;

  // Loading states
  isStatusLoading: boolean;
  isStatsLoading: boolean;
  isUsersLoading: boolean;
  isUserDetailsLoading: boolean;
  isGuildsLoading: boolean;
  isGuildDetailsLoading: boolean;
  isAuditLogLoading: boolean;
  isElevating: boolean;
  isBulkActionLoading: boolean;
  isExporting: boolean;
  isObsSummaryLoading: boolean;
  isObsTrendsLoading: boolean;
  isObsTopRoutesLoading: boolean;
  isObsTopErrorsLoading: boolean;
  isObsLogsLoading: boolean;
  isObsTracesLoading: boolean;
  isObsLinksLoading: boolean;

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
  usersSearch: "",
  selectedUserId: null,
  selectedUserDetails: null,
  selectedUserIds: new Set(),

  // Guilds list
  guilds: [],
  guildsPagination: { page: 1, pageSize: DEFAULT_PAGE_SIZE, total: 0 },
  guildsSearch: "",
  selectedGuildId: null,
  selectedGuildDetails: null,
  selectedGuildIds: new Set(),

  // Audit log
  auditLog: [],
  auditLogPagination: { page: 1, pageSize: DEFAULT_PAGE_SIZE, total: 0 },
  auditLogFilter: null,
  auditLogFilters: {
    action: null,
    actionType: null,
    fromDate: null,
    toDate: null,
  },

  // Observability (Command Center)
  obsSummary: null,
  obsTrends: null,
  obsTopRoutes: null,
  obsTopErrors: null,
  obsLogs: [],
  obsLogsCursor: null,
  obsLogsHasMore: false,
  obsTraces: [],
  obsTracesCursor: null,
  obsTracesHasMore: false,
  obsLinks: null,
  obsTimeRange: "1h",
  obsLastRefresh: null,

  // Loading states
  isStatusLoading: false,
  isStatsLoading: false,
  isUsersLoading: false,
  isUserDetailsLoading: false,
  isGuildsLoading: false,
  isGuildDetailsLoading: false,
  isAuditLogLoading: false,
  isElevating: false,
  isBulkActionLoading: false,
  isExporting: false,
  isObsSummaryLoading: false,
  isObsTrendsLoading: false,
  isObsTopRoutesLoading: false,
  isObsTopErrorsLoading: false,
  isObsLogsLoading: false,
  isObsTracesLoading: false,
  isObsLinksLoading: false,

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
      error:
        err instanceof Error ? err.message : "Failed to check admin status",
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
 * Elevate admin session
 */
export async function elevateSession(
  reason?: string,
): Promise<boolean> {
  setAdminState({ isElevating: true, error: null });

  try {
    const response = await tauri.adminElevate(reason);
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
      error:
        err instanceof Error ? err.message : "Failed to de-elevate session",
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
 * Load users list with pagination and optional search
 */
export async function loadUsers(
  page: number = 1,
  search?: string,
): Promise<void> {
  // Update search state if provided
  if (search !== undefined) {
    setAdminState({ usersSearch: search });
  }

  setAdminState({ isUsersLoading: true, error: null });

  try {
    const offset = (page - 1) * adminState.usersPagination.pageSize;
    const searchQuery = search !== undefined ? search : adminState.usersSearch;
    const response: PaginatedResponse<UserSummary> = await tauri.adminListUsers(
      adminState.usersPagination.pageSize,
      offset,
      searchQuery || undefined,
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
 * Set users search query and reload
 */
export function setUsersSearch(search: string): void {
  setAdminState({ usersSearch: search });
}

/**
 * Search users with debounce (should be called from component with debounce)
 */
export async function searchUsers(query: string): Promise<void> {
  await loadUsers(1, query);
}

/**
 * Ban a user
 */
export async function banUser(
  userId: string,
  reason: string,
): Promise<boolean> {
  try {
    const result = await tauri.adminBanUser(userId, reason);

    // Update user in local state
    if (result.banned) {
      setAdminState("users", (users) =>
        users.map((u) => (u.id === userId ? { ...u, is_banned: true } : u)),
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
 * Permanently delete a user
 */
export async function deleteUser(userId: string): Promise<boolean> {
  try {
    const result = await tauri.adminDeleteUser(userId);

    if (result.deleted) {
      // Remove user from local state
      setAdminState("users", (users) => users.filter((u) => u.id !== userId));

      // Update stats
      if (adminState.stats) {
        setAdminState("stats", {
          ...adminState.stats,
          user_count: adminState.stats.user_count - 1,
        });
      }

      // Clear selection if this user was selected
      if (adminState.selectedUserId === userId) {
        setAdminState({ selectedUserId: null, selectedUserDetails: null });
      }
    }

    return result.deleted;
  } catch (err) {
    console.error("[Admin] Failed to delete user:", err);
    setAdminState({
      error: err instanceof Error ? err.message : "Failed to delete user",
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
        users.map((u) => (u.id === userId ? { ...u, is_banned: false } : u)),
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
  setAdminState({ selectedUserId: userId, selectedUserDetails: null });
}

/**
 * Load detailed user information
 */
export async function loadUserDetails(userId: string): Promise<void> {
  setAdminState({ isUserDetailsLoading: true });

  try {
    const details = await tauri.adminGetUserDetails(userId);
    setAdminState({
      selectedUserDetails: details,
      isUserDetailsLoading: false,
    });
  } catch (err) {
    console.error("[Admin] Failed to load user details:", err);
    setAdminState({
      error: err instanceof Error ? err.message : "Failed to load user details",
      isUserDetailsLoading: false,
    });
  }
}

// ============================================================================
// User Selection Functions
// ============================================================================

/**
 * Toggle user selection for bulk actions
 */
export function toggleUserSelection(userId: string): void {
  const newSet = new Set(adminState.selectedUserIds);
  if (newSet.has(userId)) {
    newSet.delete(userId);
  } else {
    newSet.add(userId);
  }
  setAdminState({ selectedUserIds: newSet });
}

/**
 * Select all users on current page
 */
export function selectAllUsers(): void {
  const newSet = new Set(adminState.users.map((u) => u.id));
  setAdminState({ selectedUserIds: newSet });
}

/**
 * Clear user selection
 */
export function clearUserSelection(): void {
  setAdminState({ selectedUserIds: new Set() });
}

/**
 * Check if a user is selected
 */
export function isUserSelected(userId: string): boolean {
  return adminState.selectedUserIds.has(userId);
}

/**
 * Get count of selected users
 */
export function getSelectedUserCount(): number {
  return adminState.selectedUserIds.size;
}

/**
 * Export users to CSV
 */
export async function exportUsersCsv(): Promise<void> {
  setAdminState({ isExporting: true, error: null });

  try {
    const blob = await tauri.adminExportUsersCsv(
      adminState.usersSearch || undefined,
    );

    // Create download link
    const url = window.URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `users_export_${new Date().toISOString().split("T")[0]}.csv`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    window.URL.revokeObjectURL(url);

    setAdminState({ isExporting: false });
  } catch (err) {
    console.error("[Admin] Failed to export users:", err);
    setAdminState({
      error: err instanceof Error ? err.message : "Failed to export users",
      isExporting: false,
    });
  }
}

/**
 * Bulk ban selected users
 */
export async function bulkBanUsers(
  reason: string,
): Promise<BulkBanResponse | null> {
  if (adminState.selectedUserIds.size === 0) {
    setAdminState({ error: "No users selected" });
    return null;
  }

  setAdminState({ isBulkActionLoading: true, error: null });

  try {
    const userIds = Array.from(adminState.selectedUserIds);
    const result = await tauri.adminBulkBanUsers(userIds, reason);

    // Update local state for banned users
    setAdminState("users", (users) =>
      users.map((u) =>
        userIds.includes(u.id) && !result.failed.some((f) => f.id === u.id)
          ? { ...u, is_banned: true }
          : u,
      ),
    );

    // Update stats
    if (adminState.stats) {
      setAdminState("stats", {
        ...adminState.stats,
        banned_count: adminState.stats.banned_count + result.banned_count,
      });
    }

    // Clear selection
    setAdminState({ selectedUserIds: new Set(), isBulkActionLoading: false });

    return result;
  } catch (err) {
    console.error("[Admin] Failed to bulk ban users:", err);
    setAdminState({
      error: err instanceof Error ? err.message : "Failed to bulk ban users",
      isBulkActionLoading: false,
    });
    return null;
  }
}

// ============================================================================
// Guilds Functions
// ============================================================================

/**
 * Load guilds list with pagination and optional search
 */
export async function loadGuilds(
  page: number = 1,
  search?: string,
): Promise<void> {
  // Update search state if provided
  if (search !== undefined) {
    setAdminState({ guildsSearch: search });
  }

  setAdminState({ isGuildsLoading: true, error: null });

  try {
    const offset = (page - 1) * adminState.guildsPagination.pageSize;
    const searchQuery = search !== undefined ? search : adminState.guildsSearch;
    const response: PaginatedResponse<GuildSummary> =
      await tauri.adminListGuilds(
        adminState.guildsPagination.pageSize,
        offset,
        searchQuery || undefined,
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
 * Set guilds search query
 */
export function setGuildsSearch(search: string): void {
  setAdminState({ guildsSearch: search });
}

/**
 * Search guilds with debounce (should be called from component with debounce)
 */
export async function searchGuilds(query: string): Promise<void> {
  await loadGuilds(1, query);
}

/**
 * Suspend a guild
 */
export async function suspendGuild(
  guildId: string,
  reason: string,
): Promise<boolean> {
  try {
    const result = await tauri.adminSuspendGuild(guildId, reason);

    // Update guild in local state
    if (result.suspended) {
      setAdminState("guilds", (guilds) =>
        guilds.map((g) =>
          g.id === guildId
            ? { ...g, suspended_at: new Date().toISOString() }
            : g,
        ),
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
        guilds.map((g) =>
          g.id === guildId ? { ...g, suspended_at: null } : g,
        ),
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
 * Permanently delete a guild
 */
export async function deleteGuild(guildId: string): Promise<boolean> {
  try {
    const result = await tauri.adminDeleteGuild(guildId);

    if (result.deleted) {
      // Remove guild from local state
      setAdminState("guilds", (guilds) =>
        guilds.filter((g) => g.id !== guildId),
      );

      // Update stats
      if (adminState.stats) {
        setAdminState("stats", {
          ...adminState.stats,
          guild_count: adminState.stats.guild_count - 1,
        });
      }

      // Clear selection if this guild was selected
      if (adminState.selectedGuildId === guildId) {
        setAdminState({ selectedGuildId: null, selectedGuildDetails: null });
      }
    }

    return result.deleted;
  } catch (err) {
    console.error("[Admin] Failed to delete guild:", err);
    setAdminState({
      error: err instanceof Error ? err.message : "Failed to delete guild",
    });
    return false;
  }
}

/**
 * Select a guild in the list
 */
export function selectGuild(guildId: string | null): void {
  setAdminState({ selectedGuildId: guildId, selectedGuildDetails: null });
}

/**
 * Load detailed guild information
 */
export async function loadGuildDetails(guildId: string): Promise<void> {
  setAdminState({ isGuildDetailsLoading: true });

  try {
    const details = await tauri.adminGetGuildDetails(guildId);
    setAdminState({
      selectedGuildDetails: details,
      isGuildDetailsLoading: false,
    });
  } catch (err) {
    console.error("[Admin] Failed to load guild details:", err);
    setAdminState({
      error:
        err instanceof Error ? err.message : "Failed to load guild details",
      isGuildDetailsLoading: false,
    });
  }
}

// ============================================================================
// Guild Selection Functions
// ============================================================================

/**
 * Toggle guild selection for bulk actions
 */
export function toggleGuildSelection(guildId: string): void {
  const newSet = new Set(adminState.selectedGuildIds);
  if (newSet.has(guildId)) {
    newSet.delete(guildId);
  } else {
    newSet.add(guildId);
  }
  setAdminState({ selectedGuildIds: newSet });
}

/**
 * Select all guilds on current page
 */
export function selectAllGuilds(): void {
  const newSet = new Set(adminState.guilds.map((g) => g.id));
  setAdminState({ selectedGuildIds: newSet });
}

/**
 * Clear guild selection
 */
export function clearGuildSelection(): void {
  setAdminState({ selectedGuildIds: new Set() });
}

/**
 * Check if a guild is selected
 */
export function isGuildSelected(guildId: string): boolean {
  return adminState.selectedGuildIds.has(guildId);
}

/**
 * Get count of selected guilds
 */
export function getSelectedGuildCount(): number {
  return adminState.selectedGuildIds.size;
}

/**
 * Export guilds to CSV
 */
export async function exportGuildsCsv(): Promise<void> {
  setAdminState({ isExporting: true, error: null });

  try {
    const blob = await tauri.adminExportGuildsCsv(
      adminState.guildsSearch || undefined,
    );

    // Create download link
    const url = window.URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `guilds_export_${new Date().toISOString().split("T")[0]}.csv`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    window.URL.revokeObjectURL(url);

    setAdminState({ isExporting: false });
  } catch (err) {
    console.error("[Admin] Failed to export guilds:", err);
    setAdminState({
      error: err instanceof Error ? err.message : "Failed to export guilds",
      isExporting: false,
    });
  }
}

/**
 * Bulk suspend selected guilds
 */
export async function bulkSuspendGuilds(
  reason: string,
): Promise<BulkSuspendResponse | null> {
  if (adminState.selectedGuildIds.size === 0) {
    setAdminState({ error: "No guilds selected" });
    return null;
  }

  setAdminState({ isBulkActionLoading: true, error: null });

  try {
    const guildIds = Array.from(adminState.selectedGuildIds);
    const result = await tauri.adminBulkSuspendGuilds(guildIds, reason);

    // Update local state for suspended guilds
    setAdminState("guilds", (guilds) =>
      guilds.map((g) =>
        guildIds.includes(g.id) && !result.failed.some((f) => f.id === g.id)
          ? { ...g, suspended_at: new Date().toISOString() }
          : g,
      ),
    );

    // Clear selection
    setAdminState({ selectedGuildIds: new Set(), isBulkActionLoading: false });

    return result;
  } catch (err) {
    console.error("[Admin] Failed to bulk suspend guilds:", err);
    setAdminState({
      error:
        err instanceof Error ? err.message : "Failed to bulk suspend guilds",
      isBulkActionLoading: false,
    });
    return null;
  }
}

// ============================================================================
// Audit Log Functions
// ============================================================================

/**
 * Load audit log with pagination and optional filters
 */
export async function loadAuditLog(
  page: number = 1,
  filters?: AuditLogFilters | string,
): Promise<void> {
  setAdminState({ isAuditLogLoading: true, error: null });

  try {
    const offset = (page - 1) * adminState.auditLogPagination.pageSize;

    // Build filters object
    let filterObj: AuditLogFilters;
    if (typeof filters === "string") {
      // Legacy support: string is action prefix filter
      filterObj = { action: filters };
    } else if (filters) {
      filterObj = filters;
    } else {
      // Use existing filters from state
      filterObj = {
        action: adminState.auditLogFilters.action || undefined,
        actionType: adminState.auditLogFilters.actionType || undefined,
        fromDate: adminState.auditLogFilters.fromDate || undefined,
        toDate: adminState.auditLogFilters.toDate || undefined,
      };
    }

    const response: PaginatedResponse<AuditLogEntry> =
      await tauri.adminGetAuditLog(
        adminState.auditLogPagination.pageSize,
        offset,
        filterObj,
      );

    setAdminState({
      auditLog: response.items,
      auditLogPagination: {
        page,
        pageSize: response.limit,
        total: response.total,
      },
      auditLogFilter: filterObj.action || filterObj.actionType || null,
      auditLogFilters: {
        action: filterObj.action || null,
        actionType: filterObj.actionType || null,
        fromDate: filterObj.fromDate || null,
        toDate: filterObj.toDate || null,
      },
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

/**
 * Set audit log filters and reload
 */
export async function setAuditLogFilters(
  filters: AuditLogFilters,
): Promise<void> {
  await loadAuditLog(1, filters);
}

/**
 * Clear all audit log filters and reload
 */
export async function clearAuditLogFilters(): Promise<void> {
  await loadAuditLog(1, {});
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
  unsubscribeFromAdminEvents();

  setAdminState({
    isAdmin: false,
    isElevated: false,
    elevationExpiresAt: null,
    stats: null,
    users: [],
    usersPagination: { page: 1, pageSize: DEFAULT_PAGE_SIZE, total: 0 },
    usersSearch: "",
    selectedUserId: null,
    selectedUserDetails: null,
    selectedUserIds: new Set(),
    guilds: [],
    guildsPagination: { page: 1, pageSize: DEFAULT_PAGE_SIZE, total: 0 },
    guildsSearch: "",
    selectedGuildId: null,
    selectedGuildDetails: null,
    selectedGuildIds: new Set(),
    auditLog: [],
    auditLogPagination: { page: 1, pageSize: DEFAULT_PAGE_SIZE, total: 0 },
    auditLogFilter: null,
    auditLogFilters: {
      action: null,
      actionType: null,
      fromDate: null,
      toDate: null,
    },
    obsSummary: null,
    obsTrends: null,
    obsTopRoutes: null,
    obsTopErrors: null,
    obsLogs: [],
    obsLogsCursor: null,
    obsLogsHasMore: false,
    obsTraces: [],
    obsTracesCursor: null,
    obsTracesHasMore: false,
    obsLinks: null,
    obsTimeRange: "1h",
    obsLastRefresh: null,
    isStatusLoading: false,
    isStatsLoading: false,
    isUsersLoading: false,
    isUserDetailsLoading: false,
    isGuildsLoading: false,
    isGuildDetailsLoading: false,
    isAuditLogLoading: false,
    isElevating: false,
    isBulkActionLoading: false,
    isExporting: false,
    isObsSummaryLoading: false,
    isObsTrendsLoading: false,
    isObsTopRoutesLoading: false,
    isObsTopErrorsLoading: false,
    isObsLogsLoading: false,
    isObsTracesLoading: false,
    isObsLinksLoading: false,
    error: null,
  });
}

// ============================================================================
// Admin Event Subscription
// ============================================================================

let isAdminSubscribed = false;

/**
 * Subscribe to admin events via WebSocket
 */
export async function subscribeToAdminEvents(): Promise<void> {
  if (isAdminSubscribed) return;

  try {
    await tauri.wsAdminSubscribe();
    isAdminSubscribed = true;
    console.log("[Admin] Subscribed to admin events");
  } catch (err) {
    console.error("[Admin] Failed to subscribe to admin events:", err);
  }
}

/**
 * Unsubscribe from admin events
 */
export async function unsubscribeFromAdminEvents(): Promise<void> {
  if (!isAdminSubscribed) return;

  try {
    await tauri.wsAdminUnsubscribe();
    isAdminSubscribed = false;
    console.log("[Admin] Unsubscribed from admin events");
  } catch (err) {
    console.error("[Admin] Failed to unsubscribe from admin events:", err);
  }
}

// ============================================================================
// WebSocket Event Handlers
// ============================================================================

/**
 * Handle user banned event from WebSocket
 */
export function handleUserBannedEvent(userId: string, username: string): void {
  console.log(`[Admin] User banned event: ${username} (${userId})`);

  // Update user in local state if present
  setAdminState("users", (users) =>
    users.map((u) => (u.id === userId ? { ...u, is_banned: true } : u)),
  );

  // Update stats
  if (adminState.stats) {
    setAdminState("stats", {
      ...adminState.stats,
      banned_count: adminState.stats.banned_count + 1,
    });
  }
}

/**
 * Handle user unbanned event from WebSocket
 */
export function handleUserUnbannedEvent(
  userId: string,
  username: string,
): void {
  console.log(`[Admin] User unbanned event: ${username} (${userId})`);

  // Update user in local state if present
  setAdminState("users", (users) =>
    users.map((u) => (u.id === userId ? { ...u, is_banned: false } : u)),
  );

  // Update stats
  if (adminState.stats && adminState.stats.banned_count > 0) {
    setAdminState("stats", {
      ...adminState.stats,
      banned_count: adminState.stats.banned_count - 1,
    });
  }
}

/**
 * Handle guild suspended event from WebSocket
 */
export function handleGuildSuspendedEvent(
  guildId: string,
  guildName: string,
): void {
  console.log(`[Admin] Guild suspended event: ${guildName} (${guildId})`);

  // Update guild in local state if present
  setAdminState("guilds", (guilds) =>
    guilds.map((g) =>
      g.id === guildId ? { ...g, suspended_at: new Date().toISOString() } : g,
    ),
  );
}

/**
 * Handle guild unsuspended event from WebSocket
 */
export function handleGuildUnsuspendedEvent(
  guildId: string,
  guildName: string,
): void {
  console.log(`[Admin] Guild unsuspended event: ${guildName} (${guildId})`);

  // Update guild in local state if present
  setAdminState("guilds", (guilds) =>
    guilds.map((g) => (g.id === guildId ? { ...g, suspended_at: null } : g)),
  );
}

/**
 * Handle user deleted event from WebSocket
 */
export function handleUserDeletedEvent(userId: string, username: string): void {
  console.log(`[Admin] User deleted event: ${username} (${userId})`);

  // Remove user from local state
  setAdminState("users", (users) => users.filter((u) => u.id !== userId));

  // Update stats
  if (adminState.stats) {
    setAdminState("stats", {
      ...adminState.stats,
      user_count: adminState.stats.user_count - 1,
    });
  }

  // Clear selection if this user was selected
  if (adminState.selectedUserId === userId) {
    setAdminState({ selectedUserId: null, selectedUserDetails: null });
  }
}

/**
 * Handle guild deleted event from WebSocket
 */
export function handleGuildDeletedEvent(
  guildId: string,
  guildName: string,
): void {
  console.log(`[Admin] Guild deleted event: ${guildName} (${guildId})`);

  // Remove guild from local state
  setAdminState("guilds", (guilds) => guilds.filter((g) => g.id !== guildId));

  // Update stats
  if (adminState.stats) {
    setAdminState("stats", {
      ...adminState.stats,
      guild_count: adminState.stats.guild_count - 1,
    });
  }

  // Clear selection if this guild was selected
  if (adminState.selectedGuildId === guildId) {
    setAdminState({ selectedGuildId: null, selectedGuildDetails: null });
  }
}

// ============================================================================
// Report Event Handlers
// ============================================================================

/**
 * Handle new report created event from WebSocket
 */
export function handleReportCreatedEvent(
  reportId: string,
  category: string,
  targetType: string,
): void {
  console.log(
    `[Admin] Report created: ${reportId} (${category}, ${targetType})`,
  );
  // The admin dashboard will reload reports when the panel is active
}

/**
 * Handle report resolved event from WebSocket
 */
export function handleReportResolvedEvent(reportId: string): void {
  console.log(`[Admin] Report resolved: ${reportId}`);
}

// ============================================================================
// Undo Functionality
// ============================================================================

/** Pending undo action type */
interface PendingUndo {
  id: string;
  type: "ban" | "suspend";
  targetId: string;
  targetName: string;
  executeAt: number;
  timer: ReturnType<typeof setTimeout>;
}

/** Map of pending undo actions */
const pendingUndos = new Map<string, PendingUndo>();

/** Undo delay in milliseconds (5 seconds) */
const UNDO_DELAY_MS = 5000;

/**
 * Schedule a ban action with undo capability.
 * Returns the undo ID for cancellation.
 */
export function scheduleBanWithUndo(
  userId: string,
  username: string,
  reason: string,
  onExecute: () => void,
  _onUndo: () => void,
): string {
  const undoId = `ban-${userId}-${Date.now()}`;

  const timer = setTimeout(async () => {
    // Execute the ban
    const success = await banUser(userId, reason);
    if (success) {
      onExecute();
    }
    pendingUndos.delete(undoId);
  }, UNDO_DELAY_MS);

  pendingUndos.set(undoId, {
    id: undoId,
    type: "ban",
    targetId: userId,
    targetName: username,
    executeAt: Date.now() + UNDO_DELAY_MS,
    timer,
  });

  return undoId;
}

/**
 * Schedule a suspend action with undo capability.
 * Returns the undo ID for cancellation.
 */
export function scheduleSuspendWithUndo(
  guildId: string,
  guildName: string,
  reason: string,
  onExecute: () => void,
  _onUndo: () => void,
): string {
  const undoId = `suspend-${guildId}-${Date.now()}`;

  const timer = setTimeout(async () => {
    // Execute the suspend
    const success = await suspendGuild(guildId, reason);
    if (success) {
      onExecute();
    }
    pendingUndos.delete(undoId);
  }, UNDO_DELAY_MS);

  pendingUndos.set(undoId, {
    id: undoId,
    type: "suspend",
    targetId: guildId,
    targetName: guildName,
    executeAt: Date.now() + UNDO_DELAY_MS,
    timer,
  });

  return undoId;
}

/**
 * Cancel a pending undo action
 */
export function cancelPendingAction(undoId: string): boolean {
  const pending = pendingUndos.get(undoId);
  if (!pending) return false;

  clearTimeout(pending.timer);
  pendingUndos.delete(undoId);
  console.log(
    `[Admin] Cancelled pending ${pending.type} for ${pending.targetName}`,
  );
  return true;
}

/**
 * Get pending undo action info
 */
export function getPendingAction(undoId: string): PendingUndo | undefined {
  return pendingUndos.get(undoId);
}

/**
 * Check if there's a pending action for a target
 */
export function hasPendingAction(targetId: string): boolean {
  for (const pending of pendingUndos.values()) {
    if (pending.targetId === targetId) return true;
  }
  return false;
}

// ============================================================================
// Observability Functions (Command Center)
// ============================================================================

const DEFAULT_OBS_METRICS = [
  "kaiku_http_request_duration_ms",
  "kaiku_http_errors_total",
  "kaiku_ws_connections_active",
  "kaiku_voice_sessions_active",
];

export async function loadObsSummary(): Promise<void> {
  setAdminState({ isObsSummaryLoading: true });
  try {
    const summary = await tauri.adminObsSummary();
    setAdminState({
      obsSummary: summary,
      obsLastRefresh: Date.now(),
      isObsSummaryLoading: false,
    });
  } catch (err) {
    console.error("[Admin] Failed to load obs summary:", err);
    setAdminState({ isObsSummaryLoading: false });
  }
}

export async function loadObsTrends(
  range?: ObsTimeRange,
  metrics?: string[],
): Promise<void> {
  const r = range ?? adminState.obsTimeRange;
  setAdminState({ isObsTrendsLoading: true });
  try {
    const trends = await tauri.adminObsTrends(
      r,
      metrics ?? DEFAULT_OBS_METRICS,
    );
    setAdminState({ obsTrends: trends, isObsTrendsLoading: false });
  } catch (err) {
    console.error("[Admin] Failed to load obs trends:", err);
    setAdminState({ isObsTrendsLoading: false });
  }
}

export async function loadObsTopRoutes(
  range?: ObsTimeRange,
  sort?: "latency" | "errors",
): Promise<void> {
  const r = range ?? adminState.obsTimeRange;
  setAdminState({ isObsTopRoutesLoading: true });
  try {
    const routes = await tauri.adminObsTopRoutes(r, sort);
    setAdminState({ obsTopRoutes: routes, isObsTopRoutesLoading: false });
  } catch (err) {
    console.error("[Admin] Failed to load obs top routes:", err);
    setAdminState({ isObsTopRoutesLoading: false });
  }
}

export async function loadObsTopErrors(range?: ObsTimeRange): Promise<void> {
  const r = range ?? adminState.obsTimeRange;
  setAdminState({ isObsTopErrorsLoading: true });
  try {
    const errors = await tauri.adminObsTopErrors(r);
    setAdminState({ obsTopErrors: errors, isObsTopErrorsLoading: false });
  } catch (err) {
    console.error("[Admin] Failed to load obs top errors:", err);
    setAdminState({ isObsTopErrorsLoading: false });
  }
}

export async function loadObsLogs(
  reset: boolean = false,
  level?: string,
  domain?: string,
  search?: string,
): Promise<void> {
  setAdminState({ isObsLogsLoading: true });
  const cursor = reset ? undefined : adminState.obsLogsCursor ?? undefined;
  try {
    const response = await tauri.adminObsLogs(
      level,
      domain,
      search,
      cursor,
      50,
    );
    if (reset) {
      setAdminState({
        obsLogs: response.logs,
        obsLogsCursor: response.next_cursor,
        obsLogsHasMore: response.next_cursor !== null,
        isObsLogsLoading: false,
      });
    } else {
      setAdminState({
        obsLogs: [...adminState.obsLogs, ...response.logs],
        obsLogsCursor: response.next_cursor,
        obsLogsHasMore: response.next_cursor !== null,
        isObsLogsLoading: false,
      });
    }
  } catch (err) {
    console.error("[Admin] Failed to load obs logs:", err);
    setAdminState({ isObsLogsLoading: false });
  }
}

export async function loadObsTraces(
  reset: boolean = false,
  status?: string,
  domain?: string,
): Promise<void> {
  setAdminState({ isObsTracesLoading: true });
  const cursor = reset ? undefined : adminState.obsTracesCursor ?? undefined;
  try {
    const response = await tauri.adminObsTraces(status, domain, cursor, 50);
    if (reset) {
      setAdminState({
        obsTraces: response.traces,
        obsTracesCursor: response.next_cursor,
        obsTracesHasMore: response.next_cursor !== null,
        isObsTracesLoading: false,
      });
    } else {
      setAdminState({
        obsTraces: [...adminState.obsTraces, ...response.traces],
        obsTracesCursor: response.next_cursor,
        obsTracesHasMore: response.next_cursor !== null,
        isObsTracesLoading: false,
      });
    }
  } catch (err) {
    console.error("[Admin] Failed to load obs traces:", err);
    setAdminState({ isObsTracesLoading: false });
  }
}

export async function loadObsLinks(): Promise<void> {
  setAdminState({ isObsLinksLoading: true });
  try {
    const links = await tauri.adminObsLinks();
    setAdminState({ obsLinks: links, isObsLinksLoading: false });
  } catch (err) {
    console.error("[Admin] Failed to load obs links:", err);
    setAdminState({ isObsLinksLoading: false });
  }
}

export function setObsTimeRange(range: ObsTimeRange): void {
  setAdminState({ obsTimeRange: range });
}

// Export the store for reading
export { adminState };
