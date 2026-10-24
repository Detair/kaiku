/**
 * Authentication Store
 *
 * Manages user authentication state and actions.
 */

import { createStore } from "solid-js/store";
import type { User } from "@/lib/types";
import * as tauri from "@/lib/tauri";
import {
  initWebSocket,
  connect as wsConnect,
  disconnect as wsDisconnect,
  cleanupWebSocket,
} from "./websocket";
import {
  initPresence,
  cleanupPresence,
  initIdleDetection,
  stopIdleDetectionCleanup,
} from "./presence";
import { initPreferences } from "./preferences";
import { clearAllDrafts, cleanupDrafts } from "./drafts";
import { resetE2EEState } from "./e2ee";
import { showToast } from "@/components/ui/Toast";
import { t } from "@/i18n";
import { isTauri, PASSWORD_CHANGE_REQUIRED_EVENT } from "@/lib/tauri/common";

// Auth state interface
interface AuthState {
  user: User | null;
  serverUrl: string | null;
  isLoading: boolean;
  isInitialized: boolean;
  error: string | null;
  setupRequired: boolean;
  mfaRequired: boolean;
  sessionExpired: boolean;
  /**
   * A stored login exists but the server could not be reached at start.
   * The login is kept and the restore retried; the UI shows a
   * "can't reach the server" state instead of the login form.
   */
  restorePending: boolean;
  /**
   * The account still has to replace an admin-issued temporary password
   * (contract C4). While true only the change-password screen is shown and
   * no session services (WebSocket, presence, preferences) run.
   */
  passwordChangeRequired: boolean;
}

// Create the store
const [authState, setAuthState] = createStore<AuthState>({
  user: null,
  serverUrl: null,
  isLoading: false,
  isInitialized: false,
  error: null,
  setupRequired: false,
  mfaRequired: false,
  sessionExpired: false,
  restorePending: false,
  passwordChangeRequired: false,
});

// Derived state
export const isAuthenticated = () => authState.user !== null;
export const currentUser = () => authState.user;

/**
 * The temporary password the user just signed in with, kept so the forced
 * change screen does not ask for it again. Memory only: never persisted or
 * logged; cleared on success, logout and session expiry.
 */
let pendingTemporaryPassword: string | null = null;

/** Whether the change screen can reuse the password from this sign-in. */
export function hasPendingTemporaryPassword(): boolean {
  return pendingTemporaryPassword !== null;
}

/** Stop everything `startSessionServices()` started. Never throws. */
async function stopSessionServices(): Promise<void> {
  try {
    await wsDisconnect();
    await cleanupWebSocket();
    stopIdleDetectionCleanup();
    cleanupPresence();
  } catch (err) {
    console.error("[Auth] Stopping session services failed:", err);
  }
}

// Session-expired listener (registered once globally to prevent leaks)
let sessionExpiredListenerRegistered = false;

function registerSessionExpiredListener() {
  if (typeof window === "undefined" || sessionExpiredListenerRegistered) return;

  window.addEventListener("kaiku:session-expired", async () => {
    // Ignore if not authenticated
    if (!authState.user) return;

    // Native mode: Rust already tried the refresh and the server rejected it;
    // retrying would only report the same expiry again.
    let success = false;
    if (!isTauri()) {
      console.warn("[Kaiku:Auth] Session expired: attempting silent retry...");
      success = await tauri.refreshAccessToken();
    }
    if (success) {
      console.log("[Kaiku:Auth] Silent retry: success — session recovered");
      // The browser reconnect supervisor stops when it reports an expired
      // session; restart it (no-op while it is still running). Not while a
      // password change is pending: the server refuses the socket then.
      if (!isTauri() && !authState.passwordChangeRequired) {
        try {
          await wsConnect();
        } catch (err) {
          console.error("[Auth] WebSocket reconnect after session recovery failed:", err);
        }
      }
      return;
    }

    console.warn("[Kaiku:Auth] Silent retry: failed — showing expiry modal");

    // Clean up connections and drafts
    try {
      await wsDisconnect();
      await cleanupWebSocket();
      stopIdleDetectionCleanup();
      cleanupPresence();
      clearAllDrafts();
      cleanupDrafts();
    } catch (err) {
      console.error("[Auth] Cleanup during session expiry failed:", err);
    }

    pendingTemporaryPassword = null;
    setAuthState({
      user: null,
      sessionExpired: true,
      passwordChangeRequired: false,
      error: null,
    });
  });

  // A request was refused because the temporary password is still in use
  // (e.g. a session restored after a restart whose restore response did not
  // say so). Switch to the change screen and stop the services that would
  // only collect more 403s.
  window.addEventListener(PASSWORD_CHANGE_REQUIRED_EVENT, () => {
    if (!authState.user || authState.passwordChangeRequired) return;
    console.warn("[Kaiku:Auth] Password change required — showing the change screen");
    setAuthState({ passwordChangeRequired: true, error: null });
    void stopSessionServices();
  });

  sessionExpiredListenerRegistered = true;
}

