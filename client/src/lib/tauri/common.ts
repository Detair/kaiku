/**
 * Shared helpers for the Tauri command wrapper layer.
 *
 * Contains browser state (server URL, access/refresh tokens), HTTP request
 * helpers, upload authentication, file-size validation, and the Tauri/browser
 * mode detection flag. Other domain modules in this directory import from
 * here rather than duplicating state.
 */

import type { User } from "../types";

// ============================================================================
// Tauri detection
// ============================================================================

export const isTauri =
  typeof window !== "undefined" && "__TAURI__" in window;

// ============================================================================
// Session types
// ============================================================================

/** Result from get_current_user Tauri command. */
export interface SessionRestoreResult {
  user: User | null;
  error_reason: string | null;
}

/** Auth response type from server. */
export interface AuthResponse {
  access_token: string;
  refresh_token?: string;
  expires_in: number;
  token_type: string;
  setup_required: boolean;
}

/** Auth result type (returned by login/register). */
export interface AuthResult {
  user: User;
  setup_required: boolean;
}

// ============================================================================
// File upload limits
// ============================================================================

/** Upload limits response from server. */
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

export type UploadType = "avatar" | "emoji" | "attachment";

/**
 * Format bytes to human-readable size.
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
 * Get formatted upload size limit for UI display.
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
export function validateFileSize(
  file: File,
  type: UploadType,
): string | null {
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

// ============================================================================
// Browser auth state (when not in Tauri)
// ============================================================================

export const browserState = {
  serverUrl: typeof window !== "undefined" ? window.location.origin : "",
  accessToken: null as string | null,
  refreshToken: null as string | null,
  tokenExpiresAt: null as number | null,
  refreshTimer: null as ReturnType<typeof setTimeout> | null,
};

const SESSION_RESTORE_BLOCK_KEY = "kaiku:skip-session-restore";

export function setSessionRestoreBlocked(blocked: boolean) {
  if (typeof localStorage === "undefined") {
    return;
  }

  if (blocked) {
    localStorage.setItem(SESSION_RESTORE_BLOCK_KEY, "1");
  } else {
    localStorage.removeItem(SESSION_RESTORE_BLOCK_KEY);
  }
}

export function isSessionRestoreBlocked(): boolean {
  if (typeof localStorage === "undefined") {
    return false;
  }

  return localStorage.getItem(SESSION_RESTORE_BLOCK_KEY) === "1";
}

// Initialize server URL from localStorage (tokens are in-memory only)
if (typeof localStorage !== "undefined") {
  browserState.serverUrl =
    localStorage.getItem("serverUrl") || browserState.serverUrl;
}

/** Clear in-memory browser auth state. */
export function clearBrowserTokens() {
  if (browserState.refreshTimer) {
    clearTimeout(browserState.refreshTimer);
    browserState.refreshTimer = null;
  }
  browserState.accessToken = null;
  browserState.refreshToken = null;
  browserState.tokenExpiresAt = null;
}

/**
 * Schedule automatic token refresh before expiration.
 * Refreshes 1 minute before the token expires.
 */
export function scheduleTokenRefresh() {
  // Clear any existing timer
  if (browserState.refreshTimer) {
    clearTimeout(browserState.refreshTimer);
    browserState.refreshTimer = null;
  }

  if (!browserState.tokenExpiresAt) {
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
    const success = await refreshAccessToken();
    if (!success) {
      console.warn("[Auth] Scheduled token refresh failed — clearing session");
      clearBrowserTokens();
      window.dispatchEvent(new CustomEvent("kaiku:session-expired"));
    }
  }, refreshIn);
}

/**
 * Refresh the access token. Browser mode sends a credentialed request so the
 * server reads the HttpOnly cookie; Tauri mode sends the refresh token in the
 * request body.
 */
export async function refreshAccessToken(): Promise<boolean> {
  try {
    console.log("[Auth] Refreshing access token...");

    const baseUrl = browserState.serverUrl.replace(/\/+$/, "");

    // Tauri clients send refresh_token in body; browser sends nothing
    // (server reads the HttpOnly cookie automatically).
    const fetchOptions: RequestInit = {
      method: "POST",
      credentials: "include",
    };
    if (isTauri) {
      const refreshToken = browserState.refreshToken;
      if (!refreshToken) {
        clearBrowserTokens();
        return false;
      }

      fetchOptions.headers = { "Content-Type": "application/json" };
      fetchOptions.body = JSON.stringify({
        refresh_token: refreshToken,
      });
    }

    const response = await fetch(`${baseUrl}/auth/refresh`, fetchOptions);

    if (!response.ok) {
      console.error("[Auth] Token refresh failed:", response.status);
      clearBrowserTokens();
      return false;
    }

    let data: AuthResponse;
    try {
      data = await response.json();
    } catch (parseError) {
      console.error(
        "[Auth] Token refresh returned invalid JSON:",
        parseError,
      );
      clearBrowserTokens();
      return false;
    }

    if (!data.access_token) {
      console.error("[Auth] Token refresh returned empty access token");
      clearBrowserTokens();
      return false;
    }

    // Store access token in memory; browser mode relies on HttpOnly cookie
    // for the refresh token so we don't keep it in JS-accessible memory.
    browserState.accessToken = data.access_token;
    if (isTauri) {
      if (!data.refresh_token) {
        console.error("[Auth] Token refresh returned empty refresh token");
        clearBrowserTokens();
        return false;
      }
      browserState.refreshToken = data.refresh_token;
    }
    browserState.tokenExpiresAt = Date.now() + data.expires_in * 1000;

    console.log("[Auth] Token refreshed successfully");

    // Schedule the next refresh
    scheduleTokenRefresh();

    return true;
  } catch (error) {
    console.error("[Auth] Token refresh error:", error);
    return false;
  }
}

