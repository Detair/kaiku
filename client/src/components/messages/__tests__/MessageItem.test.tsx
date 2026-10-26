import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup } from "@solidjs/testing-library";

vi.mock("@/lib/tauri", () => ({
  addReaction: vi.fn(),
  removeReaction: vi.fn(),
  deleteMessage: vi.fn(),
  editMessage: vi.fn(),
  getAccessToken: () => "tok",
  fetchSignedUrl: vi.fn(),
}));
vi.mock("@/components/ui/ContextMenu", () => ({
  showContextMenu: vi.fn(),
  showContextMenuAt: vi.fn(),
}));
vi.mock("@/lib/createLongPress", () => ({
  createLongPress: () => ({
    onContextMenu: vi.fn(),
    onTouchStart: vi.fn(),
    onTouchEnd: vi.fn(),
    onTouchMove: vi.fn(),
    onTouchCancel: vi.fn(),
  }),
}));
vi.mock("@/lib/contextMenuBuilders", () => ({
  showUserContextMenu: vi.fn(),
  triggerReport: vi.fn(),
}));
vi.mock("@/stores/threads", () => ({
  openThread: vi.fn(),
  threadsState: { threadInfoCache: {} },
}));
vi.mock("@/stores/channelPins", () => ({
  pinMessageAction: vi.fn(),
  unpinMessageAction: vi.fn(),
}));
vi.mock("@/stores/permissions", () => ({ memberHasPermission: () => false }));
vi.mock("@/stores/guilds", () => ({ isGuildOwner: () => false }));
vi.mock("@/stores/auth", () => ({
  currentUser: () => ({ id: "me" }),
  authState: { user: { id: "me" } },
}));
vi.mock("@/stores/messages", () => ({
  removeMessage: vi.fn(),
  updateMessage: vi.fn(),
  editingMessageId: () => null,
  setEditingMessageId: vi.fn(),
}));
vi.mock("@/components/ui/Toast", () => ({ showToast: vi.fn() }));
vi.mock("@/components/emoji/PositionedEmojiPicker", () => ({ default: () => null }));

import MessageItem from "../MessageItem";
import type { Message } from "@/lib/types";

afterEach(cleanup);

const message = {
  id: "m1",
  channel_id: "c1",
  author: { id: "u2", username: "anna", display_name: "Anna", avatar_url: null },
  content: "hello",
  message_type: "user",
  created_at: "2026-10-20T10:00:00Z",
  edited_at: "2026-10-20T11:00:00Z",
  attachments: [],
  reactions: [],
  pinned: false,
  encrypted: false,
} as unknown as Message;

describe("MessageItem", () => {
  it("edited tooltip contains the formatted time", () => {
    render(() => <MessageItem message={message} />);
    const badge = screen.getByText("(edited)");
    expect(badge.getAttribute("title")?.startsWith("Edited ")).toBe(true);
    expect(badge.getAttribute("title")!.length).toBeGreaterThan("Edited ".length);
  });
});
