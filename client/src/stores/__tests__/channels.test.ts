import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/tauri", () => ({
  getChannels: vi.fn(),
  getGuildChannels: vi.fn(),
  createChannel: vi.fn(),
  markChannelAsRead: vi.fn(),
  markAllGuildChannelsRead: vi.fn(),
  reorderGuildChannels: vi.fn(),
  wsStatus: vi.fn(),
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
import type { Channel, ChannelWithUnread } from "@/lib/types";
import {
  channelsState,
  setChannelsState,
  loadChannels,
  loadChannelsForGuild,
  selectChannel,
  clearSelection,
  getChannel,
  getUnreadCount,
  getTotalUnreadCount,
  incrementUnreadCount,
  markChannelAsRead,
  markAllGuildChannelsAsRead,
  handleChannelReadEvent,
  createChannel,
  textChannels,
  voiceChannels,
} from "../channels";

function createChannelWithUnread(
  overrides: Partial<ChannelWithUnread> = {},
): ChannelWithUnread {
  return {
    id: "ch-1",
    name: "general",
    channel_type: "text",
    category_id: null,
    guild_id: "guild-1",
    topic: null,
    icon_url: null,
    user_limit: null,
    position: 0,
    created_at: "2025-01-01T00:00:00Z",
    unread_count: 0,
    ...overrides,
  };
}

