import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/tauri", () => ({
  getFriends: vi.fn(),
  getPendingFriends: vi.fn(),
  getBlockedFriends: vi.fn(),
  sendFriendRequest: vi.fn(),
  acceptFriendRequest: vi.fn(),
  rejectFriendRequest: vi.fn(),
  removeFriend: vi.fn(),
  blockUser: vi.fn(),
  unblockUser: vi.fn(),
}));

vi.mock("@/components/ui/Toast", () => ({
  showToast: vi.fn(),
}));

import * as tauri from "@/lib/tauri";
import { showToast } from "@/components/ui/Toast";
import type { Friend } from "@/lib/types";
import {
  friendsState,
  setFriendsState,
  loadFriends,
  loadPendingRequests,
  loadBlocked,
  sendFriendRequest,
  acceptFriendRequest,
  rejectFriendRequest,
  removeFriend,
  blockUser,
  unblockUser,
  handleUserBlocked,
  handleUserUnblocked,
  getOnlineFriends,
} from "../friends";

function createFriend(overrides: Partial<Friend> = {}): Friend {
  return {
    user_id: "user-1",
    username: "alice",
    display_name: "Alice",
    avatar_url: null,
    status_message: null,
    is_online: false,
    friendship_id: "fs-1",
    friendship_status: "accepted",
    created_at: "2025-01-01T00:00:00Z",
    ...overrides,
  };
}

