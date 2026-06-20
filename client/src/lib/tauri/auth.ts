/**
 * Authentication commands: login, register, logout, session restore,
 * password updates, session management, MFA, and QR login.
 */

import type {
  IdentityListResponse,
  OidcLoginResult,
  RevokeAllResponse,
  ServerSettings,
  SessionListResponse,
  User,
} from "../types";
import {
  browserState,
  clearBrowserTokens,
  fetchApi,
  getUploadAuth,
  HttpError,
  httpRequest,
  isTauri,
  refreshAccessToken,
  scheduleTokenRefresh,
  setSessionRestoreBlocked,
  validateFileSize,
  type AuthResponse,
  type AuthResult,
  type SessionRestoreResult,
} from "./common";
import { getBrowserWebSocket } from "./websocket";

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
  setSessionRestoreBlocked(false);

  const body: Record<string, unknown> = { username, password };
  if (mfaCode) {
    body.mfa_code = mfaCode;
  }

  const response = await httpRequest<AuthResponse>("POST", "/auth/login", body);

  // Store access token in memory; browser relies on HttpOnly cookie for refresh
  browserState.accessToken = response.access_token;
  if (isTauri) {
    if (!response.refresh_token) {
      throw new Error("Login response missing refresh token");
    }
    browserState.refreshToken = response.refresh_token;
  }
  browserState.tokenExpiresAt = Date.now() + response.expires_in * 1000;

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
  getBrowserWebSocket()?.send(
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
  setSessionRestoreBlocked(false);

  const response = await httpRequest<AuthResponse>("POST", "/auth/register", {
    username,
    password,
    email,
    display_name: displayName,
  });

  // Store access token in memory; browser relies on HttpOnly cookie for refresh
  browserState.accessToken = response.access_token;
  if (isTauri) {
    if (!response.refresh_token) {
      throw new Error("Registration response missing refresh token");
    }
    browserState.refreshToken = response.refresh_token;
  }
  browserState.tokenExpiresAt = Date.now() + response.expires_in * 1000;

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

  let logoutError: unknown = null;

  try {
    await httpRequest("POST", "/auth/logout");
  } catch (error) {
    const isAuthError =
      error instanceof HttpError &&
      (error.status === 401 || error.status === 403);

    if (isAuthError) {
      const refreshed = await refreshAccessToken();
      if (refreshed) {
        try {
          await httpRequest("POST", "/auth/logout");
        } catch (retryError) {
          logoutError = retryError;
        }
      } else {
        logoutError = error;
      }
    } else {
      logoutError = error;
    }
  }

  clearBrowserTokens();
  setSessionRestoreBlocked(true);

  if (logoutError) {
    const message =
      logoutError instanceof Error
        ? logoutError.message
        : String(logoutError ?? "unknown error");
    throw new Error(`Server logout failed: ${message}`);
  }
}

export async function getCurrentUser(): Promise<SessionRestoreResult> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<SessionRestoreResult>("get_current_user");
  }

  // Browser mode - check if we have a token
  if (!browserState.accessToken) {
    // Try to refresh via HttpOnly cookie
    const refreshed = await refreshAccessToken();
    if (!refreshed) {
      return { user: null, error_reason: null };
    }
  }

  try {
    const user = await httpRequest<User>("GET", "/auth/me");
    return { user, error_reason: null };
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
      clearBrowserTokens();
      return { user: null, error_reason: null };
    }

    if (isAuthError) {
      console.log("[Auth] Token appears invalid, attempting refresh...");
      const refreshed = await refreshAccessToken();
      if (refreshed) {
        try {
          const user = await httpRequest<User>("GET", "/auth/me");
          return { user, error_reason: null };
        } catch (retryError) {
          console.error("[Auth] Retry after refresh failed:", retryError);
          // Refresh didn't help, clear everything below
        }
      }
    }

    // Only clear tokens if we confirmed auth failure, not on network errors
    if (isAuthError) {
      console.warn("[Auth] Authentication failed, clearing session");
      clearBrowserTokens();
    } else {
      console.warn("[Auth] Non-auth error, keeping tokens for retry");
    }

    return { user: null, error_reason: null };
  }
}

/** Update the current user's password. */
export async function updatePassword(
  current_password: string,
  new_password: string,
  revoke_others: boolean = false,
): Promise<{ success: boolean; message: string; revoked_count: number }> {
  return fetchApi("/auth/me/password", {
    method: "POST",
    body: { current_password, new_password, revoke_others },
  });
}

// ============================================================================
// Session Management Commands
// ============================================================================

/**
 * List all active sessions for the current user.
 */
export async function listSessions(): Promise<SessionListResponse> {
  return fetchApi<SessionListResponse>("/auth/sessions");
}

/**
 * Revoke a specific session by ID.
 */
export async function revokeSession(sessionId: string): Promise<void> {
  await fetchApi<void>(`/auth/sessions/${sessionId}`, { method: "DELETE" });
}

/**
 * Revoke all sessions except the current one.
 */
export async function revokeAllOtherSessions(): Promise<RevokeAllResponse> {
  return fetchApi<RevokeAllResponse>("/auth/sessions", { method: "DELETE" });
}

// ============================================================================
// Linked external (OIDC) identities
// ============================================================================

/**
 * List the external identities linked to the current account.
 */
export async function listIdentities(): Promise<IdentityListResponse> {
  return fetchApi<IdentityListResponse>("/auth/me/identities");
}

/**
 * Unlink an external identity by ID.
 *
 * The server returns 409 when this would remove the only login method of a
 * passwordless account; the caller surfaces that message to the user.
 */
export async function unlinkIdentity(identityId: string): Promise<void> {
  await fetchApi<void>(`/auth/me/identities/${identityId}`, {
    method: "DELETE",
  });
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

// ============================================================================
// QR Login Commands
// ============================================================================

export interface QrLoginCreateResponse {
  token: string;
  expires_in: number;
}

/** Create a QR login token for mobile scanning. */
export async function qrLoginCreate(): Promise<QrLoginCreateResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("qr_login_create");
  }
  return httpRequest<QrLoginCreateResponse>("POST", "/auth/qr/create");
}

// ============================================================================
// Avatar upload
// ============================================================================

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
  refreshToken: string | undefined,
  expiresIn: number,
): Promise<void> {
  const baseUrl = serverUrl.replace(/\/+$/, "");

  // Store tokens; browser relies on HttpOnly cookie for refresh
  browserState.serverUrl = baseUrl;
  browserState.accessToken = accessToken;
  if (isTauri) {
    if (!refreshToken) {
      throw new Error("OIDC login missing refresh token");
    }
    browserState.refreshToken = refreshToken;
  }
  browserState.tokenExpiresAt = Date.now() + expiresIn * 1000;

  localStorage.setItem("serverUrl", baseUrl);
  setSessionRestoreBlocked(false);

  scheduleTokenRefresh();
}
