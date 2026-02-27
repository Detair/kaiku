import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/tauri", () => ({
  wsConnect: vi.fn(),
  wsDisconnect: vi.fn(),
  wsSubscribe: vi.fn(),
  wsUnsubscribe: vi.fn(),
  wsTyping: vi.fn(),
  wsStopTyping: vi.fn(),
  getBrowserWebSocket: vi.fn(),
  wsStatus: vi.fn(),
  wsSend: vi.fn(),
}));

vi.mock("@/stores/auth", () => ({
  currentUser: vi.fn(() => ({ id: "me", username: "me" })),
}));

// Stub heavy cross-store dependencies to prevent import side effects
vi.mock("@/stores/presence", () => ({
  updateUserPresence: vi.fn(),
  updateUserActivity: vi.fn(),
}));

vi.mock("@/stores/messages", () => ({
  addMessage: vi.fn(),
  removeMessage: vi.fn(),
  messagesState: { byChannel: {} },
  setMessagesState: vi.fn(),
}));

vi.mock("@/stores/threads", () => ({
  addThreadReply: vi.fn(),
  removeThreadReply: vi.fn(),
  setThreadReadState: vi.fn(),
  updateThreadInfo: vi.fn(),
  updateParentThreadIndicator: vi.fn(),
  markThreadUnread: vi.fn(),
  clearThreadUnread: vi.fn(),
  threadsState: { activeThreadId: null },
}));

vi.mock("@/stores/preferences", () => ({
  handlePreferencesUpdated: vi.fn(),
}));

vi.mock("@/stores/call", () => ({
  receiveIncomingCall: vi.fn(),
  callConnected: vi.fn(),
  callEndedExternally: vi.fn(),
  participantJoined: vi.fn(),
  participantLeft: vi.fn(),
}));

vi.mock("@/stores/friends", () => ({
  loadFriends: vi.fn(),
  loadPendingRequests: vi.fn(),
  handleUserBlocked: vi.fn(),
  handleUserUnblocked: vi.fn(),
}));

vi.mock("@/stores/channels", () => ({
  getChannel: vi.fn(),
  channelsState: { selectedChannelId: null },
  handleChannelReadEvent: vi.fn(),
  incrementUnreadCount: vi.fn(),
}));

vi.mock("@/stores/guilds", () => ({
  guildsState: { activeGuildId: null },
  getGuildIdForChannel: vi.fn(),
  incrementGuildUnread: vi.fn(),
}));

vi.mock("@/stores/dms", () => ({
  handleDMReadEvent: vi.fn(),
  handleDMNameUpdated: vi.fn(),
}));

vi.mock("@/lib/sound", () => ({
  playNotification: vi.fn(),
}));

import * as tauri from "@/lib/tauri";
import {
  wsState,
  setWsState,
  setTypingState,
  connect,
  disconnect,
  subscribeChannel,
  unsubscribeChannel,
  sendTyping,
  stopTyping,
  getTypingUsers,
  isConnected,
  cleanupWebSocket,
} from "../websocket";

