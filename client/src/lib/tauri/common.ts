/**
 * Shared helpers for the Tauri command wrapper layer.
 *
 * Contains browser state (server URL, access token), the native session
 * cache (native mode: tokens are owned by Rust, the webview only sees the
 * access token), HTTP request helpers, upload authentication, file-size
 * validation, and the Tauri/browser mode detection flag. Other domain modules in this directory import from
 * here rather than duplicating state.
 */

import { isTauri as coreIsTauri } from "@tauri-apps/api/core";
import { codeIs } from "../errorCode";
import type { User } from "../types";

// ============================================================================
// Tauri detection
// ============================================================================

/**
 * True when the webview runs inside the Tauri desktop shell (contract C6).
 *
 * Uses the Tauri core's own flag, which is set regardless of
 * `withGlobalTauri`. Always call it — an uncalled function is truthy
 * (`bun run lint:tauri-commands` enforces this).
 */
export function isTauri(): boolean {
  return coreIsTauri();
}

// ============================================================================
// Session types
// ============================================================================

/** Result from get_current_user Tauri command. */
export interface SessionRestoreResult {
  user: User | null;
  error_reason: string | null;
  password_change_required?: boolean;
}

/** Auth response type from server (browser mode). */
export interface AuthResponse {
  access_token: string;
  expires_in: number;
  token_type: string;
  setup_required: boolean;
  password_change_required?: boolean;
}

/** Auth result type (returned by login/register). */
export interface AuthResult {
  user: User;
  setup_required: boolean;
  password_change_required: boolean;
  /** Native mode: `false` when the OS keyring could not store the login. */
  persisted?: boolean;
}

/** Native mode: base URL and access token provided by the Rust session. */
export interface NativeAuthInfo {
  server_url: string;
  access_token: string;
  expires_at_ms: number;
}

