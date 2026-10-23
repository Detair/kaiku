import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { listenMock } = vi.hoisted(() => ({ listenMock: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

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
  wsPing: vi.fn(),
  wsReconnectNow: vi.fn().mockResolvedValue(undefined),
  setBrowserSocketHooks: vi.fn(),
  ensureFreshAccessToken: vi.fn().mockResolvedValue(true),
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
  updateChannelLastMessageId: vi.fn(),
  refreshChannelsForGuild: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@/stores/guilds", () => ({
  guildsState: { activeGuildId: null as string | null },
  getGuildIdForChannel: vi.fn(),
  incrementGuildUnread: vi.fn(),
  DISCOVERY_SENTINEL: "__discovery__",
}));

vi.mock("@/stores/dms", () => ({
  dmsState: { selectedDMId: null, isShowingFriends: false },
  handleDMReadEvent: vi.fn(),
  handleDMNameUpdated: vi.fn(),
  updateDMLastMessage: vi.fn(),
  loadDMs: vi.fn().mockResolvedValue(undefined),
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
  refreshUnreadStateAfterReconnect,
} from "../websocket";
import { refreshChannelsForGuild } from "@/stores/channels";
import { handlePreferencesUpdated } from "@/stores/preferences";
import { guildsState } from "@/stores/guilds";
import { loadDMs } from "@/stores/dms";

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

  afterEach(async () => {
    // Stop the reconnect supervisor a test may have started.
    await disconnect();
  });

  describe("connect", () => {
    it("connects successfully", async () => {
      vi.mocked(tauri.wsConnect).mockResolvedValue(undefined);

      await connect();

      expect(tauri.wsConnect).toHaveBeenCalled();
      // Status is set to "connecting" before await
    });

    it("connect() while the supervisor is already running does not run recovery", async () => {
      vi.mocked(tauri.wsConnect).mockResolvedValue(undefined);
      await connect();
      setWsState("subscribedChannels", new Set(["ch-1"]));

      await connect();

      expect(tauri.wsConnect).toHaveBeenCalledTimes(1);
      expect(tauri.wsSubscribe).not.toHaveBeenCalled();
      expect(loadDMs).not.toHaveBeenCalled();
    });

    it("connect() while a retry is pending does not fake a connection", async () => {
      vi.mocked(tauri.wsConnect).mockRejectedValueOnce(new Error("down"));
      await expect(connect()).rejects.toThrow("down");
      expect(wsState.status).toBe("reconnecting");
      setWsState("subscribedChannels", new Set(["ch-1"]));

      await connect();

      expect(wsState.status).toBe("reconnecting");
      expect(tauri.wsSubscribe).not.toHaveBeenCalled();
    });

    it("sets error on failure", async () => {
      vi.mocked(tauri.wsConnect).mockRejectedValue(
        new Error("Connection failed"),
      );

      await expect(connect()).rejects.toThrow("Connection failed");
      // Browser mode keeps retrying with backoff after a failed first attempt.
      expect(wsState.status).toBe("reconnecting");
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

  describe("refreshUnreadStateAfterReconnect", () => {
    const mutableGuilds = guildsState as { activeGuildId: string | null };

    beforeEach(() => {
      mutableGuilds.activeGuildId = null;
      vi.mocked(refreshChannelsForGuild).mockClear().mockResolvedValue();
      vi.mocked(loadDMs).mockClear().mockResolvedValue();
    });

    it("refreshes the active guild's channels and the DM list", async () => {
      mutableGuilds.activeGuildId = "g1";

      await refreshUnreadStateAfterReconnect();

      expect(refreshChannelsForGuild).toHaveBeenCalledWith("g1");
      expect(loadDMs).toHaveBeenCalled();
    });

    it("skips guild refresh when on home (no active guild)", async () => {
      await refreshUnreadStateAfterReconnect();

      expect(refreshChannelsForGuild).not.toHaveBeenCalled();
      expect(loadDMs).toHaveBeenCalled();
    });

    it("skips guild refresh on the discovery view", async () => {
      mutableGuilds.activeGuildId = "__discovery__";

      await refreshUnreadStateAfterReconnect();

      expect(refreshChannelsForGuild).not.toHaveBeenCalled();
      expect(loadDMs).toHaveBeenCalled();
    });

    it("does not reject when a refresh fails", async () => {
      mutableGuilds.activeGuildId = "g1";
      vi.mocked(refreshChannelsForGuild).mockRejectedValueOnce(
        new Error("network"),
      );
      vi.mocked(loadDMs).mockRejectedValueOnce(new Error("network"));

      await expect(refreshUnreadStateAfterReconnect()).resolves.toBeUndefined();
    });
  });

  describe("browser reconnect", () => {
    type Hooks = { onClose(intentional: boolean): void; onFrame(): void };
    const lastHooks = (): Hooks => {
      const calls = vi.mocked(tauri.setBrowserSocketHooks).mock.calls;
      return calls[calls.length - 1][0] as Hooks;
    };

    afterEach(() => {
      vi.restoreAllMocks();
      vi.useRealTimers();
    });

    it("isConnected() is false after the browser socket closes", async () => {
      vi.mocked(tauri.wsConnect).mockResolvedValue(undefined);
      vi.mocked(tauri.getBrowserWebSocket).mockReturnValue(
        new EventTarget() as unknown as WebSocket,
      );

      await connect();
      expect(isConnected()).toBe(true);

      lastHooks().onClose(false);

      expect(isConnected()).toBe(false);
      expect(wsState.status).toBe("reconnecting");
    });

    it("one inbound frame after two reconnects is handled once", async () => {
      vi.useFakeTimers();
      vi.spyOn(Math, "random").mockReturnValue(0.5);
      const sockets = [new EventTarget(), new EventTarget(), new EventTarget()];
      let current = 0;
      vi.mocked(tauri.wsConnect).mockImplementation(async () => {
        current = vi.mocked(tauri.wsConnect).mock.calls.length - 1;
      });
      vi.mocked(tauri.getBrowserWebSocket).mockImplementation(
        () => sockets[current] as unknown as WebSocket,
      );

      await connect();
      lastHooks().onClose(false);
      await vi.advanceTimersByTimeAsync(1000);
      lastHooks().onClose(false);
      await vi.advanceTimersByTimeAsync(1000);

      expect(tauri.wsConnect).toHaveBeenCalledTimes(3);
      expect(current).toBe(2);
      expect(isConnected()).toBe(true);

      const frame = () =>
        new MessageEvent("message", {
          data: JSON.stringify({
            type: "preferences_updated",
            preferences: {},
            updated_at: "2026-10-22T00:00:00Z",
          }),
        });
      sockets[2].dispatchEvent(frame());
      // Old sockets must not carry a stale handler either.
      sockets[0].dispatchEvent(frame());
      sockets[1].dispatchEvent(frame());
      await vi.advanceTimersByTimeAsync(0);

      expect(handlePreferencesUpdated).toHaveBeenCalledTimes(1);
    });
  });

  describe("native mode", () => {
    type Listener = (event: { payload: unknown }) => unknown;
    let listeners: Map<string, Listener>;
    let ws: typeof import("../websocket");
    let nativeTauri: typeof import("@/lib/tauri");

    const fire = async (name: string, payload: unknown = null) => {
      const listener = listeners.get(name);
      expect(listener, `listener for ${name}`).toBeDefined();
      await listener!({ payload });
      await new Promise((resolve) => setTimeout(resolve, 0));
    };

    beforeEach(async () => {
      listeners = new Map();
      listenMock.mockReset().mockImplementation(async (name: string, cb: Listener) => {
        listeners.set(name, cb);
        return () => listeners.delete(name);
      });
      (window as unknown as Record<string, unknown>).__TAURI__ = {};
      vi.resetModules();
      ws = await import("../websocket");
      nativeTauri = await import("@/lib/tauri");
      await ws.initWebSocket();
    });

    afterEach(async () => {
      await ws.disconnect();
      delete (window as unknown as Record<string, unknown>).__TAURI__;
    });

    it("registers one server-event listener plus the status listeners", () => {
      expect([...listeners.keys()].sort()).toEqual([
        "ws:connected",
        "ws:connecting",
        "ws:disconnected",
        "ws:reconnecting",
        "ws:server_event",
      ]);
    });

    it("native ws:server_event is handled by the shared handler", async () => {
      const { addMessage } = await import("@/stores/messages");
      const message = { id: "m1", channel_id: "c1", content: "hi" };

      await fire("ws:server_event", {
        type: "message_new",
        channel_id: "c1",
        message,
      });

      expect(addMessage).toHaveBeenCalledWith(message);
    });

    it("native ws:connected after ws:reconnecting runs handleReconnect", async () => {
      const { loadDMs } = await import("@/stores/dms");
      await ws.connect();
      expect(ws.isConnected()).toBe(true);
      vi.mocked(loadDMs).mockClear();

      await fire("ws:reconnecting", 1);
      expect(ws.wsState.status).toBe("reconnecting");

      await fire("ws:connected");

      expect(ws.isConnected()).toBe(true);
      expect(loadDMs).toHaveBeenCalledTimes(1);
    });

    it("online event in native mode invokes ws_reconnect_now", async () => {
      await ws.connect();
      await fire("ws:reconnecting", 1);

      window.dispatchEvent(new Event("online"));

      expect(nativeTauri.wsReconnectNow).toHaveBeenCalledTimes(1);
      expect(ws.canRetryConnectionNow()).toBe(true);
    });

    it("a server error event is shown as the connection error", async () => {
      await fire("ws:server_event", { type: "error", code: "x", message: "Nope" });

      expect(ws.wsState.error).toBe("Nope");
    });

    it("ws:disconnected from Rust ends the connection", async () => {
      await ws.connect();

      await fire("ws:disconnected");

      expect(ws.wsState.status).toBe("disconnected");
    });
  });
});
