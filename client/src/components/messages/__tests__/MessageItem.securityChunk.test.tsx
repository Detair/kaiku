import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup } from "@solidjs/testing-library";

vi.mock("@/components/ui/ContextMenu", () => ({ showContextMenu: vi.fn(), showContextMenuAt: vi.fn() }));
vi.mock("@/components/emoji/PositionedEmojiPicker", () => ({ default: () => null }));
// The lazy notice card's chunk cannot be loaded (offline, a new deploy).
vi.mock("@/components/security/SecurityNoticeCard", () => {
  throw new Error("chunk load failed");
});

import MessageItem from "../MessageItem";
import type { Message } from "@/lib/types";
import { notice } from "@/components/security/__tests__/fixtures";

afterEach(cleanup);

describe("security messages", () => {
  it("a notice card that cannot load still points to the security log", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const msg = {
      id: "m1",
      channel_id: "k1",
      author: { id: "6b61696b-7500-4000-8000-000000000001", username: "kaiku", display_name: "Kaiku", avatar_url: null, status: "online", is_system: true },
      content: "New sign-in",
      encrypted: false,
      attachments: [],
      reply_to: null,
      parent_id: null,
      thread_reply_count: 0,
      thread_last_reply_at: null,
      edited_at: null,
      created_at: "2026-11-03T10:00:00Z",
      mention_type: null,
      pinned: false,
      message_type: "security",
      security_notice: notice(),
    } as unknown as Message;
    render(() => <MessageItem message={msg} />);
    expect(await screen.findByTestId("security-notice-missing")).toBeTruthy();
    expect(screen.queryByTestId("message-item")).toBeNull();
  });
});
