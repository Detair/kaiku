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
import { isTauri } from "@/lib/tauri/common";

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
});

// Derived state
export const isAuthenticated = () => authState.user !== null;
export const currentUser = () => authState.user;

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
      // session; restart it (no-op while it is still running).
      if (!isTauri()) {
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

    setAuthState({
      user: null,
      sessionExpired: true,
      error: null,
    });
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
// Backoff doubles from 2 s and is capped at 30 s; `online` and the window
// becoming visible retry at once. Only the server rejecting the stored
// login (`session_expired`) or the user choosing to sign in ends it.
const RESTORE_RETRY_BASE_MS = 2_000;
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
      setAuthState({
        user: result.user,
        serverUrl: tauri.getServerUrl() || null,
        isLoading: false,
        isInitialized: true,
        restorePending: false,
      });

      await startSessionServices();
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
    setAuthState({
      user: result.user,
      serverUrl,
      isLoading: false,
      isInitialized: true,
      error: null,
      setupRequired: result.setup_required,
      mfaRequired: false,
      sessionExpired: false,
    });

    await startSessionServices();

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
    setAuthState({
      user: result.user,
      serverUrl,
      isLoading: false,
      isInitialized: true,
      error: null,
      setupRequired: result.setup_required,
      sessionExpired: false,
    });

    await startSessionServices({ showWsError: false });

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
  setAuthState({
    user: result.user,
    serverUrl: tauri.getServerUrl() || serverUrl,
    isLoading: false,
    isInitialized: true,
    error: null,
    setupRequired: result.setup_required,
    mfaRequired: false,
    sessionExpired: false,
  });
  await startSessionServices();
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
    });

    await startSessionServices();
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
  setAuthState({ isLoading: true, error: null });

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
