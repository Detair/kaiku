/**
 * Authentication commands: login, register, logout, session restore,
 * password updates, session management, MFA, and QR login.
 */

import type {
  IdentityListResponse,
  ReauthMethodsResponse,
  RevokeAllResponse,
  ServerSettings,
  SessionListResponse,
  User,
  UserStatus,
} from "../types";
import { codeIs, messageHasCode } from "@/lib/errorCode";
import { decodeLoginStep, type LoginStep, type RegistrationStarted } from "../types/login";
import {
  browserState,
  clearBrowserTokens,
  expireNativeSession,
  fetchApi,
  getServerUrl,
  getUploadAuth,
  HttpError,
  httpRequest,
  isTauri,
  loadNativeAuthInfo,
  NATIVE_SESSION_EXPIRED,
  refreshAccessToken,
  setNativeAuthInfo,
  setSessionRestoreBlocked,
  validateFileSize,
  type SessionRestoreResult,
} from "./common";
import { getBrowserWebSocket, wsSend } from "./websocket";

type InvokeFn = typeof import("@tauri-apps/api/core").invoke;

/**
 * Native mode: call a Rust auth command and cache the session's auth info.
 * The caller names the command with a literal so `lint:tauri-commands` sees it.
 */
async function invokeNativeAuth<T>(
  call: (invoke: InvokeFn) => Promise<T>,
): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  const result = await call(invoke);
  await loadNativeAuthInfo();
  return result;
}

/**
 * Native mode: Rust reports sign-in failures as plain strings
 * (`SessionError` Display). Give the ones the UI maps a code, so
 * `errorText()` treats them like the browser-mode `HttpError`s.
 */
export function nativeAuthError(error: unknown): unknown {
  const text = typeof error === "string" ? error : null;
  if (text === "Invalid username or password") {
    return new HttpError(401, text, "INVALID_CREDENTIALS");
  }
  if (text?.startsWith("Connection failed:")) {
    return new TypeError(text);
  }
  // Bare server codes (`SessionError::Server` displays the code), optionally
  // with `:<seconds>` from a Retry-After and `:{json}` with the extra error
  // fields (`ENROLL_CODE_WRONG::{"tries_left":2}`).
  const bare = text ? /^([A-Z][A-Z0-9_]*)(?::(\d*)(?::(\{.*\}))?)?$/s.exec(text) : null;
  if (bare) {
    const code = bare[1];
    let details: Record<string, unknown> = {};
    if (bare[3]) {
      try {
        const parsed: unknown = JSON.parse(bare[3]);
        if (parsed && typeof parsed === "object") details = parsed as Record<string, unknown>;
      } catch {
        details = {};
      }
    }
    return new HttpError(
      NATIVE_CODE_STATUS[code] ?? 400,
      code,
      code,
      details,
      bare[2] ? Number(bare[2]) : undefined,
    );
  }
  // A proxy or server failure without a JSON body (`SessionError::Server`).
  const status = text ? /^Server error \((\d{3})\)$/.exec(text) : null;
  if (status) return new HttpError(Number(status[1]), text!);
  return error;
}

/** Status for bare native codes whose fallback text depends on it. */
const NATIVE_CODE_STATUS: Partial<Record<string, number>> = {
  INVALID_CREDENTIALS: 401,
  INVALID_MFA: 401,
  ACCOUNT_BANNED: 403,
  USER_EXISTS: 409,
  EMAIL_TAKEN: 409,
  LOGIN_ATTEMPT_UNKNOWN: 404,
  LOGIN_ATTEMPT_ENDED: 410,
  TOO_MANY_ATTEMPTS: 429,
  SERVICE_UNAVAILABLE: 503,
};

/** Server codes the native SSO commands report as bare strings. */
const NATIVE_SSO_CODES = new Set([
  "OIDC_CLIENT_OUTDATED",
  "OIDC_HANDOFF_INVALID",
  "OIDC_PROVIDER_ERROR",
  "OIDC_EXCHANGE_FAILED",
  "IDENTITY_ALREADY_LINKED",
]);

/**
 * Native mode: the SSO commands (`oidc_authorize`, `oidc_link_identity`)
 * fail with a bare server code; give it an `HttpError` code for `errorText()`
 * with `SSO_ERRORS`. Anything else goes through `nativeAuthError`.
 */
export function nativeSsoError(error: unknown): unknown {
  if (typeof error === "string" && NATIVE_SSO_CODES.has(error)) {
    return new HttpError(400, error, error);
  }
  return nativeAuthError(error);
}

