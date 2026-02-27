import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/tauri", () => ({
  getMessages: vi.fn(),
  sendMessageWithStatus: vi.fn(),
  sendMessage: vi.fn(),
  getOurCurve25519Key: vi.fn(),
}));

vi.mock("@/stores/e2ee", () => ({
  e2eeStore: {
    status: vi.fn(() => ({ initialized: false })),
    decrypt: vi.fn(),
  },
}));

vi.mock("@/stores/auth", () => ({
  currentUser: vi.fn(() => ({ id: "me", username: "me" })),
}));

vi.mock("@/components/ui/Toast", () => ({
  showToast: vi.fn(),
}));

import * as tauri from "@/lib/tauri";
import { showToast } from "@/components/ui/Toast";
import type { Message, PaginatedMessages } from "@/lib/types";
import {
  messagesState,
  setMessagesState,
  loadMessages,
  loadInitialMessages,
  sendMessage,
  addMessage,
  updateMessage,
  removeMessage,
  getChannelMessages,
  isLoadingMessages,
  hasMoreMessages,
  clearChannelMessages,
  clearCurve25519KeyCache,
} from "../messages";

function createMessage(id: string, channelId = "ch-1"): Message {
  return {
    id,
    channel_id: channelId,
    author: {
      id: "user-1",
      username: "alice",
      display_name: "Alice",
      avatar_url: null,
      status: "online",
    },
    content: `content-${id}`,
    encrypted: false,
    attachments: [],
    reply_to: null,
    parent_id: null,
    thread_reply_count: 0,
    thread_last_reply_at: null,
    edited_at: null,
    created_at: new Date().toISOString(),
    mention_type: null,
    reactions: [],
  };
}

