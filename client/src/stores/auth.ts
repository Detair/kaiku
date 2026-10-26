/**
 * Authentication Store
 *
 * Manages user authentication state and actions.
 */

import { createStore } from "solid-js/store";
import { codeIs } from "@/lib/errorCode";
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
import { cleanupPreferences, initPreferences } from "./preferences";
import { clearAllDrafts, cleanupDrafts } from "./drafts";
import { resetE2EEState } from "./e2ee";
import { showToast } from "@/components/ui/Toast";
import { t } from "@/i18n";
import { errorText } from "@/i18n/errors";
import { LOGIN_ERRORS, REGISTER_ERRORS, REGISTER_STATUS_ERRORS } from "@/lib/authErrors";
import {
  isTauri,
  PASSWORD_CHANGE_REQUIRED_EVENT,
  SESSION_ENDED_EVENT,
} from "@/lib/tauri/common";

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
  /** Why the session ended while `sessionExpired` is set (selects the text). */
  sessionEndReason: tauri.SessionEndReason;
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
  sessionEndReason: "expired",
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

/**
 * Set once a forced change was accepted by the server but the follow-up
 * token refresh failed transiently: retrying must not resubmit the change.
 */
let passwordChangeSaved = false;

/** Whether the new password is saved and only the session start is left. */
export function isForcedPasswordChangeSaved(): boolean {
  return passwordChangeSaved;
}

// ---------------------------------------------------------------------------
// Session-services lifecycle
//
// The session services (WebSocket, presence, preferences, idle detection)
// are module-level singletons. Each service store is cancellation-aware on
// its own: an init whose IPC or network step settles after a cleanup, or
// after a newer init, releases what it obtained (the WebSocket listeners, the
// presence listener, a preferences response), and a disconnect that settles
// after a newer connect leaves that connection's state alone. This section
// only orders starts and stops.
//
// `ownerId` grows monotonically: `startSessionServices()` takes a new id
// synchronously, `requestStop()` bumps it synchronously. So the moment a stop
// or a newer start is requested, every older start is superseded, before
// anything is awaited.
//
// Stops never wait for a start. A stop's teardown runs on `teardownChain`,
// which only ever holds teardowns. Each teardown step is bounded
// (`TEARDOWN_STEP_TIMEOUT_MS`): a step that does not settle in time is
// abandoned, so logout, session expiry and the switch to the change screen
// always finish; the stores' tokens make the abandoned step's late
// continuation harmless.
//
// A start waits for the pending teardowns (bounded by construction) and
// never for an older start: that start is superseded by the new id, and the
// stores cancel whatever it still has in flight. A start job re-checks
// between its steps that it still owns the services; once superseded it
// undoes exactly what it started (on the teardown chain, so it never
// interleaves with a stop) and exits.
//
// `activeStartId` is the start job that last began its steps. Every teardown
// action, from a stop or from an undo, is skipped once a start newer than the
// one it was requested for has begun: that start owns the singletons now, and
// its own stop cleans them up. This keeps a late-settling superseded start
// from tearing down the replacement session.
// ---------------------------------------------------------------------------

/** How long one teardown step may take before it is abandoned. Exported for tests. */
export const TEARDOWN_STEP_TIMEOUT_MS = 5_000;

let ownerId = 0;
let activeStartId = 0;
let teardownChain: Promise<void> = Promise.resolve();

const noop = (): void => undefined;

/** Append a teardown to the chain; a failed one never blocks the next. */
function enqueueTeardown(job: () => Promise<void>): Promise<void> {
  const result = teardownChain.then(job, job);
  teardownChain = result.then(noop, noop);
  return result;
}

/**
 * Run one teardown step. A failure is logged; a step that has not settled
 * within `TEARDOWN_STEP_TIMEOUT_MS` is abandoned (its store ignores a late
 * settle on its own). Never throws.
 */
