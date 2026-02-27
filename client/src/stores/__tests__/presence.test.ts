import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/tauri", () => ({
  updateStatus: vi.fn(),
}));

vi.mock("@/lib/idleDetector", () => ({
  startIdleDetection: vi.fn(),
  stopIdleDetection: vi.fn(),
  setIdleTimeout: vi.fn(),
}));

vi.mock("@/stores/preferences", () => ({
  preferences: vi.fn(() => ({ display: { idleTimeoutMinutes: 5 } })),
}));

vi.mock("@/stores/auth", () => ({
  currentUser: vi.fn(() => ({
    id: "me",
    username: "me",
    display_name: "Me",
    avatar_url: null,
    status: "online",
    email: null,
    mfa_enabled: false,
    created_at: "2025-01-01T00:00:00Z",
  })),
  updateUser: vi.fn(),
}));

import { updateStatus } from "@/lib/tauri";
import { startIdleDetection, stopIdleDetection } from "@/lib/idleDetector";
import { currentUser } from "@/stores/auth";
import {
  presenceState,
  setPresenceState,
  updateUserPresence,
  updateUserActivity,
  setInitialPresence,
  getUserStatus,
  isUserOnline,
  clearPresence,
  setMyStatus,
  patchUser,
  initIdleDetection,
  markManualStatusChange,
  stopIdleDetectionCleanup,
} from "../presence";

describe("presence store", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setPresenceState({ users: {} });
  });

  describe("initial state", () => {
    it("has empty users map", () => {
      expect(presenceState.users).toEqual({});
    });
  });

  describe("updateUserPresence", () => {
    it("sets status for a user", () => {
      updateUserPresence("user-1", "online");

      expect(presenceState.users["user-1"].status).toBe("online");
    });

    it("preserves existing activity when not provided", () => {
      setPresenceState("users", "user-1", {
        status: "online",
        activity: {
          type: "game",
          name: "Minecraft",
          started_at: "2025-01-01T00:00:00Z",
        },
      });

      updateUserPresence("user-1", "idle");

      expect(presenceState.users["user-1"].status).toBe("idle");
      expect(presenceState.users["user-1"].activity?.name).toBe("Minecraft");
    });

    it("sets lastSeen when user goes offline", () => {
      updateUserPresence("user-1", "offline");

      expect(presenceState.users["user-1"].lastSeen).toBeDefined();
    });

    it("does not set lastSeen for non-offline statuses", () => {
      updateUserPresence("user-1", "online");

      expect(presenceState.users["user-1"].lastSeen).toBeUndefined();
    });
  });

  describe("updateUserActivity", () => {
    it("updates activity for existing user", () => {
      setPresenceState("users", "user-1", { status: "online" });
      const activity = {
        type: "game" as const,
        name: "Chess",
        started_at: "2025-01-01T00:00:00Z",
      };

      updateUserActivity("user-1", activity);

      expect(presenceState.users["user-1"].activity?.name).toBe("Chess");
      expect(presenceState.users["user-1"].status).toBe("online");
    });

    it("creates user with online status if not present", () => {
      const activity = {
        type: "coding" as const,
        name: "VS Code",
        started_at: "2025-01-01T00:00:00Z",
      };

      updateUserActivity("new-user", activity);

      expect(presenceState.users["new-user"].status).toBe("online");
      expect(presenceState.users["new-user"].activity?.name).toBe("VS Code");
    });

    it("clears activity with null", () => {
      setPresenceState("users", "user-1", {
        status: "online",
        activity: {
          type: "game",
          name: "Chess",
          started_at: "2025-01-01T00:00:00Z",
        },
      });

      updateUserActivity("user-1", null);

      expect(presenceState.users["user-1"].activity).toBeNull();
    });
  });

  describe("setInitialPresence", () => {
    it("sets status for multiple users in bulk", () => {
      setInitialPresence([
        { id: "u1", status: "online" },
        { id: "u2", status: "idle" },
        { id: "u3", status: "dnd" },
      ]);

      expect(presenceState.users["u1"].status).toBe("online");
      expect(presenceState.users["u2"].status).toBe("idle");
      expect(presenceState.users["u3"].status).toBe("dnd");
    });
  });

  describe("getUserStatus", () => {
    it("returns status for known user", () => {
      setPresenceState("users", "user-1", { status: "dnd" });

      expect(getUserStatus("user-1")).toBe("dnd");
    });

    it("returns 'offline' for unknown user", () => {
      expect(getUserStatus("unknown")).toBe("offline");
    });
  });

  describe("isUserOnline", () => {
    it("returns true for online/idle/dnd users", () => {
      setPresenceState("users", "u1", { status: "online" });
      setPresenceState("users", "u2", { status: "idle" });
      setPresenceState("users", "u3", { status: "dnd" });

      expect(isUserOnline("u1")).toBe(true);
      expect(isUserOnline("u2")).toBe(true);
      expect(isUserOnline("u3")).toBe(true);
    });

    it("returns false for offline users", () => {
      setPresenceState("users", "u1", { status: "offline" });

      expect(isUserOnline("u1")).toBe(false);
    });

    it("returns false for unknown users", () => {
      expect(isUserOnline("unknown")).toBe(false);
    });
  });

  describe("clearPresence", () => {
    it("clears all user presence data", () => {
      setPresenceState("users", "u1", { status: "online" });
      setPresenceState("users", "u2", { status: "idle" });

      clearPresence();

      expect(presenceState.users).toEqual({});
    });
  });

  describe("setMyStatus", () => {
    it("updates server and local state", async () => {
      vi.mocked(updateStatus).mockResolvedValue(undefined);

      await setMyStatus("dnd");

      expect(updateStatus).toHaveBeenCalledWith("dnd");
      expect(presenceState.users["me"].status).toBe("dnd");
    });

    it("does nothing without a current user", async () => {
      vi.mocked(currentUser).mockReturnValue(null);

      await setMyStatus("online");

      expect(updateStatus).not.toHaveBeenCalled();
    });
  });

  describe("patchUser", () => {
    it("updates presence fields for known user", () => {
      setPresenceState("users", "user-1", { status: "online" });

      patchUser("user-1", { status: "idle" });

      expect(presenceState.users["user-1"].status).toBe("idle");
    });

    it("ignores patch for unknown presence user", () => {
      patchUser("unknown", { status: "online" });

      // Should not create a new entry via the presence path
      expect(presenceState.users["unknown"]).toBeUndefined();
    });
  });

  describe("initIdleDetection", () => {
    it("starts idle detection with timeout from preferences", () => {
      initIdleDetection();

      expect(startIdleDetection).toHaveBeenCalledWith(expect.any(Function), 5);
    });
  });

  describe("stopIdleDetectionCleanup", () => {
    it("calls stopIdleDetection", () => {
      stopIdleDetectionCleanup();

      expect(stopIdleDetection).toHaveBeenCalled();
    });
  });

  describe("markManualStatusChange", () => {
    it("does not throw for valid statuses", () => {
      expect(() => markManualStatusChange("idle")).not.toThrow();
      expect(() => markManualStatusChange("dnd")).not.toThrow();
      expect(() => markManualStatusChange("online")).not.toThrow();
      expect(() => markManualStatusChange("invisible")).not.toThrow();
    });
  });
});
