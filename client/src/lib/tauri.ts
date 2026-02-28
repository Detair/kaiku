/**
 * Tauri Command Wrappers
 * Type-safe wrappers for Tauri commands
 * Falls back to HTTP API when running in browser
 */

import type {
  User,
  Channel,
  ChannelCategory,
  ChannelWithUnread,
  Message,
  AppSettings,
  Guild,
  GuildMember,
  GuildInvite,
  InviteResponse,
  InviteExpiry,
  Friend,
  Friendship,
  DMChannel,
  DMListItem,
  Page,
  PageListItem,
  GuildRole,
  GuildEmoji,
  ChannelOverride,
  CreateRoleRequest,
  UpdateRoleRequest,
  SetChannelOverrideRequest,
  AssignRoleResponse,
  RemoveRoleResponse,
  DeleteRoleResponse,
  AdminStats,
  AdminStatus,
  UserSummary,
  GuildSummary,
  AuditLogEntry,
  PaginatedResponse,
  ElevateResponse,
  UserDetailsResponse,
  GuildDetailsResponse,
  BulkBanResponse,
  BulkSuspendResponse,
  CallEndReason,
  CallStateResponse,
  E2EEStatus,
  InitE2EEResponse,
  PrekeyData,
  E2EEContent,
  ClaimedPrekeyInput,
  UserKeysResponse,
  ClaimedPrekeyResponse,
  SearchResponse,
  SearchFilters,
  GlobalSearchResponse,
  PaginatedMessages,
  Pin,
  CreatePinRequest,
  UpdatePinRequest,
  ServerSettings,
  OidcProvider,
  OidcLoginResult,
  AuthSettingsResponse,
  AuthMethodsConfig,
  AdminOidcProvider,
  UiState,
  GuildSettings,
  GuildUsageStats,
  DiscoverResponse,
  JoinDiscoverableResponse,
  PageRevision,
  RevisionListItem,
  PageCategory,
  ObservabilitySummary,
  TrendsResponse,
  TopRoutesResponse,
  TopErrorsResponse,
  LogsResponse,
  TracesResponse,
  ObsLinksResponse,
  ObsTimeRange,
} from "./types";

// Re-export types for convenience
export type {
  User,
  Channel,
  ChannelCategory,
  ChannelWithUnread,
  Message,
  AppSettings,
  Guild,
  GuildMember,
  GuildInvite,
  InviteResponse,
  InviteExpiry,
  Friend,
  Friendship,
  DMChannel,
  DMListItem,
  Page,
  PageListItem,
  GuildRole,
  GuildEmoji,
  ChannelOverride,
  CreateRoleRequest,
  UpdateRoleRequest,
  SetChannelOverrideRequest,
  AssignRoleResponse,
  RemoveRoleResponse,
  DeleteRoleResponse,
  AdminStats,
  AdminStatus,
  UserSummary,
  GuildSummary,
  AuditLogEntry,
  PaginatedResponse,
  ElevateResponse,
  UserDetailsResponse,
  GuildDetailsResponse,
  BulkBanResponse,
  BulkSuspendResponse,
  CallEndReason,
  CallStateResponse,
  E2EEStatus,
  InitE2EEResponse,
  PrekeyData,
  E2EEContent,
  ClaimedPrekeyInput,
  UserKeysResponse,
  ClaimedPrekeyResponse,
  SearchResponse,
  SearchFilters,
  GlobalSearchResponse,
  Pin,
  CreatePinRequest,
  UpdatePinRequest,
  ServerSettings,
  OidcProvider,
  OidcLoginResult,
  AuthSettingsResponse,
  AuthMethodsConfig,
  AdminOidcProvider,
  GuildSettings,
  GuildUsageStats,
  DiscoverResponse,
  JoinDiscoverableResponse,
  PageRevision,
  RevisionListItem,
  PageCategory,
};

/**
 * Unread aggregation types
 */
export interface ChannelUnread {
  channel_id: string;
  channel_name: string;
  unread_count: number;
}

export interface GuildUnreadSummary {
  guild_id: string;
  guild_name: string;
  channels: ChannelUnread[];
  total_unread: number;
}

export interface UnreadAggregate {
  guilds: GuildUnreadSummary[];
  dms: ChannelUnread[];
  total: number;
}

// Detect if running in Tauri
const isTauri = typeof window !== "undefined" && "__TAURI__" in window;

// Auth response type from server
interface AuthResponse {
  access_token: string;
  refresh_token: string;
  expires_in: number;
  token_type: string;
  setup_required: boolean;
}

// Auth result type (returned by login/register)
export interface AuthResult {
  user: User;
  setup_required: boolean;
}

// ============================================================================
// File Upload Size Limits
// ============================================================================

/**
 * Upload limits response from server
 */
interface UploadLimitsResponse {
  max_avatar_size: number;
  max_emoji_size: number;
  max_upload_size: number;
}

/**
 * File upload size limits fetched from server.
 * Falls back to defaults if fetch fails.
 */
let uploadLimits: UploadLimitsResponse = {
  max_avatar_size: 5 * 1024 * 1024, // 5MB default
  max_emoji_size: 256 * 1024, // 256KB default
  max_upload_size: 50 * 1024 * 1024, // 50MB default
};

/**
 * Fetch upload size limits from server.
 * Should be called on app startup.
 */
export async function fetchUploadLimits(): Promise<void> {
  try {
    const serverUrl = getServerUrl();
    const response = await fetch(`${serverUrl}/api/config/upload-limits`);

    if (!response.ok) {
      console.error(
        `[Upload Limits] Failed to fetch (HTTP ${response.status}), using defaults`,
      );
      return;
    }

    let data: unknown;
    try {
      data = await response.json();
    } catch (parseError) {
      console.error(
        "[Upload Limits] Failed to parse JSON response:",
        parseError,
      );
      console.error(
        "[Upload Limits] Response was not valid JSON - using defaults",
      );
      return;
    }

    // Validate response structure
    const obj = data as Record<string, unknown>;
    if (
      !data ||
      typeof data !== "object" ||
      typeof obj["max_avatar_size"] !== "number" ||
      typeof obj["max_emoji_size"] !== "number" ||
      typeof obj["max_upload_size"] !== "number"
    ) {
      console.error("[Upload Limits] Invalid response structure:", data);
      console.error(
        "[Upload Limits] Expected {max_avatar_size: number, max_emoji_size: number, max_upload_size: number}",
      );
      return;
    }

    // Validate limits are positive
    const limits = data as UploadLimitsResponse;
    if (
      limits.max_avatar_size <= 0 ||
      limits.max_emoji_size <= 0 ||
      limits.max_upload_size <= 0
    ) {
      console.error(
        "[Upload Limits] Invalid limit values (must be positive):",
        limits,
      );
      return;
    }

    uploadLimits = limits;
    console.log(
      "[Upload Limits] Successfully fetched from server:",
      uploadLimits,
    );
  } catch (error) {
    console.error("[Upload Limits] Unexpected error fetching limits:", error);
    console.error("[Upload Limits] Using defaults as fallback");
  }
}

type UploadType = "avatar" | "emoji" | "attachment";

/**
 * Format bytes to human-readable size
 *
 * Matches server implementation in util.rs for consistency.
 */
