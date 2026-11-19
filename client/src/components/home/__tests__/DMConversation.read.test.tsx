// N1: an open conversation on Home is marked read through the DM route, on
// opening and when a new message arrives while the window is active.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render } from "@solidjs/testing-library";
import { createStore } from "solid-js/store";

const api = vi.hoisted(() => ({ markDMAsRead: vi.fn(async () => {}) }));
const [msgs, setMsgs] = createStore({ byChannel: {} as Record<string, unknown[]> });

vi.mock("@/lib/tauri", () => ({
  markDMAsRead: api.markDMAsRead,
  markAllDMsRead: vi.fn(),
  getDMList: vi.fn(),
  createDM: vi.fn(),
  startDMCall: vi.fn(),
  joinVoice: vi.fn(),
  uploadDMAvatar: vi.fn(),
  updateDMName: vi.fn(),
  validateFileSize: vi.fn(),
  getUploadLimitText: () => "5 MB",
}));
vi.mock("@/stores/websocket", () => ({ subscribeChannel: vi.fn(), waitForConnection: vi.fn() }));
vi.mock("@/stores/auth", () => ({ currentUser: () => ({ id: "me" }) }));
vi.mock("@/stores/call", () => ({
  callState: { currentCall: { status: "idle" } },
  startCall: vi.fn(),
  endCall: vi.fn(),
  isInCallForChannel: () => false,
}));
vi.mock("@/stores/messages", () => ({
  loadInitialMessages: vi.fn(async () => {}),
  get messagesState() {
    return msgs;
  },
}));
vi.mock("@/components/messages/MessageList", () => ({ default: () => null }));
vi.mock("@/components/messages/MessageInput", () => ({ default: () => null }));
vi.mock("@/components/messages/TypingIndicator", () => ({ default: () => null }));
vi.mock("@/components/call", () => ({ CallBanner: () => null }));
vi.mock("@/components/ui/Toast", () => ({ showToast: vi.fn() }));

import DMConversation from "../DMConversation";
import { dmsState, setDmsState, getTotalUnreadCount, updateDMLastMessage } from "@/stores/dms";

const anna = { user_id: "u2", username: "anna", display_name: "Anna", avatar_url: null, joined_at: "" };
const message = (id: string, extra: Record<string, unknown> = {}) => ({
  id,
  channel_id: "dm-1",
  parent_id: null,
  content: "hi",
  created_at: new Date().toISOString(),
  author: { id: "u2", username: "anna", display_name: "Anna" },
  ...extra,
});

beforeEach(() => {
  vi.spyOn(document, "hasFocus").mockReturnValue(true);
  setMsgs({ byChannel: { "dm-1": [message("m1"), message("m2")] } });
  setDmsState({
    dms: [
      {
        id: "dm-1",
        name: "Anna",
        channel_type: "dm",
        participants: [anna],
        unread_count: 2,
        last_message_id: "m2",
        last_read_message_id: "m0",
        last_message: null,
        created_at: "2025-01-01T00:00:00Z",
      } as never,
    ],
    selectedDMId: "dm-1",
    isShowingFriends: false,
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  api.markDMAsRead.mockClear();
});

describe("DMConversation read state", () => {
  it("marks the conversation read on opening, so Home and the title drop it", () => {
    render(() => <DMConversation />);
    expect(api.markDMAsRead).toHaveBeenCalledWith("dm-1", "m2");
    expect(dmsState.dms[0].unread_count).toBe(0);
    expect(getTotalUnreadCount()).toBe(0);
  });

  it("marks a new message read while the conversation is open", () => {
    render(() => <DMConversation />);
    api.markDMAsRead.mockClear();
    setMsgs("byChannel", "dm-1", (list) => [...list, message("m3")]);
    updateDMLastMessage("dm-1", message("m3") as never);
    expect(api.markDMAsRead).toHaveBeenCalledWith("dm-1", "m3");
    expect(dmsState.dms[0].unread_count).toBe(0);
  });

  it("skips thread replies and unsent messages as cursor", () => {
    setMsgs({
      byChannel: {
        "dm-1": [message("m2"), message("r1", { parent_id: "m2" }), message("pending:x")],
      },
    });
    render(() => <DMConversation />);
    expect(api.markDMAsRead).toHaveBeenCalledWith("dm-1", "m2");
  });

  it("leaves it unread while the window is in the background", () => {
    vi.spyOn(document, "hasFocus").mockReturnValue(false);
    render(() => <DMConversation />);
    expect(api.markDMAsRead).not.toHaveBeenCalled();
    expect(getTotalUnreadCount()).toBe(2);
  });
});