// Register immediately — the listener checks auth state internally
registerSessionExpiredListener();

/** Native mode: follow token refreshes and expiry from the Rust session. */
async function initNativeSession(): Promise<void> {
  if (!isTauri()) return;
  try {
    await tauri.initNativeAuthEvents();
  } catch (err) {
    console.error("[Auth] Native auth event setup failed:", err);
  }
}

/** Tell the user when the OS keyring could not keep their login. */
function warnIfNotRemembered(result: tauri.AuthResult): void {
  if (result.persisted === false) {
    showToast({
      type: "warning",
      title: t("auth.notRememberedTitle"),
      message: t("auth.notRemembered"),
    });
  }
}

/**
 * Shared post-sign-in setup: WebSocket, presence, preferences, idle
 * detection. Used after session restore, login, registration and both OIDC
 * flows. `showWsError: false` keeps registration's behaviour of not showing
 * the "real-time unavailable" banner when the first connect fails.
 */
async function startSessionServices(
  options: { showWsError?: boolean } = {},
): Promise<void> {
  const { showWsError = true } = options;

  // Initialize WebSocket listeners and presence in parallel (independent)
  await Promise.all([initWebSocket(), initPresence()]);

  // Connect WebSocket and sync preferences in parallel (independent)
  await Promise.all([
    wsConnect()
      .then(() => console.log("[Auth] WebSocket connected"))
      .catch((wsErr) => {
        console.error("[Auth] WebSocket connection failed:", wsErr);
        if (showWsError) {
          setAuthState({
            error: "Real-time messaging temporarily unavailable. Reconnecting...",
          });
        }
      }),
    initPreferences()
      .then(() => console.log("[Auth] Preferences initialized"))
      .catch((prefErr) => {
        console.error("[Auth] Preferences initialization failed:", prefErr);
      }),
  ]);

  // Initialize idle detection after preferences (uses idle_timeout_minutes setting)
  initIdleDetection();
}

// Actions

// Session restore retry (stored login, server unreachable at start).
// Backoff doubles from 5 s and is capped at 30 s; `online` and the window
// becoming visible retry at once. Only the server rejecting the stored
// login (`session_expired`) or the user choosing to sign in ends it.
const RESTORE_RETRY_BASE_MS = 5_000;
const RESTORE_RETRY_MAX_MS = 30_000;
let restoreRetryTimer: ReturnType<typeof setTimeout> | null = null;
let restoreRetryAttempt = 0;
let restoreInFlight = false;
let restoreTriggersAttached = false;

/** Delay before restore retry `attempt` (1-based). Exported for tests. */
export function restoreRetryDelayMs(attempt: number): number {
  return Math.min(
    RESTORE_RETRY_BASE_MS * 2 ** Math.max(0, attempt - 1),
    RESTORE_RETRY_MAX_MS,
  );
}

function onRestoreTrigger(): void {
  if (typeof document !== "undefined" && document.visibilityState === "hidden") {
    return;
  }
  retryRestoreNow();
}

function attachRestoreTriggers(): void {
  if (restoreTriggersAttached || typeof window === "undefined") return;
  restoreTriggersAttached = true;
  window.addEventListener("online", onRestoreTrigger);
  document.addEventListener("visibilitychange", onRestoreTrigger);
}

function stopRestoreRetry(): void {
  if (restoreRetryTimer) {
    clearTimeout(restoreRetryTimer);
    restoreRetryTimer = null;
  }
  restoreRetryAttempt = 0;
  if (restoreTriggersAttached) {
    window.removeEventListener("online", onRestoreTrigger);
    document.removeEventListener("visibilitychange", onRestoreTrigger);
    restoreTriggersAttached = false;
  }
}

function scheduleRestoreRetry(): void {
  attachRestoreTriggers();
  if (restoreRetryTimer) clearTimeout(restoreRetryTimer);
  restoreRetryAttempt += 1;
  const delay = restoreRetryDelayMs(restoreRetryAttempt);
  console.warn(
    `[Auth] Server unreachable during session restore — retrying in ${Math.round(delay / 1000)}s`,
  );
  restoreRetryTimer = setTimeout(() => {
    restoreRetryTimer = null;
    void attemptRestore();
  }, delay);
}