function formatFileSize(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} bytes`;
  } else if (bytes < 1024 * 1024) {
    return `${Math.floor(bytes / 1024)}KB`;
  } else {
    return `${(bytes / (1024 * 1024)).toFixed(1)}MB`;
  }
}

/**
 * Get formatted upload size limit for UI display
 * @param type - Type of upload (avatar, emoji, or attachment)
 * @returns Human-readable size string (e.g., "5MB", "256KB")
 */
export function getUploadLimitText(type: UploadType): string {
  const maxSize =
    type === "avatar"
      ? uploadLimits.max_avatar_size
      : type === "emoji"
        ? uploadLimits.max_emoji_size
        : uploadLimits.max_upload_size;

  return formatFileSize(maxSize);
}

/**
 * Validate file size on frontend before upload.
 * Uses limits fetched from server, with fallback to hardcoded defaults.
 *
 * @param file - File to validate
 * @param type - Type of upload (avatar, emoji, or attachment)
 * @returns Error message if file is too large, null if valid
 */
export function validateFileSize(file: File, type: UploadType): string | null {
  const maxSize =
    type === "avatar"
      ? uploadLimits.max_avatar_size
      : type === "emoji"
        ? uploadLimits.max_emoji_size
        : uploadLimits.max_upload_size;

  if (file.size > maxSize) {
    return `File too large (${formatFileSize(file.size)}). Maximum size is ${formatFileSize(maxSize)}.`;
  }
  return null;
}

// Browser state (when not in Tauri)
const browserState = {
  serverUrl: typeof window !== "undefined" ? window.location.origin : "",
  accessToken: null as string | null,
  refreshToken: null as string | null,
  tokenExpiresAt: null as number | null,
  refreshTimer: null as ReturnType<typeof setTimeout> | null,
};

// Initialize from localStorage if available
if (typeof localStorage !== "undefined") {
  browserState.serverUrl =
    localStorage.getItem("serverUrl") || browserState.serverUrl;
  browserState.accessToken = localStorage.getItem("accessToken");
  browserState.refreshToken = localStorage.getItem("refreshToken");
  const expiresAt = localStorage.getItem("tokenExpiresAt");
  browserState.tokenExpiresAt = expiresAt ? parseInt(expiresAt, 10) : null;
}

/**
 * Schedule automatic token refresh before expiration.
 * Refreshes 1 minute before the token expires.
 */
function scheduleTokenRefresh() {
  // Clear any existing timer
  if (browserState.refreshTimer) {
    clearTimeout(browserState.refreshTimer);
    browserState.refreshTimer = null;
  }

  if (!browserState.tokenExpiresAt || !browserState.refreshToken) {
    return;
  }

  const now = Date.now();
  const expiresAt = browserState.tokenExpiresAt;
  // Refresh 60 seconds before expiration, but at least 10 seconds from now
  const refreshIn = Math.max(expiresAt - now - 60000, 10000);

  console.log(
    `[Auth] Scheduling token refresh in ${Math.round(refreshIn / 1000)}s`,
  );

  browserState.refreshTimer = setTimeout(async () => {
    try {
      await refreshAccessToken();
    } catch (error) {
      console.error("[Auth] Auto-refresh failed:", error);
      // Token refresh failed - user will need to log in again
    }
  }, refreshIn);
}

/**
 * Refresh the access token using the refresh token.
 */
export async function refreshAccessToken(): Promise<boolean> {
  if (!browserState.refreshToken) {
    console.log("[Auth] No refresh token available");
    return false;
  }

  try {
    console.log("[Auth] Refreshing access token...");

    const baseUrl = browserState.serverUrl.replace(/\/+$/, "");
    const response = await fetch(`${baseUrl}/auth/refresh`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ refresh_token: browserState.refreshToken }),
    });

    if (!response.ok) {
      console.error("[Auth] Token refresh failed:", response.status);
      // Clear tokens if refresh fails
      browserState.accessToken = null;
      browserState.refreshToken = null;
      browserState.tokenExpiresAt = null;
      localStorage.removeItem("accessToken");
      localStorage.removeItem("refreshToken");
      localStorage.removeItem("tokenExpiresAt");
      return false;
    }

    let data: AuthResponse;
    try {
      data = await response.json();
    } catch (parseError) {
      console.error(
        "[Auth] Failed to parse token refresh response as JSON:",
        parseError,
      );

      // Clear tokens - refresh failed
      browserState.accessToken = null;
      browserState.refreshToken = null;
      browserState.tokenExpiresAt = null;
      localStorage.removeItem("accessToken");
      localStorage.removeItem("refreshToken");
      localStorage.removeItem("tokenExpiresAt");

      throw new Error(
        `Token refresh returned invalid JSON: ${parseError instanceof Error ? parseError.message : "Parse failed"}`,
      );
    }

    // Validate tokens are not empty
    if (!data.access_token || !data.refresh_token) {
      console.error("[Auth] Token refresh returned empty tokens");

      // Clear any existing tokens
      browserState.accessToken = null;
      browserState.refreshToken = null;
      browserState.tokenExpiresAt = null;
      localStorage.removeItem("accessToken");
      localStorage.removeItem("refreshToken");
      localStorage.removeItem("tokenExpiresAt");

      throw new Error("Token refresh returned empty tokens");
    }

    // Store new tokens
    browserState.accessToken = data.access_token;
    browserState.refreshToken = data.refresh_token;
    browserState.tokenExpiresAt = Date.now() + data.expires_in * 1000;

    localStorage.setItem("accessToken", data.access_token);
    localStorage.setItem("refreshToken", data.refresh_token);
    localStorage.setItem(
      "tokenExpiresAt",
      browserState.tokenExpiresAt.toString(),
    );

    console.log("[Auth] Token refreshed successfully");

    // Schedule the next refresh
    scheduleTokenRefresh();

    return true;
  } catch (error) {
    console.error("[Auth] Token refresh error:", error);
    return false;
  }
}

// Start token refresh schedule on load if we have tokens
if (browserState.accessToken && browserState.refreshToken) {
  scheduleTokenRefresh();
}

// HTTP helper for browser mode
async function httpRequest<T>(
  method: string,
  path: string,
  body?: unknown,
): Promise<T> {
  // Always read token fresh from localStorage to handle HMR reloads
  const token = browserState.accessToken || localStorage.getItem("accessToken");

  const headers: Record<string, string> = {
    "Content-Type": "application/json",
  };

  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  // Remove trailing slash from serverUrl and ensure path starts with /
  const baseUrl = browserState.serverUrl.replace(/\/+$/, "");
  const cleanPath = path.startsWith("/") ? path : `/${path}`;

  console.log(`[httpRequest] ${method} ${path}`, {
    hasToken: !!token,
    hasAuthHeader: !!headers["Authorization"],
    headers: JSON.stringify(headers),
  });

  const response = await fetch(`${baseUrl}${cleanPath}`, {
    method,
    headers,
    body: body ? JSON.stringify(body) : undefined,
  });

  if (!response.ok) {
    let errorMessage = `HTTP ${response.status}: ${response.statusText}`;

    try {
      const errorBody = await response.json();
      // Detect MFA_REQUIRED from server
      if (response.status === 403 && errorBody.error === "MFA_REQUIRED") {
        throw new Error("MFA_REQUIRED");
      }
      errorMessage = errorBody.message || errorBody.error || errorMessage;
    } catch (parseError) {
      // Re-throw MFA_REQUIRED without wrapping
      if (
        parseError instanceof Error &&
        parseError.message === "MFA_REQUIRED"
      ) {
        throw parseError;
      }
      // Log parse failure but continue with text fallback
      console.warn(
        `[httpRequest] Failed to parse error response as JSON for ${path}:`,
        parseError,
      );

      try {
        const text = await response.text();
        if (text.length > 0 && text.length < 500) {
          errorMessage = text;
        }
      } catch (textError) {
        // Log double failure (both JSON and text parsing failed)
        console.error(
          `[httpRequest] Failed to parse error response as both JSON and text for ${path}:`,
          textError,
        );
        // Use statusText as final fallback
      }
    }

    throw new Error(errorMessage);
  }

  // Handle empty responses
  const text = await response.text();
  if (!text) return null as T;
  return JSON.parse(text);
}

// Auth Commands

export async function login(
  serverUrl: string,
  username: string,
  password: string,
  mfaCode?: string,
): Promise<AuthResult> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("login", {
      request: {
        server_url: serverUrl,
        username,
        password,
        mfa_code: mfaCode ?? null,
      },
    });
  }

  // Browser mode
  browserState.serverUrl = serverUrl;
  localStorage.setItem("serverUrl", serverUrl);

  const body: Record<string, unknown> = { username, password };
  if (mfaCode) {
    body.mfa_code = mfaCode;
  }

  const response = await httpRequest<AuthResponse>("POST", "/auth/login", body);

  // Store all token data
  browserState.accessToken = response.access_token;
  browserState.refreshToken = response.refresh_token;
  browserState.tokenExpiresAt = Date.now() + response.expires_in * 1000;

  localStorage.setItem("accessToken", response.access_token);
  localStorage.setItem("refreshToken", response.refresh_token);
  localStorage.setItem(
    "tokenExpiresAt",
    browserState.tokenExpiresAt.toString(),
  );

  // Schedule automatic token refresh
  scheduleTokenRefresh();

  console.log(
    `[Auth] Login successful, token expires in ${response.expires_in}s`,
  );

  // Fetch user profile after login
  const user = await httpRequest<User>("GET", "/auth/me");

  return {
    user,
    setup_required: response.setup_required,
  };
}

/**
 * Update the user's presence status (online, idle, dnd, invisible, offline).
 */
export async function updateStatus(
  status: "online" | "idle" | "dnd" | "invisible" | "offline",
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("update_status", { status });
  }

  // Map client status names to server enum values
  const statusMap: Record<string, string> = {
    online: "online",
    idle: "away",
    dnd: "busy",
    invisible: "offline",
    offline: "offline",
  };
  browserWs?.send(
    JSON.stringify({ type: "set_status", status: statusMap[status] ?? "online" }),
  );
}

export async function register(
  serverUrl: string,
  username: string,
  password: string,
  email?: string,
  displayName?: string,
): Promise<AuthResult> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("register", {
      request: {
        server_url: serverUrl,
        username,
        email,
        password,
        display_name: displayName,
      },
    });
  }

  // Browser mode
  browserState.serverUrl = serverUrl;
  localStorage.setItem("serverUrl", serverUrl);

  const response = await httpRequest<AuthResponse>("POST", "/auth/register", {
    username,
    password,
    email,
    display_name: displayName,
  });

  // Store all token data
  browserState.accessToken = response.access_token;
  browserState.refreshToken = response.refresh_token;
  browserState.tokenExpiresAt = Date.now() + response.expires_in * 1000;

  localStorage.setItem("accessToken", response.access_token);
  localStorage.setItem("refreshToken", response.refresh_token);
  localStorage.setItem(
    "tokenExpiresAt",
    browserState.tokenExpiresAt.toString(),
  );

  // Schedule automatic token refresh
  scheduleTokenRefresh();

  console.log(
    `[Auth] Registration successful, token expires in ${response.expires_in}s`,
  );

  // Fetch user profile after registration
  const user = await httpRequest<User>("GET", "/auth/me");

  return {
    user,
    setup_required: response.setup_required,
  };
}

export async function logout(): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("logout");
  }

  // Browser mode - clear all token state
  if (browserState.refreshTimer) {
    clearTimeout(browserState.refreshTimer);
    browserState.refreshTimer = null;
  }

  browserState.accessToken = null;
  browserState.refreshToken = null;
  browserState.tokenExpiresAt = null;

  localStorage.removeItem("accessToken");
  localStorage.removeItem("refreshToken");
  localStorage.removeItem("tokenExpiresAt");

  console.log("[Auth] Logged out, tokens cleared");
}

export async function getCurrentUser(): Promise<User | null> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_current_user");
  }

  // Browser mode - check if we have a token
  if (!browserState.accessToken) {
    // Try to refresh if we have a refresh token
    if (browserState.refreshToken) {
      const refreshed = await refreshAccessToken();
      if (!refreshed) {
        return null;
      }
    } else {
      return null;
    }
  }

  try {
    return await httpRequest<User>("GET", "/auth/me");
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : String(error);
    console.warn(`[Auth] Failed to fetch current user: ${errorMessage}`);

    // Determine if this is an auth failure or other error
    const isAuthError =
      errorMessage.includes("401") ||
      errorMessage.includes("403") ||
      errorMessage.includes("Unauthorized") ||
      errorMessage.includes("Forbidden");

    const isJsonParseError =
      errorMessage.includes("invalid JSON") ||
      errorMessage.includes("Parse failed");

    // If JSON parse failed on what might be an auth response, assume auth failure
    // We cannot reliably determine auth state with malformed responses
    if (isJsonParseError && errorMessage.includes("HTTP")) {
      console.error(
        "[Auth] JSON parse error on HTTP response - cannot determine auth state, clearing tokens",
      );
      // Clear all token state - safest approach when we can't parse server response
      if (browserState.refreshTimer) {
        clearTimeout(browserState.refreshTimer);
        browserState.refreshTimer = null;
      }
      browserState.accessToken = null;
      browserState.refreshToken = null;
      browserState.tokenExpiresAt = null;
      localStorage.removeItem("accessToken");
      localStorage.removeItem("refreshToken");
      localStorage.removeItem("tokenExpiresAt");
      return null;
    }

    if (isAuthError && browserState.refreshToken) {
      console.log("[Auth] Token appears invalid, attempting refresh...");
      const refreshed = await refreshAccessToken();
      if (refreshed) {
        try {
          return await httpRequest<User>("GET", "/auth/me");
        } catch (retryError) {
          console.error("[Auth] Retry after refresh failed:", retryError);
          // Refresh didn't help, clear everything below
        }
      }
    }

    // Only clear tokens if we confirmed auth failure, not on network errors
    if (isAuthError) {
      console.warn("[Auth] Authentication failed, clearing session");
      // Clear all token state
      if (browserState.refreshTimer) {
        clearTimeout(browserState.refreshTimer);
        browserState.refreshTimer = null;
      }
      browserState.accessToken = null;
      browserState.refreshToken = null;
      browserState.tokenExpiresAt = null;
      localStorage.removeItem("accessToken");
      localStorage.removeItem("refreshToken");
      localStorage.removeItem("tokenExpiresAt");
    } else {
      console.warn("[Auth] Non-auth error, keeping tokens for retry");
    }

    return null;
  }
}

/** Update the current user's password. */
export async function updatePassword(current_password: string, new_password: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("update_password", { currentPassword: current_password, newPassword: new_password });
  }
  return httpRequest("POST", "/auth/me/password", { current_password, new_password });
}

// ============================================================================
// MFA Commands
// ============================================================================

export interface MfaSetupResponse {
  secret: string;
  qr_code_url: string;
}

export interface MfaVerifyResponse {
  success: boolean;
  message: string;
  backup_codes?: string[];
}

export interface MfaBackupCodesResponse {
  codes: string[];
}

export interface MfaBackupCodeCountResponse {
  remaining: number;
  total: number;
}

/** Setup MFA — returns TOTP secret and QR code URL. */
export async function mfaSetup(): Promise<MfaSetupResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("mfa_setup");
  }
  return httpRequest<MfaSetupResponse>("POST", "/auth/mfa/setup");
}

/** Verify MFA code (TOTP or backup). Returns backup codes on first verify. */
export async function mfaVerify(code: string): Promise<MfaVerifyResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("mfa_verify", { code });
  }
  return httpRequest<MfaVerifyResponse>("POST", "/auth/mfa/verify", { code });
}

/** Disable MFA (requires valid TOTP or backup code). */
export async function mfaDisable(code: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("mfa_disable", { code });
  }
  await httpRequest<unknown>("POST", "/auth/mfa/disable", { code });
}

/** Generate (or regenerate) MFA backup codes. */
export async function mfaGenerateBackupCodes(): Promise<MfaBackupCodesResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("mfa_generate_backup_codes");
  }
  return httpRequest<MfaBackupCodesResponse>("POST", "/auth/mfa/backup-codes");
}

/** Get remaining MFA backup code count. */
export async function mfaBackupCodeCount(): Promise<MfaBackupCodeCountResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("mfa_backup_code_count");
  }
  return httpRequest<MfaBackupCodeCountResponse>(
    "GET",
    "/auth/mfa/backup-codes/count",
  );
}

/**
 * Get auth credentials for fetch-based uploads.
 * In Tauri mode, retrieves from Rust backend state.
 * In browser mode, reads from browserState/localStorage.
 */
async function getUploadAuth(): Promise<{
  token: string | null;
  baseUrl: string;
}> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    const authInfo = await invoke<[string, string] | null>("get_auth_info");
    if (!authInfo) {
      throw new Error("Not authenticated");
    }
    return {
      baseUrl: authInfo[0].replace(/\/+$/, ""),
      token: authInfo[1],
    };
  }
  return {
    token: browserState.accessToken || localStorage.getItem("accessToken"),
    baseUrl: (browserState.serverUrl || window.location.origin).replace(
      /\/+$/,
      "",
    ),
  };
}

export async function uploadAvatar(file: File): Promise<User> {
  // Frontend validation
  const error = validateFileSize(file, "avatar");
  if (error) {
    console.warn("[uploadAvatar] Frontend validation failed:", error);
    throw new Error(error);
  }

  const { token, baseUrl } = await getUploadAuth();

  const headers: Record<string, string> = {};
  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  const formData = new FormData();
  formData.append("avatar", file);

  const response = await fetch(`${baseUrl}/auth/me/avatar`, {
    method: "POST",
    headers,
    body: formData,
  });

  if (!response.ok) {
    let errorMessage = `Upload failed (HTTP ${response.status})`;

    try {
      const errorBody = await response.json();
      errorMessage = errorBody.message || errorBody.error || errorMessage;
    } catch (parseError) {
      console.warn(
        "[uploadAvatar] Failed to parse error response:",
        parseError,
      );
      errorMessage = response.statusText || errorMessage;
    }

    console.error("[uploadAvatar] Upload failed:", {
      status: response.status,
      error: errorMessage,
      fileSize: file.size,
      fileName: file.name,
    });

    throw new Error(errorMessage);
  }

  try {
    return await response.json();
  } catch (parseError) {
    console.error(
      "[uploadAvatar] Failed to parse success response:",
      parseError,
    );
    throw new Error("Server returned invalid response");
  }
}

// Chat Commands

export async function getChannels(): Promise<Channel[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_channels");
  }

  return httpRequest<Channel[]>("GET", "/api/channels");
}

export async function createChannel(
  name: string,
  channelType: "text" | "voice",
  guildId?: string,
  topic?: string,
  categoryId?: string,
): Promise<Channel> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("create_channel", {
      name,
      channelType,
      guildId,
      topic,
      categoryId,
    });
  }

  return httpRequest<Channel>("POST", "/api/channels", {
    name,
    channel_type: channelType,
    guild_id: guildId,
    topic,
    category_id: categoryId,
  });
}

export async function getMessages(
  channelId: string,
  before?: string,
  limit?: number,
): Promise<PaginatedMessages> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_messages", { channelId, before, limit });
  }

  const params = new URLSearchParams();
  if (before) params.set("before", before);
  if (limit) params.set("limit", limit.toString());
  const query = params.toString();

  return httpRequest<PaginatedMessages>(
    "GET",
    `/api/messages/channel/${channelId}${query ? `?${query}` : ""}`,
  );
}

export async function sendMessage(
  channelId: string,
  content: string,
  options?: { encrypted?: boolean; nonce?: string },
): Promise<Message> {
  const result = await sendMessageWithStatus(channelId, content, options);
  return result.message;
}

export interface SendMessageResult {
  message: Message;
  status: number;
}

export async function sendMessageWithStatus(
  channelId: string,
  content: string,
  options?: { encrypted?: boolean; nonce?: string },
): Promise<SendMessageResult> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    const message = await invoke<Message>("send_message", {
      channelId,
      content,
      encrypted: options?.encrypted,
      nonce: options?.nonce,
    });

    // Tauri command interface currently does not expose HTTP status.
    return { message, status: 201 };
  }

  const token = browserState.accessToken || localStorage.getItem("accessToken");
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
  };

  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  const baseUrl = browserState.serverUrl.replace(/\/+$/, "");
  const response = await fetch(`${baseUrl}/api/messages/channel/${channelId}`, {
    method: "POST",
    headers,
    body: JSON.stringify({
      content,
      encrypted: options?.encrypted ?? false,
      nonce: options?.nonce,
    }),
  });

  if (!response.ok) {
    let errorMessage = `HTTP ${response.status}: ${response.statusText}`;
    const rawErrorBody = await response.text();

    try {
      const errorBody = rawErrorBody ? JSON.parse(rawErrorBody) : null;
      errorMessage = errorBody.message || errorBody.error || errorMessage;
    } catch (_parseError) {
      if (rawErrorBody.length > 0 && rawErrorBody.length < 500) {
        errorMessage = rawErrorBody;
      }
    }

    throw new Error(errorMessage);
  }

  const message = (await response.json()) as Message;
  return { message, status: response.status };
}

// ============================================================================
// Thread API Functions
// ============================================================================

export async function getThreadReplies(
  parentId: string,
  after?: string,
  limit?: number,
): Promise<PaginatedMessages> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_thread_replies", { parentId, after, limit });
  }

  const params = new URLSearchParams();
  if (after) params.set("after", after);
  if (limit) params.set("limit", limit.toString());
  const query = params.toString();

  return httpRequest<PaginatedMessages>(
    "GET",
    `/api/messages/${parentId}/thread${query ? `?${query}` : ""}`,
  );
}

export async function sendThreadReply(
  parentId: string,
  channelId: string,
  content: string,
  options?: { encrypted?: boolean; nonce?: string },
): Promise<Message> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("send_thread_reply", {
      parentId,
      channelId,
      content,
      encrypted: options?.encrypted,
      nonce: options?.nonce,
    });
  }

  return httpRequest<Message>("POST", `/api/messages/channel/${channelId}`, {
    content,
    encrypted: options?.encrypted ?? false,
    nonce: options?.nonce,
    parent_id: parentId,
  });
}

export async function markThreadRead(parentId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("mark_thread_read", { parentId });
  }

  return httpRequest<void>("POST", `/api/messages/${parentId}/thread/read`);
}

export async function uploadFile(messageId: string, file: File): Promise<any> {
  // Frontend validation
  const error = validateFileSize(file, "attachment");
  if (error) {
    console.warn("[uploadFile] Frontend validation failed:", error);
    throw new Error(error);
  }

  const { token, baseUrl } = await getUploadAuth();

  const headers: Record<string, string> = {};
  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  const formData = new FormData();
  formData.append("message_id", messageId);
  formData.append("file", file);

  const response = await fetch(`${baseUrl}/api/messages/upload`, {
    method: "POST",
    headers,
    body: formData,
  });

  if (!response.ok) {
    let errorMessage = `Upload failed (HTTP ${response.status})`;

    try {
      const errorBody = await response.json();
      errorMessage = errorBody.message || errorBody.error || errorMessage;
    } catch (parseError) {
      console.warn("[uploadFile] Failed to parse error response:", parseError);
      errorMessage = response.statusText || errorMessage;
    }

    console.error("[uploadFile] Upload failed:", {
      status: response.status,
      error: errorMessage,
      messageId,
      fileSize: file.size,
      fileName: file.name,
    });

    throw new Error(errorMessage);
  }

  try {
    return await response.json();
  } catch (parseError) {
    console.error("[uploadFile] Failed to parse success response:", parseError);
    throw new Error("Server returned invalid response");
  }
}

/**
 * Upload a file and create a message in one request.
 * Uses the combined endpoint that creates the message and attaches the file.
 */
export async function uploadMessageWithFile(
  channelId: string,
  file: File,
  content?: string,
): Promise<Message> {
  // Frontend validation
  const error = validateFileSize(file, "attachment");
  if (error) {
    console.warn("[uploadMessageWithFile] Frontend validation failed:", error);
    throw new Error(error);
  }

  const { token, baseUrl } = await getUploadAuth();

  const headers: Record<string, string> = {};
  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  const formData = new FormData();
  formData.append("file", file);
  if (content) {
    formData.append("content", content);
  }

  const response = await fetch(
    `${baseUrl}/api/messages/channel/${channelId}/upload`,
    {
      method: "POST",
      headers,
      body: formData,
    },
  );

  if (!response.ok) {
    let errorMessage = `Upload failed (HTTP ${response.status})`;

    try {
      const errorBody = await response.json();
      errorMessage = errorBody.message || errorBody.error || errorMessage;
    } catch (parseError) {
      console.warn(
        "[uploadMessageWithFile] Failed to parse error response:",
        parseError,
      );
      errorMessage = response.statusText || errorMessage;
    }

    console.error("[uploadMessageWithFile] Upload failed:", {
      status: response.status,
      error: errorMessage,
      channelId,
      fileSize: file.size,
      fileName: file.name,
    });

    throw new Error(errorMessage);
  }

  try {
    return await response.json();
  } catch (parseError) {
    console.error(
      "[uploadMessageWithFile] Failed to parse success response:",
      parseError,
    );
    throw new Error("Server returned invalid response");
  }
}

// Guild Commands

export async function getGuilds(): Promise<Guild[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_guilds");
  }

  return httpRequest<Guild[]>("GET", "/api/guilds");
}

export async function getGuild(guildId: string): Promise<Guild> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_guild", { guildId });
  }

  return httpRequest<Guild>("GET", `/api/guilds/${guildId}`);
}

export async function createGuild(
  name: string,
  description?: string,
): Promise<Guild> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("create_guild", { name, description });
  }

  return httpRequest<Guild>("POST", "/api/guilds", { name, description });
}

export async function updateGuild(
  guildId: string,
  name?: string,
  description?: string,
  icon_url?: string,
): Promise<Guild> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("update_guild", {
      guildId,
      name,
      description,
      iconUrl: icon_url,
    });
  }

  return httpRequest<Guild>("PATCH", `/api/guilds/${guildId}`, {
    name,
    description,
    icon_url,
  });
}

export async function deleteGuild(guildId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("delete_guild", { guildId });
  }

  await httpRequest<void>("DELETE", `/api/guilds/${guildId}`);
}

export async function joinGuild(
  _guildId: string,
  inviteCode: string,
): Promise<void> {
  // Guild join always requires a valid invite code — route through the invite endpoint
  await joinViaInvite(inviteCode);
}

export async function leaveGuild(guildId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("leave_guild", { guildId });
  }

  await httpRequest<void>("POST", `/api/guilds/${guildId}/leave`);
}

export async function getGuildMembers(guildId: string): Promise<GuildMember[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_guild_members", { guildId });
  }

  return httpRequest<GuildMember[]>("GET", `/api/guilds/${guildId}/members`);
}

export async function getGuildChannels(
  guildId: string,
): Promise<ChannelWithUnread[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_guild_channels", { guildId });
  }

  return httpRequest<ChannelWithUnread[]>(
    "GET",
    `/api/guilds/${guildId}/channels`,
  );
}

/**
 * Get guild settings.
 */
export async function getGuildSettings(
  guildId: string,
): Promise<GuildSettings> {
  return fetchApi<GuildSettings>(`/api/guilds/${guildId}/settings`);
}

/**
 * Get guild resource usage stats (members, channels, roles, emojis, bots).
 */
export async function getGuildUsage(guildId: string): Promise<GuildUsageStats> {
  return fetchApi<GuildUsageStats>(`/api/guilds/${guildId}/usage`);
}

/**
 * Update guild settings (requires MANAGE_GUILD).
 */
export async function updateGuildSettings(
  guildId: string,
  settings: {
    threads_enabled?: boolean;
    discoverable?: boolean;
    tags?: string[];
    banner_url?: string | null;
  },
): Promise<GuildSettings> {
  return fetchApi<GuildSettings>(`/api/guilds/${guildId}/settings`, {
    method: "PATCH",
    body: settings,
  });
}

/**
 * Browse discoverable guilds (public, no auth required).
 */
export async function discoverGuilds(params?: {
  q?: string;
  tags?: string[];
  sort?: "members" | "newest";
  limit?: number;
  offset?: number;
}): Promise<DiscoverResponse> {
  const searchParams = new URLSearchParams();
  if (params?.q) searchParams.set("q", params.q);
  if (params?.tags?.length) searchParams.set("tags", params.tags.join(","));
  if (params?.sort) searchParams.set("sort", params.sort);
  if (params?.limit != null) searchParams.set("limit", String(params.limit));
  if (params?.offset != null) searchParams.set("offset", String(params.offset));
  const qs = searchParams.toString();
  return fetchApi<DiscoverResponse>(
    `/api/discover/guilds${qs ? `?${qs}` : ""}`,
  );
}

/**
 * Join a discoverable guild (requires auth).
 */
export async function joinDiscoverable(
  guildId: string,
): Promise<JoinDiscoverableResponse> {
  return fetchApi<JoinDiscoverableResponse>(
    `/api/discover/guilds/${guildId}/join`,
    {
      method: "POST",
    },
  );
}

/**
 * Mark a guild channel as read.
 * @param channelId - Channel ID to mark as read
 * @param lastReadMessageId - Optional ID of the last read message
 */
export async function markChannelAsRead(
  channelId: string,
  lastReadMessageId?: string,
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("mark_channel_as_read", { channelId, lastReadMessageId });
  }

  await httpRequest<void>("POST", `/api/channels/${channelId}/read`, {
    last_read_message_id: lastReadMessageId,
  });
}

/**
 * Search messages in a guild using full-text search.
 */
export async function searchGuildMessages(
  guildId: string,
  query: string,
  limit: number = 25,
  offset: number = 0,
  filters?: SearchFilters,
): Promise<SearchResponse> {
  // Always use HTTP for search - no Tauri command needed since search is server-side
  const params = new URLSearchParams({
    q: query,
    limit: limit.toString(),
    offset: offset.toString(),
  });
  if (filters?.date_from) params.set("date_from", filters.date_from);
  if (filters?.date_to) params.set("date_to", filters.date_to);
  if (filters?.channel_id) params.set("channel_id", filters.channel_id);
  if (filters?.author_id) params.set("author_id", filters.author_id);
  if (filters?.has) params.set("has", filters.has);
  if (filters?.sort) params.set("sort", filters.sort);
  return httpRequest<SearchResponse>(
    "GET",
    `/api/guilds/${guildId}/search?${params}`,
  );
}

/**
 * Search messages in DM channels using full-text search.
 */
export async function searchDMMessages(
  query: string,
  limit: number = 25,
  offset: number = 0,
  filters?: SearchFilters,
): Promise<SearchResponse> {
  const params = new URLSearchParams({
    q: query,
    limit: limit.toString(),
    offset: offset.toString(),
  });
  if (filters?.date_from) params.set("date_from", filters.date_from);
  if (filters?.date_to) params.set("date_to", filters.date_to);
  if (filters?.channel_id) params.set("channel_id", filters.channel_id);
  if (filters?.author_id) params.set("author_id", filters.author_id);
  if (filters?.has) params.set("has", filters.has);
  if (filters?.sort) params.set("sort", filters.sort);
  return httpRequest<SearchResponse>("GET", `/api/dm/search?${params}`);
}

/**
 * Search messages across all guilds and DMs using full-text search.
 */
export async function searchGlobalMessages(
  query: string,
  limit: number = 25,
  offset: number = 0,
  filters?: SearchFilters,
): Promise<GlobalSearchResponse> {
  const params = new URLSearchParams({
    q: query,
    limit: limit.toString(),
    offset: offset.toString(),
  });
  if (filters?.date_from) params.set("date_from", filters.date_from);
  if (filters?.date_to) params.set("date_to", filters.date_to);
  if (filters?.author_id) params.set("author_id", filters.author_id);
  if (filters?.has) params.set("has", filters.has);
  if (filters?.sort) params.set("sort", filters.sort);
  return httpRequest<GlobalSearchResponse>("GET", `/api/search?${params}`);
}

// Guild Invite Commands

/**
 * Get invites for a guild (owner only)
 */
export async function getGuildInvites(guildId: string): Promise<GuildInvite[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_guild_invites", { guildId });
  }

  return httpRequest<GuildInvite[]>("GET", `/api/guilds/${guildId}/invites`);
}

/**
 * Create a new invite for a guild (owner only)
 */
export async function createGuildInvite(
  guildId: string,
  expiresIn: InviteExpiry = "7d",
): Promise<GuildInvite> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("create_guild_invite", { guildId, expiresIn });
  }

  return httpRequest<GuildInvite>("POST", `/api/guilds/${guildId}/invites`, {
    expires_in: expiresIn,
  });
}

/**
 * Delete/revoke an invite (owner only)
 */
export async function deleteGuildInvite(
  guildId: string,
  code: string,
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("delete_guild_invite", { guildId, code });
  }

  await httpRequest<void>("DELETE", `/api/guilds/${guildId}/invites/${code}`);
}

/**
 * Join a guild via invite code
 */
export async function joinViaInvite(code: string): Promise<InviteResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("join_via_invite", { code });
  }

  return httpRequest<InviteResponse>("POST", `/api/invites/${code}/join`);
}

/**
 * Kick a member from a guild (owner only)
 */
export async function kickGuildMember(
  guildId: string,
  userId: string,
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("kick_guild_member", { guildId, userId });
  }

  await httpRequest<void>("DELETE", `/api/guilds/${guildId}/members/${userId}`);
}

// Guild Category Commands

/**
 * Get all categories for a guild.
 */
export async function getGuildCategories(
  guildId: string,
): Promise<ChannelCategory[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_guild_categories", { guildId });
  }

  return httpRequest<ChannelCategory[]>(
    "GET",
    `/api/guilds/${guildId}/categories`,
  );
}

/**
 * Create a new category in a guild.
 */
export async function createGuildCategory(
  guildId: string,
  name: string,
  parentId?: string,
): Promise<ChannelCategory> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("create_guild_category", { guildId, name, parentId });
  }

  return httpRequest<ChannelCategory>(
    "POST",
    `/api/guilds/${guildId}/categories`,
    {
      name,
      parent_id: parentId,
    },
  );
}

/**
 * Update a category.
 */
export async function updateGuildCategory(
  guildId: string,
  categoryId: string,
  updates: {
    name?: string;
    position?: number;
    parentId?: string | null;
  },
): Promise<ChannelCategory> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("update_guild_category", { guildId, categoryId, ...updates });
  }

  return httpRequest<ChannelCategory>(
    "PATCH",
    `/api/guilds/${guildId}/categories/${categoryId}`,
    {
      name: updates.name,
      position: updates.position,
      parent_id: updates.parentId,
    },
  );
}

// Guild Emoji Commands

export async function getGuildEmojis(guildId: string): Promise<GuildEmoji[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_guild_emojis", { guildId });
  }

  return httpRequest<GuildEmoji[]>("GET", `/api/guilds/${guildId}/emojis`);
}

export async function uploadGuildEmoji(
  guildId: string,
  name: string,
  file: File,
): Promise<GuildEmoji> {
  // Frontend validation
  const validationError = validateFileSize(file, "emoji");
  if (validationError) {
    console.warn(
      "[uploadGuildEmoji] Frontend validation failed:",
      validationError,
    );
    throw new Error(validationError);
  }

  const { token, baseUrl } = await getUploadAuth();

  const headers: Record<string, string> = {};
  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  const formData = new FormData();
  formData.append("name", name);
  formData.append("file", file);

  const response = await fetch(`${baseUrl}/api/guilds/${guildId}/emojis`, {
    method: "POST",
    headers,
    body: formData,
  });

  if (!response.ok) {
    let errorMessage = `Upload failed (HTTP ${response.status})`;

    try {
      const errorBody = await response.json();
      errorMessage = errorBody.message || errorBody.error || errorMessage;
    } catch (parseError) {
      console.warn(
        "[uploadGuildEmoji] Failed to parse error response:",
        parseError,
      );
      errorMessage = response.statusText || errorMessage;
    }

    console.error("[uploadGuildEmoji] Upload failed:", {
      status: response.status,
      error: errorMessage,
      guildId,
      emojiName: name,
      fileSize: file.size,
      fileName: file.name,
    });

    throw new Error(errorMessage);
  }

  try {
    return await response.json();
  } catch (parseError) {
    console.error(
      "[uploadGuildEmoji] Failed to parse success response:",
      parseError,
    );
    throw new Error("Server returned invalid response");
  }
}

export async function updateGuildEmoji(
  guildId: string,
  emojiId: string,
  name: string,
): Promise<GuildEmoji> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("update_guild_emoji", { guildId, emojiId, name });
  }

  return httpRequest<GuildEmoji>(
    "PATCH",
    `/api/guilds/${guildId}/emojis/${emojiId}`,
    {
      name,
    },
  );
}

export async function deleteGuildEmoji(
  guildId: string,
  emojiId: string,
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("delete_guild_emoji", { guildId, emojiId });
  }

  await httpRequest<void>("DELETE", `/api/guilds/${guildId}/emojis/${emojiId}`);
}

/**
 * Delete a category.
 */
export async function deleteGuildCategory(
  guildId: string,
  categoryId: string,
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("delete_guild_category", { guildId, categoryId });
  }

  await httpRequest<void>(
    "DELETE",
    `/api/guilds/${guildId}/categories/${categoryId}`,
  );
}

/**
 * Reorder categories in a guild.
 */
export async function reorderGuildCategories(
  guildId: string,
  categories: Array<{ id: string; position: number; parentId?: string | null }>,
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("reorder_guild_categories", { guildId, categories });
  }

  await httpRequest<void>("POST", `/api/guilds/${guildId}/categories/reorder`, {
    categories: categories.map((c) => ({
      id: c.id,
      position: c.position,
      parent_id: c.parentId,
    })),
  });
}

/**
 * Position specification for channel reorder.
 */
export interface ChannelPosition {
  id: string;
  position: number;
  category_id: string | null;
}

/**
 * Reorder channels in a guild.
 * Requires MANAGE_CHANNELS permission.
 */
export async function reorderGuildChannels(
  guildId: string,
  channels: ChannelPosition[],
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("reorder_guild_channels", { guildId, channels });
  }

  await httpRequest<void>("POST", `/api/guilds/${guildId}/channels/reorder`, {
    channels,
  });
}

// Friends Commands

export async function getFriends(): Promise<Friend[]> {
  return httpRequest<Friend[]>("GET", "/api/friends");
}

export async function getPendingFriends(): Promise<Friend[]> {
  return httpRequest<Friend[]>("GET", "/api/friends/pending");
}

export async function getBlockedFriends(): Promise<Friend[]> {
  return httpRequest<Friend[]>("GET", "/api/friends/blocked");
}

export async function sendFriendRequest(username: string): Promise<Friendship> {
  return httpRequest<Friendship>("POST", "/api/friends/request", { username });
}

// Pins Commands

export async function fetchPins(): Promise<Pin[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("fetch_pins");
  }

  return httpRequest<Pin[]>("GET", "/api/me/pins");
}

export async function createPin(request: CreatePinRequest): Promise<Pin> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("create_pin", { request });
  }

  return httpRequest<Pin>("POST", "/api/me/pins", request);
}

export async function updatePin(
  pinId: string,
  request: UpdatePinRequest,
): Promise<Pin> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("update_pin", { pin_id: pinId, request });
  }

  return httpRequest<Pin>("PUT", `/api/me/pins/${pinId}`, request);
}

export async function deletePin(pinId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("delete_pin", { pin_id: pinId });
  }

  await httpRequest<void>("DELETE", `/api/me/pins/${pinId}`);
}

export async function reorderPins(pinIds: string[]): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("reorder_pins", { pin_ids: pinIds });
  }

  await httpRequest<void>("PUT", "/api/me/pins/reorder", { pin_ids: pinIds });
}

export async function acceptFriendRequest(
  friendshipId: string,
): Promise<Friendship> {
  return httpRequest<Friendship>("POST", `/api/friends/${friendshipId}/accept`);
}

export async function rejectFriendRequest(friendshipId: string): Promise<void> {
  await httpRequest<void>("POST", `/api/friends/${friendshipId}/reject`);
}

export async function removeFriend(friendshipId: string): Promise<void> {
  await httpRequest<void>("DELETE", `/api/friends/${friendshipId}`);
}

export async function blockUser(userId: string): Promise<Friendship> {
  return httpRequest<Friendship>("POST", `/api/friends/${userId}/block`);
}

export async function unblockUser(userId: string): Promise<void> {
  await httpRequest<void>("DELETE", `/api/friends/${userId}/block`);
}

// Report Commands

export interface CreateReportRequest {
  target_type: "user" | "message";
  target_user_id: string;
  target_message_id?: string;
  category:
  | "harassment"
  | "spam"
  | "inappropriate_content"
  | "impersonation"
  | "other";
  description?: string;
}

export interface ReportResponse {
  id: string;
  reporter_id: string;
  target_type: string;
  target_user_id: string;
  target_message_id: string | null;
  category: string;
  description: string | null;
  status: string;
  created_at: string;
}

export async function createReport(
  request: CreateReportRequest,
): Promise<ReportResponse> {
  return httpRequest<ReportResponse>("POST", "/api/reports", request);
}

// Admin Report Commands

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

// DM Commands

export interface DMIconResponse {
  icon_url: string;
}

export async function uploadDMAvatar(
  channelId: string,
  file: File,
): Promise<DMIconResponse> {
  // Frontend validation
  const validationError = validateFileSize(file, "avatar");
  if (validationError) {
    console.warn(
      "[uploadDMAvatar] Frontend validation failed:",
      validationError,
    );
    throw new Error(validationError);
  }

  const formData = new FormData();
  formData.append("file", file);

  const token = getAccessToken();
  const headers: HeadersInit = {};
  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  const response = await fetch(`${getServerUrl()}/api/dm/${channelId}/icon`, {
    method: "POST",
    headers,
    body: formData,
  });

  if (!response.ok) {
    let errorMessage = `Upload failed (HTTP ${response.status})`;

    try {
      const errorBody = await response.json();
      errorMessage = errorBody.message || errorBody.error || errorMessage;
    } catch (parseError) {
      console.warn(
        "[uploadDMAvatar] Failed to parse error response:",
        parseError,
      );
      errorMessage = response.statusText || errorMessage;
    }

    console.error("[uploadDMAvatar] Upload failed:", {
      status: response.status,
      error: errorMessage,
      channelId,
      fileSize: file.size,
      fileName: file.name,
    });

    throw new Error(errorMessage);
  }

  try {
    return await response.json();
  } catch (parseError) {
    console.error(
      "[uploadDMAvatar] Failed to parse success response:",
      parseError,
    );
    throw new Error("Server returned invalid response");
  }
}

export async function getDMs(): Promise<DMChannel[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_dms");
  }

  return httpRequest<DMChannel[]>("GET", "/api/dm");
}

export async function getDM(channelId: string): Promise<DMChannel> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_dm", { channelId });
  }

  return httpRequest<DMChannel>("GET", `/api/dm/${channelId}`);
}

export async function createDM(
  participantIds: string[],
  name?: string,
): Promise<DMChannel> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("create_dm", { participantIds, name });
  }

  return httpRequest<DMChannel>("POST", "/api/dm", {
    participant_ids: participantIds,
    name,
  });
}

export async function leaveDM(channelId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("leave_dm", { channelId });
  }

  await httpRequest<void>("POST", `/api/dm/${channelId}/leave`);
}

export async function getDMList(): Promise<DMListItem[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_dm_list");
  }

  return httpRequest<DMListItem[]>("GET", "/api/dm");
}

export async function markDMAsRead(
  channelId: string,
  lastReadMessageId?: string,
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("mark_dm_as_read", { channelId, lastReadMessageId });
  }

  await httpRequest<void>("POST", `/api/dm/${channelId}/read`, {
    last_read_message_id: lastReadMessageId,
  });
}

/**
 * Update the display name of a group DM channel.
 */
export async function updateDMName(
  channelId: string,
  name: string,
): Promise<void> {
  await httpRequest<void>("PATCH", `/api/dm/${channelId}/name`, { name });
}

// DM Call Commands

/**
 * Get the current call state for a DM channel.
 */
export async function getCallState(
  channelId: string,
): Promise<CallStateResponse | null> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_call_state", { channelId });
  }

  return httpRequest<CallStateResponse | null>(
    "GET",
    `/api/dm/${channelId}/call`,
  );
}

/**
 * Start a new call in a DM channel.
 */
export async function startDMCall(
  channelId: string,
): Promise<CallStateResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("start_dm_call", { channelId });
  }

  return httpRequest<CallStateResponse>(
    "POST",
    `/api/dm/${channelId}/call/start`,
  );
}

/**
 * Join an active call in a DM channel.
 */
export async function joinDMCall(
  channelId: string,
): Promise<CallStateResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("join_dm_call", { channelId });
  }

  return httpRequest<CallStateResponse>(
    "POST",
    `/api/dm/${channelId}/call/join`,
  );
}

/**
 * Decline an incoming call in a DM channel.
 */
export async function declineDMCall(
  channelId: string,
): Promise<CallStateResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("decline_dm_call", { channelId });
  }

  return httpRequest<CallStateResponse>(
    "POST",
    `/api/dm/${channelId}/call/decline`,
  );
}

/**
 * Leave an active call in a DM channel.
 */
export async function leaveDMCall(
  channelId: string,
): Promise<CallStateResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("leave_dm_call", { channelId });
  }

  return httpRequest<CallStateResponse>(
    "POST",
    `/api/dm/${channelId}/call/leave`,
  );
}

// Voice Commands (browser mode stubs - voice requires Tauri)

export async function joinVoice(channelId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("join_voice", { channelId });
  }
  console.warn("Voice chat requires the native app");
}

export async function leaveVoice(): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("leave_voice");
  }
}

export async function setMute(muted: boolean): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("set_mute", { muted });
  }
}

export async function setDeafen(deafened: boolean): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("set_deafen", { deafened });
  }
}

// Settings Commands

export async function getSettings(): Promise<AppSettings> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_settings");
  }

  // Browser mode - return defaults
  return {
    audio: {
      input_device: null,
      output_device: null,
      input_volume: 100,
      output_volume: 100,
      noise_suppression: true,
      echo_cancellation: true,
    },
    voice: {
      push_to_talk: false,
      push_to_talk_key: null,
      voice_activity_detection: true,
      vad_threshold: 0.5,
    },
    theme: "dark",
    notifications_enabled: true,
  };
}

export async function updateSettings(settings: AppSettings): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("update_settings", { settings });
  }
  // Browser mode - no-op
}

export async function getUiState(): Promise<UiState> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_ui_state");
  }
  return { category_collapse: {} };
}

export async function updateCategoryCollapse(
  categoryId: string,
  collapsed: boolean,
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("update_category_collapse", { categoryId, collapsed });
  }
}

// WebSocket Commands

export type ConnectionStatus =
  | { type: "disconnected" }
  | { type: "connecting" }
  | { type: "connected" }
  | { type: "reconnecting"; attempt: number };

// Browser WebSocket instance
let browserWs: WebSocket | null = null;
let browserWsStatus: ConnectionStatus = { type: "disconnected" };

export async function wsConnect(): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("ws_connect");
  }

  // Browser mode
  if (browserWs?.readyState === WebSocket.OPEN) return;

  if (!browserState.accessToken) {
    throw new Error("No access token available for WebSocket connection");
  }

  browserWsStatus = { type: "connecting" };
  // Server expects token in Sec-WebSocket-Protocol header.
  // Send both the token protocol and "access_token" so the server can echo
  // back "access_token" (which the browser accepts as a matching protocol).
  const wsUrl = browserState.serverUrl.replace(/^http/, "ws") + "/ws";
  const wsTokenProtocol = `access_token.${browserState.accessToken}`;

  return new Promise((resolve, reject) => {
    browserWs = new WebSocket(wsUrl, [wsTokenProtocol, "access_token"]);

    browserWs.onopen = async () => {
      browserWsStatus = { type: "connected" };
      console.log("[WebSocket] Connected to server");

      // Re-initialize WebSocket event listeners
      try {
        const { reinitWebSocketListeners } = await import("@/stores/websocket");
        await reinitWebSocketListeners();
        console.log("[WebSocket] Event listeners reinitialized");
      } catch (err) {
        console.error("[WebSocket] Failed to reinitialize listeners:", err);
      }

      resolve();
    };

    browserWs.onerror = (err) => {
      browserWsStatus = { type: "disconnected" };
      console.error("[WebSocket] Connection error:", err);
      reject(err);
    };

    browserWs.onclose = () => {
      browserWsStatus = { type: "disconnected" };
      console.log("[WebSocket] Connection closed");
    };
  });
}

export async function wsDisconnect(): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("ws_disconnect");
  }

  browserWs?.close();
  browserWs = null;
  browserWsStatus = { type: "disconnected" };
}

export async function wsStatus(): Promise<ConnectionStatus> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("ws_status");
  }

  return browserWsStatus;
}

export async function wsSubscribe(channelId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("ws_subscribe", { channelId });
  }

  browserWs?.send(JSON.stringify({ type: "subscribe", channel_id: channelId }));
}

export async function wsUnsubscribe(channelId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("ws_unsubscribe", { channelId });
  }

  browserWs?.send(
    JSON.stringify({ type: "unsubscribe", channel_id: channelId }),
  );
}

export async function wsTyping(channelId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("ws_typing", { channelId });
  }

  browserWs?.send(JSON.stringify({ type: "typing", channel_id: channelId }));
}

export async function wsStopTyping(channelId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("ws_stop_typing", { channelId });
  }

  browserWs?.send(
    JSON.stringify({ type: "stop_typing", channel_id: channelId }),
  );
}

export async function wsPing(): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("ws_ping");
  }

  browserWs?.send(JSON.stringify({ type: "ping" }));
}

// Export browser WebSocket for event handling
export function getBrowserWebSocket(): WebSocket | null {
  return isTauri ? null : browserWs;
}

export function getServerUrl(): string {
  return browserState.serverUrl;
}

/**
 * Get the current access token (for use in URLs that can't use headers).
 */
export function getAccessToken(): string | null {
  return browserState.accessToken || localStorage.getItem("accessToken");
}

/**
 * Send a WebSocket message (works in both browser and Tauri modes)
 */
export async function wsSend(message: any): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("ws_send", { message: JSON.stringify(message) });
  } else {
    if (!browserWs || browserWs.readyState !== WebSocket.OPEN) {
      throw new Error(
        "WebSocket not connected. Current state: " +
        (browserWs ? browserWs.readyState : "null"),
      );
    }
    browserWs.send(JSON.stringify(message));
  }
}

/**
 * Subscribe to admin events (requires elevated admin).
 */
export async function wsAdminSubscribe(): Promise<void> {
  await wsSend({ type: "admin_subscribe" });
}

/**
 * Unsubscribe from admin events.
 */
export async function wsAdminUnsubscribe(): Promise<void> {
  await wsSend({ type: "admin_unsubscribe" });
}

/**
 * Start screen sharing in a voice channel (notifies server).
 */
export async function wsScreenShareStart(
  channelId: string,
  quality: "low" | "medium" | "high" | "premium",
  hasAudio: boolean,
  sourceLabel: string,
): Promise<void> {
  await wsSend({
    type: "voice_screen_share_start",
    channel_id: channelId,
    quality,
    has_audio: hasAudio,
    source_label: sourceLabel,
  });
}

/**
 * Stop screen sharing in a voice channel (notifies server).
 */
export async function wsScreenShareStop(channelId: string): Promise<void> {
  await wsSend({
    type: "voice_screen_share_stop",
    channel_id: channelId,
  });
}

/**
 * Start webcam in a voice channel (notifies server).
 */
export async function wsWebcamStart(
  channelId: string,
  quality: "low" | "medium" | "high" | "premium",
): Promise<void> {
  await wsSend({
    type: "voice_webcam_start",
    channel_id: channelId,
    quality,
  });
}

/**
 * Stop webcam in a voice channel (notifies server).
 */
export async function wsWebcamStop(channelId: string): Promise<void> {
  await wsSend({
    type: "voice_webcam_stop",
    channel_id: channelId,
  });
}

// Pages Commands

/**
 * List all platform pages.
 */
export async function listPlatformPages(): Promise<PageListItem[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("list_platform_pages");
  }

  return httpRequest<PageListItem[]>("GET", "/api/pages");
}

/**
 * Get a platform page by slug.
 */
export async function getPlatformPage(slug: string): Promise<Page> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_platform_page", { slug });
  }

  return httpRequest<Page>("GET", `/api/pages/by-slug/${slug}`);
}

/**
 * Create a platform page (admin only).
 */
export async function createPlatformPage(
  title: string,
  content: string,
  slug?: string,
  requiresAcceptance?: boolean,
): Promise<Page> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("create_platform_page", {
      title,
      content,
      slug,
      requiresAcceptance,
    });
  }

  return httpRequest<Page>("POST", "/api/pages", {
    title,
    content,
    slug,
    requires_acceptance: requiresAcceptance,
  });
}

/**
 * Update a platform page (admin only).
 */
export async function updatePlatformPage(
  pageId: string,
  title?: string,
  slug?: string,
  content?: string,
  requiresAcceptance?: boolean,
): Promise<Page> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("update_platform_page", {
      pageId,
      title,
      slug,
      content,
      requiresAcceptance,
    });
  }

  return httpRequest<Page>("PATCH", `/api/pages/${pageId}`, {
    title,
    slug,
    content,
    requires_acceptance: requiresAcceptance,
  });
}

/**
 * Delete a platform page (admin only).
 */
export async function deletePlatformPage(pageId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("delete_platform_page", { pageId });
  }

  await httpRequest<void>("DELETE", `/api/pages/${pageId}`);
}

/**
 * Reorder platform pages (admin only).
 */
export async function reorderPlatformPages(pageIds: string[]): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("reorder_platform_pages", { pageIds });
  }

  await httpRequest<void>("POST", "/api/pages/reorder", { page_ids: pageIds });
}

/**
 * List guild pages.
 */
export async function listGuildPages(guildId: string): Promise<PageListItem[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("list_guild_pages", { guildId });
  }

  return httpRequest<PageListItem[]>("GET", `/api/guilds/${guildId}/pages`);
}

/**
 * Get a guild page by slug.
 */
export async function getGuildPage(
  guildId: string,
  slug: string,
): Promise<Page> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_guild_page", { guildId, slug });
  }

  return httpRequest<Page>(
    "GET",
    `/api/guilds/${guildId}/pages/by-slug/${slug}`,
  );
}

/**
 * Create a guild page.
 */
export async function createGuildPage(
  guildId: string,
  title: string,
  content: string,
  slug?: string,
  requiresAcceptance?: boolean,
  categoryId?: string,
): Promise<Page> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("create_guild_page", {
      guildId,
      title,
      content,
      slug,
      requiresAcceptance,
      categoryId,
    });
  }

  return httpRequest<Page>("POST", `/api/guilds/${guildId}/pages`, {
    title,
    content,
    slug,
    requires_acceptance: requiresAcceptance,
    category_id: categoryId,
  });
}

/**
 * Update a guild page.
 */
export async function updateGuildPage(
  guildId: string,
  pageId: string,
  title?: string,
  slug?: string,
  content?: string,
  requiresAcceptance?: boolean,
  categoryId?: string | null,
): Promise<Page> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("update_guild_page", {
      guildId,
      pageId,
      title,
      slug,
      content,
      requiresAcceptance,
      categoryId,
    });
  }

  // Build body — only include category_id if explicitly provided
  const body: Record<string, unknown> = {
    title,
    slug,
    content,
    requires_acceptance: requiresAcceptance,
  };
  if (categoryId !== undefined) {
    body.category_id = categoryId;
  }

  return httpRequest<Page>(
    "PATCH",
    `/api/guilds/${guildId}/pages/${pageId}`,
    body,
  );
}

/**
 * Delete a guild page.
 */
export async function deleteGuildPage(
  guildId: string,
  pageId: string,
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("delete_guild_page", { guildId, pageId });
  }

  await httpRequest<void>("DELETE", `/api/guilds/${guildId}/pages/${pageId}`);
}

/**
 * Reorder guild pages.
 */
export async function reorderGuildPages(
  guildId: string,
  pageIds: string[],
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("reorder_guild_pages", { guildId, pageIds });
  }

  await httpRequest<void>("POST", `/api/guilds/${guildId}/pages/reorder`, {
    page_ids: pageIds,
  });
}

/**
 * Accept a page.
 */
export async function acceptPage(pageId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("accept_page", { pageId });
  }

  await httpRequest<void>("POST", `/api/pages/${pageId}/accept`);
}

/**
 * Get pages pending acceptance.
 */
export async function getPendingAcceptance(): Promise<PageListItem[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_pending_acceptance");
  }

  return httpRequest<PageListItem[]>("GET", "/api/pages/pending-acceptance");
}

// Revision Commands

/**
 * List all revisions for a guild page.
 */
export async function listPageRevisions(
  guildId: string,
  pageId: string,
): Promise<RevisionListItem[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("list_page_revisions", { guildId, pageId });
  }

  return httpRequest<RevisionListItem[]>(
    "GET",
    `/api/guilds/${guildId}/pages/${pageId}/revisions`,
  );
}

/**
 * Get a specific revision of a guild page.
 */
export async function getPageRevision(
  guildId: string,
  pageId: string,
  revisionNumber: number,
): Promise<PageRevision> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_page_revision", { guildId, pageId, revisionNumber });
  }

  return httpRequest<PageRevision>(
    "GET",
    `/api/guilds/${guildId}/pages/${pageId}/revisions/${revisionNumber}`,
  );
}

/**
 * Restore a guild page to a specific revision.
 */
export async function restorePageRevision(
  guildId: string,
  pageId: string,
  revisionNumber: number,
): Promise<Page> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("restore_page_revision", { guildId, pageId, revisionNumber });
  }

  return httpRequest<Page>(
    "POST",
    `/api/guilds/${guildId}/pages/${pageId}/revisions/${revisionNumber}/restore`,
  );
}

// Page Category Commands

/**
 * List all page categories for a guild.
 */
export async function listPageCategories(
  guildId: string,
): Promise<PageCategory[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("list_page_categories", { guildId });
  }

  return httpRequest<PageCategory[]>(
    "GET",
    `/api/guilds/${guildId}/page-categories`,
  );
}

/**
 * Create a page category in a guild.
 */
export async function createPageCategory(
  guildId: string,
  name: string,
): Promise<PageCategory> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("create_page_category", { guildId, name });
  }

  return httpRequest<PageCategory>(
    "POST",
    `/api/guilds/${guildId}/page-categories`,
    {
      name,
    },
  );
}

/**
 * Update a page category in a guild.
 */
export async function updatePageCategory(
  guildId: string,
  categoryId: string,
  name: string,
): Promise<PageCategory> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("update_page_category", { guildId, categoryId, name });
  }

  return httpRequest<PageCategory>(
    "PATCH",
    `/api/guilds/${guildId}/page-categories/${categoryId}`,
    { name },
  );
}

/**
 * Delete a page category from a guild.
 */
export async function deletePageCategory(
  guildId: string,
  categoryId: string,
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("delete_page_category", { guildId, categoryId });
  }

  await httpRequest<void>(
    "DELETE",
    `/api/guilds/${guildId}/page-categories/${categoryId}`,
  );
}

/**
 * Reorder page categories in a guild.
 */
export async function reorderPageCategories(
  guildId: string,
  categoryIds: string[],
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("reorder_page_categories", { guildId, categoryIds });
  }

  await httpRequest<void>(
    "POST",
    `/api/guilds/${guildId}/page-categories/reorder`,
    {
      category_ids: categoryIds,
    },
  );
}

// ============================================================================
// Role Commands
// ============================================================================

/**
 * Get all roles for a guild.
 */
export async function getGuildRoles(guildId: string): Promise<GuildRole[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_guild_roles", { guildId });
  }

  return httpRequest<GuildRole[]>("GET", `/api/guilds/${guildId}/roles`);
}

/**
 * Create a new role in a guild.
 */
export async function createGuildRole(
  guildId: string,
  request: CreateRoleRequest,
): Promise<GuildRole> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("create_guild_role", { guildId, request });
  }

  return httpRequest<GuildRole>(
    "POST",
    `/api/guilds/${guildId}/roles`,
    request,
  );
}

/**
 * Update an existing role.
 */
export async function updateGuildRole(
  guildId: string,
  roleId: string,
  request: UpdateRoleRequest,
): Promise<GuildRole> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("update_guild_role", { guildId, roleId, request });
  }

  return httpRequest<GuildRole>(
    "PATCH",
    `/api/guilds/${guildId}/roles/${roleId}`,
    request,
  );
}

/**
 * Delete a role from a guild.
 */
export async function deleteGuildRole(
  guildId: string,
  roleId: string,
): Promise<DeleteRoleResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("delete_guild_role", { guildId, roleId });
  }

  return httpRequest<DeleteRoleResponse>(
    "DELETE",
    `/api/guilds/${guildId}/roles/${roleId}`,
  );
}

/**
 * Get all member role assignments for a guild.
 * Returns a map of user_id -> list of role_ids.
 */
export async function getGuildMemberRoles(
  guildId: string,
): Promise<Record<string, string[]>> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_guild_member_roles", { guildId });
  }

  return httpRequest<Record<string, string[]>>(
    "GET",
    `/api/guilds/${guildId}/member-roles`,
  );
}

/**
 * Assign a role to a guild member.
 */
export async function assignMemberRole(
  guildId: string,
  userId: string,
  roleId: string,
): Promise<AssignRoleResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("assign_member_role", { guildId, userId, roleId });
  }

  return httpRequest<AssignRoleResponse>(
    "POST",
    `/api/guilds/${guildId}/members/${userId}/roles/${roleId}`,
  );
}

/**
 * Remove a role from a guild member.
 */
export async function removeMemberRole(
  guildId: string,
  userId: string,
  roleId: string,
): Promise<RemoveRoleResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("remove_member_role", { guildId, userId, roleId });
  }

  return httpRequest<RemoveRoleResponse>(
    "DELETE",
    `/api/guilds/${guildId}/members/${userId}/roles/${roleId}`,
  );
}

// ============================================================================
// Channel Override Commands
// ============================================================================

/**
 * Get permission overrides for a channel.
 */
export async function getChannelOverrides(
  channelId: string,
): Promise<ChannelOverride[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_channel_overrides", { channelId });
  }

  return httpRequest<ChannelOverride[]>(
    "GET",
    `/api/channels/${channelId}/overrides`,
  );
}

/**
 * Set a permission override for a role in a channel.
 */
export async function setChannelOverride(
  channelId: string,
  roleId: string,
  request: SetChannelOverrideRequest,
): Promise<ChannelOverride> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("set_channel_override", { channelId, roleId, request });
  }

  return httpRequest<ChannelOverride>(
    "PUT",
    `/api/channels/${channelId}/overrides/${roleId}`,
    request,
  );
}

/**
 * Delete a permission override for a role in a channel.
 */
export async function deleteChannelOverride(
  channelId: string,
  roleId: string,
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("delete_channel_override", { channelId, roleId });
  }

  await httpRequest<void>(
    "DELETE",
    `/api/channels/${channelId}/overrides/${roleId}`,
  );
}

// ============================================================================
// Admin API
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
// Generic API Helpers
// ============================================================================

/**
 * Generic fetch helper for API calls.
 * Handles authentication and error handling.
 */
export async function fetchApi<T>(
  path: string,
  options?: {
    method?: string;
    body?: unknown;
  },
): Promise<T> {
  return httpRequest<T>(options?.method ?? "GET", path, options?.body);
}

// ============================================================================
// E2EE Commands
// ============================================================================

/**
 * Get the current E2EE status (initialization state, device ID, etc.).
 * Note: E2EE commands require Tauri - they are not available in browser mode.
 */
export async function getE2EEStatus(): Promise<E2EEStatus> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<E2EEStatus>("get_e2ee_status");
  }

  // Browser mode - E2EE not available
  return {
    initialized: false,
    device_id: null,
    has_identity_keys: false,
  };
}

/**
 * Initialize E2EE with the given encryption key (derived from user password).
 * This generates identity keys and prekeys for the device.
 * Note: E2EE commands require Tauri - they are not available in browser mode.
 */
export async function initE2EE(
  encryptionKey: string,
): Promise<InitE2EEResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<InitE2EEResponse>("init_e2ee", { encryptionKey });
  }

  throw new Error("E2EE requires the native Tauri app");
}

/**
 * Encrypt a message for the given recipients.
 * Recipients must include their claimed prekeys from the server.
 * Note: E2EE commands require Tauri - they are not available in browser mode.
 */
export async function encryptMessage(
  plaintext: string,
  recipients: ClaimedPrekeyInput[],
): Promise<E2EEContent> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<E2EEContent>("encrypt_message", { plaintext, recipients });
  }

  throw new Error("E2EE requires the native Tauri app");
}

/**
 * Decrypt a message from another user.
 * Note: E2EE commands require Tauri - they are not available in browser mode.
 */
export async function decryptMessage(
  senderUserId: string,
  senderKey: string,
  messageType: number,
  ciphertext: string,
): Promise<string> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<string>("decrypt_message", {
      senderUserId,
      senderKey,
      messageType,
      ciphertext,
    });
  }

  throw new Error("E2EE requires the native Tauri app");
}

// =============================================================================
// Megolm Group E2EE Commands
// =============================================================================

/**
 * Create a new Megolm outbound session for a group/channel.
 * Returns the exportable session key (base64) that should be shared with other members.
 */
export async function createMegolmSession(roomId: string): Promise<string> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<string>("create_megolm_session", { roomId });
  }

  throw new Error("E2EE requires the native Tauri app");
}

/**
 * Encrypt a message for a group using Megolm.
 */
export async function encryptGroupMessage(roomId: string, plaintext: string): Promise<string> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<string>("encrypt_group_message", { roomId, plaintext });
  }

  throw new Error("E2EE requires the native Tauri app");
}

/**
 * Store an inbound Megolm session key received from another user.
 */
export async function addInboundGroupSession(
  roomId: string,
  senderKey: string,
  sessionKey: string
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<void>("add_inbound_group_session", { roomId, senderKey, sessionKey });
  }

  throw new Error("E2EE requires the native Tauri app");
}

/**
 * Decrypt a Megolm group message.
 */
export async function decryptGroupMessage(
  roomId: string,
  senderKey: string,
  ciphertext: string
): Promise<string> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<string>("decrypt_group_message", { roomId, senderKey, ciphertext });
  }

  throw new Error("E2EE requires the native Tauri app");
}

/**
 * Mark prekeys as published after uploading them to the server.
 * Note: E2EE commands require Tauri - they are not available in browser mode.
 */
export async function markPrekeysPublished(): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<void>("mark_prekeys_published");
  }

  throw new Error("E2EE requires the native Tauri app");
}

/**
 * Generate additional prekeys (one-time keys).
 * Note: E2EE commands require Tauri - they are not available in browser mode.
 */
export async function generatePrekeys(count: number): Promise<PrekeyData[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<PrekeyData[]>("generate_prekeys", { count });
  }

  throw new Error("E2EE requires the native Tauri app");
}

/**
 * Check if the device needs to upload more prekeys to the server.
 * Note: E2EE commands require Tauri - they are not available in browser mode.
 */
export async function needsPrekeyUpload(): Promise<boolean> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<boolean>("needs_prekey_upload");
  }

  // Browser mode - always return false
  return false;
}

/**
 * Get our Curve25519 public key (base64).
 * This is needed for looking up our ciphertext in encrypted messages.
 * Note: E2EE commands require Tauri - they are not available in browser mode.
 */
export async function getOurCurve25519Key(): Promise<string | null> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<string>("get_our_curve25519_key");
  }

  // Browser mode - not available
  return null;
}

// ============================================================================
// E2EE Key API Endpoints
// ============================================================================

/**
 * Get another user's device keys for establishing encrypted sessions.
 * Returns all devices and their public identity keys.
 */
export async function getUserKeys(userId: string): Promise<UserKeysResponse> {
  return httpRequest<UserKeysResponse>("GET", `/api/users/${userId}/keys`);
}

/**
 * Claim a prekey from a specific device to establish an encrypted session.
 * The prekey is consumed and cannot be reused.
 */
export async function claimPrekey(
  userId: string,
  deviceId: string,
): Promise<ClaimedPrekeyResponse> {
  return httpRequest<ClaimedPrekeyResponse>(
    "POST",
    `/api/users/${userId}/keys/claim`,
    { device_id: deviceId },
  );
}

/**
 * Upload identity keys and prekeys to the server.
 * Creates or updates the device record.
 */
export async function uploadKeys(
  deviceName: string | null,
  identityKeyEd25519: string,
  identityKeyCurve25519: string,
  oneTimePrekeys: PrekeyData[],
): Promise<{
  device_id: string;
  prekeys_uploaded: number;
  prekeys_skipped: number;
}> {
  return httpRequest<{
    device_id: string;
    prekeys_uploaded: number;
    prekeys_skipped: number;
  }>("POST", "/api/keys/upload", {
    device_name: deviceName,
    identity_key_ed25519: identityKeyEd25519,
    identity_key_curve25519: identityKeyCurve25519,
    one_time_prekeys: oneTimePrekeys.map((pk) => ({
      key_id: pk.key_id,
      public_key: pk.public_key,
    })),
  });
}

// ============================================================================
// Reaction Commands
// ============================================================================

/**
 * Add a reaction to a message.
 */
export async function addReaction(
  channelId: string,
  messageId: string,
  emoji: string,
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("add_reaction", { channelId, messageId, emoji });
  }

  await httpRequest<void>(
    "PUT",
    `/api/channels/${channelId}/messages/${messageId}/reactions`,
    { emoji },
  );
}

/**
 * Remove a reaction from a message.
 */
export async function removeReaction(
  channelId: string,
  messageId: string,
  emoji: string,
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("remove_reaction", { channelId, messageId, emoji });
  }

  await httpRequest<void>(
    "DELETE",
    `/api/channels/${channelId}/messages/${messageId}/reactions/${encodeURIComponent(emoji)}`,
  );
}

/**
 * Delete a message (soft delete, own messages only).
 */
export async function deleteMessage(messageId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("delete_message", { messageId });
  }

  await httpRequest<void>("DELETE", `/api/messages/${messageId}`);
}

/**
 * Get aggregate unread counts across all guilds and DMs.
 * Returns unread counts grouped by guild, plus DM unreads.
 */
export async function getUnreadAggregate(): Promise<UnreadAggregate> {
  return fetchApi<UnreadAggregate>("/api/me/unread");
}

/**
 * Mark all text channels in a guild as read.
 */
export async function markAllGuildChannelsRead(guildId: string): Promise<void> {
  await fetchApi<void>(`/api/guilds/${guildId}/read-all`, { method: "POST" });
}

/**
 * Mark all DM channels as read.
 */
export async function markAllDMsRead(): Promise<void> {
  await fetchApi<void>("/api/dm/read-all", { method: "POST" });
}

/**
 * Mark everything (guilds + DMs) as read.
 */
export async function markAllRead(): Promise<void> {
  await fetchApi<void>("/api/me/read-all", { method: "POST" });
}

// ============================================================================
// OIDC / SSO
// ============================================================================

/**
 * Fetch server settings (public, no auth required).
 * Used pre-login to determine available auth methods and OIDC providers.
 */
export async function fetchServerSettings(
  serverUrl: string,
): Promise<ServerSettings> {
  const baseUrl = serverUrl.replace(/\/+$/, "");
  const resp = await fetch(`${baseUrl}/api/settings`);
  if (!resp.ok) {
    throw new Error(`Failed to fetch server settings: ${resp.status}`);
  }
  return resp.json();
}

/**
 * Initiate OIDC login flow.
 *
 * In Tauri mode: handles the entire flow (opens browser, waits for callback,
 * returns tokens). Returns { mode: "tauri", tokens }.
 *
 * In browser mode: returns the authorize URL for popup flow.
 * Returns { mode: "browser", authUrl }.
 */
export async function oidcAuthorize(
  serverUrl: string,
  providerSlug: string,
): Promise<
  | { mode: "tauri"; tokens: OidcLoginResult }
  | { mode: "browser"; authUrl: string }
> {
  const baseUrl = serverUrl.replace(/\/+$/, "");

  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    const tokens = await invoke<OidcLoginResult>("oidc_authorize", {
      serverUrl: baseUrl,
      providerSlug,
    });
    return { mode: "tauri", tokens };
  }

  // Browser: return the authorize endpoint URL for popup flow
  const authUrl = `${baseUrl}/auth/oidc/authorize/${encodeURIComponent(providerSlug)}`;
  return { mode: "browser", authUrl };
}

/**
 * Complete OIDC login after callback.
 * In browser mode, tokens are delivered via postMessage from the callback page.
 */
export async function oidcCompleteLogin(
  serverUrl: string,
  accessToken: string,
  refreshToken: string,
  expiresIn: number,
): Promise<void> {
  const baseUrl = serverUrl.replace(/\/+$/, "");

  // Store tokens (browser mode)
  browserState.serverUrl = baseUrl;
  browserState.accessToken = accessToken;
  browserState.refreshToken = refreshToken;
  browserState.tokenExpiresAt = Date.now() + expiresIn * 1000;

  localStorage.setItem("serverUrl", baseUrl);
  localStorage.setItem("accessToken", accessToken);
  localStorage.setItem("refreshToken", refreshToken);
  localStorage.setItem("tokenExpiresAt", String(browserState.tokenExpiresAt));

  scheduleTokenRefresh();
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
