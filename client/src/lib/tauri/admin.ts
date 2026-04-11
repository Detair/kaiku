/**
 * Admin API commands: stats, user/guild management, audit log, bulk actions,
 * reports, observability, auth settings, and OIDC provider management.
 */

import type {
  AdminOidcProvider,
  AdminStats,
  AdminStatus,
  AuditLogEntry,
  AuthMethodsConfig,
  AuthSettingsResponse,
  BulkBanResponse,
  BulkSuspendResponse,
  ElevateResponse,
  GuildDetailsResponse,
  GuildSummary,
  LogsResponse,
  ObsLinksResponse,
  ObservabilitySummary,
  ObsTimeRange,
  PaginatedResponse,
  TopErrorsResponse,
  TopRoutesResponse,
  TracesResponse,
  TrendsResponse,
  UserDetailsResponse,
  UserSummary,
} from "../types";
import { getAccessToken, getServerUrl, httpRequest, isTauri } from "./common";

// ============================================================================
// Admin Reports
// ============================================================================

export interface AdminReportResponse {
  id: string;
  reporter_id: string;
  target_type: string;
  target_user_id: string;
  target_message_id: string | null;
  category: string;
  description: string | null;
  status: string;
  assigned_admin_id: string | null;
  resolution_action: string | null;
  resolution_note: string | null;
  resolved_at: string | null;
  created_at: string;
  updated_at: string;
}

export interface PaginatedReports {
  items: AdminReportResponse[];
  total: number;
  limit: number;
  offset: number;
}

export interface ReportStatsResponse {
  pending: number;
  reviewing: number;
  resolved: number;
  dismissed: number;
}

export async function adminListReports(
  limit: number,
  offset: number,
  status?: string,
  category?: string,
): Promise<PaginatedReports> {
  const params = new URLSearchParams();
  params.set("limit", String(limit));
  params.set("offset", String(offset));
  if (status) params.set("status", status);
  if (category) params.set("category", category);
  return httpRequest<PaginatedReports>(
    "GET",
    `/api/admin/reports?${params.toString()}`,
  );
}

export async function adminGetReport(
  reportId: string,
): Promise<AdminReportResponse> {
  return httpRequest<AdminReportResponse>(
    "GET",
    `/api/admin/reports/${reportId}`,
  );
}

export async function adminClaimReport(
  reportId: string,
): Promise<AdminReportResponse> {
  return httpRequest<AdminReportResponse>(
    "POST",
    `/api/admin/reports/${reportId}/claim`,
  );
}

export async function adminResolveReport(
  reportId: string,
  resolution_action: string,
  resolution_note?: string,
): Promise<AdminReportResponse> {
  return httpRequest<AdminReportResponse>(
    "POST",
    `/api/admin/reports/${reportId}/resolve`,
    {
      resolution_action,
      resolution_note,
    },
  );
}

export async function adminGetReportStats(): Promise<ReportStatsResponse> {
  return httpRequest<ReportStatsResponse>("GET", "/api/admin/reports/stats");
}

// ============================================================================
// Admin Status / Stats
// ============================================================================

/**
 * Check if current user is a system admin.
 */
export async function checkAdminStatus(): Promise<AdminStatus> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<AdminStatus>("check_admin_status");
  }

  return httpRequest<AdminStatus>("GET", "/api/admin/status");
}

/**
 * Get admin statistics.
 */
export async function getAdminStats(): Promise<AdminStats> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<AdminStats>("get_admin_stats");
  }

  return httpRequest<AdminStats>("GET", "/api/admin/stats");
}

// ============================================================================
// User / Guild listing (admin)
// ============================================================================

/**
 * List users (admin only).
 */
export async function adminListUsers(
  limit?: number,
  offset?: number,
  search?: string,
): Promise<PaginatedResponse<UserSummary>> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<PaginatedResponse<UserSummary>>("admin_list_users", {
      limit,
      offset,
      search,
    });
  }

  const params = new URLSearchParams();
  if (limit !== undefined) params.set("limit", limit.toString());
  if (offset !== undefined) params.set("offset", offset.toString());
  if (search) params.set("search", search);
  const query = params.toString();

  return httpRequest<PaginatedResponse<UserSummary>>(
    "GET",
    `/api/admin/users${query ? `?${query}` : ""}`,
  );
}

/**
 * List guilds (admin only).
 */
export async function adminListGuilds(
  limit?: number,
  offset?: number,
  search?: string,
): Promise<PaginatedResponse<GuildSummary>> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<PaginatedResponse<GuildSummary>>("admin_list_guilds", {
      limit,
      offset,
      search,
    });
  }

  const params = new URLSearchParams();
  if (limit !== undefined) params.set("limit", limit.toString());
  if (offset !== undefined) params.set("offset", offset.toString());
  if (search) params.set("search", search);
  const query = params.toString();

  return httpRequest<PaginatedResponse<GuildSummary>>(
    "GET",
    `/api/admin/guilds${query ? `?${query}` : ""}`,
  );
}

