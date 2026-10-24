import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/tauri", () => ({
  getCurrentUser: vi.fn(),
  login: vi.fn(),
  register: vi.fn(),
  logout: vi.fn(),
  oidcCompleteLogin: vi.fn(),
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
} from "../auth";

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
        new Error("Invalid credentials"),
      );

      await expect(
        login("https://server.com", "alice", "wrong"),
      ).rejects.toThrow("Invalid credentials");
      expect(authState.error).toBe("Invalid credentials");
      expect(authState.isLoading).toBe(false);
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
      vi.mocked(tauri.register).mockRejectedValue(new Error("Username taken"));

      await expect(
        register("https://server.com", "alice", "password"),
      ).rejects.toThrow("Username taken");
      expect(authState.error).toBe("Username taken");
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
  });
});
