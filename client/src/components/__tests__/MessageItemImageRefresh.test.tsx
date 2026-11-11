/**
 * R5 / Codex 10: an inline image whose URL fails to load (e.g. expired
 * while it waited off-screen) asks for a fresh URL exactly once.
 * (Kept outside components/messages/ on purpose: that folder is owned by a
 * parallel work stream.)
 */
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";

const urls = vi.hoisted(() => ({
  getAttachmentUrl: vi.fn(),
  invalidateAttachmentUrl: vi.fn(),
}));

vi.mock("@/lib/attachmentUrls", () => ({
  getAttachmentUrl: urls.getAttachmentUrl,
  invalidateAttachmentUrl: urls.invalidateAttachmentUrl,
  isAttachmentTooLarge: () => false,
}));
vi.mock("@/components/ui/ContextMenu", () => ({ showContextMenu: vi.fn(), showContextMenuAt: vi.fn() }));
vi.mock("@/components/emoji/PositionedEmojiPicker", () => ({ default: () => null }));

import MessageItem from "@/components/messages/MessageItem";
import type { Message } from "@/lib/types";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

function imageMessage(): Message {
  return {
    id: "m1",
    channel_id: "k1",
    author: { id: "u1", username: "ann", display_name: "Ann", avatar_url: null, status: "online" },
    content: "",
    encrypted: false,
    attachments: [
      { id: "att-1", filename: "cat.png", mime_type: "image/png", size: 1000 },
    ],
    reply_to: null,
    parent_id: null,
    thread_reply_count: 0,
    thread_last_reply_at: null,
    edited_at: null,
    created_at: "2026-11-11T10:00:00Z",
    mention_type: null,
    pinned: false,
  } as unknown as Message;
}

describe("MessageItem image URL refresh (R5)", () => {
  it("refreshes the URL once on an image error, never in a loop", async () => {
    urls.getAttachmentUrl
      .mockResolvedValueOnce("https://s3.example/expired")
      .mockResolvedValueOnce("https://s3.example/fresh")
      .mockResolvedValue("https://s3.example/again");
    const { container } = render(() => <MessageItem message={imageMessage()} />);
    const img = await waitFor(() => {
      const el = container.querySelector("img[alt='cat.png']") as HTMLImageElement | null;
      expect(el?.getAttribute("src")).toBe("https://s3.example/expired");
      return el!;
    });

    fireEvent.error(img);
    await waitFor(() =>
      expect(
        (container.querySelector("img[alt='cat.png']") as HTMLImageElement).getAttribute("src"),
      ).toBe("https://s3.example/fresh"),
    );
    expect(urls.invalidateAttachmentUrl).toHaveBeenCalledWith("att-1", undefined);

    // A second failure does not ask again.
    fireEvent.error(container.querySelector("img[alt='cat.png']") as HTMLImageElement);
    await new Promise((r) => setTimeout(r, 20));
    expect(urls.getAttachmentUrl).toHaveBeenCalledTimes(2);
    expect(urls.invalidateAttachmentUrl).toHaveBeenCalledTimes(1);
  });
});