async function boundedStep(step: () => Promise<void>, what: string): Promise<void> {
  const run = (async () => {
    await step();
    return "done" as const;
  })().catch((err: unknown) => {
    console.error(`[Auth] ${what} failed:`, err);
    return "failed" as const;
  });
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<"timeout">((resolve) => {
    timer = setTimeout(() => resolve("timeout"), TEARDOWN_STEP_TIMEOUT_MS);
  });
  try {
    const outcome = await Promise.race([run, timeout]);
    if (outcome === "timeout") {
      console.warn(`[Auth] ${what} did not finish in time — continuing without it`);
    }
  } finally {
    clearTimeout(timer);
  }
}

/** Services may run: signed in and no forced password change pending. */
function servicesEligible(): boolean {
  return authState.user !== null && !authState.passwordChangeRequired;
}

/** Which services a teardown covers: what one start job started, or all. */
interface StartedServices {
  websocket: boolean;
  presence: boolean;
  idle: boolean;
}

const ALL_SERVICES: StartedServices = { websocket: true, presence: true, idle: true };

/** A start requested after `sinceId` has begun: it owns the services now. */
function newerStartBegan(sinceId: number): boolean {
  return activeStartId > sinceId;
}

/**
 * Tear down `which` services on behalf of the owner id `sinceId` (a stop's
 * id, or a superseded start's own id). Each action is skipped once a newer
 * start has begun. Never throws.
 */
async function teardownServices(sinceId: number, which: StartedServices): Promise<void> {
  if (which.websocket && !newerStartBegan(sinceId)) {
    await boundedStep(wsDisconnect, "Stopping the WebSocket");
    if (newerStartBegan(sinceId)) return;
    await boundedStep(cleanupWebSocket, "Removing the WebSocket listeners");
  }
  if (newerStartBegan(sinceId)) return;
  try {
    if (which.idle) stopIdleDetectionCleanup();
    if (which.presence) cleanupPresence();
  } catch (err) {
    console.error("[Auth] Stopping presence/idle failed:", err);
  }
}

/**
 * Request a stop of the session services: supersedes any queued or running
 * start at once, then tears everything down without waiting for that start.
 */
function requestStop(): Promise<void> {
  const stopId = ++ownerId;
  // Synchronously, before any teardown step can hang: a preferences response
  // or debounced push of this session must never reach the next one.
  cleanupPreferences();
  return enqueueTeardown(() => teardownServices(stopId, ALL_SERVICES));
}

/** Cancel a running startup and stop the services. */
function stopSessionServices(): Promise<void> {
  return requestStop();
}

// Session-expired listener (registered once globally to prevent leaks)
let sessionExpiredListenerRegistered = false;

/** Tear down the session and show the sign-in prompt for `reason`. */
async function endSession(reason: tauri.SessionEndReason): Promise<void> {
  // Claim the end first so a concurrent expiry report does not repeat it.
  setAuthState({ user: null });
  try {
    await requestStop();
  } catch (err) {
    console.error("[Auth] Stopping services during session end failed:", err);
  }
  try {
    clearAllDrafts();
    cleanupDrafts();
  } catch (err) {
    console.error("[Auth] Cleanup during session end failed:", err);
  }
  pendingTemporaryPassword = null;
  passwordChangeSaved = false;
  setAuthState({
    user: null,
    sessionExpired: true,
    sessionEndReason: reason,
    passwordChangeRequired: false,
    error: null,
  });
}

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

    // The session may have been ended for a specific reason while the retry ran.
    if (!authState.user) return;
    console.warn("[Kaiku:Auth] Silent retry: failed — showing expiry modal");
    await endSession("expired");
  });

  // The session is over for good (revoked, banned, or the server kept
  // refusing a fresh token): no silent retry, straight to the sign-in prompt.
  window.addEventListener(SESSION_ENDED_EVENT, (event) => {
    if (!authState.user) return;
    const reason = (event as CustomEvent<{ reason: tauri.SessionEndReason }>)
      .detail?.reason;
    console.warn(`[Kaiku:Auth] Session ended (${reason}) — showing sign-in prompt`);
    void endSession(reason ?? "expired");
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
      title: t("auth.session.notRememberedTitle"),
      message: t("auth.session.notRemembered"),
    });
  }
}

