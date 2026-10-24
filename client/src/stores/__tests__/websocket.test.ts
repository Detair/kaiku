import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { listenMock, invokeMock, tauriMode } = vi.hoisted(() => ({
  listenMock: vi.fn(),
  invokeMock: vi.fn(),
  tauriMode: { native: false },
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));
vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/core")>()),
  isTauri: () => tauriMode.native,
  invoke: invokeMock,
}));

vi.mock("@/lib/tauri", async () => {
  // The real auth cache + session-expiry dedupe, so the test can observe them.
  const common =
    await vi.importActual<typeof import("@/lib/tauri/common")>(
      "@/lib/tauri/common",
    );
  return {
    expireNativeSession: common.expireNativeSession,
    getAuthContext: common.getAuthContext,
    setNativeAuthInfo: common.setNativeAuthInfo,
    refreshAccessToken: common.refreshAccessToken,
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
    isPasswordChangePending: vi.fn().mockResolvedValue(false),
  };
});

vi.mock("@/stores/auth", () => ({
  currentUser: vi.fn(() => ({ id: "me", username: "me" })),
}));

// Stub heavy cross-store dependencies to prevent import side effects
vi.mock("@/stores/presence", () => ({
  updateUserPresence: vi.fn(),
  updateUserActivity: vi.fn(),
}));