/** Native mode: session-management request sent by Rust with the refresh token. */
export async function nativeSessionRequest<T>(
  method: string,
  path: string,
  body?: unknown,
): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  try {
    return await invoke<T>("session_request", {
      method,
      path,
      body: body ?? null,
    });
  } catch (error) {
    // Rust ended the session (refresh token rejected during the request).
    if (String(error) === NATIVE_SESSION_EXPIRED) {
      expireNativeSession();
      throw error;
    }
    // /auth/me/password: Rust passes on the server's INVALID_CREDENTIALS text.
    if (error === "Invalid credentials") {
      throw new HttpError(401, error, "INVALID_CREDENTIALS");
    }
    // Bare codes with their fields (STEP_UP_REQUIRED, PASSWORD_REJECTED {reason}, …).
    throw nativeAuthError(error);
  }
}

/**
 * Update the user's presence status (online, idle, dnd, offline). The removed "invisible" status is never sent; it falls back to online.
 */
export async function updateStatus(
  status: UserStatus,
): Promise<void> {
  // Map client status names to server enum values. The server accepts only
  // online, away and busy; anything else (offline, a stale invisible) is online.
  const statusMap: Record<string, string> = {
    online: "online",
    idle: "away",
    dnd: "busy",
  };
  const message = { type: "set_status", status: statusMap[status] ?? "online" };
  if (isTauri()) {
    await wsSend(message);
    return;
  }
  getBrowserWebSocket()?.send(JSON.stringify(message));
}

/**
 * Create an account (F8). Never a session: the answer names the account and
 * when its registration ticket ends (30 min). Browser: the ticket is the
 * `__Secure-kaiku_ticket` cookie; desktop: Rust keeps it.
 */
export async function register(
  serverUrl: string,
  username: string,
  password: string,
  email?: string,
  displayName?: string,
): Promise<RegistrationStarted> {
  if (isTauri()) {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      return await invoke<RegistrationStarted>("register", {
        request: {
          server_url: serverUrl,
          username,
          email,
          password,
          display_name: displayName,
        },
      });
    } catch (error) {
      throw nativeAuthError(error);
    }
  }

  browserState.serverUrl = serverUrl;
  localStorage.setItem("serverUrl", serverUrl);
  setSessionRestoreBlocked(false);

  return httpRequest<RegistrationStarted>("POST", "/auth/register", {
    username,
    password,
    email,
    display_name: displayName,
  });
}

export async function logout(): Promise<void> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    try {
      await invoke("logout");
    } finally {
      setNativeAuthInfo(null);
    }
    return;
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

/** `GET /auth/me` body; carries the C4 forced-change flag. */
type CurrentUserResponse = User & { password_change_required?: boolean };

/**
 * Ask the server whether the signed-in account still has to replace an
 * admin-issued temporary password (`GET /auth/me` stays allowed while it is
 * pending). Rejects when the request fails.
 */
export async function isPasswordChangePending(): Promise<boolean> {
  const me = await httpRequest<CurrentUserResponse>("GET", "/auth/me");
  return me?.password_change_required === true;
}