/** Base URL and access token for a request, in either mode. */
export interface AuthContext {
  baseUrl: string;
  accessToken: string | null;
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
export function formatFileSize(bytes: number): string {
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

/** Window event: the session ended for good (no silent refresh retry). */
export const SESSION_ENDED_EVENT = "kaiku:session-ended";

/** Why the session ended; selects the message shown on the sign-in prompt. */
export type SessionEndReason = "expired" | "revoked" | "banned";

/** Browser: drop the local session and tell the app to show the sign-in prompt. */
export function endBrowserSession(reason: SessionEndReason): void {
  clearBrowserTokens();
  window.dispatchEvent(
    new CustomEvent(SESSION_ENDED_EVENT, { detail: { reason } }),
  );
}

/** Clear in-memory browser auth state. */
export function clearBrowserTokens() {
  if (browserState.refreshTimer) {
    clearTimeout(browserState.refreshTimer);
    browserState.refreshTimer = null;
  }
  browserState.accessToken = null;
  browserState.tokenExpiresAt = null;
}

// ============================================================================
// Native session cache (Tauri)
// ============================================================================

let nativeAuthInfo: NativeAuthInfo | null = null;

/**
 * Whether `kaiku:session-expired` was already dispatched for the current
 * native session. Kept apart from the cache: the cache can be empty while a
 * session is live (startup before the first `get_auth_info`, a failed
 * `loadNativeAuthInfo`), and an expiry reported then must still be shown.
 * Reset when a session hands out a token again.
 */
let nativeSessionExpiryReported = false;

/** Cache (or clear) the auth info handed out by the Rust session. */
export function setNativeAuthInfo(info: NativeAuthInfo | null): void {
  nativeAuthInfo = info;
  if (info) {
    browserState.serverUrl = info.server_url;
    nativeSessionExpiryReported = false;
  }
}

/** Fetch the current auth info from Rust and cache it. */
export async function loadNativeAuthInfo(): Promise<NativeAuthInfo | null> {
  const { invoke } = await import("@tauri-apps/api/core");
  const info = await invoke<NativeAuthInfo | null>("get_auth_info");
  setNativeAuthInfo(info);
  return info;
}

/** Base URL and access token for the next request. */
export async function getAuthContext(): Promise<AuthContext> {
  if (!isTauri()) {
    return {
      baseUrl: getServerUrl().replace(/\/+$/, ""),
      accessToken: browserState.accessToken,
    };
  }
  const info = nativeAuthInfo ?? (await loadNativeAuthInfo());
  return {
    baseUrl: (info?.server_url ?? getServerUrl()).replace(/\/+$/, ""),
    accessToken: info?.access_token ?? null,
  };
}

/**
 * Native mode: the Rust session has ended (refresh token rejected). Clears
 * the cache and dispatches `kaiku:session-expired` once, however many paths
 * report it (the `auth:session_expired` event, `refresh_session` returning
 * `null`, a `session_request` failing with "Session expired").
 */
export function expireNativeSession(): void {
  if (nativeSessionExpiryReported) return;
  nativeSessionExpiryReported = true;
  setNativeAuthInfo(null);
  window.dispatchEvent(new CustomEvent("kaiku:session-expired"));
}

/** `Display` of the Rust `SessionError::Expired`. */
export const NATIVE_SESSION_EXPIRED = "Session expired";

/** Set once both native auth listeners are registered. */
let nativeAuthEventsInitialized = false;
let nativeAuthEventsInit: Promise<void> | null = null;

/**
 * Follow the Rust refresh scheduler: new access tokens update the cache, an
 * ended session becomes `kaiku:session-expired`. Idempotent; concurrent
 * callers share one setup, and a failed setup is retried by the next call
 * instead of leaving the events unwired.
 */
export function initNativeAuthEvents(): Promise<void> {
  if (!isTauri() || nativeAuthEventsInitialized) return Promise.resolve();
  nativeAuthEventsInit ??= registerNativeAuthEvents().finally(() => {
    nativeAuthEventsInit = null;
  });
  return nativeAuthEventsInit;
}

async function registerNativeAuthEvents(): Promise<void> {
  const { listen } = await import("@tauri-apps/api/event");
  const unlistenRefreshed = await listen<NativeAuthInfo>(
    "auth:token_refreshed",
    (event) => {
      setNativeAuthInfo(event.payload);
    },
  );
  try {
    await listen("auth:session_expired", () => {
      expireNativeSession();
    });
  } catch (err) {
    unlistenRefreshed();
    throw err;
  }
  nativeAuthEventsInitialized = true;
}

/** Wait after a transient refresh failure (offline, 5xx) before retrying. */
const REFRESH_RETRY_MS = 30_000;

/**
 * Schedule automatic token refresh before expiration (browser mode).
 * Refreshes 1 minute before the token expires. Only a rejected refresh
 * (401/403, tokens cleared) ends the session; transient failures keep the
 * tokens and retry after 30 s.
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

  browserState.refreshTimer = setTimeout(() => {
    void runScheduledRefresh();
  }, refreshIn);
}

async function runScheduledRefresh(): Promise<void> {
  browserState.refreshTimer = null;
  if (await refreshAccessToken()) return;
  if (browserState.accessToken === null) {
    console.warn("[Auth] Scheduled token refresh rejected — session ended");
    window.dispatchEvent(new CustomEvent("kaiku:session-expired"));
    return;
  }
  console.warn("[Auth] Scheduled token refresh failed — retrying in 30s");
  if (browserState.refreshTimer) clearTimeout(browserState.refreshTimer);
  browserState.refreshTimer = setTimeout(() => {
    void runScheduledRefresh();
  }, REFRESH_RETRY_MS);
}

/**
 * Refresh the access token. Browser mode sends a credentialed request so the
 * server reads the HttpOnly cookie; native mode asks the Rust session
 * (`refresh_session`), which owns the refresh token.
 *
 * Single-flight: concurrent callers share one in-flight request. The server
 * rotates the refresh token on every use, so two parallel refreshes race —
 * the winner rotates the cookie and the loser's 401 used to clear the very
 * session the winner just established (observed as pairs of
 * "Token refresh failed: 401" and a bounce to /login on reload).
 */
let refreshInFlight: Promise<boolean> | null = null;

export function refreshAccessToken(): Promise<boolean> {
  if (refreshInFlight) {
    return refreshInFlight;
  }
  refreshInFlight = doRefreshAccessToken().finally(() => {
    refreshInFlight = null;
  });
  return refreshInFlight;
}

/** Whether a 403 refresh response carries the `ACCOUNT_BANNED` code. */
async function isAccountBanned(response: Response): Promise<boolean> {
  try {
    const body = (await response.json()) as { error?: unknown };
    return codeIs(body?.error, "ACCOUNT_BANNED");
  } catch {
    return false;
  }
}

/**
 * Native refresh. `null` from Rust means the session has ended; a rejected
 * invoke (offline, 5xx) is transient and keeps the cached token.
 */
async function doRefreshNative(): Promise<boolean> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const info = await invoke<NativeAuthInfo | null>("refresh_session");
    if (!info) {
      console.warn("[Auth] Session ended (refresh rejected)");
      expireNativeSession();
      return false;
    }
    setNativeAuthInfo(info);
    return true;
  } catch (error) {
    console.error("[Auth] Token refresh error:", error);
    return false;
  }
}