vi.mock("@/stores/messages", () => {
  const messagesState: { byChannel: Record<string, { id: string }[]> } = {
    byChannel: {},
  };
  return {
    addMessage: vi.fn(),
    removeMessage: vi.fn(),
    messagesState,
    setMessagesState: vi.fn(),
    loadInitialMessages: vi.fn().mockResolvedValue(undefined),
    catchUpChannel: vi.fn().mockResolvedValue("caught_up"),
    catchUpAnchor: (id: string) =>
      [...(messagesState.byChannel[id] ?? [])]
        .reverse()
        .find((m) => !m.id.startsWith("pending:"))?.id,
    clearChannelMessages: vi.fn((id: string) => {
      delete messagesState.byChannel[id];
    }),
    getChannelMessages: (id: string) => messagesState.byChannel[id] ?? [],
  };
});

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
  channelsState: { selectedChannelId: null, channels: [] as unknown[] },
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
  handleDmCreated: vi.fn(),
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
  handleServerEvent,
  getResyncWatermark,
} from "../websocket";
import { handleDmCreated } from "@/stores/dms";
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

    it("a new session does not inherit the previous session's watermark", async () => {
      await handleServerEvent({
        type: "ready",
        user_id: "a",
        server_time: "2026-11-01T10:00:00Z",
      });
      expect(getResyncWatermark()).toBe("2026-11-01T10:00:00Z");

      await cleanupWebSocket();

      expect(getResyncWatermark()).toBeNull();
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

    it("subscribes the active guild's text channels after the refresh", async () => {
      const { channelsState } = await import("@/stores/channels");
      const mutableChannels = channelsState as unknown as {
        channels: unknown[];
      };
      mutableGuilds.activeGuildId = "g1";
      // A channel created while offline shows up in the refreshed list.
      vi.mocked(refreshChannelsForGuild).mockImplementationOnce(async () => {
        mutableChannels.channels = [
          { id: "new-text", guild_id: "g1", channel_type: "text" },
          { id: "voice", guild_id: "g1", channel_type: "voice" },
        ];
      });
      vi.mocked(tauri.wsSubscribe).mockResolvedValue(undefined);
      try {
        await refreshUnreadStateAfterReconnect();

        expect(tauri.wsSubscribe).toHaveBeenCalledWith("new-text");
        expect(tauri.wsSubscribe).not.toHaveBeenCalledWith("voice");
      } finally {
        mutableChannels.channels = [];
      }
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

  describe("C2/C3 server events", () => {
    it("ready stores server_time as watermark", async () => {
      await handleServerEvent({
        type: "ready",
        user_id: "u",
        server_time: "2026-11-01T10:00:00Z",
      });

      expect(getResyncWatermark()).toBe("2026-11-01T10:00:00Z");
    });

    it("dm_created routes to handleDmCreated", async () => {
      const channel = { id: "dm-new" } as never;

      await handleServerEvent({ type: "dm_created", channel });

      expect(handleDmCreated).toHaveBeenCalledWith(channel);
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
      listenMock
        .mockReset()
        .mockImplementation(async (name: string, cb: Listener) => {
          listeners.set(name, cb);
          return () => listeners.delete(name);
        });
      tauriMode.native = true;
      vi.resetModules();
      ws = await import("../websocket");
      nativeTauri = await import("@/lib/tauri");
      // Shared mock state survives resetModules; start each test clean.
      const { channelsState } = await import("@/stores/channels");
      const { guildsState } = await import("@/stores/guilds");
      const { messagesState } = await import("@/stores/messages");
      (channelsState as unknown as { channels: unknown[] }).channels = [];
      (guildsState as { activeGuildId: string | null }).activeGuildId = null;
      (
        messagesState as unknown as { byChannel: Record<string, unknown[]> }
      ).byChannel = {};
      await ws.initWebSocket();
    });

    afterEach(async () => {
      await ws.disconnect();
      tauriMode.native = false;
    });

    it("registers one server-event listener plus the status listeners", () => {
      expect([...listeners.keys()].sort()).toEqual([
        "ws:auth_rejected",
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
      await fire("ws:server_event", {
        type: "error",
        code: "x",
        message: "Nope",
      });

      expect(ws.wsState.error).toBe("Nope");
    });

    it("a guild opened while the socket was down is subscribed after reconnect", async () => {
      const { channelsState } = await import("@/stores/channels");
      const { guildsState } = await import("@/stores/guilds");
      vi.mocked(nativeTauri.wsSubscribe).mockResolvedValue(undefined);
      await ws.connect();
      await fire("ws:reconnecting", 1);

      // loadChannelsForGuild ran during the outage: REST list, no subscribe.
      (guildsState as { activeGuildId: string | null }).activeGuildId = "g1";
      (channelsState as unknown as { channels: unknown[] }).channels = [
        { id: "t1", guild_id: "g1", channel_type: "text" },
        { id: "t2", guild_id: "g1", channel_type: "text" },
        { id: "v1", guild_id: "g1", channel_type: "voice" },
      ];

      await fire("ws:connected");

      const subscribed = vi
        .mocked(nativeTauri.wsSubscribe)
        .mock.calls.map((c) => c[0]);
      expect(subscribed).toEqual(expect.arrayContaining(["t1", "t2"]));
      expect(subscribed).not.toContain("v1");
      expect(ws.wsState.subscribedChannels.has("t1")).toBe(true);
      expect(ws.wsState.subscribedChannels.has("t2")).toBe(true);
    });

    const ready = (server_time = "2026-11-01T10:00:00Z") =>
      fire("ws:server_event", { type: "ready", user_id: "me", server_time });
    const subscribed = (channel_id: string) =>
      fire("ws:server_event", { type: "subscribed", channel_id });
    const setCache = async (byChannel: Record<string, { id: string }[]>) => {
      const { messagesState } = await import("@/stores/messages");
      (
        messagesState as unknown as { byChannel: Record<string, unknown[]> }
      ).byChannel = byChannel;
    };

    it("handleReconnect catches up the open channel and clears the others", async () => {
      const { channelsState } = await import("@/stores/channels");
      const { catchUpChannel, getChannelMessages } =
        await import("@/stores/messages");
      vi.mocked(nativeTauri.wsSubscribe).mockResolvedValue(undefined);
      await ws.connect();
      await ready("2026-11-01T09:00:00Z");
      await ws.subscribeChannel("c-open");
      await ws.subscribeChannel("c-other");
      await setCache({ "c-open": [{ id: "m1" }], "c-other": [{ id: "x1" }] });
      (channelsState as { selectedChannelId: string | null }).selectedChannelId =
        "c-open";
      try {
        await fire("ws:reconnecting", 1);
        await fire("ws:connected");
        await ready();
        await subscribed("c-open");

        expect(catchUpChannel).toHaveBeenCalledTimes(1);
        expect(vi.mocked(catchUpChannel).mock.calls[0][0]).toBe("c-open");
        expect(vi.mocked(catchUpChannel).mock.calls[0][2]).toBe("m1");
        expect(getChannelMessages("c-other")).toEqual([]);
      } finally {
        (
          channelsState as { selectedChannelId: string | null }
        ).selectedChannelId = null;
      }
    });

    it("reconnect re-subscribes, waits for ready and the ack, then catches up", async () => {
      const { channelsState } = await import("@/stores/channels");
      const { catchUpChannel } = await import("@/stores/messages");
      const order: string[] = [];
      vi.mocked(nativeTauri.wsSubscribe).mockImplementation(
        async (id: string) => {
          order.push(`subscribe:${id}`);
        },
      );
      vi.mocked(catchUpChannel).mockImplementation(async (id: string) => {
        order.push(`catchup:${id}`);
        return "caught_up";
      });
      await ws.connect();
      await ws.subscribeChannel("c1");
      await setCache({ c1: [{ id: "m1" }] });
      (channelsState as { selectedChannelId: string | null }).selectedChannelId =
        "c1";
      order.length = 0;
      try {
        await fire("ws:reconnecting", 1);
        await fire("ws:connected");
        expect(order).toEqual(["subscribe:c1"]);

        await ready();
        expect(order).toEqual(["subscribe:c1"]);

        await subscribed("c1");
        expect(order).toEqual(["subscribe:c1", "catchup:c1"]);
      } finally {
        (
          channelsState as { selectedChannelId: string | null }
        ).selectedChannelId = null;
      }
    });

    it("an open DM is caught up after reconnect", async () => {
      const { dmsState } = await import("@/stores/dms");
      const { catchUpChannel } = await import("@/stores/messages");
      vi.mocked(nativeTauri.wsSubscribe).mockResolvedValue(undefined);
      await ws.connect();
      await setCache({ "dm-1": [{ id: "d1" }] });
      const dms = dmsState as { selectedDMId: string | null };
      dms.selectedDMId = "dm-1";
      try {
        await fire("ws:reconnecting", 1);
        await fire("ws:connected");
        await ready();
        await subscribed("dm-1");

        expect(nativeTauri.wsSubscribe).toHaveBeenCalledWith("dm-1");
        expect(vi.mocked(catchUpChannel).mock.calls[0][0]).toBe("dm-1");
      } finally {
        dms.selectedDMId = null;
      }
    });

    it("an open channel with an empty cache is loaded after reconnect", async () => {
      const { channelsState } = await import("@/stores/channels");
      const { catchUpChannel } = await import("@/stores/messages");
      vi.mocked(nativeTauri.wsSubscribe).mockResolvedValue(undefined);
      await ws.connect();
      await setCache({ "c-quiet": [] });
      (channelsState as { selectedChannelId: string | null }).selectedChannelId =
        "c-quiet";
      try {
        await fire("ws:reconnecting", 1);
        await fire("ws:connected");
        await ready();
        await subscribed("c-quiet");

        expect(nativeTauri.wsSubscribe).toHaveBeenCalledWith("c-quiet");
        // No anchor: catchUpChannel falls back to loadInitialMessages.
        expect(catchUpChannel).toHaveBeenCalledWith(
          "c-quiet",
          undefined,
          undefined,
        );
      } finally {
        (
          channelsState as { selectedChannelId: string | null }
        ).selectedChannelId = null;
      }
    });

    it("a gap of more than 7 days reloads the open channel instead of catching up", async () => {
      const { channelsState } = await import("@/stores/channels");
      const { catchUpChannel, loadInitialMessages } =
        await import("@/stores/messages");
      vi.mocked(nativeTauri.wsSubscribe).mockResolvedValue(undefined);
      await ws.connect();
      await ready("2026-10-01T10:00:00Z");
      await setCache({ c1: [{ id: "m1" }] });
      (channelsState as { selectedChannelId: string | null }).selectedChannelId =
        "c1";
      try {
        await fire("ws:reconnecting", 1);
        await fire("ws:connected");
        await ready("2026-11-01T10:00:00Z");
        await subscribed("c1");

        expect(catchUpChannel).not.toHaveBeenCalled();
        expect(loadInitialMessages).toHaveBeenCalledWith("c1");
      } finally {
        (
          channelsState as { selectedChannelId: string | null }
        ).selectedChannelId = null;
      }
    });

    it("ws:auth_rejected stops reconnecting and reports an expired session", async () => {
      await ws.connect();
      await fire("ws:reconnecting", 1);
      vi.mocked(nativeTauri.wsDisconnect)
        .mockClear()
        .mockResolvedValue(undefined);
      const expired = vi.fn();
      window.addEventListener("kaiku:session-expired", expired);
      try {
        await fire("ws:auth_rejected");

        expect(ws.wsState.status).toBe("disconnected");
        expect(nativeTauri.wsDisconnect).toHaveBeenCalledTimes(1);
        expect(expired).toHaveBeenCalledTimes(1);

        // The supervisor is stopped: no further "try now" reaches Rust.
        vi.mocked(nativeTauri.wsReconnectNow).mockClear();
        window.dispatchEvent(new Event("online"));
        expect(nativeTauri.wsReconnectNow).not.toHaveBeenCalled();
      } finally {
        window.removeEventListener("kaiku:session-expired", expired);
      }
    });

    it("ws:auth_rejected for a pending password change shows the change screen instead of ending the session", async () => {
      await ws.connect();
      vi.mocked(nativeTauri.wsDisconnect)
        .mockClear()
        .mockResolvedValue(undefined);
      vi.mocked(nativeTauri.isPasswordChangePending).mockResolvedValueOnce(true);
      const expired = vi.fn();
      const required = vi.fn();
      window.addEventListener("kaiku:session-expired", expired);
      window.addEventListener("kaiku:password-change-required", required);
      try {
        await fire("ws:auth_rejected");

        expect(ws.wsState.status).toBe("disconnected");
        expect(nativeTauri.wsDisconnect).toHaveBeenCalledTimes(1);
        expect(required).toHaveBeenCalledTimes(1);
        expect(expired).not.toHaveBeenCalled();
      } finally {
        window.removeEventListener("kaiku:session-expired", expired);
        window.removeEventListener("kaiku:password-change-required", required);
      }
    });

    it("ws:auth_rejected ends the session when the account check fails", async () => {
      // A live session hands out a token again (resets the expiry dedupe).
      nativeTauri.setNativeAuthInfo({
        server_url: "https://s.example",
        access_token: "bearer",
      } as never);
      await ws.connect();
      vi.mocked(nativeTauri.wsDisconnect).mockResolvedValue(undefined);
      vi.mocked(nativeTauri.isPasswordChangePending).mockRejectedValueOnce(
        new Error("HTTP 401"),
      );
      const expired = vi.fn();
      window.addEventListener("kaiku:session-expired", expired);
      try {
        await fire("ws:auth_rejected");
        expect(expired).toHaveBeenCalledTimes(1);
      } finally {
        window.removeEventListener("kaiku:session-expired", expired);
      }
    });

    it("auth_rejected then refresh_session → null reports expiry once and drops the bearer", async () => {
      nativeTauri.setNativeAuthInfo({
        server_url: "https://s.example",
        access_token: "old-bearer",
      } as never);
      expect((await nativeTauri.getAuthContext()).accessToken).toBe(
        "old-bearer",
      );
      invokeMock.mockResolvedValue(null);
      vi.mocked(nativeTauri.wsDisconnect).mockResolvedValue(undefined);
      const expired = vi.fn();
      window.addEventListener("kaiku:session-expired", expired);
      invokeMock.mockClear();
      try {
        await fire("ws:auth_rejected");
        expect(await nativeTauri.refreshAccessToken()).toBe(false);

        expect(expired).toHaveBeenCalledTimes(1);
        invokeMock.mockResolvedValue(null); // get_auth_info reload
        expect((await nativeTauri.getAuthContext()).accessToken).not.toBe(
          "old-bearer",
        );
      } finally {
        window.removeEventListener("kaiku:session-expired", expired);
      }
    });

    it("ws:disconnected from Rust ends the connection", async () => {
      await ws.connect();

      await fire("ws:disconnected");

      expect(ws.wsState.status).toBe("disconnected");
    });

    it("two overlapping connect() calls after ws:disconnected recover the retained subscription once", async () => {
      const { loadDMs } = await import("@/stores/dms");
      vi.mocked(nativeTauri.wsSubscribe).mockResolvedValue(undefined);
      vi.mocked(nativeTauri.wsConnect).mockResolvedValue(undefined);
      await ws.connect();
      await ws.subscribeChannel("c1");
      await fire("ws:disconnected");
      expect(ws.wsState.status).toBe("disconnected");
      expect(ws.wsState.subscribedChannels.has("c1")).toBe(true);
      vi.mocked(nativeTauri.wsSubscribe).mockClear();
      vi.mocked(nativeTauri.wsConnect).mockClear();
      vi.mocked(loadDMs).mockClear();

      // The first call opens the socket; the second one overlaps it while
      // the open is in flight (supervisor already opening → no-op).
      let open!: () => void;
      vi.mocked(nativeTauri.wsConnect).mockReturnValueOnce(
        new Promise<void>((resolve) => {
          open = resolve;
        }),
      );
      const first = ws.connect();
      const second = ws.connect();
      await second;
      open();
      await first;
      await new Promise((resolve) => setTimeout(resolve, 0));

      expect(nativeTauri.wsConnect).toHaveBeenCalledTimes(1);
      expect(nativeTauri.wsSubscribe).toHaveBeenCalledTimes(1);
      expect(nativeTauri.wsSubscribe).toHaveBeenCalledWith("c1");
      expect(loadDMs).toHaveBeenCalledTimes(1);
      expect(ws.wsState.subscribedChannels.has("c1")).toBe(true);
      expect(ws.isConnected()).toBe(true);
    });

    it("a late ws_connect failure after Rust reconnected leaves the live connection alone", async () => {
      const { loadDMs } = await import("@/stores/dms");
      vi.mocked(loadDMs).mockClear();
      let reject!: (err: Error) => void;
      vi.mocked(nativeTauri.wsConnect).mockReturnValueOnce(
        new Promise<void>((_, rej) => {
          reject = rej;
        }),
      );
      const connecting = ws.connect();
      await new Promise((resolve) => setTimeout(resolve, 0));

      // Rust: first attempt failed, retry connected, before the command settles.
      await fire("ws:reconnecting", 1);
      expect(ws.wsState.status).toBe("reconnecting");
      await fire("ws:connected");
      expect(ws.isConnected()).toBe(true);
      expect(loadDMs).toHaveBeenCalledTimes(1);

      reject(new Error("WebSocket connection failed; retrying"));
      await expect(connecting).rejects.toThrow("retrying");

      expect(ws.isConnected()).toBe(true);
      expect(ws.wsState.error).toBeNull();
    });
  });
});
