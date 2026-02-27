import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/tauri", () => ({
  getDMList: vi.fn(),
  wsStatus: vi.fn(),
  markDMAsRead: vi.fn(),
  markAllDMsRead: vi.fn(),
}));

vi.mock("@/stores/websocket", () => ({
  subscribeChannel: vi.fn(),
}));

vi.mock("@/components/ui/Toast", () => ({
  showToast: vi.fn(),
}));

import * as tauri from "@/lib/tauri";
import { subscribeChannel } from "@/stores/websocket";
import { showToast } from "@/components/ui/Toast";
import type { DMListItem, Message } from "@/lib/types";
import {
  dmsState,
  setDmsState,
  loadDMs,
  selectDM,
  selectFriendsTab,
  updateDMLastMessage,
  markDMAsRead,
  markAllDMsAsRead,
  handleDMReadEvent,
  handleDMNameUpdated,
  getSelectedDM,
  getTotalUnreadCount,
} from "../dms";

function createDM(overrides: Partial<DMListItem> = {}): DMListItem {
  return {
    id: "dm-1",
    name: "Alice",
    channel_type: "dm",
    category_id: null,
    guild_id: null,
    topic: null,
    icon_url: null,
    user_limit: null,
    position: 0,
    created_at: "2025-01-01T00:00:00Z",
    participants: [
      {
        user_id: "user-1",
        username: "alice",
        display_name: "Alice",
        avatar_url: null,
        joined_at: "2025-01-01T00:00:00Z",
      },
    ],
    last_message: null,
    unread_count: 0,
    ...overrides,
  };
}

function createMessage(overrides: Partial<Message> = {}): Message {
  return {
    id: "msg-1",
    channel_id: "dm-1",
    author: {
      id: "user-1",
      username: "alice",
      display_name: "Alice",
      avatar_url: null,
      status: "online",
    },
    content: "hello",
    encrypted: false,
    attachments: [],
    reply_to: null,
    parent_id: null,
    thread_reply_count: 0,
    thread_last_reply_at: null,
    edited_at: null,
    created_at: "2025-01-01T12:00:00Z",
    mention_type: null,
    reactions: [],
    ...overrides,
  };
}