/**
 * Get detailed user information (admin only).
 */
export async function adminGetUserDetails(
  userId: string,
): Promise<UserDetailsResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<UserDetailsResponse>("admin_get_user_details", {
      user_id: userId,
    });
  }

  return httpRequest<UserDetailsResponse>(
    "GET",
    `/api/admin/users/${userId}/details`,
  );
}

/**
 * Get detailed guild information (admin only).
 */
export async function adminGetGuildDetails(
  guildId: string,
): Promise<GuildDetailsResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<GuildDetailsResponse>("admin_get_guild_details", {
      guild_id: guildId,
    });
  }

  return httpRequest<GuildDetailsResponse>(
    "GET",
    `/api/admin/guilds/${guildId}/details`,
  );
}

// ============================================================================
// Audit log
// ============================================================================

/**
 * Audit log filter options.
 */
export interface AuditLogFilters {
  /** Filter by action prefix (e.g., "admin." for all admin actions) */
  action?: string;
  /** Filter by exact action type (e.g., "admin.users.ban") */
  actionType?: string;
  /** Filter entries created on or after this date (ISO 8601) */
  fromDate?: string;
  /** Filter entries created on or before this date (ISO 8601) */
  toDate?: string;
}

/**
 * Get audit log (admin only).
 */
export async function adminGetAuditLog(
  limit?: number,
  offset?: number,
  filters?: AuditLogFilters | string,
): Promise<PaginatedResponse<AuditLogEntry>> {
  // Support legacy string parameter (action filter prefix)
  const filterObj: AuditLogFilters =
    typeof filters === "string" ? { action: filters } : filters || {};

  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<PaginatedResponse<AuditLogEntry>>("admin_get_audit_log", {
      limit,
      offset,
      action_filter: filterObj.action,
      action_type: filterObj.actionType,
      from_date: filterObj.fromDate,
      to_date: filterObj.toDate,
    });
  }

  const params = new URLSearchParams();
  if (limit !== undefined) params.set("limit", limit.toString());
  if (offset !== undefined) params.set("offset", offset.toString());
  if (filterObj.action) params.set("action", filterObj.action);
  if (filterObj.actionType) params.set("action_type", filterObj.actionType);
  if (filterObj.fromDate) params.set("from_date", filterObj.fromDate);
  if (filterObj.toDate) params.set("to_date", filterObj.toDate);
  const query = params.toString();

  return httpRequest<PaginatedResponse<AuditLogEntry>>(
    "GET",
    `/api/admin/audit-log${query ? `?${query}` : ""}`,
  );
}

// ============================================================================
// Admin elevation / moderation actions
// ============================================================================

/**
 * Elevate admin session.
 */
export async function adminElevate(
  reason?: string,
): Promise<ElevateResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<ElevateResponse>("admin_elevate", {
      reason,
    });
  }

  return httpRequest<ElevateResponse>("POST", "/api/admin/elevate", {
    reason,
  });
}

/**
 * De-elevate admin session.
 */
export async function adminDeElevate(): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<void>("admin_de_elevate");
  }

  await httpRequest<void>("POST", "/api/admin/de-elevate");
}

/**
 * Ban a user (requires elevation).
 */
export async function adminBanUser(
  userId: string,
  reason: string,
  expiresAt?: string,
): Promise<{ banned: boolean; user_id: string }> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("admin_ban_user", {
      user_id: userId,
      reason,
      expires_at: expiresAt,
    });
  }

  return httpRequest<{ banned: boolean; user_id: string }>(
    "POST",
    `/api/admin/users/${userId}/ban`,
    { reason, expires_at: expiresAt },
  );
}

/**
 * Unban a user (requires elevation).
 */
export async function adminUnbanUser(
  userId: string,
): Promise<{ banned: boolean; user_id: string }> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("admin_unban_user", { user_id: userId });
  }

  return httpRequest<{ banned: boolean; user_id: string }>(
    "POST",
    `/api/admin/users/${userId}/unban`,
  );
}

/**
 * Suspend a guild (requires elevation).
 */
export async function adminSuspendGuild(
  guildId: string,
  reason: string,
): Promise<{ suspended: boolean; guild_id: string }> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("admin_suspend_guild", { guild_id: guildId, reason });
  }

  return httpRequest<{ suspended: boolean; guild_id: string }>(
    "POST",
    `/api/admin/guilds/${guildId}/suspend`,
    { reason },
  );
}

/**
 * Unsuspend a guild (requires elevation).
 */