/**
 * Shared post-sign-in setup: WebSocket, presence, preferences, idle
 * detection. Used after session restore, login, registration and both OIDC
 * flows. `showWsError: false` keeps registration's behaviour of not showing
 * the "real-time unavailable" banner when the first connect fails.
 */
function startSessionServices(
  options: { showWsError?: boolean } = {},
): Promise<void> {
  // The id is taken now, before anything is awaited: a stop requested from
  // here on supersedes this start even while it still waits for teardowns.
  const myId = ++ownerId;
  return runStart(myId, teardownChain, options);
}

/** One start job; `pendingTeardowns` are the teardowns requested before it. */
async function runStart(
  myId: number,
  pendingTeardowns: Promise<void>,
  options: { showWsError?: boolean },
): Promise<void> {
  const { showWsError = true } = options;
  // Teardowns requested before this start finish first (each step bounded,
  // so this wait is too). An older start is not waited for: it is superseded
  // by this id, and the stores cancel whatever it still has in flight.
  await pendingTeardowns;
  const current = () => myId === ownerId && servicesEligible();
  if (!current()) return;
  activeStartId = myId;

  const started: StartedServices = { websocket: false, presence: false, idle: false };
  // Superseded while a step ran: undo what this job started and stop. The
  // undo runs on the teardown chain, after any stop already requested.
  const superseded = async (): Promise<boolean> => {
    if (current()) return false;
    await enqueueTeardown(() => teardownServices(myId, started));
    return true;
  };

  // Initialize WebSocket listeners and presence in parallel (independent).
  // allSettled: a rejection must not skip the check while the other init is
  // still running. A failed init still counts as started for the undo.
  started.websocket = true;
  started.presence = true;
  const initResults = await Promise.allSettled([initWebSocket(), initPresence()]);
  if (await superseded()) return;
  const initFailure = initResults.find(
    (r): r is PromiseRejectedResult => r.status === "rejected",
  );
  if (initFailure) throw initFailure.reason;

  // Connect WebSocket and sync preferences in parallel (independent; both
  // handle their own errors, so this never rejects)
  await Promise.allSettled([
    wsConnect()
      .then(() => console.log("[Auth] WebSocket connected"))
      .catch((wsErr) => {
        console.error("[Auth] WebSocket connection failed:", wsErr);
        if (showWsError) {
          setAuthState({
            error: t("connection.realtime.unavailable"),
          });
        }
      }),
    initPreferences()
      .then(() => console.log("[Auth] Preferences initialized"))
      .catch((prefErr) => {
        console.error("[Auth] Preferences initialization failed:", prefErr);
      }),
  ]);
  if (await superseded()) return;

  // Initialize idle detection after preferences (uses idle_timeout_minutes setting)
  initIdleDetection();
  started.idle = true;
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
        title: t("auth.sessionExpired.title"),
        message: t("auth.sessionExpired.toast"),
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
    const raw = err instanceof Error ? err.message : String(err);

    // Detect MFA_REQUIRED
    if (raw === "MFA_REQUIRED") {
      setAuthState({
        isLoading: false,
        error: null,
        mfaRequired: true,
        serverUrl,
      });
      throw new Error("MFA_REQUIRED", { cause: err });
    }

    const error = errorText(err, "auth.login.failed", LOGIN_ERRORS);
    setAuthState({ isLoading: false, error });
    throw new Error(raw, { cause: err });
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
    const error = errorText(
      err,
      "auth.register.failed",
      REGISTER_ERRORS,
      REGISTER_STATUS_ERRORS,
    );
    setAuthState({ isLoading: false, error });
    throw new Error(err instanceof Error ? err.message : String(err), { cause: err });
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
    const error = errorText(err, "auth.login.ssoFailed");
    setAuthState({ isLoading: false, error });
    throw new Error(err instanceof Error ? err.message : String(err), { cause: err });
  }
}