export async function getCurrentUser(): Promise<SessionRestoreResult> {
  if (isTauri()) {
    return invokeNativeAuth((invoke) =>
      invoke<SessionRestoreResult>("get_current_user"),
    );
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
    const user = await httpRequest<CurrentUserResponse>("GET", "/auth/me");
    return {
      user,
      error_reason: null,
      password_change_required: user.password_change_required ?? false,
    };
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
          const user = await httpRequest<CurrentUserResponse>("GET", "/auth/me");
          return {
            user,
            error_reason: null,
            password_change_required: user.password_change_required ?? false,
          };
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
  if (isTauri()) {
    return nativeSessionRequest("POST", "/auth/me/password", {
      current_password,
      new_password,
      revoke_others,
    });
  }
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
  if (isTauri()) {
    return nativeSessionRequest<SessionListResponse>("GET", "/auth/sessions");
  }
  return fetchApi<SessionListResponse>("/auth/sessions");
}

/**
 * Revoke a specific session by ID.
 */
export async function revokeSession(sessionId: string): Promise<void> {
  if (isTauri()) {
    await nativeSessionRequest<unknown>("DELETE", `/auth/sessions/${sessionId}`);
    return;
  }
  await fetchApi<void>(`/auth/sessions/${sessionId}`, { method: "DELETE" });
}

/**
 * Revoke all sessions except the current one.
 */
export async function revokeAllOtherSessions(): Promise<RevokeAllResponse> {
  if (isTauri()) {
    return nativeSessionRequest<RevokeAllResponse>("DELETE", "/auth/sessions");
  }
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

/**
 * Start linking an additional external identity to the current account.
 *
 * Tauri: the native command runs the full flow (opens the system browser, waits
 * on a localhost callback) and resolves once the identity is linked.
 *
 * Browser: a top-level popup can't send the Bearer header the link-authorize
 * endpoint requires, so we fetch the provider URL as an authenticated XHR and
 * return it for the caller to open in a popup; the result then arrives via an
 * `oidc-link-callback` postMessage from the server callback page.
 */
export async function linkIdentity(
  providerSlug: string,
): Promise<{ mode: "tauri" } | { mode: "browser"; authUrl: string }> {
  const baseUrl = getServerUrl().replace(/\/+$/, "");
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    try {
      await invoke("oidc_link_identity", { serverUrl: baseUrl, providerSlug });
    } catch (error) {
      throw nativeSsoError(error);
    }
    return { mode: "tauri" };
  }
  const resp = await fetchApi<{ url: string }>(
    `/auth/me/identities/authorize/${encodeURIComponent(providerSlug)}?response=json`,
  );
  return { mode: "browser", authUrl: resp.url };
}

/**
 * True when linking failed because a system admin needs an elevated session
 * first (server code `ELEVATION_REQUIRED`, any casing, HTTP 403). The desktop command
 * reports the response body inside its error string.
 */
export function isElevationRequiredError(err: unknown): boolean {
  if (err instanceof HttpError) return codeIs(err.code, "ELEVATION_REQUIRED");
  const text = typeof err === "string" ? err : err instanceof Error ? err.message : "";
  return messageHasCode(text, "ELEVATION_REQUIRED");
}

// ============================================================================
// Re-authentication (contract C24)
// ============================================================================

/** Which re-authentication methods this session can use. */
export async function getReauthMethods(): Promise<ReauthMethodsResponse> {
  return httpRequest<ReauthMethodsResponse>("GET", "/auth/reauth/methods");
}

/**
 * Start re-authenticating with a sign-in provider (C24, browser only).
 *
 * C27 removes this route (404) in favour of step-up grants; LP's rebase
 * deletes the whole path (Task A5). The desktop commands `oidc_reauth` /
 * `oidc_reauth_cancel` are already gone, so the app refuses at once.
 * Browser: the server returns the provider URL for the caller to open in a
 * popup; the result arrives as an `oidc-reauth-callback` message.
 */
export async function startOidcReauth(
  providerId: string,
  opts: { reason?: string | null; mfaCode?: string | null; signal?: AbortSignal },
): Promise<{ mode: "browser"; url: string }> {
  if (opts.signal?.aborted) {
    const e = new Error("aborted");
    e.name = "AbortError";
    throw e;
  }
  if (isTauri()) {
    throw new HttpError(404, "REAUTH_UNAVAILABLE", "REAUTH_UNAVAILABLE");
  }
  const resp = await httpRequest<{ url: string }>(
    "POST",
    `/auth/reauth/oidc/authorize/${encodeURIComponent(providerId)}`,
    {
      redirect_uri: null,
      reason: opts.reason ?? null,
      mfa_code: opts.mfaCode ?? null,
    },
  );
  return { mode: "browser", url: resp.url };
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

/** Setup MFA — returns TOTP secret and QR code URL. */
export async function mfaSetup(): Promise<MfaSetupResponse> {
  return httpRequest<MfaSetupResponse>("POST", "/auth/mfa/setup");
}

/** Verify MFA code (TOTP or backup). Returns backup codes on first verify. */
export async function mfaVerify(code: string): Promise<MfaVerifyResponse> {
  return httpRequest<MfaVerifyResponse>("POST", "/auth/mfa/verify", { code });
}

/** Disable MFA (requires valid TOTP or backup code). */
export async function mfaDisable(code: string): Promise<void> {
  await httpRequest<unknown>("POST", "/auth/mfa/disable", { code });
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
    throw new Error("Server returned invalid response", { cause: parseError });
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
 * Initiate an SSO sign-in (F3 OIDC variant). Never a session by itself: the
 * answer is a `LoginStep`, confirmed by the phone or the TOTP step (C27).
 *
 * Tauri: Rust runs the browser flow and redeems the hand-off code; the
 * verifier stays in Rust. Returns { mode: "tauri", step }.
 *
 * Browser: returns the authorize URL for the popup, whose callback page posts
 * `oidc-callback {status, attempt}`. Returns { mode: "browser", authUrl }.
 */
export async function oidcAuthorize(
  serverUrl: string,
  providerSlug: string,
): Promise<
  | { mode: "tauri"; step: LoginStep }
  | { mode: "browser"; authUrl: string }
> {
  const baseUrl = serverUrl.replace(/\/+$/, "");

  if (isTauri()) {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const raw = await invoke<unknown>("oidc_authorize", {
        serverUrl: baseUrl,
        providerSlug,
      });
      return { mode: "tauri", step: decodeLoginStep(raw) };
    } catch (error) {
      throw nativeSsoError(error);
    }
  }

  // Browser: return the authorize endpoint URL for popup flow
  const authUrl = `${baseUrl}/auth/oidc/authorize/${encodeURIComponent(providerSlug)}`;
  return { mode: "browser", authUrl };
}