export async function adminUnsuspendGuild(
  guildId: string,
): Promise<{ suspended: boolean; guild_id: string }> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("admin_unsuspend_guild", { guild_id: guildId });
  }

  return httpRequest<{ suspended: boolean; guild_id: string }>(
    "POST",
    `/api/admin/guilds/${guildId}/unsuspend`,
  );
}

/**
 * Permanently delete a user (requires elevation).
 */
export async function adminDeleteUser(
  userId: string,
): Promise<{ deleted: boolean; id: string }> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("admin_delete_user", { user_id: userId });
  }

  return httpRequest<{ deleted: boolean; id: string }>(
    "DELETE",
    `/api/admin/users/${userId}`,
  );
}

/**
 * Permanently delete a guild (requires elevation).
 */
export async function adminDeleteGuild(
  guildId: string,
): Promise<{ deleted: boolean; id: string }> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("admin_delete_guild", { guild_id: guildId });
  }

  return httpRequest<{ deleted: boolean; id: string }>(
    "DELETE",
    `/api/admin/guilds/${guildId}`,
  );
}

// ============================================================================
// CSV Export
// ============================================================================

/**
 * Export users to CSV (admin only).
 * Returns CSV content as a blob for download.
 */
export async function adminExportUsersCsv(search?: string): Promise<Blob> {
  const params = new URLSearchParams();
  if (search) params.set("search", search);
  const query = params.toString();

  const baseUrl = getServerUrl().replace(/\/+$/, "");
  const token = getAccessToken();

  const response = await fetch(
    `${baseUrl}/api/admin/users/export${query ? `?${query}` : ""}`,
    {
      method: "GET",
      headers: {
        Authorization: `Bearer ${token}`,
      },
    },
  );

  if (!response.ok) {
    throw new Error(`Export failed: ${response.statusText}`);
  }

  return response.blob();
}

/**
 * Export guilds to CSV (admin only).
 * Returns CSV content as a blob for download.
 */
export async function adminExportGuildsCsv(search?: string): Promise<Blob> {
  const params = new URLSearchParams();
  if (search) params.set("search", search);
  const query = params.toString();

  const baseUrl = getServerUrl().replace(/\/+$/, "");
  const token = getAccessToken();

  const response = await fetch(
    `${baseUrl}/api/admin/guilds/export${query ? `?${query}` : ""}`,
    {
      method: "GET",
      headers: {
        Authorization: `Bearer ${token}`,
      },
    },
  );

  if (!response.ok) {
    throw new Error(`Export failed: ${response.statusText}`);
  }

  return response.blob();
}

// ============================================================================
// Bulk actions
// ============================================================================

/**
 * Bulk ban multiple users (requires elevation).
 */
export async function adminBulkBanUsers(
  userIds: string[],
  reason: string,
  expiresAt?: string,
): Promise<BulkBanResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("admin_bulk_ban_users", {
      user_ids: userIds,
      reason,
      expires_at: expiresAt,
    });
  }

  return httpRequest<BulkBanResponse>("POST", "/api/admin/users/bulk-ban", {
    user_ids: userIds,
    reason,
    expires_at: expiresAt,
  });
}

/**
 * Bulk suspend multiple guilds (requires elevation).
 */
export async function adminBulkSuspendGuilds(
  guildIds: string[],
  reason: string,
): Promise<BulkSuspendResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("admin_bulk_suspend_guilds", {
      guild_ids: guildIds,
      reason,
    });
  }

  return httpRequest<BulkSuspendResponse>(
    "POST",
    "/api/admin/guilds/bulk-suspend",
    {
      guild_ids: guildIds,
      reason,
    },
  );
}

// ============================================================================
// Admin Auth Settings & OIDC Provider Management
// ============================================================================

/**
 * Get admin auth settings (requires elevation).
 */
export async function adminGetAuthSettings(): Promise<AuthSettingsResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<AuthSettingsResponse>("admin_get_auth_settings");
  }
  return httpRequest<AuthSettingsResponse>("GET", "/api/admin/auth-settings");
}

/**
 * Update admin auth settings (requires elevation).
 */
export async function adminUpdateAuthSettings(body: {
  auth_methods?: AuthMethodsConfig;
  registration_policy?: string;
}): Promise<AuthSettingsResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<AuthSettingsResponse>("admin_update_auth_settings", { body });
  }
  return httpRequest<AuthSettingsResponse>(
    "PUT",
    "/api/admin/auth-settings",
    body,
  );
}

/**
 * List all OIDC providers (admin, requires elevation).
 */
export async function adminListOidcProviders(): Promise<AdminOidcProvider[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<AdminOidcProvider[]>("admin_list_oidc_providers");
  }
  return httpRequest<AdminOidcProvider[]>("GET", "/api/admin/oidc-providers");
}

/**
 * Create an OIDC provider (admin, requires elevation).
 */