// On browser load, attempt to restore session from HttpOnly cookie.
// The cookie is sent automatically; the server returns a fresh access token.
if (!isTauri && !browserState.accessToken) {
  if (!isSessionRestoreBlocked()) {
    // refreshAccessToken never throws (returns false on failure), so no .catch() needed.
    void refreshAccessToken();
  }
}

// When the tab becomes visible again, check if the token needs refreshing.
// Browsers throttle/pause setTimeout in background tabs, so the scheduled
// refresh may not fire before the access token expires.
if (!isTauri && typeof document !== "undefined") {
  document.addEventListener("visibilitychange", async () => {
    if (document.hidden) return;
    if (!browserState.accessToken) return;

    const now = Date.now();
    const expiresAt = browserState.tokenExpiresAt;

    // Refresh if expired or within 60s of expiry
    if (expiresAt && expiresAt - now < 60000) {
      console.warn(
        "[Kaiku:Auth] Visibility refresh: token expired/near-expiry, refreshing...",
      );
      const success = await refreshAccessToken();
      if (success) {
        console.log("[Kaiku:Auth] Visibility refresh: success");
      }
      // If refresh fails, refreshAccessToken clears in-memory tokens.
      // The scheduled refresh timer (if it fires late after being throttled)
      // will dispatch kaiku:session-expired, which the auth store handles.
      // This visibility handler is a fallback for when the timer was
      // throttled in a background tab and hasn't fired yet.
    }
  });
}

// ============================================================================
// HTTP helpers
// ============================================================================

/** Error with HTTP status code for structured error detection. */
export class HttpError extends Error {
  constructor(
    public readonly status: number,
    message: string,
  ) {
    super(message);
    this.name = "HttpError";
  }
}

/** HTTP helper for browser mode. */
export async function httpRequest<T>(
  method: string,
  path: string,
  body?: unknown,
): Promise<T> {
  const token = browserState.accessToken;

  const headers: Record<string, string> = {};
  if (body !== undefined) {
    headers["Content-Type"] = "application/json";
  }

  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  // Tauri clients send the refresh token via header so the server can identify
  // the current session (it cannot read HttpOnly cookies like browsers do).
  if (isTauri && browserState.refreshToken) {
    headers["X-Refresh-Token"] = browserState.refreshToken;
  }

  // Remove trailing slash from serverUrl and ensure path starts with /
  const baseUrl = browserState.serverUrl.replace(/\/+$/, "");
  const cleanPath = path.startsWith("/") ? path : `/${path}`;

  const isAuthEndpoint =
    cleanPath === "/auth/login" ||
    cleanPath === "/auth/register" ||
    cleanPath === "/auth/refresh";

  if (token && !isAuthEndpoint) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  const logHeaders = { ...headers };
  if (logHeaders.Authorization) {
    logHeaders.Authorization = "Bearer [REDACTED]";
  }
  if (logHeaders["X-Refresh-Token"]) {
    logHeaders["X-Refresh-Token"] = "[REDACTED]";
  }

  console.log(`[httpRequest] ${method} ${path}`, {
    hasToken: !!token,
    hasAuthHeader: !!headers["Authorization"],
    headers: JSON.stringify(logHeaders),
  });

  const response = await fetch(`${baseUrl}${cleanPath}`, {
    method,
    headers,
    body: body ? JSON.stringify(body) : undefined,
    credentials: "include",
  });

  if (!response.ok) {
    let errorMessage = `HTTP ${response.status}: ${response.statusText}`;

    try {
      const errorBody = await response.json();
      // Detect MFA_REQUIRED from server — throw before falling into catch
      if (response.status === 403 && errorBody.error === "MFA_REQUIRED") {
        throw new HttpError(403, "MFA_REQUIRED");
      }
      errorMessage = errorBody.message || errorBody.error || errorMessage;
    } catch (parseError) {
      // Re-throw HttpError (including MFA_REQUIRED) without wrapping
      if (parseError instanceof HttpError) {
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

    throw new HttpError(response.status, errorMessage);
  }

  // Handle empty responses
  const text = await response.text();
  if (!text) return null as T;

  try {
    return JSON.parse(text);
  } catch (parseError) {
    console.error(
      `[httpRequest] Failed to parse success response as JSON for ${cleanPath}:`,
      text.slice(0, 200),
    );
    throw new HttpError(
      response.status,
      `Invalid JSON response from ${cleanPath}`,
    );
  }
}

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

/**
 * Get auth credentials for fetch-based uploads.
 * In Tauri mode, retrieves from Rust backend state.
 * In browser mode, reads from browserState/localStorage.
 */
export async function getUploadAuth(): Promise<{
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
    token: browserState.accessToken,
    baseUrl: (browserState.serverUrl || window.location.origin).replace(
      /\/+$/,
      "",
    ),
  };
}

// ============================================================================
// Server URL / token accessors
// ============================================================================

export function getServerUrl(): string {
  return browserState.serverUrl;
}

/**
 * Get the current access token (for use in URLs that can't use headers).
 */
export function getAccessToken(): string | null {
  return browserState.accessToken;
}