async function doRefreshAccessToken(): Promise<boolean> {
  if (isTauri()) return doRefreshNative();
  try {
    console.log("[Auth] Refreshing access token...");

    const baseUrl = browserState.serverUrl.replace(/\/+$/, "");

    // The server reads the HttpOnly refresh cookie.
    const response = await fetch(`${baseUrl}/auth/refresh`, {
      method: "POST",
      credentials: "include",
    });

    if (!response.ok) {
      console.error("[Auth] Token refresh failed:", response.status);
      // Only an explicit rejection ends the session. Other statuses (e.g. a
      // 502/503 from a proxy while the server restarts) are transient, like a
      // network error: keep the tokens so the caller can retry later.
      if (response.status === 403 && (await isAccountBanned(response))) {
        endBrowserSession("banned");
        return false;
      }
      if (response.status === 401 || response.status === 403) {
        clearBrowserTokens();
      }
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

/**
 * Refresh now, whatever the token's remaining lifetime. Resolves `false` only
 * when the session is definitively over; throws on a transient failure
 * (offline, 5xx) so the caller backs off instead of treating the stale token
 * as renewed.
 */
export async function forceRefreshAccessToken(): Promise<boolean> {
  if (await refreshAccessToken()) return true;
  const kept = isTauri() ? nativeAuthInfo !== null : browserState.accessToken !== null;
  if (!kept) return false;
  throw new Error("Token refresh failed; will retry");
}

/**
 * Make sure the access token is valid for at least `minValidityMs` more,
 * refreshing it if not. Used before a WebSocket reconnect (the socket
 * authenticates with the access token, which may have expired during sleep).
 *
 * Returns `false` only when the session is gone (the refresh was rejected
 * with 401/403 and the tokens were cleared). A refresh that failed for
 * transient reasons (offline, 5xx while the server restarts) returns `true`:
 * the reconnect attempt then fails and backs off, and the next attempt
 * refreshes again.
 */
export async function ensureFreshAccessToken(
  minValidityMs = 60_000,
): Promise<boolean> {
  if (isTauri()) {
    if (
      nativeAuthInfo &&
      nativeAuthInfo.expires_at_ms - Date.now() >= minValidityMs
    ) {
      return true;
    }
    if (await refreshAccessToken()) return true;
    return nativeAuthInfo !== null;
  }
  const expiresAt = browserState.tokenExpiresAt;
  if (
    browserState.accessToken &&
    expiresAt !== null &&
    expiresAt - Date.now() >= minValidityMs
  ) {
    return true;
  }
  if (await refreshAccessToken()) return true;
  return browserState.accessToken !== null;
}

// On browser load, attempt to restore session from HttpOnly cookie.
// The cookie is sent automatically; the server returns a fresh access token.
if (!isTauri() && !browserState.accessToken) {
  if (!isSessionRestoreBlocked()) {
    // refreshAccessToken never throws (returns false on failure), so no .catch() needed.
    void refreshAccessToken();
  }
}

// When the tab becomes visible again, check if the token needs refreshing.
// Browsers throttle/pause setTimeout in background tabs, so the scheduled
// refresh may not fire before the access token expires.
if (!isTauri() && typeof document !== "undefined") {
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
    /** Server error code from the body's `error` field (contract C16). */
    public readonly code?: string,
  ) {
    super(message);
    this.name = "HttpError";
  }
}

/** Server error code (contract C4) for a pending forced password change. */
export const PASSWORD_CHANGE_REQUIRED = "PASSWORD_CHANGE_REQUIRED";

/** Window event dispatched when a request reports `PASSWORD_CHANGE_REQUIRED`. */
export const PASSWORD_CHANGE_REQUIRED_EVENT = "kaiku:password-change-required";

const AUTH_ENDPOINTS = new Set(["/auth/login", "/auth/register", "/auth/refresh"]);

/**
 * HTTP helper for both modes. Base URL and bearer come from
 * `getAuthContext()`. A 401 from a non-auth endpoint triggers one refresh and
 * one retry.
 */
export async function httpRequest<T>(
  method: string,
  path: string,
  body?: unknown,
): Promise<T> {
  return (await sendHttpRequest<T>(method, path, body, false)).data;
}

/** Like `httpRequest`, but also returns the HTTP status of the success response. */
export async function httpRequestWithStatus<T>(
  method: string,
  path: string,
  body?: unknown,
): Promise<{ data: T; status: number }> {
  return sendHttpRequest<T>(method, path, body, false);
}

async function sendHttpRequest<T>(
  method: string,
  path: string,
  body: unknown,
  isRetry: boolean,
): Promise<{ data: T; status: number }> {
  const { baseUrl, accessToken: token } = await getAuthContext();

  const headers: Record<string, string> = {};
  if (body !== undefined) {
    headers["Content-Type"] = "application/json";
  }

  const cleanPath = path.startsWith("/") ? path : `/${path}`;
  const isAuthEndpoint = AUTH_ENDPOINTS.has(cleanPath);

  if (token && !isAuthEndpoint) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  console.log(`[httpRequest] ${method} ${path}`, {
    hasToken: !!token,
    hasAuthHeader: !!headers["Authorization"],
  });

  const init: RequestInit = {
    method,
    headers,
    body: body ? JSON.stringify(body) : undefined,
  };
  // Cookies (refresh cookie, CSRF) only exist in browser mode.
  if (!isTauri()) {
    init.credentials = "include";
  }

  const response = await fetch(`${baseUrl}${cleanPath}`, init);

  if (response.status === 401 && !isAuthEndpoint && !isRetry) {
    if (await refreshAccessToken()) {
      return sendHttpRequest<T>(method, path, body, true);
    }
  }

  if (!response.ok) {
    let errorMessage = `HTTP ${response.status}: ${response.statusText}`;

    // Read the body once as text: a failed json() would consume it.
    let rawBody = "";
    try {
      rawBody = await response.text();
    } catch (textError) {
      console.error(
        `[httpRequest] Failed to read error response body for ${path}:`,
        textError,
      );
    }

    let errorBody: { error?: string; message?: string } | null = null;
    try {
      errorBody = rawBody ? JSON.parse(rawBody) : null;
    } catch {
      if (rawBody.length > 0 && rawBody.length < 500) {
        errorMessage = rawBody;
      }
    }

    if (errorBody && typeof errorBody === "object") {
      // Detect MFA_REQUIRED from server
      if (response.status === 403 && codeIs(errorBody.error, "MFA_REQUIRED")) {
        throw new HttpError(403, "MFA_REQUIRED", "MFA_REQUIRED");
      }
      // Contract C4: the account still has to replace an admin-issued
      // temporary password. The auth store switches to the change screen.
      if (
        response.status === 403 &&
        codeIs(errorBody.error, PASSWORD_CHANGE_REQUIRED)
      ) {
        if (typeof window !== "undefined") {
          window.dispatchEvent(new CustomEvent(PASSWORD_CHANGE_REQUIRED_EVENT));
        }
        throw new HttpError(403, PASSWORD_CHANGE_REQUIRED, PASSWORD_CHANGE_REQUIRED);
      }
      errorMessage = errorBody.message || errorBody.error || errorMessage;
    }

    const code =
      errorBody && typeof errorBody === "object" && typeof errorBody.error === "string"
        ? errorBody.error
        : undefined;
    throw new HttpError(response.status, errorMessage, code);
  }

  // Handle empty responses
  const text = await response.text();
  if (!text) return { data: null as T, status: response.status };

  try {
    return { data: JSON.parse(text) as T, status: response.status };
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
 * Get auth credentials for fetch-based uploads (both modes).
 */
export async function getUploadAuth(): Promise<{
  token: string | null;
  baseUrl: string;
}> {
  const { baseUrl, accessToken } = await getAuthContext();
  if (isTauri() && !accessToken) {
    throw new Error("Not authenticated");
  }
  return { token: accessToken, baseUrl };
}

// ============================================================================
// Server URL / token accessors
// ============================================================================

/**
 * Server base URL. In browser mode this falls back to the page origin; in
 * native mode `window.location.origin` is `tauri://…` and must never be used.
 */
export function getServerUrl(): string {
  const stored = browserState.serverUrl;
  if (isTauri()) {
    return /^https?:\/\//i.test(stored) ? stored : "";
  }
  return stored || (typeof window !== "undefined" ? window.location.origin : "");
}

/**
 * Get the current access token (for use in URLs that can't use headers).
 */
export function getAccessToken(): string | null {
  if (isTauri()) return nativeAuthInfo?.access_token ?? null;
  return browserState.accessToken;
}