describe("websocket store", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setWsState({
      status: "disconnected",
      reconnectAttempt: 0,
      subscribedChannels: new Set(),
      error: null,
    });
    setTypingState({ byChannel: {} });
  });

  describe("connect", () => {
    it("connects successfully", async () => {
      vi.mocked(tauri.wsConnect).mockResolvedValue(undefined);

      await connect();

      expect(tauri.wsConnect).toHaveBeenCalled();
      // Status is set to "connecting" before await
    });

    it("sets error on failure", async () => {
      vi.mocked(tauri.wsConnect).mockRejectedValue(
        new Error("Connection failed"),
      );

      await expect(connect()).rejects.toThrow("Connection failed");
      expect(wsState.status).toBe("disconnected");
      expect(wsState.error).toBe("Connection failed");
    });
  });

  describe("disconnect", () => {
    it("disconnects and clears subscriptions", async () => {
      setWsState({
        status: "connected",
        subscribedChannels: new Set(["ch-1"]),
      });
      vi.mocked(tauri.wsDisconnect).mockResolvedValue(undefined);

      await disconnect();

      expect(wsState.status).toBe("disconnected");
      expect(wsState.subscribedChannels.size).toBe(0);
    });
  });

  describe("subscribeChannel", () => {
    it("subscribes to a channel", async () => {
      vi.mocked(tauri.wsSubscribe).mockResolvedValue(undefined);

      await subscribeChannel("ch-1");

      expect(tauri.wsSubscribe).toHaveBeenCalledWith("ch-1");
      expect(wsState.subscribedChannels.has("ch-1")).toBe(true);
    });

    it("no-ops if already subscribed", async () => {
      setWsState({ subscribedChannels: new Set(["ch-1"]) });

      await subscribeChannel("ch-1");

      expect(tauri.wsSubscribe).not.toHaveBeenCalled();
    });
  });

  describe("unsubscribeChannel", () => {
    it("unsubscribes from a channel", async () => {
      setWsState({ subscribedChannels: new Set(["ch-1"]) });
      vi.mocked(tauri.wsUnsubscribe).mockResolvedValue(undefined);

      await unsubscribeChannel("ch-1");

      expect(tauri.wsUnsubscribe).toHaveBeenCalledWith("ch-1");
      expect(wsState.subscribedChannels.has("ch-1")).toBe(false);
    });

    it("no-ops if not subscribed", async () => {
      await unsubscribeChannel("ch-1");

      expect(tauri.wsUnsubscribe).not.toHaveBeenCalled();
    });
  });

  describe("sendTyping", () => {
    it("sends typing indicator", async () => {
      vi.useFakeTimers();
      try {
        // Advance far enough to clear any prior debounce
        vi.advanceTimersByTime(5000);
        vi.mocked(tauri.wsTyping).mockResolvedValue(undefined);

        await sendTyping("ch-1");

        expect(tauri.wsTyping).toHaveBeenCalledWith("ch-1");
      } finally {
        vi.useRealTimers();
      }
    });

    it("debounces within 3 seconds", async () => {
      vi.useFakeTimers();
      try {
        // Advance well past any prior debounce window (module-level lastTypingSent persists)
        vi.setSystemTime(Date.now() + 10000);
        vi.mocked(tauri.wsTyping).mockResolvedValue(undefined);

        await sendTyping("ch-1"); // First call succeeds
        await sendTyping("ch-1"); // Should be debounced (< 3s since first)

        expect(tauri.wsTyping).toHaveBeenCalledTimes(1);
      } finally {
        vi.useRealTimers();
      }
    });
  });

  describe("stopTyping", () => {
    it("sends stop typing", async () => {
      vi.mocked(tauri.wsStopTyping).mockResolvedValue(undefined);

      await stopTyping("ch-1");

      expect(tauri.wsStopTyping).toHaveBeenCalledWith("ch-1");
    });
  });

  describe("getTypingUsers", () => {
    it("returns empty array for unknown channel", () => {
      expect(getTypingUsers("ch-1")).toEqual([]);
    });

    it("returns users from typing state", () => {
      setTypingState("byChannel", "ch-1", new Set(["user-1", "user-2"]));

      const users = getTypingUsers("ch-1");
      expect(users).toHaveLength(2);
      expect(users).toContain("user-1");
      expect(users).toContain("user-2");
    });
  });

  describe("isConnected", () => {
    it("returns true when connected", () => {
      setWsState({ status: "connected" });

      expect(isConnected()).toBe(true);
    });

    it("returns false when not connected", () => {
      expect(isConnected()).toBe(false);
    });
  });

  describe("cleanupWebSocket", () => {
    it("does not throw", async () => {
      await expect(cleanupWebSocket()).resolves.not.toThrow();
    });
  });
});
