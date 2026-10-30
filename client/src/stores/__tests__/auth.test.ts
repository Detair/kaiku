import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockedFunction } from "vitest";
import { HttpError, isSessionRestoreBlocked, setSessionRestoreBlocked } from "@/lib/tauri/common";

vi.mock("@/lib/tauri", () => ({
  getCurrentUser: vi.fn(),
  login: vi.fn(),
  register: vi.fn(),
  logout: vi.fn(),
  oidcCompleteLogin: vi.fn(),
  getSetupStatus: vi.fn(),
  refreshDetached: vi.fn(),
  getCurrentUserWithToken: vi.fn(),
  revokeDetachedSession: vi.fn(),
  refreshAccessToken: vi.fn(),
  updatePassword: vi.fn(),
  // A live session has a token unless a test says otherwise.
  getAccessToken: vi.fn(() => "token"),
  expireNativeSession: vi.fn(),
  // Mirrors the real fallback: the stored server address survives a restart.
  getServerUrl: vi.fn(() => localStorage.getItem("serverUrl") ?? ""),
}));

vi.mock("@/stores/websocket", () => ({
  initWebSocket: vi.fn(),
  connect: vi.fn(),
  disconnect: vi.fn(),
  cleanupWebSocket: vi.fn(),
}));

vi.mock("@/stores/presence", () => ({
  initPresence: vi.fn(),
  cleanupPresence: vi.fn(),
  initIdleDetection: vi.fn(),
  stopIdleDetectionCleanup: vi.fn(),
}));

vi.mock("@/stores/preferences", () => ({
  initPreferences: vi.fn(),
  cleanupPreferences: vi.fn(),
}));

vi.mock("@/stores/drafts", () => ({
  clearAllDrafts: vi.fn(),
  cleanupDrafts: vi.fn(),
}));

vi.mock("@/components/ui/Toast", () => ({
  showToast: vi.fn(),
}));

import * as tauri from "@/lib/tauri";
import {
  initWebSocket,
  connect as wsConnect,
  disconnect as wsDisconnect,
  cleanupWebSocket,
} from "@/stores/websocket";
import {
  initPresence,
  cleanupPresence,
  initIdleDetection,
  stopIdleDetectionCleanup,
} from "@/stores/presence";
import { initPreferences } from "@/stores/preferences";
import { clearAllDrafts, cleanupDrafts } from "@/stores/drafts";
import { showToast } from "@/components/ui/Toast";
import type { User } from "@/lib/types";
import {
  authState,
  setAuthState,
  initAuth,
  login,
  register,
  loginWithOidc,
  restoreSessionAfterPopup,
  logout,
  clearError,
  updateUser,
  clearSetupRequired,
  isAuthenticated,
  currentUser,
  abandonRestore,
  retryRestoreNow,
  restoreRetryDelayMs,
  completeForcedPasswordChange,
  hasPendingTemporaryPassword,
  isForcedPasswordChangeSaved,
  retryForcedPasswordChangeFinish,
  PASSWORD_SAVED_REFRESH_FAILED,
  TEARDOWN_STEP_TIMEOUT_MS,
} from "../auth";

/** Resolve `p`, or fail after `ms`: a hung lifecycle must fail, not time out. */
async function withinMs<T>(p: Promise<T>, ms = 1000): Promise<T> {
  let timer!: ReturnType<typeof setTimeout>;
  const deadline = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error(`still pending after ${ms} ms`)), ms);
  });
  try {
    return await Promise.race([p, deadline]);
  } finally {
    clearTimeout(timer);
  }
}

/** Make the next call of `step` stay pending until the returned function runs. */
function holdOnce(step: MockedFunction<() => Promise<void>>): () => void {
  let release!: () => void;
  step.mockImplementationOnce(() => new Promise<void>((resolve) => (release = resolve)));
  return () => release();
}

const tick = () => new Promise((r) => setTimeout(r, 0));

function createUser(overrides: Partial<User> = {}): User {
  return {
    id: "user-1",
    username: "alice",
    display_name: "Alice",
    avatar_url: null,
    status: "online",
    email: "alice@example.com",
    mfa_enabled: false,
    created_at: "2025-01-01T00:00:00Z",
    ...overrides,
  };
}