export async function adminCreateOidcProvider(body: {
  slug: string;
  display_name: string;
  icon_hint?: string;
  provider_type?: string;
  issuer_url?: string;
  authorization_url?: string;
  token_url?: string;
  userinfo_url?: string;
  client_id: string;
  client_secret: string;
  scopes?: string;
}): Promise<AdminOidcProvider> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<AdminOidcProvider>("admin_create_oidc_provider", { body });
  }
  return httpRequest<AdminOidcProvider>(
    "POST",
    "/api/admin/oidc-providers",
    body,
  );
}

/**
 * Update an OIDC provider (admin, requires elevation).
 */
export async function adminUpdateOidcProvider(
  id: string,
  body: {
    display_name: string;
    icon_hint?: string;
    issuer_url?: string;
    authorization_url?: string;
    token_url?: string;
    userinfo_url?: string;
    client_id: string;
    client_secret?: string;
    scopes: string;
    enabled: boolean;
  },
): Promise<AdminOidcProvider> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<AdminOidcProvider>("admin_update_oidc_provider", {
      id,
      body,
    });
  }
  return httpRequest<AdminOidcProvider>(
    "PUT",
    `/api/admin/oidc-providers/${id}`,
    body,
  );
}

/**
 * Delete an OIDC provider (admin, requires elevation).
 */
export async function adminDeleteOidcProvider(id: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<void>("admin_delete_oidc_provider", { id });
  }
  await httpRequest<{ success: boolean }>(
    "DELETE",
    `/api/admin/oidc-providers/${id}`,
  );
}

// ============================================================================
// Admin Observability Commands (Command Center)
// ============================================================================

export async function adminObsSummary(): Promise<ObservabilitySummary> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<ObservabilitySummary>("admin_obs_summary");
  }
  return httpRequest<ObservabilitySummary>(
    "GET",
    "/api/admin/observability/summary",
  );
}

export async function adminObsTrends(
  range: ObsTimeRange,
  metrics: string[],
): Promise<TrendsResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<TrendsResponse>("admin_obs_trends", { range, metrics });
  }
  const params = new URLSearchParams();
  params.set("range", range);
  for (const m of metrics) {
    params.append("metric", m);
  }
  return httpRequest<TrendsResponse>(
    "GET",
    `/api/admin/observability/trends?${params.toString()}`,
  );
}

export async function adminObsTopRoutes(
  range: ObsTimeRange,
  sort?: "latency" | "errors",
  limit?: number,
): Promise<TopRoutesResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<TopRoutesResponse>("admin_obs_top_routes", {
      range,
      sort,
      limit,
    });
  }
  const params = new URLSearchParams();
  params.set("range", range);
  if (sort) params.set("sort", sort);
  if (limit) params.set("limit", String(limit));
  return httpRequest<TopRoutesResponse>(
    "GET",
    `/api/admin/observability/top-routes?${params.toString()}`,
  );
}

export async function adminObsTopErrors(
  range: ObsTimeRange,
  limit?: number,
): Promise<TopErrorsResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<TopErrorsResponse>("admin_obs_top_errors", { range, limit });
  }
  const params = new URLSearchParams();
  params.set("range", range);
  if (limit) params.set("limit", String(limit));
  return httpRequest<TopErrorsResponse>(
    "GET",
    `/api/admin/observability/top-errors?${params.toString()}`,
  );
}

export async function adminObsLogs(
  level?: string,
  domain?: string,
  search?: string,
  cursor?: string,
  limit?: number,
): Promise<LogsResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<LogsResponse>("admin_obs_logs", {
      level,
      domain,
      search,
      cursor,
      limit,
    });
  }
  const params = new URLSearchParams();
  if (level) params.set("level", level);
  if (domain) params.set("domain", domain);
  if (search) params.set("search", search);
  if (cursor) params.set("cursor", cursor);
  if (limit) params.set("limit", String(limit));
  const query = params.toString();
  return httpRequest<LogsResponse>(
    "GET",
    `/api/admin/observability/logs${query ? `?${query}` : ""}`,
  );
}

export async function adminObsTraces(
  status?: string,
  domain?: string,
  cursor?: string,
  limit?: number,
): Promise<TracesResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<TracesResponse>("admin_obs_traces", {
      status,
      domain,
      cursor,
      limit,
    });
  }
  const params = new URLSearchParams();
  if (status) params.set("status", status);
  if (domain) params.set("domain", domain);
  if (cursor) params.set("cursor", cursor);
  if (limit) params.set("limit", String(limit));
  const query = params.toString();
  return httpRequest<TracesResponse>(
    "GET",
    `/api/admin/observability/traces${query ? `?${query}` : ""}`,
  );
}

export async function adminObsLinks(): Promise<ObsLinksResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<ObsLinksResponse>("admin_obs_links");
  }
  return httpRequest<ObsLinksResponse>(
    "GET",
    "/api/admin/observability/links",
  );
}