describe("friends store", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setFriendsState({
      friends: [],
      pendingRequests: [],
      blocked: [],
      isLoading: false,
      isPendingLoading: false,
      isBlockedLoading: false,
      error: null,
    });
  });

  describe("initial state", () => {
    it("has empty arrays and no loading/error", () => {
      expect(friendsState.friends).toEqual([]);
      expect(friendsState.pendingRequests).toEqual([]);
      expect(friendsState.blocked).toEqual([]);
      expect(friendsState.isLoading).toBe(false);
      expect(friendsState.error).toBeNull();
    });
  });

  describe("loadFriends", () => {
    it("loads friends on success", async () => {
      const friends = [
        createFriend(),
        createFriend({ user_id: "user-2", friendship_id: "fs-2" }),
      ];
      vi.mocked(tauri.getFriends).mockResolvedValue(friends);

      await loadFriends();

      expect(friendsState.friends).toEqual(friends);
      expect(friendsState.isLoading).toBe(false);
      expect(friendsState.error).toBeNull();
    });

    it("sets error on failure", async () => {
      vi.mocked(tauri.getFriends).mockRejectedValue(new Error("Network error"));

      await loadFriends();

      expect(friendsState.friends).toEqual([]);
      expect(friendsState.isLoading).toBe(false);
      expect(friendsState.error).toBe("Network error");
    });
  });

  describe("loadPendingRequests", () => {
    it("loads pending requests on success", async () => {
      const pending = [createFriend({ friendship_status: "pending" })];
      vi.mocked(tauri.getPendingFriends).mockResolvedValue(pending);

      await loadPendingRequests();

      expect(friendsState.pendingRequests).toEqual(pending);
      expect(friendsState.isPendingLoading).toBe(false);
    });

    it("clears loading on failure", async () => {
      vi.mocked(tauri.getPendingFriends).mockRejectedValue(new Error("fail"));

      await loadPendingRequests();

      expect(friendsState.isPendingLoading).toBe(false);
    });
  });

  describe("loadBlocked", () => {
    it("loads blocked users on success", async () => {
      const blocked = [createFriend({ friendship_status: "blocked" })];
      vi.mocked(tauri.getBlockedFriends).mockResolvedValue(blocked);

      await loadBlocked();

      expect(friendsState.blocked).toEqual(blocked);
      expect(friendsState.isBlockedLoading).toBe(false);
    });

    it("clears loading on failure", async () => {
      vi.mocked(tauri.getBlockedFriends).mockRejectedValue(new Error("fail"));

      await loadBlocked();

      expect(friendsState.isBlockedLoading).toBe(false);
    });
  });

  describe("sendFriendRequest", () => {
    it("sends request and reloads pending", async () => {
      vi.mocked(tauri.sendFriendRequest).mockResolvedValue({} as any);
      vi.mocked(tauri.getPendingFriends).mockResolvedValue([]);

      await sendFriendRequest("bob");

      expect(tauri.sendFriendRequest).toHaveBeenCalledWith("bob");
      expect(tauri.getPendingFriends).toHaveBeenCalled();
    });

    it("shows toast on error and re-throws", async () => {
      vi.mocked(tauri.sendFriendRequest).mockRejectedValue(
        new Error("not found"),
      );

      await expect(sendFriendRequest("bob")).rejects.toThrow();
      expect(showToast).toHaveBeenCalledWith(
        expect.objectContaining({
          type: "error",
          title: "Friend Request Failed",
        }),
      );
    });
  });

  describe("acceptFriendRequest", () => {
    it("accepts and reloads friends + pending", async () => {
      vi.mocked(tauri.acceptFriendRequest).mockResolvedValue({} as any);
      vi.mocked(tauri.getFriends).mockResolvedValue([]);
      vi.mocked(tauri.getPendingFriends).mockResolvedValue([]);

      await acceptFriendRequest("fs-1");

      expect(tauri.acceptFriendRequest).toHaveBeenCalledWith("fs-1");
      expect(tauri.getFriends).toHaveBeenCalled();
      expect(tauri.getPendingFriends).toHaveBeenCalled();
    });
  });

  describe("rejectFriendRequest", () => {
    it("rejects and reloads pending", async () => {
      vi.mocked(tauri.rejectFriendRequest).mockResolvedValue(undefined);
      vi.mocked(tauri.getPendingFriends).mockResolvedValue([]);

      await rejectFriendRequest("fs-1");

      expect(tauri.rejectFriendRequest).toHaveBeenCalledWith("fs-1");
      expect(tauri.getPendingFriends).toHaveBeenCalled();
    });
  });

  describe("removeFriend", () => {
    it("removes friend optimistically from list", async () => {
      const friend = createFriend({ friendship_id: "fs-1" });
      setFriendsState({ friends: [friend] });
      vi.mocked(tauri.removeFriend).mockResolvedValue(undefined);

      await removeFriend("fs-1");

      expect(friendsState.friends).toEqual([]);
    });

    it("shows toast on error and re-throws", async () => {
      setFriendsState({ friends: [createFriend()] });
      vi.mocked(tauri.removeFriend).mockRejectedValue(new Error("fail"));

      await expect(removeFriend("fs-1")).rejects.toThrow();
      expect(showToast).toHaveBeenCalledWith(
        expect.objectContaining({ type: "error", title: "Remove Failed" }),
      );
    });
  });

  describe("blockUser", () => {
    it("blocks user and reloads all lists", async () => {
      vi.mocked(tauri.blockUser).mockResolvedValue({} as any);
      vi.mocked(tauri.getFriends).mockResolvedValue([]);
      vi.mocked(tauri.getPendingFriends).mockResolvedValue([]);
      vi.mocked(tauri.getBlockedFriends).mockResolvedValue([]);

      await blockUser("user-1");

      expect(tauri.blockUser).toHaveBeenCalledWith("user-1");
      expect(tauri.getFriends).toHaveBeenCalled();
      expect(tauri.getPendingFriends).toHaveBeenCalled();
      expect(tauri.getBlockedFriends).toHaveBeenCalled();
    });
  });

  describe("unblockUser", () => {
    it("removes user from blocked list optimistically", async () => {
      const blocked = createFriend({
        user_id: "user-1",
        friendship_status: "blocked",
      });
      setFriendsState({ blocked: [blocked] });
      vi.mocked(tauri.unblockUser).mockResolvedValue(undefined);

      await unblockUser("user-1");

      expect(friendsState.blocked).toEqual([]);
    });
  });

  describe("handleUserBlocked (WS handler)", () => {
    it("removes user from friends and pending, reloads blocked", async () => {
      const friend = createFriend({ user_id: "user-1" });
      const pending = createFriend({
        user_id: "user-1",
        friendship_status: "pending",
      });
      setFriendsState({ friends: [friend], pendingRequests: [pending] });
      vi.mocked(tauri.getBlockedFriends).mockResolvedValue([]);

      handleUserBlocked("user-1");

      expect(friendsState.friends).toEqual([]);
      expect(friendsState.pendingRequests).toEqual([]);
    });
  });

  describe("handleUserUnblocked (WS handler)", () => {
    it("removes user from blocked list", () => {
      const blocked = createFriend({
        user_id: "user-1",
        friendship_status: "blocked",
      });
      setFriendsState({ blocked: [blocked] });

      handleUserUnblocked("user-1");

      expect(friendsState.blocked).toEqual([]);
    });
  });

  describe("getOnlineFriends", () => {
    it("returns only online friends", () => {
      setFriendsState({
        friends: [
          createFriend({ user_id: "u1", is_online: true }),
          createFriend({ user_id: "u2", is_online: false }),
          createFriend({ user_id: "u3", is_online: true }),
        ],
      });

      const online = getOnlineFriends();
      expect(online).toHaveLength(2);
      expect(online.map((f) => f.user_id)).toEqual(["u1", "u3"]);
    });
  });
});