describe("channels store", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setChannelsState({
      channels: [],
      selectedChannelId: null,
      isLoading: false,
      error: null,
    });
  });

  describe("initial state", () => {
    it("has empty channels and no selection", () => {
      expect(channelsState.channels).toEqual([]);
      expect(channelsState.selectedChannelId).toBeNull();
      expect(channelsState.isLoading).toBe(false);
      expect(channelsState.error).toBeNull();
    });
  });

  describe("loadChannels", () => {
    it("loads channels and auto-selects first text channel", async () => {
      const channels: Channel[] = [
        {
          id: "v1",
          name: "voice",
          channel_type: "voice",
          category_id: null,
          guild_id: "g1",
          topic: null,
          icon_url: null,
          user_limit: null,
          position: 0,
          created_at: "2025-01-01T00:00:00Z",
        },
        {
          id: "t1",
          name: "general",
          channel_type: "text",
          category_id: null,
          guild_id: "g1",
          topic: null,
          icon_url: null,
          user_limit: null,
          position: 1,
          created_at: "2025-01-01T00:00:00Z",
        },
      ];
      vi.mocked(tauri.getChannels).mockResolvedValue(channels);

      await loadChannels();

      expect(channelsState.channels).toHaveLength(2);
      expect(channelsState.selectedChannelId).toBe("t1");
      expect(channelsState.isLoading).toBe(false);
    });

    it("sets error on failure", async () => {
      vi.mocked(tauri.getChannels).mockRejectedValue(
        new Error("Network error"),
      );

      await loadChannels();

      expect(channelsState.error).toBe("Network error");
      expect(channelsState.isLoading).toBe(false);
    });
  });

  describe("loadChannelsForGuild", () => {
    it("loads guild channels, auto-selects first text, and subscribes", async () => {
      const channels: ChannelWithUnread[] = [
        createChannelWithUnread({
          id: "t1",
          name: "general",
          channel_type: "text",
          position: 0,
        }),
        createChannelWithUnread({
          id: "v1",
          name: "voice",
          channel_type: "voice",
          position: 1,
        }),
      ];
      vi.mocked(tauri.getGuildChannels).mockResolvedValue(channels);
      vi.mocked(tauri.wsStatus).mockResolvedValue({ type: "connected" } as any);
      vi.mocked(subscribeChannel).mockResolvedValue(undefined);

      await loadChannelsForGuild("guild-1");

      expect(channelsState.channels).toEqual(channels);
      expect(channelsState.selectedChannelId).toBe("t1");
      expect(subscribeChannel).toHaveBeenCalledWith("t1");
      expect(subscribeChannel).not.toHaveBeenCalledWith("v1"); // voice channels not subscribed
    });

    it("clears selection when no text channels", async () => {
      const channels: ChannelWithUnread[] = [
        createChannelWithUnread({ id: "v1", channel_type: "voice" }),
      ];
      vi.mocked(tauri.getGuildChannels).mockResolvedValue(channels);
      vi.mocked(tauri.wsStatus).mockResolvedValue({ type: "connected" } as any);

      await loadChannelsForGuild("guild-1");

      expect(channelsState.selectedChannelId).toBeNull();
    });

    it("skips WS subscribe when disconnected", async () => {
      vi.useFakeTimers();
      try {
        vi.mocked(tauri.getGuildChannels).mockResolvedValue([
          createChannelWithUnread(),
        ]);
        vi.mocked(tauri.wsStatus).mockResolvedValue({
          type: "disconnected",
        } as any);

        const promise = loadChannelsForGuild("guild-1");
        // Advance past the 5s polling timeout
        await vi.advanceTimersByTimeAsync(6000);
        await promise;

        expect(subscribeChannel).not.toHaveBeenCalled();
      } finally {
        vi.useRealTimers();
      }
    });

    it("sets error on failure", async () => {
      vi.mocked(tauri.getGuildChannels).mockRejectedValue(new Error("fail"));

      await loadChannelsForGuild("guild-1");

      expect(channelsState.error).toBe("fail");
    });
  });

  describe("selectChannel", () => {
    it("sets selected channel ID", () => {
      selectChannel("ch-1");

      expect(channelsState.selectedChannelId).toBe("ch-1");
    });
  });

  describe("clearSelection", () => {
    it("clears channel selection", () => {
      setChannelsState({ selectedChannelId: "ch-1" });

      clearSelection();

      expect(channelsState.selectedChannelId).toBeNull();
    });
  });

  describe("getChannel", () => {
    it("returns channel by ID", () => {
      setChannelsState({ channels: [createChannelWithUnread({ id: "ch-1" })] });

      expect(getChannel("ch-1")?.id).toBe("ch-1");
    });

    it("returns undefined for unknown channel", () => {
      expect(getChannel("unknown")).toBeUndefined();
    });
  });

  describe("getUnreadCount", () => {
    it("returns unread count for a channel", () => {
      setChannelsState({
        channels: [createChannelWithUnread({ id: "ch-1", unread_count: 5 })],
      });

      expect(getUnreadCount("ch-1")).toBe(5);
    });

    it("returns 0 for unknown channel", () => {
      expect(getUnreadCount("unknown")).toBe(0);
    });
  });

  describe("getTotalUnreadCount", () => {
    it("sums unread across text channels only", () => {
      setChannelsState({
        channels: [
          createChannelWithUnread({
            id: "t1",
            channel_type: "text",
            unread_count: 3,
          }),
          createChannelWithUnread({
            id: "t2",
            channel_type: "text",
            unread_count: 5,
          }),
          createChannelWithUnread({
            id: "v1",
            channel_type: "voice",
            unread_count: 2,
          }),
        ],
      });

      expect(getTotalUnreadCount()).toBe(8); // voice excluded
    });
  });

  describe("incrementUnreadCount", () => {
    it("increments unread count by 1", () => {
      setChannelsState({
        channels: [createChannelWithUnread({ id: "ch-1", unread_count: 3 })],
      });

      incrementUnreadCount("ch-1");

      expect(channelsState.channels[0].unread_count).toBe(4);
    });
  });

  describe("markChannelAsRead", () => {
    it("optimistically zeros unread and calls API", async () => {
      setChannelsState({
        channels: [createChannelWithUnread({ id: "ch-1", unread_count: 5 })],
      });
      vi.mocked(tauri.markChannelAsRead).mockResolvedValue(undefined);

      await markChannelAsRead("ch-1");

      expect(channelsState.channels[0].unread_count).toBe(0);
      expect(tauri.markChannelAsRead).toHaveBeenCalledWith("ch-1");
    });

    it("skips if already read", async () => {
      setChannelsState({
        channels: [createChannelWithUnread({ id: "ch-1", unread_count: 0 })],
      });

      await markChannelAsRead("ch-1");

      expect(tauri.markChannelAsRead).not.toHaveBeenCalled();
    });

    it("shows toast on API error", async () => {
      setChannelsState({
        channels: [createChannelWithUnread({ id: "ch-1", unread_count: 3 })],
      });
      vi.mocked(tauri.markChannelAsRead).mockRejectedValue(new Error("fail"));

      await markChannelAsRead("ch-1");

      expect(showToast).toHaveBeenCalledWith(
        expect.objectContaining({ type: "error" }),
      );
    });
  });

  describe("markAllGuildChannelsAsRead", () => {
    it("optimistically zeros unread for guild text channels", async () => {
      setChannelsState({
        channels: [
          createChannelWithUnread({
            id: "t1",
            guild_id: "g1",
            channel_type: "text",
            unread_count: 5,
          }),
          createChannelWithUnread({
            id: "t2",
            guild_id: "g1",
            channel_type: "text",
            unread_count: 3,
          }),
          createChannelWithUnread({
            id: "v1",
            guild_id: "g1",
            channel_type: "voice",
            unread_count: 1,
          }),
        ],
      });
      vi.mocked(tauri.markAllGuildChannelsRead).mockResolvedValue(undefined);

      await markAllGuildChannelsAsRead("g1");

      expect(channelsState.channels[0].unread_count).toBe(0);
      expect(channelsState.channels[1].unread_count).toBe(0);
      expect(channelsState.channels[2].unread_count).toBe(1); // voice untouched
    });
  });

  describe("handleChannelReadEvent", () => {
    it("zeros unread count for the channel", () => {
      setChannelsState({
        channels: [createChannelWithUnread({ id: "ch-1", unread_count: 7 })],
      });

      handleChannelReadEvent("ch-1");

      expect(channelsState.channels[0].unread_count).toBe(0);
    });
  });

  describe("createChannel", () => {
    it("adds channel to store", async () => {
      const channel: Channel = {
        id: "new-ch",
        name: "new-channel",
        channel_type: "text",
        category_id: null,
        guild_id: "g1",
        topic: null,
        icon_url: null,
        user_limit: null,
        position: 0,
        created_at: "2025-01-01T00:00:00Z",
      };
      vi.mocked(tauri.createChannel).mockResolvedValue(channel);

      const result = await createChannel("new-channel", "text", "g1");

      expect(result.id).toBe("new-ch");
      expect(result.unread_count).toBe(0);
      expect(channelsState.channels).toHaveLength(1);
    });
  });

  describe("textChannels / voiceChannels derived", () => {
    it("filters and sorts by position", () => {
      setChannelsState({
        channels: [
          createChannelWithUnread({
            id: "t2",
            channel_type: "text",
            position: 2,
          }),
          createChannelWithUnread({
            id: "v1",
            channel_type: "voice",
            position: 0,
          }),
          createChannelWithUnread({
            id: "t1",
            channel_type: "text",
            position: 1,
          }),
        ],
      });

      const tc = textChannels();
      expect(tc).toHaveLength(2);
      expect(tc[0].id).toBe("t1");
      expect(tc[1].id).toBe("t2");

      const vc = voiceChannels();
      expect(vc).toHaveLength(1);
      expect(vc[0].id).toBe("v1");
    });
  });
});