/** Try the pending session restore now ("Try again", `online`, focus). */
export function retryRestoreNow(): void {
  if (!authState.restorePending || restoreInFlight) return;
  if (restoreRetryTimer) {
    clearTimeout(restoreRetryTimer);
    restoreRetryTimer = null;
  }
  void attemptRestore();
}

/**
 * Give up waiting for the server and show the login form. The stored login
 * stays; a successful sign-in replaces it.
 */
export function abandonRestore(): void {
  if (!authState.restorePending) return;
  stopRestoreRetry();
  setAuthState({ restorePending: false, isInitialized: true, isLoading: false });
}

/** One restore attempt; reschedules itself while the server is unreachable. */
async function attemptRestore(): Promise<void> {
  if (restoreInFlight) return;
  restoreInFlight = true;
  try {
    const result = await tauri.getCurrentUser();
    // The user chose to sign in while this attempt was running.
    if (authState.isInitialized) return;

    if (result.user) {
      stopRestoreRetry();
      const passwordChangeRequired = result.password_change_required === true;
      setAuthState({
        user: result.user,
        serverUrl: tauri.getServerUrl() || null,
        isLoading: false,
        isInitialized: true,
        restorePending: false,
        passwordChangeRequired,
      });

      if (!passwordChangeRequired) await startSessionServices();
      return;
    }

    // Native mode reports `network_error` only when a stored login exists
    // and the server could not be reached (offline, timeout, 5xx). Keep the
    // login and retry instead of dropping the user on the login form.
    if (result.error_reason === "network_error") {
      setAuthState({ isLoading: false, restorePending: true });
      scheduleRestoreRetry();
      return;
    }

    stopRestoreRetry();
    setAuthState({
      user: null,
      isLoading: false,
      isInitialized: true,
      restorePending: false,
    });

    // Show toast for failed restore (only when there WAS a stored session)
    if (result.error_reason === "session_expired") {
      showToast({
        type: "warning",
        title: "Session Expired",
        message: "Please log in again.",
      });
    }
  } catch (err) {
    console.error("Failed to restore session:", err);
    stopRestoreRetry();
    setAuthState({
      user: null,
      isLoading: false,
      isInitialized: true,
      restorePending: false,
      error: null,
    });
  } finally {
    restoreInFlight = false;
  }
}

/**
 * Initialize auth state by checking for existing session.
 */
export async function initAuth(): Promise<void> {
  if (authState.isInitialized || authState.restorePending || restoreInFlight) {
    return;
  }

  setAuthState({ isLoading: true, error: null });

  await initNativeSession();
  await attemptRestore();
}

/**
 * Login with username and password.
 * If MFA is required, sets mfaRequired=true and throws — caller should
 * show MFA input, then call login() again with the mfaCode.
 */
export async function login(
  serverUrl: string,
  username: string,
  password: string,
  mfaCode?: string,
): Promise<User> {
  setAuthState({ isLoading: true, error: null });

  try {
    await initNativeSession();
    const result = await tauri.login(serverUrl, username, password, mfaCode);
    warnIfNotRemembered(result);
    stopRestoreRetry();
    const passwordChangeRequired = result.password_change_required === true;
    // Only a temporary password is remembered, and only until it is replaced.
    pendingTemporaryPassword = passwordChangeRequired ? password : null;
    setAuthState({
      restorePending: false,
      user: result.user,
      serverUrl,
      isLoading: false,
      isInitialized: true,
      error: null,
      setupRequired: result.setup_required,
      mfaRequired: false,
      sessionExpired: false,
      passwordChangeRequired,
    });

    if (!passwordChangeRequired) await startSessionServices();

    return result.user;
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);

    // Detect MFA_REQUIRED
    if (error === "MFA_REQUIRED") {
      setAuthState({
        isLoading: false,
        error: null,
        mfaRequired: true,
        serverUrl,
      });
      throw new Error("MFA_REQUIRED", { cause: err });
    }

    setAuthState({ isLoading: false, error });
    throw new Error(error, { cause: err });
  }
}

/**
 * Register a new account.
 */
export async function register(
  serverUrl: string,
  username: string,
  password: string,
  email?: string,
  displayName?: string,
): Promise<User> {
  setAuthState({ isLoading: true, error: null });

  try {
    await initNativeSession();
    const result = await tauri.register(
      serverUrl,
      username,
      password,
      email,
      displayName,
    );
    warnIfNotRemembered(result);
    stopRestoreRetry();
    setAuthState({
      restorePending: false,
      user: result.user,
      serverUrl,
      isLoading: false,
      isInitialized: true,
      error: null,
      setupRequired: result.setup_required,
      sessionExpired: false,
      passwordChangeRequired: result.password_change_required === true,
    });

    if (!authState.passwordChangeRequired) {
      await startSessionServices({ showWsError: false });
    }

    return result.user;
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    setAuthState({ isLoading: false, error });
    throw new Error(error, { cause: err });
  }
}

