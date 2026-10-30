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
  scheduleTokenRefresh,
  setNativeAuthInfo,
  setSessionRestoreBlocked,
  validateFileSize,
  type AuthResponse,
  type AuthResult,
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
  if (text === "ACCOUNT_BANNED") {
    return new HttpError(403, text, "ACCOUNT_BANNED");
  }
  if (text?.startsWith("Connection failed:")) {
    return new TypeError(text);
  }
  if (text === "Username or email already exists") {
    return new HttpError(409, text);
  }
  return error;
}

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
async function nativeSessionRequest<T>(
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
    throw error;
  }
}

export async function login(
  serverUrl: string,
  username: string,
  password: string,
  mfaCode?: string,
): Promise<AuthResult> {
  if (isTauri()) {
    try {
      return await invokeNativeAuth((invoke) =>
        invoke<AuthResult>("login", {
          request: {
            server_url: serverUrl,
            username,
            password,
            mfa_code: mfaCode ?? null,
          },
        }),
      );
    } catch (error) {
      throw nativeAuthError(error);
    }
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
    password_change_required: response.password_change_required ?? false,
  };
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

export async function register(
  serverUrl: string,
  username: string,
  password: string,
  email?: string,
  displayName?: string,
): Promise<AuthResult> {
  if (isTauri()) {
    try {
      return await invokeNativeAuth((invoke) =>
        invoke<AuthResult>("register", {
          request: {
            server_url: serverUrl,
            username,
            email,
            password,
            display_name: displayName,
          },
        }),
      );
    } catch (error) {
      throw nativeAuthError(error);
    }
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
    password_change_required: response.password_change_required ?? false,
  };
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

// ----------------------------------------------------------------------------
// Detached session (browser SSO popup restore)
//
// The SSO popup restore reads the session behind the refresh cookie without
// touching the active in-memory session: nothing here installs a token or
// schedules a refresh. The auth store installs the token (via
// `oidcCompleteLogin`) only once it decided to apply the restore.
// ----------------------------------------------------------------------------

/** An access token obtained from the refresh cookie but not installed. */
export interface DetachedSession {
  accessToken: string;
  expiresIn: number;
}

const apiBase = () => getServerUrl().replace(/\/+$/, "");

/**
 * `POST /auth/refresh` with the cookie, returning the token instead of
 * installing it. Null when the server has no session for the cookie (or the
 * answer is unusable); rejects on a network error. Note the server rotates
 * the refresh cookie on this call.
 */
export async function refreshDetached(): Promise<DetachedSession | null> {
  if (isTauri()) return null;
  const response = await fetch(`${apiBase()}/auth/refresh`, {
    method: "POST",
    credentials: "include",
  });
  if (!response.ok) return null;
  const data = (await response.json()) as Partial<AuthResponse>;
  if (!data.access_token || typeof data.expires_in !== "number") return null;
  return { accessToken: data.access_token, expiresIn: data.expires_in };
}

/** `GET /auth/me` with an explicit (detached) access token. Rejects on failure. */
export async function getCurrentUserWithToken(
  accessToken: string,
): Promise<{ user: User; password_change_required: boolean }> {
  const response = await fetch(`${apiBase()}/auth/me`, {
    method: "GET",
    headers: { Authorization: `Bearer ${accessToken}` },
    credentials: "include",
  });
  if (!response.ok) {
    throw new HttpError(response.status, `HTTP ${response.status}: ${response.statusText}`);
  }
  const user = (await response.json()) as CurrentUserResponse;
  return { user, password_change_required: user.password_change_required === true };
}

/**
 * End a detached session that will not be used (best effort, never throws).
 * The logout response also clears the refresh cookie.
 */
export async function revokeDetachedSession(accessToken: string): Promise<void> {
  try {
    await fetch(`${apiBase()}/auth/logout`, {
      method: "POST",
      headers: { Authorization: `Bearer ${accessToken}` },
      credentials: "include",
    });
  } catch (error) {
    console.warn("[Auth] Could not revoke a discarded session:", error);
  }
}

/** `GET /api/setup/status` (public): whether the first-run setup is done. */
export async function getSetupStatus(): Promise<{ setup_complete: boolean }> {
  const response = await fetch(`${apiBase()}/api/setup/status`, { method: "GET" });
  if (!response.ok) {
    throw new HttpError(response.status, `HTTP ${response.status}: ${response.statusText}`);
  }
  return (await response.json()) as { setup_complete: boolean };
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

/** Turn a native command error code into an `HttpError` for the shared mapping. */
function nativeReauthError(err: unknown): HttpError {
  const raw = typeof err === "string" ? err : err instanceof Error ? err.message : "";
  // "HTTP_<status>" or "HTTP_<status>:<code>": the status always stays.
  const status = /^HTTP_(\d{3})(?::(.+))?$/.exec(raw);
  if (status) return new HttpError(Number(status[1]), raw, status[2]);
  return new HttpError(0, raw, raw || "REAUTH_FAILED");
}

/**
 * Start re-authenticating with a sign-in provider.
 *
 * Tauri: the native command runs the whole flow (system browser, loopback
 * callback) and resolves once the server reports the confirmation. Browser:
 * the server returns the provider URL for the caller to open in a popup; the
 * result arrives as an `oidc-reauth-callback` message.
 */
export async function startOidcReauth(
  providerId: string,
  opts: { reason?: string | null; mfaCode?: string | null; signal?: AbortSignal },
): Promise<{ mode: "tauri" } | { mode: "browser"; url: string }> {
  const abortIfCancelled = () => {
    if (opts.signal?.aborted) {
      const e = new Error("aborted");
      e.name = "AbortError";
      throw e;
    }
  };
  abortIfCancelled();
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    abortIfCancelled();
    // Lets the native side stop the listener (and not open the browser) when
    // the user closes the dialog mid-flow.
    const attemptId = crypto.randomUUID();
    const cancel = () => void invoke("oidc_reauth_cancel", { attemptId }).catch(() => {});
    opts.signal?.addEventListener("abort", cancel, { once: true });
    try {
      await invoke("oidc_reauth", {
        serverUrl: getServerUrl().replace(/\/+$/, ""),
        providerId,
        reason: opts.reason ?? null,
        mfaCode: opts.mfaCode ?? null,
        attemptId,
      });
    } catch (err) {
      throw nativeReauthError(err);
    } finally {
      opts.signal?.removeEventListener("abort", cancel);
    }
    return { mode: "tauri" };
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

export interface MfaBackupCodesResponse {
  codes: string[];
}

export interface MfaBackupCodeCountResponse {
  remaining: number;
  total: number;
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

/**
 * Generate (or regenerate) MFA backup codes. Needs a current authenticator
 * code; backup codes are not accepted here.
 */
export async function mfaGenerateBackupCodes(
  mfaCode: string,
): Promise<string[]> {
  const res = await httpRequest<MfaBackupCodesResponse>(
    "POST",
    "/auth/mfa/backup-codes",
    { mfa_code: mfaCode },
  );
  return res.codes;
}

/** Get remaining MFA backup code count. */
export async function mfaBackupCodeCount(): Promise<MfaBackupCodeCountResponse> {
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
 * Initiate OIDC login flow.
 *
 * In Tauri mode: Rust handles the entire flow (opens browser, waits for
 * callback, keeps the tokens). Returns { mode: "tauri", result }.
 *
 * In browser mode: returns the authorize URL for popup flow.
 * Returns { mode: "browser", authUrl }.
 */
export async function oidcAuthorize(
  serverUrl: string,
  providerSlug: string,
): Promise<
  | { mode: "tauri"; result: AuthResult }
  | { mode: "browser"; authUrl: string }
> {
  const baseUrl = serverUrl.replace(/\/+$/, "");

  if (isTauri()) {
    try {
      const result = await invokeNativeAuth((invoke) =>
        invoke<AuthResult>("oidc_authorize", {
          serverUrl: baseUrl,
          providerSlug,
        }),
      );
      return { mode: "tauri", result };
    } catch (error) {
      throw nativeSsoError(error);
    }
  }

  // Browser: return the authorize endpoint URL for popup flow
  const authUrl = `${baseUrl}/auth/oidc/authorize/${encodeURIComponent(providerSlug)}`;
  return { mode: "browser", authUrl };
}

/**
 * Complete OIDC login after callback (browser mode).
 * The access token arrives via postMessage from the callback page; the
 * refresh token is an HttpOnly cookie.
 */
export async function oidcCompleteLogin(
  serverUrl: string,
  accessToken: string,
  expiresIn: number,
): Promise<void> {
  const baseUrl = serverUrl.replace(/\/+$/, "");

  browserState.serverUrl = baseUrl;
  browserState.accessToken = accessToken;
  browserState.tokenExpiresAt = Date.now() + expiresIn * 1000;

  localStorage.setItem("serverUrl", baseUrl);
  setSessionRestoreBlocked(false);

  scheduleTokenRefresh();
}
