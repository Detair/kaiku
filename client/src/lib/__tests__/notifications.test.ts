import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const hoisted = vi.hoisted(() => ({
  native: true,
  windowFocused: false,
  sendNotification: vi.fn(),
  playNotification: vi.fn().mockResolvedValue(undefined),
  channelsState: { selectedChannelId: null as string | null },
  dmsState: { selectedDMId: null as string | null, isShowingFriends: false, dms: [] as { id: string }[] },
}));

vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => hoisted.native }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ isFocused: async () => hoisted.windowFocused }),
}));
vi.mock("@tauri-apps/plugin-notification", () => ({
  isPermissionGranted: async () => true,
  requestPermission: async () => "granted",
  sendNotification: hoisted.sendNotification,
}));
vi.mock("@/lib/sound", () => ({ playNotification: hoisted.playNotification }));
vi.mock("@/stores/auth", () => ({ currentUser: () => ({ id: "me" }) }));
vi.mock("@/stores/channels", () => ({
  getChannel: () => ({ id: "ch1", name: "general", guild_id: "g1", channel_type: "text" }),
  channelsState: hoisted.channelsState,
}));
vi.mock("@/stores/dms", () => ({ dmsState: hoisted.dmsState }));
vi.mock("@/stores/guilds", () => ({ guildsState: { guilds: [{ id: "g1", name: "Home" }] } }));
vi.mock("@/stores/messages", () => ({
  decryptMessageIfNeeded: vi.fn(),
  messagesState: { byChannel: {} },
  setMessagesState: vi.fn(),
}));
vi.mock("@/stores/threads", () => ({
  addThreadReply: vi.fn(),
  clearThreadUnread: vi.fn(),
  markThreadUnread: vi.fn(),
  removeThreadReply: vi.fn(),
  setThreadReadState: vi.fn(),
  threadsState: {},
  updateParentThreadIndicator: vi.fn(),
  updateThreadInfo: vi.fn(),
}));

import {
  cleanupNotifications,
  initNotifications,
  isAppFocused,
  sendOsNotification,
} from "@/lib/notifications";
import { handleMessageNotification } from "@/stores/websocket/messageEvents";
import type { Message } from "@/lib/types";

function message(channelId: string): Message {
  return {
    id: "m1",
    channel_id: channelId,
    author: { id: "alice", username: "alice", display_name: "Alice" },
    content: "hi",
    encrypted: false,
    mention_type: null,
  } as unknown as Message;
}

const event = { type: "message_channel" as const, channelId: "ch1", isDm: false };
const ctx = { username: "Alice", content: "hi", guildName: "Home", channelName: "general" };

describe("notification focus gating", () => {
  beforeEach(async () => {
    hoisted.native = true;
    hoisted.windowFocused = false;
    hoisted.channelsState.selectedChannelId = null;
    // Kaiku is visible (e.g. on a second screen): the page is not hidden.
    Object.defineProperty(document, "hidden", { configurable: true, get: () => false });
    cleanupNotifications();
    await initNotifications();
  });

  afterEach(() => {
    vi.clearAllMocks();
    vi.restoreAllMocks();
  });

  it("isAppFocused uses the native window focus in the desktop app", async () => {
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    hoisted.windowFocused = false;
    expect(await isAppFocused()).toBe(false);
    hoisted.windowFocused = true;
    expect(await isAppFocused()).toBe(true);
  });

  it("isAppFocused uses document focus in the browser", async () => {
    hoisted.native = false;
    vi.spyOn(document, "hasFocus").mockReturnValue(false);
    expect(await isAppFocused()).toBe(false);
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    expect(await isAppFocused()).toBe(true);
  });

  it("notifies when the window is visible but not focused", async () => {
    hoisted.windowFocused = false;
    await sendOsNotification(event, ctx, true);
    expect(hoisted.sendNotification).toHaveBeenCalledTimes(1);

    // The message's channel is open, but Kaiku is not focused: still notify.
    hoisted.channelsState.selectedChannelId = "ch1";
    await handleMessageNotification(message("ch1"));
    expect(hoisted.playNotification).toHaveBeenCalledTimes(1);
  });

  it("stays quiet when focused on the same channel", async () => {
    hoisted.windowFocused = true;
    hoisted.channelsState.selectedChannelId = "ch1";
    await handleMessageNotification(message("ch1"));
    expect(hoisted.playNotification).not.toHaveBeenCalled();

    // Focused, but a different channel is open: the message still notifies.
    hoisted.channelsState.selectedChannelId = "ch2";
    await handleMessageNotification(message("ch1"));
    expect(hoisted.playNotification).toHaveBeenCalledTimes(1);
  });
});