describe("messages store", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setMessagesState({
      byChannel: {},
      loadingChannels: {},
      hasMore: {},
      error: null,
    });
  });

  describe("initial state", () => {
    it("has empty message maps and no error", () => {
      expect(messagesState.byChannel).toEqual({});
      expect(messagesState.loadingChannels).toEqual({});
      expect(messagesState.hasMore).toEqual({});
      expect(messagesState.error).toBeNull();
    });
  });

  describe("loadMessages", () => {
    it("loads messages for a channel", async () => {
      const msgs = [createMessage("m1"), createMessage("m2")];
      vi.mocked(tauri.getMessages).mockResolvedValue({
        items: msgs,
        has_more: false,
        next_cursor: null,
      } as PaginatedMessages);

      await loadMessages("ch-1");

      // Messages are reversed (server returns newest-first)
      expect(messagesState.byChannel["ch-1"]).toHaveLength(2);
      expect(messagesState.byChannel["ch-1"][0].id).toBe("m2");
      expect(messagesState.byChannel["ch-1"][1].id).toBe("m1");
      expect(messagesState.hasMore["ch-1"]).toBe(false);
      expect(messagesState.loadingChannels["ch-1"]).toBe(false);
    });

    it("paginates using oldest message as cursor", async () => {
      setMessagesState("byChannel", "ch-1", [createMessage("m-old")]);
      vi.mocked(tauri.getMessages).mockResolvedValue({
        items: [createMessage("m-older")],
        has_more: true,
        next_cursor: "m-older",
      } as PaginatedMessages);

      await loadMessages("ch-1");

      expect(tauri.getMessages).toHaveBeenCalledWith("ch-1", "m-old", 50);
      expect(messagesState.hasMore["ch-1"]).toBe(true);
    });

    it("prevents concurrent loads for the same channel", async () => {
      let resolveFirst: (v: PaginatedMessages) => void;
      const firstCall = new Promise<PaginatedMessages>((resolve) => {
        resolveFirst = resolve;
      });
      vi.mocked(tauri.getMessages).mockReturnValueOnce(firstCall);

      const p1 = loadMessages("ch-1");
      const p2 = loadMessages("ch-1"); // Should be no-op

      resolveFirst!({
        items: [createMessage("m1")],
        has_more: false,
        next_cursor: null,
      });
      await p1;
      await p2;

      expect(tauri.getMessages).toHaveBeenCalledTimes(1);
    });

    it("sets error on failure", async () => {
      vi.mocked(tauri.getMessages).mockRejectedValue(
        new Error("Network error"),
      );

      await loadMessages("ch-1");

      expect(messagesState.error).toBe("Network error");
      expect(messagesState.loadingChannels["ch-1"]).toBe(false);
    });
  });

  describe("loadInitialMessages", () => {
    it("clears existing messages and loads fresh", async () => {
      setMessagesState("byChannel", "ch-1", [createMessage("old")]);
      vi.mocked(tauri.getMessages).mockResolvedValue({
        items: [createMessage("new")],
        has_more: false,
        next_cursor: null,
      } as PaginatedMessages);

      await loadInitialMessages("ch-1");

      expect(messagesState.byChannel["ch-1"]).toHaveLength(1);
      expect(messagesState.byChannel["ch-1"][0].id).toBe("new");
    });
  });

  describe("sendMessage", () => {
    it("sends message and adds to store", async () => {
      const sent = createMessage("sent-1");
      vi.mocked(tauri.sendMessageWithStatus).mockResolvedValue({
        message: sent,
        status: 201,
      });

      const result = await sendMessage("ch-1", "hello");

      expect(result?.id).toBe("sent-1");
      expect(messagesState.byChannel["ch-1"]).toHaveLength(1);
    });

    it("returns null for empty/whitespace content", async () => {
      const result = await sendMessage("ch-1", "   ");

      expect(result).toBeNull();
      expect(tauri.sendMessageWithStatus).not.toHaveBeenCalled();
    });

    it("suppresses local echo for 202 status (slash commands)", async () => {
      const sent = createMessage("cmd-1");
      vi.mocked(tauri.sendMessageWithStatus).mockResolvedValue({
        message: sent,
        status: 202,
      });

      await sendMessage("ch-1", "/roll");

      expect(messagesState.byChannel["ch-1"]).toBeUndefined();
    });

    it("deduplicates by message ID", async () => {
      const sent = createMessage("dup-1");
      setMessagesState("byChannel", "ch-1", [sent]);
      vi.mocked(tauri.sendMessageWithStatus).mockResolvedValue({
        message: sent,
        status: 201,
      });

      await sendMessage("ch-1", "hello");

      expect(messagesState.byChannel["ch-1"]).toHaveLength(1);
    });

    it("shows toast on error", async () => {
      vi.mocked(tauri.sendMessageWithStatus).mockRejectedValue(
        new Error("fail"),
      );

      const result = await sendMessage("ch-1", "hello");

      expect(result).toBeNull();
      expect(showToast).toHaveBeenCalledWith(
        expect.objectContaining({ type: "error", title: "Send Failed" }),
      );
      expect(messagesState.error).toBe("fail");
    });
  });

  describe("addMessage", () => {
    it("appends message to channel", async () => {
      const msg = createMessage("ws-1");

      await addMessage(msg);

      expect(messagesState.byChannel["ch-1"]).toHaveLength(1);
      expect(messagesState.byChannel["ch-1"][0].id).toBe("ws-1");
    });

    it("deduplicates by ID", async () => {
      const msg = createMessage("dup-1");
      setMessagesState("byChannel", "ch-1", [msg]);

      await addMessage(msg);

      expect(messagesState.byChannel["ch-1"]).toHaveLength(1);
    });
  });

  describe("updateMessage", () => {
    it("updates message in-place", () => {
      const original = createMessage("m1");
      setMessagesState("byChannel", "ch-1", [original]);

      const updated = {
        ...original,
        content: "edited content",
        edited_at: "2025-01-01T12:00:00Z",
      };
      updateMessage(updated);

      expect(messagesState.byChannel["ch-1"][0].content).toBe("edited content");
    });

    it("ignores update for missing message", () => {
      setMessagesState("byChannel", "ch-1", [createMessage("m1")]);

      updateMessage({ ...createMessage("unknown"), content: "edited" });

      expect(messagesState.byChannel["ch-1"][0].content).toBe("content-m1");
    });
  });

  describe("removeMessage", () => {
    it("removes message from channel", () => {
      setMessagesState("byChannel", "ch-1", [
        createMessage("m1"),
        createMessage("m2"),
      ]);

      removeMessage("ch-1", "m1");

      expect(messagesState.byChannel["ch-1"]).toHaveLength(1);
      expect(messagesState.byChannel["ch-1"][0].id).toBe("m2");
    });

    it("no-ops for unknown channel", () => {
      removeMessage("unknown", "m1");

      expect(messagesState.byChannel["unknown"]).toBeUndefined();
    });
  });

  describe("getChannelMessages", () => {
    it("returns messages for a channel", () => {
      setMessagesState("byChannel", "ch-1", [createMessage("m1")]);

      expect(getChannelMessages("ch-1")).toHaveLength(1);
    });

    it("returns empty array for unknown channel", () => {
      expect(getChannelMessages("unknown")).toEqual([]);
    });
  });

  describe("isLoadingMessages", () => {
    it("returns true when loading", () => {
      setMessagesState("loadingChannels", "ch-1", true);

      expect(isLoadingMessages("ch-1")).toBe(true);
    });

    it("returns false by default", () => {
      expect(isLoadingMessages("ch-1")).toBe(false);
    });
  });

  describe("hasMoreMessages", () => {
    it("returns true by default (unknown channel)", () => {
      expect(hasMoreMessages("ch-1")).toBe(true);
    });

    it("returns stored value", () => {
      setMessagesState("hasMore", "ch-1", false);

      expect(hasMoreMessages("ch-1")).toBe(false);
    });
  });

  describe("clearChannelMessages", () => {
    it("removes channel messages and hasMore entry", () => {
      setMessagesState("byChannel", "ch-1", [createMessage("m1")]);
      setMessagesState("hasMore", "ch-1", false);

      clearChannelMessages("ch-1");

      expect(messagesState.byChannel["ch-1"]).toBeUndefined();
      expect(messagesState.hasMore["ch-1"]).toBeUndefined();
    });
  });

  describe("clearCurve25519KeyCache", () => {
    it("does not throw", () => {
      expect(() => clearCurve25519KeyCache()).not.toThrow();
    });
  });
});