describe("dms store", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setDmsState({
      dms: [],
      selectedDMId: null,
      isShowingFriends: true,
      typingUsers: {},
      isLoading: false,
      error: null,
    });
  });

  describe("initial state", () => {
    it("has empty DMs and friends tab active", () => {
      expect(dmsState.dms).toEqual([]);
      expect(dmsState.selectedDMId).toBeNull();
      expect(dmsState.isShowingFriends).toBe(true);
      expect(dmsState.isLoading).toBe(false);
      expect(dmsState.error).toBeNull();
    });
  });

  describe("loadDMs", () => {
    it("loads DMs and subscribes to channels when WS connected", async () => {
      const dms = [createDM(), createDM({ id: "dm-2", name: "Bob" })];
      vi.mocked(tauri.getDMList).mockResolvedValue(dms);
      vi.mocked(tauri.wsStatus).mockResolvedValue({ type: "connected" } as any);
      vi.mocked(subscribeChannel).mockResolvedValue(undefined);

      await loadDMs();

      expect(dmsState.dms).toEqual(dms);
      expect(dmsState.isLoading).toBe(false);
      expect(subscribeChannel).toHaveBeenCalledTimes(2);
    });

    it("skips subscriptions when WS not connected", async () => {
      vi.useFakeTimers();
      try {
        const dms = [createDM()];
        vi.mocked(tauri.getDMList).mockResolvedValue(dms);
        vi.mocked(tauri.wsStatus).mockResolvedValue({
          type: "disconnected",
        } as any);

        const promise = loadDMs();
        // Advance past the 5s polling timeout
        await vi.advanceTimersByTimeAsync(6000);
        await promise;

        expect(dmsState.dms).toEqual(dms);
        expect(subscribeChannel).not.toHaveBeenCalled();
      } finally {
        vi.useRealTimers();
      }
    });

    it("sets error on failure", async () => {
      vi.mocked(tauri.getDMList).mockRejectedValue(new Error("Network error"));

      await loadDMs();

      expect(dmsState.dms).toEqual([]);
      expect(dmsState.isLoading).toBe(false);
      expect(dmsState.error).toBe("Network error");
    });
  });

  describe("selectDM", () => {
    it("sets selected DM and hides friends tab", () => {
      selectDM("dm-1");

      expect(dmsState.selectedDMId).toBe("dm-1");
      expect(dmsState.isShowingFriends).toBe(false);
    });
  });

  describe("selectFriendsTab", () => {
    it("clears DM selection and shows friends", () => {
      setDmsState({ selectedDMId: "dm-1", isShowingFriends: false });

      selectFriendsTab();

      expect(dmsState.selectedDMId).toBeNull();
      expect(dmsState.isShowingFriends).toBe(true);
    });
  });

  describe("updateDMLastMessage", () => {
    it("updates last message and increments unread", () => {
      setDmsState({ dms: [createDM({ id: "dm-1", unread_count: 0 })] });
      const msg = createMessage({ channel_id: "dm-1" });

      updateDMLastMessage("dm-1", msg);

      expect(dmsState.dms[0].last_message).toEqual({
        id: msg.id,
        content: msg.content,
        user_id: msg.author.id,
        username: msg.author.username,
        created_at: msg.created_at,
      });
      expect(dmsState.dms[0].unread_count).toBe(1);
    });

    it("ignores unknown channel", () => {
      setDmsState({ dms: [createDM()] });
      const msg = createMessage({ channel_id: "unknown" });

      updateDMLastMessage("unknown", msg);

      expect(dmsState.dms[0].last_message).toBeNull();
    });
  });

  describe("markDMAsRead", () => {
    it("marks DM as read on success", async () => {
      setDmsState({
        dms: [
          createDM({
            id: "dm-1",
            unread_count: 5,
            last_message: {
              id: "msg-1",
              content: "hi",
              user_id: "u1",
              username: "alice",
              created_at: "2025-01-01T00:00:00Z",
            },
          }),
        ],
      });
      vi.mocked(tauri.markDMAsRead).mockResolvedValue(undefined);

      await markDMAsRead("dm-1");

      expect(dmsState.dms[0].unread_count).toBe(0);
      expect(tauri.markDMAsRead).toHaveBeenCalledWith("dm-1", "msg-1");
    });

    it("skips if already read", async () => {
      setDmsState({ dms: [createDM({ id: "dm-1", unread_count: 0 })] });

      await markDMAsRead("dm-1");

      expect(tauri.markDMAsRead).not.toHaveBeenCalled();
    });

    it("shows toast on error", async () => {
      setDmsState({
        dms: [
          createDM({
            id: "dm-1",
            unread_count: 3,
            last_message: {
              id: "msg-1",
              content: "hi",
              user_id: "u1",
              username: "alice",
              created_at: "2025-01-01T00:00:00Z",
            },
          }),
        ],
      });
      vi.mocked(tauri.markDMAsRead).mockRejectedValue(new Error("fail"));

      await markDMAsRead("dm-1");

      expect(showToast).toHaveBeenCalledWith(
        expect.objectContaining({ type: "error" }),
      );
    });
  });

  describe("markAllDMsAsRead", () => {
    it("optimistically zeros all unread counts", async () => {
      setDmsState({
        dms: [
          createDM({ id: "dm-1", unread_count: 3 }),
          createDM({ id: "dm-2", unread_count: 5 }),
        ],
      });
      vi.mocked(tauri.markAllDMsRead).mockResolvedValue(undefined);

      await markAllDMsAsRead();

      expect(dmsState.dms[0].unread_count).toBe(0);
      expect(dmsState.dms[1].unread_count).toBe(0);
    });
  });

  describe("handleDMReadEvent", () => {
    it("zeros unread count for the channel", () => {
      setDmsState({ dms: [createDM({ id: "dm-1", unread_count: 7 })] });

      handleDMReadEvent("dm-1");

      expect(dmsState.dms[0].unread_count).toBe(0);
    });
  });

  describe("handleDMNameUpdated", () => {
    it("updates DM name", () => {
      setDmsState({ dms: [createDM({ id: "dm-1", name: "Old Name" })] });

      handleDMNameUpdated("dm-1", "New Name");

      expect(dmsState.dms[0].name).toBe("New Name");
    });
  });

  describe("getTotalUnreadCount", () => {
    it("sums unread counts across all DMs", () => {
      setDmsState({
        dms: [
          createDM({ id: "dm-1", unread_count: 3 }),
          createDM({ id: "dm-2", unread_count: 5 }),
        ],
      });

      expect(getTotalUnreadCount()).toBe(8);
    });
  });

  describe("getSelectedDM", () => {
    it("returns null when no DM selected", () => {
      expect(getSelectedDM()).toBeNull();
    });

    it("returns the selected DM", () => {
      const dm = createDM({ id: "dm-1" });
      setDmsState({ dms: [dm], selectedDMId: "dm-1" });

      expect(getSelectedDM()?.id).toBe("dm-1");
    });
  });
});
