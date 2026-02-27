import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/tauri", () => ({
  getCurrentUser: vi.fn(),
  login: vi.fn(),
  register: vi.fn(),
  logout: vi.fn(),
  oidcCompleteLogin: vi.fn(),
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
      vi.mocked(tauri.getCurrentUser).mockResolvedValue(user);
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

    it("handles no session gracefully", async () => {
      vi.mocked(tauri.getCurrentUser).mockResolvedValue(null);

      await initAuth();

      expect(authState.user).toBeNull();
      expect(authState.isInitialized).toBe(true);
      expect(authState.error).toBeNull();
      expect(initWebSocket).not.toHaveBeenCalled();
    });

    it("sets error on WS failure but completes", async () => {
      vi.mocked(tauri.getCurrentUser).mockResolvedValue(createUser());
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
      vi.mocked(tauri.getCurrentUser).mockResolvedValue(createUser());
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
  });

  describe("login", () => {
    it("logs in and inits subsystems", async () => {
      const user = createUser();
      vi.mocked(tauri.login).mockResolvedValue({ user, setup_required: false });
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
      vi.mocked(tauri.getCurrentUser).mockResolvedValue(user);
      vi.mocked(initWebSocket).mockResolvedValue(undefined);
      vi.mocked(initPresence).mockResolvedValue(undefined);
      vi.mocked(wsConnect).mockResolvedValue(undefined);
      vi.mocked(initPreferences).mockResolvedValue(undefined);

      await loginWithOidc("https://server.com", "token", "refresh", 3600);

      expect(authState.user).toEqual(user);
    });

    it("throws if getCurrentUser returns null", async () => {
      vi.mocked(tauri.oidcCompleteLogin).mockResolvedValue(undefined);
      vi.mocked(tauri.getCurrentUser).mockResolvedValue(null);

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
});