/**
 * Logout and clear session.
 */
export async function logout(): Promise<void> {
  pendingTemporaryPassword = null;
  passwordChangeSaved = false;
  setAuthState({ isLoading: true, error: null, passwordChangeRequired: false });

  // Disconnect WebSocket and cleanup
  await requestStop();
  try {
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
    const error = errorText(err, "auth.logout.failed");
    setAuthState({
      user: null,
      isLoading: false,
      error,
    });
  }
}

/** Thrown when the new password is saved but the session could not start yet. */
export const PASSWORD_SAVED_REFRESH_FAILED = "PASSWORD_SAVED_REFRESH_FAILED";

/** Whether a failed change means the temporary password itself was wrong. */
function isInvalidCredentials(err: unknown): boolean {
  const { status, code } = (err ?? {}) as { status?: unknown; code?: unknown };
  return status === 401 || codeIs(code, "INVALID_CREDENTIALS");
}

/**
 * Replace the admin-issued temporary password (contract C4), then start the
 * session normally. `currentPassword` is the temporary password typed on the
 * change screen; `null` reuses the one from this sign-in.
 *
 * Throws when the server refuses the change; the flag then stays. A rejected
 * remembered temporary password is dropped, so the screen asks for it. If
 * the change is saved but the token refresh fails transiently, throws
 * `PASSWORD_SAVED_REFRESH_FAILED`; `retryForcedPasswordChangeFinish()` then
 * finishes without resubmitting.
 */
export async function completeForcedPasswordChange(
  currentPassword: string | null,
  newPassword: string,
): Promise<void> {
  if (passwordChangeSaved) {
    await finishForcedPasswordChange();
    return;
  }
  const usingRemembered = currentPassword === null;
  const current = currentPassword ?? pendingTemporaryPassword;
  if (current === null) {
    throw new Error("The temporary password is required");
  }

  // Other sessions of a brand-new account were opened with the temporary
  // password; end them.
  try {
    await tauri.updatePassword(current, newPassword, true);
  } catch (err) {
    if (usingRemembered && isInvalidCredentials(err)) {
      pendingTemporaryPassword = null;
    }
    throw err;
  }

  pendingTemporaryPassword = null;
  passwordChangeSaved = true;
  await finishForcedPasswordChange();
}

/** Retry starting the session after `PASSWORD_SAVED_REFRESH_FAILED`. */
export async function retryForcedPasswordChangeFinish(): Promise<void> {
  if (!passwordChangeSaved) return;
  await finishForcedPasswordChange();
}

/**
 * After a saved change: refresh (C4: the refresh response carries the
 * cleared flag), then start the session. A rejected refresh ends the session
 * through the usual expiry path; a transient failure keeps the change saved
 * and throws `PASSWORD_SAVED_REFRESH_FAILED`.
 */
async function finishForcedPasswordChange(): Promise<void> {
  let refreshed = false;
  try {
    refreshed = await tauri.refreshAccessToken();
  } catch (err) {
    console.warn("[Auth] Token refresh after password change failed:", err);
  }

  if (!refreshed) {
    // Both modes drop the token when the server rejects the refresh.
    if (tauri.getAccessToken() === null) {
      console.warn("[Auth] Session ended right after the password change");
      passwordChangeSaved = false;
      if (isTauri()) {
        tauri.expireNativeSession();
      } else {
        window.dispatchEvent(new CustomEvent("kaiku:session-expired"));
      }
      return;
    }
    console.warn("[Auth] Token refresh after password change failed — waiting for retry");
    throw new Error(PASSWORD_SAVED_REFRESH_FAILED);
  }

  // Signed out or expired while the refresh ran.
  if (!authState.user) {
    passwordChangeSaved = false;
    return;
  }

  passwordChangeSaved = false;
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
