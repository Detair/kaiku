/**
 * Admin API commands: stats, user/guild management, audit log, bulk actions,
 * reports, observability, auth settings, and OIDC provider management.
 */

import type {
  AdminCreateUserRequest,
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
import { getAccessToken, getServerUrl, httpRequest } from "./common";

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
  return httpRequest<AdminStatus>("GET", "/api/admin/status");
}

/**
 * Get admin statistics.
 */
export async function getAdminStats(): Promise<AdminStats> {
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
 * Create an account with a temporary password (elevated admin only).
 */
export async function adminCreateUser(
  body: AdminCreateUserRequest,
): Promise<UserSummary> {
  return httpRequest<UserSummary>("POST", "/api/admin/users", body);
}

/**
 * List guilds (admin only).
 */
export async function adminListGuilds(
  limit?: number,
  offset?: number,
  search?: string,
): Promise<PaginatedResponse<GuildSummary>> {
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
  return httpRequest<ElevateResponse>("POST", "/api/admin/elevate", {
    reason,
  });
}

/**
 * De-elevate admin session.
 */
export async function adminDeElevate(): Promise<void> {
  await httpRequest<void>("DELETE", "/api/admin/elevate");
}

/**
 * Ban a user (requires elevation).
 */
export async function adminBanUser(
  userId: string,
  reason: string,
  expiresAt?: string,
): Promise<{ banned: boolean; user_id: string }> {
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
  return httpRequest<AuthSettingsResponse>("GET", "/api/admin/auth-settings");
}

/**
 * Update admin auth settings (requires elevation).
 */
export async function adminUpdateAuthSettings(body: {
  auth_methods?: AuthMethodsConfig;
  registration_policy?: string;
}): Promise<AuthSettingsResponse> {
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
  await httpRequest<{ success: boolean }>(
    "DELETE",
    `/api/admin/oidc-providers/${id}`,
  );
}

// ============================================================================
// Admin Observability Commands (Command Center)
// ============================================================================

export async function adminObsSummary(): Promise<ObservabilitySummary> {
  return httpRequest<ObservabilitySummary>(
    "GET",
    "/api/admin/observability/summary",
  );
}

export async function adminObsTrends(
  range: ObsTimeRange,
  metrics: string[],
): Promise<TrendsResponse> {
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
  return httpRequest<ObsLinksResponse>(
    "GET",
    "/api/admin/observability/links",
  );
}