/**
 * Complete a native (Tauri) OIDC login: Rust already holds the tokens and
 * returned the user.
 */
export async function completeNativeOidcLogin(
  serverUrl: string,
  result: tauri.AuthResult,
): Promise<void> {
  await initNativeSession();
  warnIfNotRemembered(result);
  stopRestoreRetry();
  setAuthState({
    restorePending: false,
    user: result.user,
    serverUrl: tauri.getServerUrl() || serverUrl,
    isLoading: false,
    isInitialized: true,
    error: null,
    setupRequired: result.setup_required,
    mfaRequired: false,
    sessionExpired: false,
    passwordChangeRequired: result.password_change_required === true,
  });
  if (!authState.passwordChangeRequired) await startSessionServices();
}

/**
 * Complete a browser OIDC login after receiving the access token from the
 * callback page. The refresh token is an HttpOnly cookie, so
 * `_refreshToken` is ignored.
 */
export async function loginWithOidc(
  serverUrl: string,
  accessToken: string,
  _refreshToken: string | undefined,
  expiresIn: number,
  setupRequired: boolean = false,
): Promise<void> {
  setAuthState({ isLoading: true, error: null });

  try {
    await tauri.oidcCompleteLogin(serverUrl, accessToken, expiresIn);

    // Fetch the current user with the new token
    const result = await tauri.getCurrentUser();
    if (!result.user) {
      throw new Error("Failed to fetch user after OIDC login");
    }

    setAuthState({
      user: result.user,
      serverUrl,
      isLoading: false,
      isInitialized: true,
      error: null,
      setupRequired,
      sessionExpired: false,
      passwordChangeRequired: result.password_change_required === true,
    });

    if (!authState.passwordChangeRequired) await startSessionServices();
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    setAuthState({ isLoading: false, error });
    throw new Error(error, { cause: err });
  }
}

/**
 * Logout and clear session.
 */
export async function logout(): Promise<void> {
  pendingTemporaryPassword = null;
  setAuthState({ isLoading: true, error: null, passwordChangeRequired: false });

  // Disconnect WebSocket and cleanup
  try {
    await wsDisconnect();
    await cleanupWebSocket();
    stopIdleDetectionCleanup();
    cleanupPresence();
    clearAllDrafts();
    cleanupDrafts();
  } catch (err) {
    console.error("Error during cleanup:", err);
  }

  try {
    await tauri.logout();
    resetE2EEState();
    setAuthState({
      user: null,
      isLoading: false,
      error: null,
    });
  } catch (err) {
    console.error("[Auth] Logout failed:", err);
    resetE2EEState();
    const error = err instanceof Error ? err.message : String(err);
    setAuthState({
      user: null,
      isLoading: false,
      error,
    });
  }
}

/**
 * Replace the admin-issued temporary password (contract C4), then start the
 * session normally. `currentPassword` is the temporary password typed on the
 * change screen; `null` reuses the one from this sign-in. Throws when the
 * server refuses the change; the flag and the remembered password then stay.
 */
export async function completeForcedPasswordChange(
  currentPassword: string | null,
  newPassword: string,
): Promise<void> {
  const current = currentPassword ?? pendingTemporaryPassword;
  if (current === null) {
    throw new Error("The temporary password is required");
  }

  // Other sessions of a brand-new account were opened with the temporary
  // password; end them.
  await tauri.updatePassword(current, newPassword, true);

  // Pick up a token issued after the change (C4: the refresh response
  // carries the cleared flag). The current token keeps working if this fails.
  try {
    if (!(await tauri.refreshAccessToken())) {
      console.warn("[Auth] Token refresh after password change failed");
    }
  } catch (err) {
    console.warn("[Auth] Token refresh after password change failed:", err);
  }

  pendingTemporaryPassword = null;
  setAuthState({ passwordChangeRequired: false, error: null });
  await startSessionServices();
}

/**
 * Clear any auth errors.
 */
export function clearError(): void {
  setAuthState({ error: null });
}

/**
 * Update local user state.
 */
export function updateUser(updates: Partial<User>): void {
  setAuthState("user", (prev) => (prev ? { ...prev, ...updates } : null));
}

/**
 * Clear the setup required flag.
 * Called after completing the setup wizard.
 */
export function clearSetupRequired(): void {
  setAuthState("setupRequired", false);
}

// Export the store for reading only
// Use the exported functions (login, register, logout, etc.) to modify state
export { authState, setAuthState };