describe("auth store", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setAuthState({
      user: null,
      serverUrl: null,
      isLoading: false,
      isInitialized: false,
      error: null,
      setupRequired: false,
      sessionExpired: false,
      restorePending: false,
      passwordChangeRequired: false,
    });
  });

  describe("initial state", () => {
    it("has null user and not initialized", () => {
      expect(authState.user).toBeNull();
      expect(authState.isLoading).toBe(false);
      expect(authState.isInitialized).toBe(false);
      expect(authState.error).toBeNull();
      expect(authState.setupRequired).toBe(false);
    });

    it("has sessionExpired false initially", () => {
      expect(authState.sessionExpired).toBe(false);
    });
  });

  describe("derived state", () => {
    it("isAuthenticated returns false when no user", () => {
      expect(isAuthenticated()).toBe(false);
    });

    it("isAuthenticated returns true when user present", () => {
      setAuthState({ user: createUser() });

      expect(isAuthenticated()).toBe(true);
    });

    it("currentUser returns the user", () => {
      const user = createUser();
      setAuthState({ user });

      expect(currentUser()?.id).toBe(user.id);
    });
  });

  describe("initAuth", () => {
    it("restores session and inits subsystems", async () => {
      const user = createUser();
      vi.mocked(tauri.getCurrentUser).mockResolvedValue({ user, error_reason: null });
      vi.mocked(initWebSocket).mockResolvedValue(undefined);
      vi.mocked(initPresence).mockResolvedValue(undefined);
      vi.mocked(wsConnect).mockResolvedValue(undefined);
      vi.mocked(initPreferences).mockResolvedValue(undefined);

      await initAuth();

      expect(authState.user).toEqual(user);
      expect(authState.isInitialized).toBe(true);
      expect(initWebSocket).toHaveBeenCalled();
      expect(initPresence).toHaveBeenCalled();
      expect(wsConnect).toHaveBeenCalled();
      expect(initPreferences).toHaveBeenCalled();
      expect(initIdleDetection).toHaveBeenCalled();
    });

    it("initAuth restores serverUrl from the stored server address", async () => {
      localStorage.setItem("serverUrl", "https://kaiku.example");
      vi.mocked(tauri.getCurrentUser).mockResolvedValue({ user: createUser(), error_reason: null });
      vi.mocked(initWebSocket).mockResolvedValue(undefined);
      vi.mocked(initPresence).mockResolvedValue(undefined);
      vi.mocked(wsConnect).mockResolvedValue(undefined);
      vi.mocked(initPreferences).mockResolvedValue(undefined);

      try {
        await initAuth();
        expect(authState.serverUrl).toBe("https://kaiku.example");
      } finally {
        localStorage.removeItem("serverUrl");
      }
    });

    it("handles no session gracefully", async () => {
      vi.mocked(tauri.getCurrentUser).mockResolvedValue({ user: null, error_reason: null });

      await initAuth();

      expect(authState.user).toBeNull();
      expect(authState.isInitialized).toBe(true);
      expect(authState.error).toBeNull();
      expect(initWebSocket).not.toHaveBeenCalled();
    });

    it("sets error on WS failure but completes", async () => {
      vi.mocked(tauri.getCurrentUser).mockResolvedValue({ user: createUser(), error_reason: null });
      vi.mocked(initWebSocket).mockResolvedValue(undefined);
      vi.mocked(initPresence).mockResolvedValue(undefined);
      vi.mocked(wsConnect).mockRejectedValue(new Error("WS fail"));
      vi.mocked(initPreferences).mockResolvedValue(undefined);

      await initAuth();

      expect(authState.user).not.toBeNull();
      expect(authState.error).toContain(
        "Real-time messaging temporarily unavailable",
      );
    });

    it("skips if already initialized", async () => {
      setAuthState({ isInitialized: true });

      await initAuth();

      expect(tauri.getCurrentUser).not.toHaveBeenCalled();
    });

    it("continues if preferences init fails", async () => {
      vi.mocked(tauri.getCurrentUser).mockResolvedValue({ user: createUser(), error_reason: null });
      vi.mocked(initWebSocket).mockResolvedValue(undefined);
      vi.mocked(initPresence).mockResolvedValue(undefined);
      vi.mocked(wsConnect).mockResolvedValue(undefined);
      vi.mocked(initPreferences).mockRejectedValue(new Error("prefs fail"));

      await initAuth();

      expect(authState.user).not.toBeNull();
      expect(initIdleDetection).toHaveBeenCalled();
    });

    it("handles session restore failure gracefully", async () => {
      vi.mocked(tauri.getCurrentUser).mockRejectedValue(
        new Error("no session"),
      );

      await initAuth();

      expect(authState.user).toBeNull();
      expect(authState.isInitialized).toBe(true);
      expect(authState.error).toBeNull(); // Don't show error for session restoration
    });

    it("shows session expired toast when error_reason is session_expired", async () => {
      vi.mocked(tauri.getCurrentUser).mockResolvedValue({
        user: null,
        error_reason: "session_expired",
      });

      await initAuth();

      expect(authState.user).toBeNull();
      expect(authState.isInitialized).toBe(true);
      expect(showToast).toHaveBeenCalledWith({
        type: "warning",
        title: "Session Expired",
        message: "Please log in again.",
      });
    });

    describe("server unreachable with a stored login (network_error)", () => {
      const offline = { user: null, error_reason: "network_error" } as const;

      beforeEach(() => {
        vi.useFakeTimers();
        vi.mocked(initWebSocket).mockResolvedValue(undefined);
        vi.mocked(initPresence).mockResolvedValue(undefined);
        vi.mocked(wsConnect).mockResolvedValue(undefined);
        vi.mocked(initPreferences).mockResolvedValue(undefined);
      });

      afterEach(() => {
        abandonRestore();
        vi.useRealTimers();
      });

      it("keeps the login and shows the pending state instead of the login form", async () => {
        vi.mocked(tauri.getCurrentUser).mockResolvedValue(offline);

        await initAuth();

        expect(authState.restorePending).toBe(true);
        expect(authState.isInitialized).toBe(false);
        expect(authState.isLoading).toBe(false);
        expect(showToast).not.toHaveBeenCalled();
      });

      it("retries with backoff and signs in once the server answers", async () => {
        const user = createUser();
        vi.mocked(tauri.getCurrentUser)
          .mockResolvedValueOnce(offline)
          .mockResolvedValueOnce(offline)
          .mockResolvedValueOnce({ user, error_reason: null });

        await initAuth();
        await vi.advanceTimersByTimeAsync(restoreRetryDelayMs(1));
        expect(tauri.getCurrentUser).toHaveBeenCalledTimes(2);
        expect(authState.restorePending).toBe(true);

        await vi.advanceTimersByTimeAsync(restoreRetryDelayMs(2));

        expect(tauri.getCurrentUser).toHaveBeenCalledTimes(3);
        expect(authState.user).toEqual(user);
        expect(authState.isInitialized).toBe(true);
        expect(authState.restorePending).toBe(false);
        expect(wsConnect).toHaveBeenCalled();

        // No further attempts once signed in.
        await vi.advanceTimersByTimeAsync(120_000);
        expect(tauri.getCurrentUser).toHaveBeenCalledTimes(3);
      });

      it("ends only when the server rejects the stored login", async () => {
        vi.mocked(tauri.getCurrentUser)
          .mockResolvedValueOnce(offline)
          .mockResolvedValueOnce({ user: null, error_reason: "session_expired" });

        await initAuth();
        await vi.advanceTimersByTimeAsync(restoreRetryDelayMs(1));

        expect(authState.user).toBeNull();
        expect(authState.isInitialized).toBe(true);
        expect(authState.restorePending).toBe(false);
        expect(showToast).toHaveBeenCalledWith({
          type: "warning",
          title: "Session Expired",
          message: "Please log in again.",
        });
      });

      it("coming back online retries at once", async () => {
        vi.mocked(tauri.getCurrentUser)
          .mockResolvedValueOnce(offline)
          .mockResolvedValueOnce({ user: createUser(), error_reason: null });

        await initAuth();
        window.dispatchEvent(new Event("online"));
        await vi.advanceTimersByTimeAsync(0);

        expect(tauri.getCurrentUser).toHaveBeenCalledTimes(2);
        expect(authState.isInitialized).toBe(true);
        expect(authState.user).not.toBeNull();
      });

      it("Try again retries at once; Sign in instead shows the login form and stops retrying", async () => {
        vi.mocked(tauri.getCurrentUser).mockResolvedValue(offline);

        await initAuth();
        retryRestoreNow();
        await vi.advanceTimersByTimeAsync(0);
        expect(tauri.getCurrentUser).toHaveBeenCalledTimes(2);

        abandonRestore();

        expect(authState.restorePending).toBe(false);
        expect(authState.isInitialized).toBe(true);
        expect(authState.user).toBeNull();
        await vi.advanceTimersByTimeAsync(120_000);
        window.dispatchEvent(new Event("online"));
        expect(tauri.getCurrentUser).toHaveBeenCalledTimes(2);
      });

      it.each(["login", "register"] as const)(
        "a successful %s while restore is pending stops the retry timer and listeners",
        async (kind) => {
          const user = createUser();
          vi.mocked(tauri.getCurrentUser).mockResolvedValue(offline);
          const result = { user, setup_required: false, password_change_required: false };
          vi.mocked(tauri.login).mockResolvedValue(result);
          vi.mocked(tauri.register).mockResolvedValue(result);

          await initAuth();
          expect(authState.restorePending).toBe(true);
          const calls = vi.mocked(tauri.getCurrentUser).mock.calls.length;

          if (kind === "login") await login("https://server.com", "a", "p");
          else await register("https://server.com", "a", "p");

          expect(authState.restorePending).toBe(false);
          await vi.advanceTimersByTimeAsync(120_000);
          window.dispatchEvent(new Event("online"));
          document.dispatchEvent(new Event("visibilitychange"));
          await vi.advanceTimersByTimeAsync(0);
          expect(tauri.getCurrentUser).toHaveBeenCalledTimes(calls);
          expect(vi.getTimerCount()).toBe(0);
        },
      );

      it("backoff doubles from 5 s and is capped at 30 s", () => {
        expect([1, 2, 3, 4, 5, 6].map(restoreRetryDelayMs)).toEqual([
          5_000, 10_000, 20_000, 30_000, 30_000, 30_000,
        ]);
      });
    });
  });

  describe("login", () => {
    it("logs in and inits subsystems", async () => {
      const user = createUser();
      vi.mocked(tauri.login).mockResolvedValue({ user, setup_required: false, password_change_required: false });
      vi.mocked(initWebSocket).mockResolvedValue(undefined);
      vi.mocked(initPresence).mockResolvedValue(undefined);
      vi.mocked(wsConnect).mockResolvedValue(undefined);
      vi.mocked(initPreferences).mockResolvedValue(undefined);

      const result = await login("https://server.com", "alice", "password");

      expect(result.id).toBe(user.id);
      expect(authState.user).toEqual(user);
      expect(authState.serverUrl).toBe("https://server.com");
      expect(authState.setupRequired).toBe(false);
      expect(initWebSocket).toHaveBeenCalled();
    });

    it("sets setupRequired from response", async () => {
      vi.mocked(tauri.login).mockResolvedValue({
        user: createUser(),
        setup_required: true,
        password_change_required: false,
      });
      vi.mocked(initWebSocket).mockResolvedValue(undefined);
      vi.mocked(initPresence).mockResolvedValue(undefined);
      vi.mocked(wsConnect).mockResolvedValue(undefined);
      vi.mocked(initPreferences).mockResolvedValue(undefined);

      await login("https://server.com", "alice", "password");

      expect(authState.setupRequired).toBe(true);
    });

    it("sets error and re-throws on failure", async () => {
      vi.mocked(tauri.login).mockRejectedValue(
        new HttpError(401, "Invalid credentials", "INVALID_CREDENTIALS"),
      );

      await expect(
        login("https://server.com", "alice", "wrong"),
      ).rejects.toThrow("Invalid credentials");
      expect(authState.error).toBe("Invalid username or password.");
      expect(authState.isLoading).toBe(false);
    });
  });

  describe("login with a suspended account", () => {
    it("says the account is suspended", async () => {
      vi.mocked(tauri.login).mockRejectedValue(
        new HttpError(403, "Account is banned", "ACCOUNT_BANNED"),
      );

      await expect(login("https://server.com", "alice", "pw")).rejects.toThrow();
      expect(authState.error).toBe("This account has been suspended. Ask the server admin.");
    });
  });

  describe("register", () => {
    it("registers and inits subsystems", async () => {
      const user = createUser();
      vi.mocked(tauri.register).mockResolvedValue({
        user,
        setup_required: false,
        password_change_required: false,
      });
      vi.mocked(initWebSocket).mockResolvedValue(undefined);
      vi.mocked(initPresence).mockResolvedValue(undefined);
      vi.mocked(wsConnect).mockResolvedValue(undefined);
      vi.mocked(initPreferences).mockResolvedValue(undefined);

      const result = await register("https://server.com", "alice", "password");

      expect(result.id).toBe(user.id);
      expect(authState.user).toEqual(user);
    });

    it("passes setupRequired flag", async () => {
      vi.mocked(tauri.register).mockResolvedValue({
        user: createUser(),
        setup_required: true,
        password_change_required: false,
      });
      vi.mocked(initWebSocket).mockResolvedValue(undefined);
      vi.mocked(initPresence).mockResolvedValue(undefined);
      vi.mocked(wsConnect).mockResolvedValue(undefined);
      vi.mocked(initPreferences).mockResolvedValue(undefined);

      await register("https://server.com", "alice", "password");

      expect(authState.setupRequired).toBe(true);
    });

    it("sets error and re-throws on failure", async () => {
      vi.mocked(tauri.register).mockRejectedValue(
        new HttpError(409, "Username taken", "USER_EXISTS"),
      );

      await expect(
        register("https://server.com", "alice", "password"),
      ).rejects.toThrow("Username taken");
      expect(authState.error).toBe("That username is already taken.");
    });
  });

  describe("loginWithOidc", () => {
    it("completes OIDC login", async () => {
      const user = createUser();
      vi.mocked(tauri.oidcCompleteLogin).mockResolvedValue(undefined);
      vi.mocked(tauri.getCurrentUser).mockResolvedValue({ user, error_reason: null });
      vi.mocked(initWebSocket).mockResolvedValue(undefined);
      vi.mocked(initPresence).mockResolvedValue(undefined);
      vi.mocked(wsConnect).mockResolvedValue(undefined);
      vi.mocked(initPreferences).mockResolvedValue(undefined);

      await loginWithOidc("https://server.com", "token", "refresh", 3600);

      expect(authState.user).toEqual(user);
    });

    it("throws if getCurrentUser returns null", async () => {
      vi.mocked(tauri.oidcCompleteLogin).mockResolvedValue(undefined);
      vi.mocked(tauri.getCurrentUser).mockResolvedValue({ user: null, error_reason: null });

      await expect(
        loginWithOidc("https://server.com", "token", "refresh", 3600),
      ).rejects.toThrow("Failed to fetch user after OIDC login");
    });
  });

  describe("restoreSessionAfterPopup", () => {
    const servicesOk = () => {
      vi.mocked(initWebSocket).mockResolvedValue(undefined);
      vi.mocked(initPresence).mockResolvedValue(undefined);
      vi.mocked(wsConnect).mockResolvedValue(undefined);
      vi.mocked(initPreferences).mockResolvedValue(undefined);
    };
    const sessionA = { accessToken: "token-a", expiresIn: 900 };
    const meA = () => ({ user: createUser(), password_change_required: false });
    function deferred<T>() {
      let resolve!: (v: T) => void;
      const promise = new Promise<T>((r) => (resolve = r));
      return { promise, resolve };
    }
    beforeEach(() => {
      localStorage.setItem("serverUrl", "https://kaiku.example");
      vi.mocked(tauri.refreshDetached).mockResolvedValue(sessionA);
      vi.mocked(tauri.getCurrentUserWithToken).mockResolvedValue(meA());
      vi.mocked(tauri.getSetupStatus).mockResolvedValue({ setup_complete: true });
      vi.mocked(tauri.oidcCompleteLogin).mockResolvedValue(undefined);
      vi.mocked(tauri.revokeDetachedSession).mockResolvedValue(undefined);
    });
    afterEach(() => {
      setSessionRestoreBlocked(false);
      localStorage.removeItem("serverUrl");
    });

    it("restores an explicit SSO sign-in despite the logout block, then clears it", async () => {
      setSessionRestoreBlocked(true);
      servicesOk();

      await expect(restoreSessionAfterPopup(new AbortController().signal)).resolves.toBe(true);

      expect(tauri.getCurrentUserWithToken).toHaveBeenCalledWith("token-a");
      expect(tauri.oidcCompleteLogin).toHaveBeenCalledWith("https://kaiku.example", "token-a", 900);
      expect(authState.user?.id).toBe("user-1");
      expect(authState.setupRequired).toBe(false);
      expect(isSessionRestoreBlocked()).toBe(false);
      expect(initWebSocket).toHaveBeenCalled();
    });

    it("keeps the logout block when there is no session yet", async () => {
      setSessionRestoreBlocked(true);
      vi.mocked(tauri.refreshDetached).mockResolvedValue(null);

      await expect(restoreSessionAfterPopup(new AbortController().signal)).resolves.toBe(false);

      expect(isSessionRestoreBlocked()).toBe(true);
      expect(authState.user).toBeNull();
      expect(tauri.oidcCompleteLogin).not.toHaveBeenCalled();
    });

    it("shows the setup wizard when the server is not set up yet", async () => {
      servicesOk();
      vi.mocked(tauri.getSetupStatus).mockResolvedValue({ setup_complete: false });

      await expect(restoreSessionAfterPopup(new AbortController().signal)).resolves.toBe(true);

      expect(authState.setupRequired).toBe(true);
    });

    it("applies nothing when the setup status cannot be read (retried later)", async () => {
      vi.mocked(tauri.getSetupStatus).mockRejectedValue(new Error("offline"));

      await expect(restoreSessionAfterPopup(new AbortController().signal)).resolves.toBe(false);

      expect(authState.user).toBeNull();
      expect(tauri.oidcCompleteLogin).not.toHaveBeenCalled();
    });

    it("cancel during the refresh installs no token and applies nothing", async () => {
      setSessionRestoreBlocked(true);
      const refresh = deferred<typeof sessionA | null>();
      vi.mocked(tauri.refreshDetached).mockReturnValue(refresh.promise);
      const ctrl = new AbortController();

      const pending = restoreSessionAfterPopup(ctrl.signal);
      ctrl.abort();
      refresh.resolve(sessionA);

      await expect(pending).resolves.toBe(false);
      expect(tauri.oidcCompleteLogin).not.toHaveBeenCalled();
      expect(tauri.revokeDetachedSession).not.toHaveBeenCalled();
      expect(authState.user).toBeNull();
      expect(isSessionRestoreBlocked()).toBe(true);
      expect(initWebSocket).not.toHaveBeenCalled();
    });

    it("cancel, sign in as B, late refresh for A: B keeps its session, A's is revoked", async () => {
      const refresh = deferred<typeof sessionA | null>();
      vi.mocked(tauri.refreshDetached).mockReturnValue(refresh.promise);
      const ctrl = new AbortController();

      const pending = restoreSessionAfterPopup(ctrl.signal);
      ctrl.abort();
      const bob = createUser({ id: "user-2", username: "bob" });
      setAuthState({ user: bob });
      refresh.resolve(sessionA);

      await expect(pending).resolves.toBe(false);
      expect(tauri.oidcCompleteLogin).not.toHaveBeenCalled();
      expect(tauri.getCurrentUserWithToken).not.toHaveBeenCalled();
      // A's late response replaced B's refresh cookie: end A's session
      // rather than let B's next refresh silently switch to A.
      expect(tauri.revokeDetachedSession).toHaveBeenCalledWith("token-a");
      expect(authState.user?.id).toBe("user-2");
    });

    it("a sign-in after the refresh answered is not overwritten and keeps its cookie", async () => {
      const me = deferred<ReturnType<typeof meA>>();
      vi.mocked(tauri.getCurrentUserWithToken).mockReturnValue(me.promise);

      const pending = restoreSessionAfterPopup(new AbortController().signal);
      await tick();
      setAuthState({ user: createUser({ id: "user-2", username: "bob" }) });
      me.resolve(meA());

      await expect(pending).resolves.toBe(false);
      expect(authState.user?.id).toBe("user-2");
      expect(tauri.oidcCompleteLogin).not.toHaveBeenCalled();
      expect(tauri.revokeDetachedSession).not.toHaveBeenCalled();
      expect(initWebSocket).not.toHaveBeenCalled();
    });

    it("applies nothing when cancelled while the setup status is in flight", async () => {
      const status = deferred<{ setup_complete: boolean }>();
      vi.mocked(tauri.getSetupStatus).mockReturnValue(status.promise);
      const ctrl = new AbortController();

      const pending = restoreSessionAfterPopup(ctrl.signal);
      await tick();
      ctrl.abort();
      status.resolve({ setup_complete: true });

      await expect(pending).resolves.toBe(false);
      expect(authState.user).toBeNull();
      expect(tauri.oidcCompleteLogin).not.toHaveBeenCalled();
    });

    it("does not even ask the server when already cancelled", async () => {
      const ctrl = new AbortController();
      ctrl.abort();
      await expect(restoreSessionAfterPopup(ctrl.signal)).resolves.toBe(false);
      expect(tauri.refreshDetached).not.toHaveBeenCalled();
    });
  });

  describe("logout", () => {
    it("runs cleanup chain and clears state", async () => {
      setAuthState({ user: createUser() });
      vi.mocked(wsDisconnect).mockResolvedValue(undefined);
      vi.mocked(cleanupWebSocket).mockResolvedValue(undefined);
      vi.mocked(tauri.logout).mockResolvedValue(undefined);

      await logout();

      expect(authState.user).toBeNull();
      expect(authState.isLoading).toBe(false);
      expect(wsDisconnect).toHaveBeenCalled();
      expect(cleanupWebSocket).toHaveBeenCalled();
      expect(stopIdleDetectionCleanup).toHaveBeenCalled();
      expect(cleanupPresence).toHaveBeenCalled();
      expect(clearAllDrafts).toHaveBeenCalled();
      expect(cleanupDrafts).toHaveBeenCalled();
    });

    it("clears state even if server logout fails", async () => {
      setAuthState({ user: createUser() });
      vi.mocked(wsDisconnect).mockResolvedValue(undefined);
      vi.mocked(cleanupWebSocket).mockResolvedValue(undefined);
      vi.mocked(tauri.logout).mockRejectedValue(new Error("Server error"));

      await logout();

      expect(authState.user).toBeNull();
    });

    it("clears state even if cleanup fails", async () => {
      setAuthState({ user: createUser() });
      vi.mocked(wsDisconnect).mockRejectedValue(new Error("cleanup fail"));
      vi.mocked(tauri.logout).mockResolvedValue(undefined);

      await logout();

      expect(authState.user).toBeNull();
    });
  });

  describe("clearError", () => {
    it("clears error", () => {
      setAuthState({ error: "some error" });

      clearError();

      expect(authState.error).toBeNull();
    });
  });

  describe("updateUser", () => {
    it("merges updates into user", () => {
      setAuthState({ user: createUser() });

      updateUser({ display_name: "Alice Updated" });

      expect(authState.user?.display_name).toBe("Alice Updated");
    });

    it("no-ops if user is null", () => {
      setAuthState({ user: null });

      updateUser({ display_name: "Alice" });

      expect(authState.user).toBeNull();
    });
  });

  describe("clearSetupRequired", () => {
    it("sets setupRequired to false", () => {
      setAuthState({ setupRequired: true });

      clearSetupRequired();

      expect(authState.setupRequired).toBe(false);
    });
  });

  describe("session expired handling", () => {
    it("retries refresh on kaiku:session-expired and recovers", async () => {
      setAuthState({ user: createUser(), isInitialized: true });
      vi.mocked(tauri.refreshAccessToken).mockResolvedValue(true);

      window.dispatchEvent(new CustomEvent("kaiku:session-expired"));

      await vi.waitFor(() => {
        expect(tauri.refreshAccessToken).toHaveBeenCalledTimes(1);
      });

      expect(authState.sessionExpired).toBe(false);
      expect(authState.user).not.toBeNull();
    });

    it("restarts the WebSocket after a silent session recovery", async () => {
      setAuthState({ user: createUser(), isInitialized: true });
      vi.mocked(tauri.refreshAccessToken).mockResolvedValue(true);
      vi.mocked(wsConnect).mockResolvedValue(undefined);

      window.dispatchEvent(new CustomEvent("kaiku:session-expired"));

      await vi.waitFor(() => {
        expect(wsConnect).toHaveBeenCalledTimes(1);
      });
      expect(authState.sessionExpired).toBe(false);
    });

    it("sets sessionExpired and cleans up when retry fails", async () => {
      setAuthState({ user: createUser(), isInitialized: true });
      vi.mocked(tauri.refreshAccessToken).mockResolvedValue(false);
      vi.mocked(wsDisconnect).mockResolvedValue(undefined);
      vi.mocked(cleanupWebSocket).mockResolvedValue(undefined);

      window.dispatchEvent(new CustomEvent("kaiku:session-expired"));

      await vi.waitFor(() => {
        expect(authState.sessionExpired).toBe(true);
      });

      expect(authState.user).toBeNull();
      expect(wsDisconnect).toHaveBeenCalled();
      expect(cleanupWebSocket).toHaveBeenCalled();
      expect(stopIdleDetectionCleanup).toHaveBeenCalled();
      expect(cleanupPresence).toHaveBeenCalled();
    });

    it("sets sessionExpired even when cleanup throws", async () => {
      setAuthState({ user: createUser(), isInitialized: true });
      vi.mocked(tauri.refreshAccessToken).mockResolvedValue(false);
      vi.mocked(wsDisconnect).mockRejectedValue(new Error("cleanup fail"));

      window.dispatchEvent(new CustomEvent("kaiku:session-expired"));

      await vi.waitFor(() => {
        expect(authState.sessionExpired).toBe(true);
      });

      expect(authState.user).toBeNull();
    });

    it.each(["revoked", "banned"] as const)(
      "kaiku:session-ended (%s) signs out without a silent refresh",
      async (reason) => {
        setAuthState({ user: createUser(), isInitialized: true });
        vi.mocked(wsDisconnect).mockResolvedValue(undefined);
        vi.mocked(cleanupWebSocket).mockResolvedValue(undefined);

        window.dispatchEvent(
          new CustomEvent("kaiku:session-ended", { detail: { reason } }),
        );

        await vi.waitFor(() => {
          expect(authState.sessionExpired).toBe(true);
        });
        expect(authState.sessionEndReason).toBe(reason);
        expect(authState.user).toBeNull();
        expect(tauri.refreshAccessToken).not.toHaveBeenCalled();
      },
    );

    it("ignores event when not authenticated", async () => {
      setAuthState({ user: null, isInitialized: true });

      window.dispatchEvent(new CustomEvent("kaiku:session-expired"));

      // Give time for any async handler
      await new Promise((r) => setTimeout(r, 50));

      expect(tauri.refreshAccessToken).not.toHaveBeenCalled();
      expect(authState.sessionExpired).toBe(false);
    });
  });
  describe("forced password change", () => {
    function mockServices() {
      vi.mocked(initWebSocket).mockResolvedValue(undefined);
      vi.mocked(initPresence).mockResolvedValue(undefined);
      vi.mocked(wsConnect).mockResolvedValue(undefined);
      vi.mocked(initPreferences).mockResolvedValue(undefined);
      vi.mocked(wsDisconnect).mockResolvedValue(undefined);
      vi.mocked(cleanupWebSocket).mockResolvedValue(undefined);
    }

    async function loginPending() {
      vi.mocked(tauri.login).mockResolvedValue({
        user: createUser(),
        setup_required: false,
        password_change_required: true,
      });
      await login("https://server.com", "alice", "temp-pw");
    }

    afterEach(async () => {
      // Clears the remembered temporary password between tests.
      vi.mocked(tauri.logout).mockResolvedValue(undefined);
      await logout();
    });

    it("login with password_change_required sets the flag and does not connect the websocket", async () => {
      mockServices();

      await loginPending();

      expect(authState.user).not.toBeNull();
      expect(authState.passwordChangeRequired).toBe(true);
      expect(hasPendingTemporaryPassword()).toBe(true);
      expect(initWebSocket).not.toHaveBeenCalled();
      expect(wsConnect).not.toHaveBeenCalled();
      expect(initPresence).not.toHaveBeenCalled();
      expect(initPreferences).not.toHaveBeenCalled();
      expect(initIdleDetection).not.toHaveBeenCalled();
    });

    it("completeForcedPasswordChange uses the remembered temporary password, refreshes, clears the flag and connects", async () => {
      mockServices();
      await loginPending();
      vi.mocked(tauri.updatePassword).mockResolvedValue({
        success: true,
        message: "ok",
        revoked_count: 0,
      });
      vi.mocked(tauri.refreshAccessToken).mockResolvedValue(true);

      await completeForcedPasswordChange(null, "new-pw-123");

      expect(tauri.updatePassword).toHaveBeenCalledWith("temp-pw", "new-pw-123", true);
      expect(tauri.refreshAccessToken).toHaveBeenCalled();
      expect(authState.passwordChangeRequired).toBe(false);
      expect(hasPendingTemporaryPassword()).toBe(false);
      expect(wsConnect).toHaveBeenCalled();
      expect(initIdleDetection).toHaveBeenCalled();
    });

    it("a failed change keeps the flag and the remembered password", async () => {
      mockServices();
      await loginPending();
      vi.mocked(tauri.updatePassword).mockRejectedValue(new Error("Server error (500)"));

      await expect(completeForcedPasswordChange(null, "new-pw-123")).rejects.toThrow();

      expect(authState.passwordChangeRequired).toBe(true);
      expect(hasPendingTemporaryPassword()).toBe(true);
      expect(wsConnect).not.toHaveBeenCalled();
    });

    it("an explicit current password wins over the remembered one", async () => {
      mockServices();
      await loginPending();
      vi.mocked(tauri.updatePassword).mockResolvedValue({
        success: true,
        message: "ok",
        revoked_count: 0,
      });
      vi.mocked(tauri.refreshAccessToken).mockResolvedValue(true);

      await completeForcedPasswordChange("typed-pw", "new-pw-123");

      expect(tauri.updatePassword).toHaveBeenCalledWith("typed-pw", "new-pw-123", true);
    });

    it("a restored session that still needs a password change goes to the change screen", async () => {
      mockServices();
      vi.mocked(tauri.getCurrentUser).mockResolvedValue({
        user: createUser(),
        error_reason: null,
        password_change_required: true,
      });

      await initAuth();

      expect(authState.isInitialized).toBe(true);
      expect(authState.passwordChangeRequired).toBe(true);
      expect(hasPendingTemporaryPassword()).toBe(false);
      expect(wsConnect).not.toHaveBeenCalled();
    });

    it("PASSWORD_CHANGE_REQUIRED from any request switches to the change screen", async () => {
      mockServices();
      // Restored session whose restore response did not carry the flag.
      vi.mocked(tauri.getCurrentUser).mockResolvedValue({
        user: createUser(),
        error_reason: null,
      });
      await initAuth();
      expect(authState.passwordChangeRequired).toBe(false);

      window.dispatchEvent(new CustomEvent("kaiku:password-change-required"));

      await vi.waitFor(() => {
        expect(authState.passwordChangeRequired).toBe(true);
      });
      // No temporary password remembered: the screen asks for it.
      expect(hasPendingTemporaryPassword()).toBe(false);
      expect(authState.user).not.toBeNull();
      expect(authState.sessionExpired).toBe(false);
      // Services that would only collect 403s are stopped.
      await vi.waitFor(() => {
        expect(cleanupPresence).toHaveBeenCalled();
      });
      expect(wsDisconnect).toHaveBeenCalled();
      expect(stopIdleDetectionCleanup).toHaveBeenCalled();
    });

    it("a password-change event during service startup leaves nothing started", async () => {
      mockServices();
      vi.mocked(tauri.getCurrentUser).mockResolvedValue({
        user: createUser(),
        error_reason: null,
      });
      // The event arrives while the first startup step is still running.
      vi.mocked(initWebSocket).mockImplementationOnce(async () => {
        window.dispatchEvent(new CustomEvent("kaiku:password-change-required"));
      });

      await initAuth();
      await new Promise((r) => setTimeout(r, 0));

      expect(authState.passwordChangeRequired).toBe(true);
      expect(wsConnect).not.toHaveBeenCalled();
      expect(initPreferences).not.toHaveBeenCalled();
      expect(initIdleDetection).not.toHaveBeenCalled();
      // What the first step started (listeners, presence) is undone.
      expect(cleanupWebSocket).toHaveBeenCalled();
      expect(cleanupPresence).toHaveBeenCalled();
    });

    it("a password-change event while the socket connects tears the socket down afterwards", async () => {
      mockServices();
      vi.mocked(tauri.getCurrentUser).mockResolvedValue({
        user: createUser(),
        error_reason: null,
      });
      let finishConnect!: () => void;
      vi.mocked(wsConnect).mockImplementationOnce(
        () => new Promise<void>((resolve) => (finishConnect = resolve)),
      );
      vi.mocked(initPreferences).mockImplementationOnce(async () => {
        window.dispatchEvent(new CustomEvent("kaiku:password-change-required"));
      });

      const restoring = initAuth();
      await vi.waitFor(() => expect(wsConnect).toHaveBeenCalled());
      await new Promise((r) => setTimeout(r, 0));
      // The connect completes after the event's cleanup already ran.
      finishConnect();
      await restoring;
      await new Promise((r) => setTimeout(r, 0));

      expect(authState.passwordChangeRequired).toBe(true);
      expect(initIdleDetection).not.toHaveBeenCalled();
      const lastDisconnect = Math.max(...vi.mocked(wsDisconnect).mock.invocationCallOrder);
      const connect = vi.mocked(wsConnect).mock.invocationCallOrder[0];
      expect(lastDisconnect).toBeGreaterThan(connect);
    });

    it("a cancelled startup whose WebSocket init rejects still tears down presence that finishes later", async () => {
      mockServices();
      vi.mocked(tauri.getCurrentUser).mockResolvedValue({
        user: createUser(),
        error_reason: null,
      });
      let finishPresence!: () => void;
      vi.mocked(initPresence).mockImplementationOnce(
        () => new Promise<void>((resolve) => (finishPresence = resolve)),
      );
      vi.mocked(initWebSocket).mockImplementationOnce(async () => {
        window.dispatchEvent(new CustomEvent("kaiku:password-change-required"));
        throw new Error("ws init failed");
      });

      const restoring = initAuth();
      await vi.waitFor(() => expect(authState.passwordChangeRequired).toBe(true));
      await new Promise((r) => setTimeout(r, 0));
      // Presence finishes starting only after the WebSocket init failed.
      finishPresence();
      await restoring;
      await vi.waitFor(() => expect(cleanupPresence).toHaveBeenCalled());

      const presenceStarted = vi.mocked(initPresence).mock.invocationCallOrder[0];
      const lastPresenceCleanup = Math.max(
        ...vi.mocked(cleanupPresence).mock.invocationCallOrder,
      );
      expect(lastPresenceCleanup).toBeGreaterThan(presenceStarted);
      expect(wsConnect).not.toHaveBeenCalled();
      expect(initIdleDetection).not.toHaveBeenCalled();
    });

    it("a cancelled old startup that settles late leaves the replacement session running", async () => {
      mockServices();
      vi.mocked(tauri.getCurrentUser).mockResolvedValue({
        user: createUser(),
        error_reason: null,
      });
      let finishOldInit!: () => void;
      vi.mocked(initWebSocket).mockImplementationOnce(
        () => new Promise<void>((resolve) => (finishOldInit = resolve)),
      );
      vi.mocked(tauri.updatePassword).mockResolvedValue({
        success: true,
        message: "ok",
        revoked_count: 0,
      });
      vi.mocked(tauri.refreshAccessToken).mockResolvedValue(true);

      // Startup A suspends in its first step and is cancelled (stop T1).
      const restoring = initAuth();
      await vi.waitFor(() => expect(initWebSocket).toHaveBeenCalledTimes(1));
      window.dispatchEvent(new CustomEvent("kaiku:password-change-required"));
      // Startup B is requested while A still owns the services.
      const changing = completeForcedPasswordChange("temp-pw", "new-pw-123");
      await vi.waitFor(() => expect(tauri.refreshAccessToken).toHaveBeenCalled());
      await new Promise((r) => setTimeout(r, 0));

      // A settles late, then B runs.
      finishOldInit();
      await restoring;
      await changing;
      await new Promise((r) => setTimeout(r, 0));

      // B is fully started and nothing tore it down afterwards.
      expect(initWebSocket).toHaveBeenCalledTimes(2);
      expect(wsConnect).toHaveBeenCalledTimes(1);
      expect(initIdleDetection).toHaveBeenCalledTimes(1);
      const bStarted = vi.mocked(initWebSocket).mock.invocationCallOrder[1];
      const lastTeardown = Math.max(
        ...vi.mocked(wsDisconnect).mock.invocationCallOrder,
        ...vi.mocked(cleanupPresence).mock.invocationCallOrder,
        ...vi.mocked(stopIdleDetectionCleanup).mock.invocationCallOrder,
      );
      expect(lastTeardown).toBeLessThan(bStarted);
      expect(authState.passwordChangeRequired).toBe(false);
    });

    it("a teardown still running when the replacement startup is requested cannot hit the replacement", async () => {
      mockServices();
      vi.mocked(tauri.getCurrentUser).mockResolvedValue({
        user: createUser(),
        error_reason: null,
      });
      let finishOldInit!: () => void;
      vi.mocked(initWebSocket).mockImplementationOnce(
        () => new Promise<void>((resolve) => (finishOldInit = resolve)),
      );
      // The stop's first disconnect is slow.
      let finishDisconnect!: () => void;
      vi.mocked(wsDisconnect).mockImplementationOnce(
        () => new Promise<void>((resolve) => (finishDisconnect = resolve)),
      );
      vi.mocked(tauri.updatePassword).mockResolvedValue({
        success: true,
        message: "ok",
        revoked_count: 0,
      });
      vi.mocked(tauri.refreshAccessToken).mockResolvedValue(true);

      const restoring = initAuth();
      await vi.waitFor(() => expect(initWebSocket).toHaveBeenCalledTimes(1));
      // Stop T1 (may wait behind A), then startup B is requested.
      window.dispatchEvent(new CustomEvent("kaiku:password-change-required"));
      const changing = completeForcedPasswordChange("temp-pw", "new-pw-123");
      await vi.waitFor(() => expect(tauri.refreshAccessToken).toHaveBeenCalled());
      await new Promise((r) => setTimeout(r, 0));

      // A notices the cancellation while the slow teardown still runs.
      finishOldInit();
      await new Promise((r) => setTimeout(r, 0));
      await vi.waitFor(() => expect(finishDisconnect).toBeTypeOf("function"));
      finishDisconnect();
      await restoring;
      await changing;
      await new Promise((r) => setTimeout(r, 10));

      expect(initWebSocket).toHaveBeenCalledTimes(2);
      expect(wsConnect).toHaveBeenCalledTimes(1);
      expect(initIdleDetection).toHaveBeenCalledTimes(1);
      const bStarted = vi.mocked(initWebSocket).mock.invocationCallOrder[1];
      const lastTeardown = Math.max(
        ...vi.mocked(wsDisconnect).mock.invocationCallOrder,
        ...vi.mocked(cleanupPresence).mock.invocationCallOrder,
        ...vi.mocked(stopIdleDetectionCleanup).mock.invocationCallOrder,
      );
      expect(lastTeardown).toBeLessThan(bStarted);
    });

    it("a sign-in while an older startup still hangs starts at once; a stop tears it down and the old startup's late settle changes nothing", async () => {
      mockServices();
      vi.mocked(tauri.getCurrentUser).mockResolvedValue({
        user: createUser(),
        error_reason: null,
      });
      const releaseOldInit = holdOnce(vi.mocked(initWebSocket));
      vi.mocked(tauri.login).mockResolvedValue({
        user: createUser(),
        setup_required: false,
        password_change_required: false,
      });

      // Startup A hangs in its first step; the sign-in's startup C does not
      // wait for it (the stores cancel A's late effects on their own).
      const restoring = initAuth();
      await vi.waitFor(() => expect(initWebSocket).toHaveBeenCalledTimes(1));
      await withinMs(login("https://server.com", "alice", "pw"));
      expect(initWebSocket).toHaveBeenCalledTimes(2);
      expect(wsConnect).toHaveBeenCalledTimes(1);
      expect(initIdleDetection).toHaveBeenCalledTimes(1);

      // A stop arrives: C's services are torn down.
      window.dispatchEvent(new CustomEvent("kaiku:password-change-required"));
      await vi.waitFor(() => expect(cleanupPresence).toHaveBeenCalled());
      expect(wsDisconnect).toHaveBeenCalled();
      expect(cleanupWebSocket).toHaveBeenCalled();
      expect(stopIdleDetectionCleanup).toHaveBeenCalled();
      const teardownCalls = () =>
        vi.mocked(wsDisconnect).mock.calls.length +
        vi.mocked(cleanupWebSocket).mock.calls.length +
        vi.mocked(cleanupPresence).mock.calls.length +
        vi.mocked(stopIdleDetectionCleanup).mock.calls.length;
      const teardownsAfterStop = teardownCalls();

      // A settles late: a newer start began since, so A owns nothing, starts
      // nothing further and tears nothing down.
      releaseOldInit();
      await withinMs(restoring);
      await tick();

      expect(authState.passwordChangeRequired).toBe(true);
      expect(wsConnect).toHaveBeenCalledTimes(1);
      expect(initIdleDetection).toHaveBeenCalledTimes(1);
      expect(teardownCalls()).toBe(teardownsAfterStop);
    });

    it("a transient refresh failure after the change keeps it saved and retries without resubmitting", async () => {
      mockServices();
      await loginPending();
      vi.mocked(tauri.updatePassword).mockResolvedValue({
        success: true,
        message: "ok",
        revoked_count: 0,
      });
      vi.mocked(tauri.refreshAccessToken).mockResolvedValueOnce(false);

      await expect(completeForcedPasswordChange(null, "new-pw-123")).rejects.toThrow(
        PASSWORD_SAVED_REFRESH_FAILED,
      );

      expect(isForcedPasswordChangeSaved()).toBe(true);
      expect(hasPendingTemporaryPassword()).toBe(false);
      expect(wsConnect).not.toHaveBeenCalled();
      expect(authState.user).not.toBeNull();

      vi.mocked(tauri.refreshAccessToken).mockResolvedValueOnce(true);
      await retryForcedPasswordChangeFinish();

      expect(tauri.updatePassword).toHaveBeenCalledTimes(1);
      expect(authState.passwordChangeRequired).toBe(false);
      expect(isForcedPasswordChangeSaved()).toBe(false);
      expect(wsConnect).toHaveBeenCalled();
    });

    it("a rejected refresh after the change ends the session instead of starting services", async () => {
      mockServices();
      await loginPending();
      vi.mocked(tauri.updatePassword).mockResolvedValue({
        success: true,
        message: "ok",
        revoked_count: 0,
      });
      vi.mocked(tauri.refreshAccessToken).mockResolvedValue(false);
      // A rejected refresh leaves no token behind.
      vi.mocked(tauri.getAccessToken).mockReturnValue(null);

      try {
        await completeForcedPasswordChange(null, "new-pw-123");

        await vi.waitFor(() => {
          expect(authState.sessionExpired).toBe(true);
        });
        expect(authState.user).toBeNull();
        expect(wsConnect).not.toHaveBeenCalled();
        expect(initIdleDetection).not.toHaveBeenCalled();
        expect(isForcedPasswordChangeSaved()).toBe(false);
      } finally {
        vi.mocked(tauri.getAccessToken).mockReturnValue("token");
      }
    });

    it("a rejected remembered temporary password is dropped so the screen asks for it", async () => {
      mockServices();
      await loginPending();
      vi.mocked(tauri.updatePassword).mockRejectedValue(
        Object.assign(new Error("Invalid credentials"), { status: 401 }),
      );

      await expect(completeForcedPasswordChange(null, "new-pw-123")).rejects.toThrow(
        "Invalid credentials",
      );

      expect(hasPendingTemporaryPassword()).toBe(false);
      expect(authState.passwordChangeRequired).toBe(true);
    });

    it("the event is ignored while signed out", async () => {
      window.dispatchEvent(new CustomEvent("kaiku:password-change-required"));
      await new Promise((r) => setTimeout(r, 0));

      expect(authState.passwordChangeRequired).toBe(false);
    });

    it("logout clears the remembered temporary password", async () => {
      mockServices();
      await loginPending();
      expect(hasPendingTemporaryPassword()).toBe(true);
      vi.mocked(tauri.logout).mockResolvedValue(undefined);

      await logout();

      expect(hasPendingTemporaryPassword()).toBe(false);
      expect(authState.passwordChangeRequired).toBe(false);
    });

    describe("a startup step that never settles", () => {
      // Each step of the start job, held pending for as long as the test wants.
      const heldSteps = [
        ["initWebSocket", () => vi.mocked(initWebSocket)],
        ["wsConnect", () => vi.mocked(wsConnect)],
        ["initPreferences", () => vi.mocked(initPreferences)],
      ] as const;

      function restoreWith(user = createUser()) {
        vi.mocked(tauri.getCurrentUser).mockResolvedValue({ user, error_reason: null });
        vi.mocked(tauri.logout).mockResolvedValue(undefined);
      }

      it.each(heldSteps)(
        "logout completes while %s is pending; releasing it later leaves nothing running",
        async (name, step) => {
          mockServices();
          restoreWith();
          const release = holdOnce(step());

          const restoring = initAuth();
          await vi.waitFor(() => expect(step()).toHaveBeenCalledTimes(1));

          await withinMs(logout());

          expect(tauri.logout).toHaveBeenCalledTimes(1);
          expect(authState.user).toBeNull();
          expect(authState.isLoading).toBe(false);
          expect(wsDisconnect).toHaveBeenCalled();
          expect(cleanupWebSocket).toHaveBeenCalled();
          expect(stopIdleDetectionCleanup).toHaveBeenCalled();
          expect(cleanupPresence).toHaveBeenCalled();

          // The held step settles much later: the superseded start undoes its
          // own late pieces and starts nothing further.
          const disconnectsBefore = vi.mocked(wsDisconnect).mock.calls.length;
          const cleanupsBefore = vi.mocked(cleanupWebSocket).mock.calls.length;
          release();
          await withinMs(restoring);
          await tick();

          expect(wsConnect).toHaveBeenCalledTimes(name === "initWebSocket" ? 0 : 1);
          expect(initIdleDetection).not.toHaveBeenCalled();
          expect(vi.mocked(wsDisconnect).mock.calls.length).toBeGreaterThan(disconnectsBefore);
          expect(vi.mocked(cleanupWebSocket).mock.calls.length).toBeGreaterThan(cleanupsBefore);
        },
      );

      it("Sign out on the change screen completes while the interrupted startup is still pending", async () => {
        mockServices();
        restoreWith();
        const release = holdOnce(vi.mocked(wsConnect));

        const restoring = initAuth();
        await vi.waitFor(() => expect(wsConnect).toHaveBeenCalledTimes(1));
        // The change screen appears while the connect hangs...
        window.dispatchEvent(new CustomEvent("kaiku:password-change-required"));
        expect(authState.passwordChangeRequired).toBe(true);

        // ...and "Sign out" on it must finish.
        await withinMs(logout());

        expect(tauri.logout).toHaveBeenCalledTimes(1);
        expect(authState.user).toBeNull();
        expect(authState.isLoading).toBe(false);
        expect(authState.passwordChangeRequired).toBe(false);

        release();
        await withinMs(restoring);
        await tick();

        expect(initIdleDetection).not.toHaveBeenCalled();
        const lastDisconnect = Math.max(...vi.mocked(wsDisconnect).mock.invocationCallOrder);
        expect(lastDisconnect).toBeGreaterThan(vi.mocked(wsConnect).mock.invocationCallOrder[0]);
      });

      it("session expiry shows the expired state while a startup step is still pending", async () => {
        mockServices();
        restoreWith();
        vi.mocked(tauri.refreshAccessToken).mockResolvedValue(false);
        const release = holdOnce(vi.mocked(initPreferences));

        const restoring = initAuth();
        await vi.waitFor(() => expect(initPreferences).toHaveBeenCalledTimes(1));

        window.dispatchEvent(new CustomEvent("kaiku:session-expired"));

        await vi.waitFor(() => expect(authState.sessionExpired).toBe(true));
        expect(authState.user).toBeNull();
        expect(wsDisconnect).toHaveBeenCalled();
        expect(cleanupPresence).toHaveBeenCalled();

        release();
        await withinMs(restoring);
        await tick();

        expect(initIdleDetection).not.toHaveBeenCalled();
        setAuthState({ sessionExpired: false });
      });

      it("a sign-in after a hung startup starts its services at once", async () => {
        mockServices();
        restoreWith();
        const release = holdOnce(vi.mocked(initWebSocket));
        vi.mocked(tauri.login).mockResolvedValue({
          user: createUser(),
          setup_required: false,
          password_change_required: false,
        });

        // Startup A hangs; the user signs out and in again. Startup B does
        // not wait for A: the stores cancel A's late effects on their own.
        const restoring = initAuth();
        await vi.waitFor(() => expect(initWebSocket).toHaveBeenCalledTimes(1));
        await withinMs(logout());
        await withinMs(login("https://server.com", "alice", "pw"));

        expect(initWebSocket).toHaveBeenCalledTimes(2);
        expect(wsConnect).toHaveBeenCalledTimes(1);
        expect(initIdleDetection).toHaveBeenCalledTimes(1);

        // A settles after B began: it owns nothing any more and must not
        // tear down B's services.
        const bStarted = vi.mocked(initWebSocket).mock.invocationCallOrder[1];
        release();
        await withinMs(restoring);
        await tick();

        const lastTeardown = Math.max(
          0,
          ...vi.mocked(wsDisconnect).mock.invocationCallOrder,
          ...vi.mocked(cleanupPresence).mock.invocationCallOrder,
          ...vi.mocked(stopIdleDetectionCleanup).mock.invocationCallOrder,
        );
        expect(lastTeardown).toBeLessThan(bStarted);
      });

      it("logout completes when the WebSocket disconnect never settles: the step is abandoned after its bound", async () => {
        vi.useFakeTimers();
        try {
          mockServices();
          restoreWith();
          await initAuth();
          expect(initIdleDetection).toHaveBeenCalledTimes(1);

          // The disconnect hangs on IPC for good. (No vi.waitFor here: under
          // fake timers it advances the clock by its poll interval.)
          holdOnce(vi.mocked(wsDisconnect));
          let done = false;
          const loggingOut = logout().then(() => {
            done = true;
          });
          await vi.advanceTimersByTimeAsync(TEARDOWN_STEP_TIMEOUT_MS - 1);
          expect(done).toBe(false);
          expect(cleanupWebSocket).not.toHaveBeenCalled();

          await vi.advanceTimersByTimeAsync(1);
          await loggingOut;

          // The remaining steps still ran, and the sign-out finished.
          expect(cleanupWebSocket).toHaveBeenCalledTimes(1);
          expect(stopIdleDetectionCleanup).toHaveBeenCalled();
          expect(cleanupPresence).toHaveBeenCalled();
          expect(tauri.logout).toHaveBeenCalledTimes(1);
          expect(authState.user).toBeNull();
          expect(authState.isLoading).toBe(false);
        } finally {
          vi.useRealTimers();
        }
      });
    });
  });
});
