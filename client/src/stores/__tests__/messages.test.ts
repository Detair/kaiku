import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/tauri", () => ({
  getMessages: vi.fn(),
  getMessagesAfter: vi.fn(),
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

const markChannelAsRead = vi.hoisted(() => vi.fn());
vi.mock("@/stores/channels", () => ({ markChannelAsRead }));

import * as tauri from "@/lib/tauri";
import { t } from "@/i18n";
import { HttpError } from "@/lib/tauri/common";
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
  editingMessageId,
  setEditingMessageId,
  decryptMessageIfNeeded,
  catchUpChannel,
  loadMessagesForJump,
  loadNewerMessages,
  hasNewerMessages,
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
    pinned: false,
    message_type: "user",
    reactions: [],
    nonce: null,
  };
}

describe("messages store", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setMessagesState({
      byChannel: {},
      loadingChannels: {},
      hasMore: {},
      loadError: null,
      sendError: null,
    });
  });

  describe("initial state", () => {
    it("has empty message maps and no error", () => {
      expect(messagesState.byChannel).toEqual({});
      expect(messagesState.loadingChannels).toEqual({});
      expect(messagesState.hasMore).toEqual({});
      expect(messagesState.loadError).toBeNull();
      expect(messagesState.sendError).toBeNull();
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

      expect(messagesState.loadError).toBe("Could not load messages. Please try again.");
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

  describe("catchUpChannel", () => {
    const page = (items: Message[], has_more: boolean): PaginatedMessages =>
      ({ items, has_more, next_cursor: null }) as PaginatedMessages;

    it("catchUpChannel appends only newer messages, deduplicated", async () => {
      const [m1, m2, m3, m4] = ["m1", "m2", "m3", "m4"].map((id) =>
        createMessage(id),
      );
      setMessagesState("byChannel", "ch-1", [m1, m2]);
      vi.mocked(tauri.getMessagesAfter)
        .mockResolvedValueOnce(page([m2, m3], true))
        .mockResolvedValueOnce(page([m4], false));

      const result = await catchUpChannel("ch-1");

      expect(messagesState.byChannel["ch-1"].map((m) => m.id)).toEqual([
        "m1",
        "m2",
        "m3",
        "m4",
      ]);
      expect(result).toBe("caught_up");
      expect(vi.mocked(tauri.getMessagesAfter).mock.calls[0][1]).toBe("m2");
      expect(vi.mocked(tauri.getMessagesAfter).mock.calls[1][1]).toBe("m3");
    });

    it("catchUpChannel reloads after maxPages", async () => {
      setMessagesState("byChannel", "ch-1", [createMessage("m1")]);
      let n = 0;
      vi.mocked(tauri.getMessagesAfter).mockImplementation(async () =>
        page([createMessage(`p${++n}`)], true),
      );
      vi.mocked(tauri.getMessages).mockResolvedValue(page([], false));

      const result = await catchUpChannel("ch-1");

      expect(result).toBe("reloaded");
      expect(tauri.getMessagesAfter).toHaveBeenCalledTimes(5);
      // loadInitialMessages ran once: one fresh newest-page fetch.
      expect(tauri.getMessages).toHaveBeenCalledTimes(1);
      expect(tauri.getMessages).toHaveBeenCalledWith("ch-1", undefined, 50);
    });

    it("catchUpChannel reloads when the server rejects the anchor (400)", async () => {
      setMessagesState("byChannel", "ch-1", [createMessage("gone")]);
      vi.mocked(tauri.getMessagesAfter).mockRejectedValue(
        Object.assign(new Error("Unknown cursor"), { status: 400 }),
      );
      vi.mocked(tauri.getMessages).mockResolvedValue(
        page([createMessage("fresh")], false),
      );

      const result = await catchUpChannel("ch-1");

      expect(result).toBe("reloaded");
      expect(messagesState.byChannel["ch-1"].map((m) => m.id)).toEqual([
        "fresh",
      ]);
    });

    it("catchUpChannel with an empty cache loads the channel", async () => {
      vi.mocked(tauri.getMessages).mockResolvedValue(
        page([createMessage("n1")], false),
      );

      const result = await catchUpChannel("ch-1");

      expect(result).toBe("reloaded");
      expect(tauri.getMessagesAfter).not.toHaveBeenCalled();
      expect(messagesState.byChannel["ch-1"].map((m) => m.id)).toEqual(["n1"]);
    });

    it("catchUpChannel keeps order when live messages arrived first", async () => {
      const at = (id: string, sec: number) => ({
        ...createMessage(id),
        created_at: `2026-11-01T10:00:${String(sec).padStart(2, "0")}Z`,
      });
      // m1 before the outage; m4 arrived live after the re-subscribe.
      setMessagesState("byChannel", "ch-1", [at("m1", 1), at("m4", 4)]);
      vi.mocked(tauri.getMessagesAfter).mockResolvedValueOnce(
        page([at("m2", 2), at("m3", 3), at("m4", 4)], false),
      );

      await catchUpChannel("ch-1", 5, "m1");

      expect(vi.mocked(tauri.getMessagesAfter).mock.calls[0][1]).toBe("m1");
      expect(messagesState.byChannel["ch-1"].map((m) => m.id)).toEqual([
        "m1",
        "m2",
        "m3",
        "m4",
      ]);
    });

    it("catchUpChannel anchors on the newest sent message, not a pending one", async () => {
      const pending = { ...createMessage("pending:x"), nonce: "x" };
      setMessagesState("byChannel", "ch-1", [createMessage("m1"), pending]);
      vi.mocked(tauri.getMessagesAfter).mockResolvedValueOnce(
        page([], false),
      );

      await catchUpChannel("ch-1");

      expect(vi.mocked(tauri.getMessagesAfter).mock.calls[0][1]).toBe("m1");
    });
  });

  describe("loadInitialMessages around-mode forward fill", () => {
    const page = (items: Message[], has_more: boolean): PaginatedMessages =>
      ({ items, has_more, next_cursor: null }) as PaginatedMessages;
    const at = (id: string, sec: number) => ({
      ...createMessage(id),
      created_at: `2026-11-01T10:00:${String(sec).padStart(2, "0")}Z`,
    });
    const ids = () => messagesState.byChannel["ch-1"].map((m) => m.id);

    it("loads the messages after the around window, in order and deduplicated", async () => {
      vi.mocked(tauri.getMessages).mockResolvedValueOnce(
        page([at("m1", 1), at("m2", 2)], true),
      );
      vi.mocked(tauri.getMessagesAfter)
        .mockResolvedValueOnce(page([at("m2", 2), at("m3", 3)], true))
        .mockResolvedValueOnce(page([at("m4", 4)], false));

      await loadInitialMessages("ch-1", "m1", 120);

      expect(ids()).toEqual(["m1", "m2", "m3", "m4"]);
      expect(vi.mocked(tauri.getMessagesAfter).mock.calls[0][1]).toBe("m2");
      expect(messagesState.loadingChannels["ch-1"]).toBe(false);
      expect(messagesState.hasMore["ch-1"]).toBe(true);
    });

    it("falls back to the latest page when the bound is exceeded", async () => {
      vi.mocked(tauri.getMessages)
        .mockResolvedValueOnce(page([at("m1", 1)], true))
        .mockResolvedValueOnce(page([at("z2", 51), at("z1", 50)], true));
      let n = 0;
      vi.mocked(tauri.getMessagesAfter).mockImplementation(async () =>
        page([at(`p${++n}`, 10 + n)], true),
      );

      await loadInitialMessages("ch-1", "m1", 120);

      expect(ids()).toEqual(["z1", "z2"]);
      expect(messagesState.loadingChannels["ch-1"]).toBe(false);
      expect(tauri.getMessages).toHaveBeenLastCalledWith("ch-1", undefined, 50);
    });

    it("falls back to the latest page when the anchor is rejected (400)", async () => {
      vi.mocked(tauri.getMessages)
        .mockResolvedValueOnce(page([at("m1", 1)], true))
        .mockResolvedValueOnce(page([at("z1", 50)], false));
      vi.mocked(tauri.getMessagesAfter).mockRejectedValue(
        Object.assign(new Error("Unknown cursor"), { status: 400 }),
      );

      await loadInitialMessages("ch-1", "m1", 120);

      expect(ids()).toEqual(["z1"]);
      expect(messagesState.loadingChannels["ch-1"]).toBe(false);
    });

    it("keeps a message that arrived live during the fetch last", async () => {
      vi.mocked(tauri.getMessages).mockResolvedValueOnce(
        page([at("m1", 1)], true),
      );
      vi.mocked(tauri.getMessagesAfter).mockImplementationOnce(async () => {
        addMessage(at("m9", 9));
        return page([at("m2", 2), at("m3", 3)], false);
      });

      await loadInitialMessages("ch-1", "m1", 120);

      expect(ids()).toEqual(["m1", "m2", "m3", "m9"]);
    });

    it("commits the initial around-load in a single byChannel write", async () => {
      const seen: string[][] = [];
      const snap = () =>
        seen.push((messagesState.byChannel["ch-1"] ?? []).map((m) => m.id));
      vi.mocked(tauri.getMessages).mockResolvedValueOnce(
        page([at("m1", 1), at("m2", 2)], true),
      );
      vi.mocked(tauri.getMessagesAfter).mockImplementation(async () => {
        snap();
        expect(messagesState.loadingChannels["ch-1"]).toBe(true);
        return page([at("m3", 3)], false);
      });

      await loadInitialMessages("ch-1", "m1", 120);

      expect(seen).toEqual([[]]);
      expect(ids()).toEqual(["m1", "m2", "m3"]);
    });

    it("keeps the around window when the forward fetch fails with a non-400 error", async () => {
      vi.mocked(tauri.getMessages).mockResolvedValueOnce(
        page([at("m1", 1), at("m2", 2)], true),
      );
      vi.mocked(tauri.getMessagesAfter).mockRejectedValue(new Error("network"));

      await loadInitialMessages("ch-1", "m1", 120);

      expect(ids()).toEqual(["m1", "m2"]);
      expect(tauri.getMessages).toHaveBeenCalledTimes(1);
      expect(messagesState.loadingChannels["ch-1"]).toBe(false);
    });

    it("buffers a live message during the around-load until the single commit", async () => {
      vi.mocked(tauri.getMessages).mockResolvedValueOnce(
        page([at("m1", 1), at("m2", 2)], true),
      );
      vi.mocked(tauri.getMessagesAfter).mockImplementationOnce(async () => {
        await addMessage(at("m9", 9));
        expect(messagesState.byChannel["ch-1"] ?? []).toEqual([]);
        return page([at("m3", 3)], false);
      });

      await loadInitialMessages("ch-1", "m1", 120);

      expect(ids()).toEqual(["m1", "m2", "m3", "m9"]);
    });

    it("buffers a live message during the fallback and keeps it last", async () => {
      vi.mocked(tauri.getMessages)
        .mockResolvedValueOnce(page([at("m1", 1)], true))
        .mockImplementationOnce(async () => {
          await addMessage(at("m9", 59));
          expect(messagesState.byChannel["ch-1"] ?? []).toEqual([]);
          return page([at("z2", 51), at("z1", 50)], true);
        });
      vi.mocked(tauri.getMessagesAfter).mockRejectedValue(
        Object.assign(new Error("Unknown cursor"), { status: 400 }),
      );

      await loadInitialMessages("ch-1", "m1", 120);

      expect(ids()).toEqual(["z1", "z2", "m9"]);
    });

    it("does not leak the live buffer into the next load", async () => {
      vi.mocked(tauri.getMessages).mockResolvedValue(page([at("a", 1)], false));
      vi.mocked(tauri.getMessagesAfter).mockImplementationOnce(async () => {
        await addMessage(at("m9", 9));
        return page([], false);
      });
      await loadInitialMessages("ch-1", "a", 5);
      vi.mocked(tauri.getMessages).mockResolvedValue(page([at("b", 2)], false));
      await loadInitialMessages("ch-1");

      expect(ids()).toEqual(["b"]);
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

      expect(messagesState.byChannel["ch-1"]).toEqual([]);
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
      expect(messagesState.sendError).toBe("Your message was not sent. It is still in the box, so you can try again.");
      expect(messagesState.loadError).toBeNull();
    });

    it("maps FORBIDDEN to the forbidden sentence", async () => {
      vi.mocked(tauri.sendMessageWithStatus).mockRejectedValue(
        new HttpError(403, "You do not have permission", "FORBIDDEN"),
      );

      await sendMessage("ch-1", "hello");

      expect(messagesState.sendError).toBe("You can't send messages here.");
      expect(showToast).toHaveBeenCalledWith(
        expect.objectContaining({ message: "You can't send messages here." }),
      );
    });

    it("maps a 413 without a code to the file-too-large sentence", async () => {
      vi.mocked(tauri.sendMessageWithStatus).mockRejectedValue(new HttpError(413, "big"));

      await sendMessage("ch-1", "hello");

      expect(messagesState.sendError).toBe("This file is too big to send.");
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

  describe("addMessage nonce matching", () => {
    it("replaces the correct pending message when multiple are pending", async () => {
      const pending1: Message = {
        ...createMessage("pending:nonce-AAA", "ch-1"),
        author: { id: "me", username: "me", display_name: "me", avatar_url: null, status: "online" },
        nonce: "nonce-AAA",
      };
      const pending2: Message = {
        ...createMessage("pending:nonce-BBB", "ch-1"),
        author: { id: "me", username: "me", display_name: "me", avatar_url: null, status: "online" },
        nonce: "nonce-BBB",
      };
      setMessagesState("byChannel", "ch-1", [pending1, pending2]);

      const real2: Message = {
        ...createMessage("msg-002", "ch-1"),
        author: { id: "me", username: "me", display_name: "me", avatar_url: null, status: "online" },
        nonce: "nonce-BBB",
      };
      await addMessage(real2);

      const msgs = messagesState.byChannel["ch-1"]!;
      expect(msgs).toHaveLength(2);
      expect(msgs[0].id).toBe("pending:nonce-AAA");
      expect(msgs[1].id).toBe("msg-002");
    });

    it("appends message from other user without touching pending messages", async () => {
      const pending1: Message = {
        ...createMessage("pending:nonce-AAA", "ch-1"),
        author: { id: "me", username: "me", display_name: "me", avatar_url: null, status: "online" },
        nonce: "nonce-AAA",
      };
      setMessagesState("byChannel", "ch-1", [pending1]);

      const otherMsg: Message = {
        ...createMessage("msg-other", "ch-1"),
        author: { id: "other", username: "other", display_name: "Other", avatar_url: null, status: "online" },
        nonce: null,
      };
      await addMessage(otherMsg);

      const msgs = messagesState.byChannel["ch-1"]!;
      expect(msgs).toHaveLength(2);
      expect(msgs[0].id).toBe("pending:nonce-AAA");
      expect(msgs[1].id).toBe("msg-other");
    });
  });

  describe("decryptMessageIfNeeded", () => {
    it("returns non-encrypted message unchanged", async () => {
      const msg = createMessage("m1");
      msg.encrypted = false;
      msg.content = "hello";
      const result = await decryptMessageIfNeeded(msg);
      expect(result.content).toBe("hello");
    });

    it("returns placeholder for encrypted message without session", async () => {
      const msg = createMessage("m1");
      msg.encrypted = true;
      msg.content = "ciphertext-blob";
      const result = await decryptMessageIfNeeded(msg);
      expect(result.content).toBe(t("chat.message.encryptedPlaceholder"));
    });

    it("encrypted messages render the placeholder when E2EE is off", async () => {
      const msg = createMessage("m2");
      msg.encrypted = true;
      msg.content = '{"sender_key":"k","recipients":{}}';
      const result = await decryptMessageIfNeeded(msg);
      expect(result.content).toBe(
        "Encrypted message — it can't be shown on this device.",
      );
    });

    it("no encrypted DM send path is exported", async () => {
      const keys = Object.keys(await import("../messages"));
      expect(keys).not.toContain("sendDMMessage");
      expect(keys).not.toContain("sendEncryptedDM");
      expect(keys).not.toContain("sendEncryptedGroupDM");
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

describe("editingMessageId", () => {
  beforeEach(() => {
    setEditingMessageId(null);
  });

  it("is null initially", () => {
    expect(editingMessageId()).toBeNull();
  });

  it("can be set and cleared", () => {
    setEditingMessageId("msg-1");
    expect(editingMessageId()).toBe("msg-1");

    setEditingMessageId(null);
    expect(editingMessageId()).toBeNull();
  });

  it("setting a new ID replaces the previous one", () => {
    setEditingMessageId("msg-1");
    setEditingMessageId("msg-2");
    expect(editingMessageId()).toBe("msg-2");
  });

  describe("loadMessagesForJump", () => {
    beforeEach(() => {
      vi.resetAllMocks();
      setMessagesState({ byChannel: {}, loadingChannels: {}, hasMore: {}, loadError: null, sendError: null, newerAfter: {} });
    });

    const at = (id: string, minute: number) => ({
      ...createMessage(id),
      created_at: new Date(Date.UTC(2026, 0, 1, 0, minute)).toISOString(),
    });

    it("loads the page around an old message and bridges to the loaded ones", async () => {
      setMessagesState("byChannel", "ch-1", [at("m-50", 50), at("m-51", 51)]);
      vi.mocked(tauri.getMessages).mockResolvedValueOnce({
        items: [at("m-09", 9), at("m-10", 10), at("m-11", 11)],
        has_more: true,
        next_cursor: null,
      } as PaginatedMessages);
      vi.mocked(tauri.getMessagesAfter).mockResolvedValueOnce({
        items: [at("m-12", 12), at("m-50", 50), at("m-51", 51)],
        has_more: false,
        next_cursor: null,
      } as PaginatedMessages);

      await expect(loadMessagesForJump("ch-1", "m-10")).resolves.toBe(true);

      expect(tauri.getMessages).toHaveBeenCalledWith("ch-1", undefined, 50, "m-10");
      expect(tauri.getMessagesAfter).toHaveBeenCalledWith("ch-1", "m-11", 100);
      expect(messagesState.byChannel["ch-1"].map((m) => m.id)).toEqual([
        "m-09", "m-10", "m-11", "m-12", "m-50", "m-51",
      ]);
      expect(messagesState.hasMore["ch-1"]).toBe(true);
      expect(messagesState.loadingChannels["ch-1"]).toBe(false);
    });

    it("does nothing when the message is already loaded", async () => {
      setMessagesState("byChannel", "ch-1", [at("m-10", 10)]);
      await expect(loadMessagesForJump("ch-1", "m-10")).resolves.toBe(true);
      expect(tauri.getMessages).not.toHaveBeenCalled();
    });

    it("keeps the list when the message is gone", async () => {
      setMessagesState("byChannel", "ch-1", [at("m-50", 50)]);
      vi.mocked(tauri.getMessages).mockResolvedValueOnce({
        items: [], has_more: false, next_cursor: null,
      } as PaginatedMessages);
      await expect(loadMessagesForJump("ch-1", "m-10")).resolves.toBe(false);
      expect(messagesState.byChannel["ch-1"].map((m) => m.id)).toEqual(["m-50"]);
      expect(messagesState.loadingChannels["ch-1"]).toBe(false);
    });

    it("shows the window alone when it is too far back to bridge", async () => {
      setMessagesState("byChannel", "ch-1", [at("m-50", 50)]);
      vi.mocked(tauri.getMessages).mockResolvedValueOnce({
        items: [at("m-10", 10)], has_more: true, next_cursor: null,
      } as PaginatedMessages);
      vi.mocked(tauri.getMessagesAfter).mockResolvedValue({
        items: [at("m-11", 11)], has_more: true, next_cursor: "m-11",
      } as PaginatedMessages);
      await expect(loadMessagesForJump("ch-1", "m-10", 2)).resolves.toBe(true);
      expect(messagesState.byChannel["ch-1"].map((m) => m.id)).toEqual(["m-10"]);
      // The list knows it does not reach the newest message.
      expect(hasNewerMessages("ch-1")).toBe(true);
    });

    it("keeps messages that arrive during a far jump", async () => {
      setMessagesState("byChannel", "ch-1", [at("m-50", 50)]);
      let release!: () => void;
      vi.mocked(tauri.getMessages).mockImplementationOnce(
        () =>
          new Promise((r) => {
            release = () => r({ items: [at("m-10", 10)], has_more: true, next_cursor: null } as PaginatedMessages);
          }),
      );
      vi.mocked(tauri.getMessagesAfter).mockResolvedValue({
        items: [at("m-11", 11)], has_more: true, next_cursor: "m-11",
      } as PaginatedMessages);
      const jump = loadMessagesForJump("ch-1", "m-10", 1);
      await Promise.resolve();
      setMessagesState("byChannel", "ch-1", [...messagesState.byChannel["ch-1"], at("m-60", 60)]);
      release();
      await jump;
      expect(messagesState.byChannel["ch-1"].map((m) => m.id)).toEqual(["m-10", "m-60"]);
    });

    it("loads newer pages after an older window until the newest", async () => {
      setMessagesState("byChannel", "ch-1", [at("m-10", 10), at("m-60", 60)]);
      setMessagesState("newerAfter", "ch-1", "m-10");
      vi.mocked(tauri.getMessagesAfter)
        .mockResolvedValueOnce({ items: [at("m-11", 11)], has_more: true, next_cursor: "m-11" } as PaginatedMessages)
        .mockResolvedValueOnce({ items: [at("m-12", 12), at("m-60", 60)], has_more: false, next_cursor: null } as PaginatedMessages);
      await loadNewerMessages("ch-1");
      expect(tauri.getMessagesAfter).toHaveBeenLastCalledWith("ch-1", "m-10", 100);
      expect(hasNewerMessages("ch-1")).toBe(true);
      await loadNewerMessages("ch-1");
      expect(tauri.getMessagesAfter).toHaveBeenLastCalledWith("ch-1", "m-11", 100);
      expect(messagesState.byChannel["ch-1"].map((m) => m.id)).toEqual(["m-10", "m-11", "m-12", "m-60"]);
      expect(hasNewerMessages("ch-1")).toBe(false);
    });

    it("N2: a later bridged jump ends the older window", async () => {
      setMessagesState("byChannel", "ch-1", [at("m-10", 10)]);
      setMessagesState("newerAfter", "ch-1", "m-10");
      vi.mocked(tauri.getMessages).mockResolvedValueOnce({
        items: [at("m-20", 20)], has_more: true, next_cursor: null,
      } as PaginatedMessages);
      vi.mocked(tauri.getMessagesAfter).mockResolvedValueOnce({
        items: [at("m-21", 21)], has_more: false, next_cursor: null,
      } as PaginatedMessages);
      await expect(loadMessagesForJump("ch-1", "m-20")).resolves.toBe(true);
      expect(hasNewerMessages("ch-1")).toBe(false);
    });

    it("N3: a deleted anchor reloads the present once instead of failing on every scroll", async () => {
      setMessagesState("byChannel", "ch-1", [at("m-10", 10)]);
      setMessagesState("newerAfter", "ch-1", "m-10");
      vi.mocked(tauri.getMessagesAfter).mockRejectedValueOnce(new HttpError(400, "unknown anchor", "VALIDATION_ERROR"));
      vi.mocked(tauri.getMessages).mockResolvedValueOnce({
        items: [at("m-90", 90)], has_more: true, next_cursor: null,
      } as PaginatedMessages);
      await loadNewerMessages("ch-1");
      expect(hasNewerMessages("ch-1")).toBe(false);
      expect(tauri.getMessages).toHaveBeenCalledTimes(1);
      expect(messagesState.byChannel["ch-1"].map((m) => m.id)).toEqual(["m-90"]);
      // Nothing left to retry.
      await loadNewerMessages("ch-1");
      expect(tauri.getMessagesAfter).toHaveBeenCalledTimes(1);
    });

    it("a fresh load leaves the older window", async () => {
      setMessagesState("newerAfter", "ch-1", "m-10");
      vi.mocked(tauri.getMessages).mockResolvedValueOnce({ items: [], has_more: false, next_cursor: null } as PaginatedMessages);
      await loadInitialMessages("ch-1");
      expect(hasNewerMessages("ch-1")).toBe(false);
    });

    it("reports a failed load without touching the list", async () => {
      setMessagesState("byChannel", "ch-1", [at("m-50", 50)]);
      vi.mocked(tauri.getMessages).mockRejectedValueOnce(new TypeError("Failed to fetch"));
      await expect(loadMessagesForJump("ch-1", "m-10")).resolves.toBe(false);
      expect(messagesState.byChannel["ch-1"].map((m) => m.id)).toEqual(["m-50"]);
    });
  });
});

describe("fix round 3", () => {
  const at = (id: string, minute: number, content = `content-${id}`): Message => ({
    ...createMessage(id),
    content,
    created_at: new Date(Date.UTC(2026, 0, 1, 0, minute)).toISOString(),
  });

  beforeEach(() => {
    vi.resetAllMocks();
    setMessagesState({ byChannel: {}, loadingChannels: {}, hasMore: {}, loadError: null, sendError: null, newerAfter: {} });
  });

  it("sending leaves the read marker to the visible list", async () => {
    setMessagesState("newerAfter", "ch-1", "m-10");
    vi.mocked(tauri.sendMessageWithStatus).mockResolvedValueOnce({
      message: at("m-99", 99, "hi"),
      status: 201,
    } as Awaited<ReturnType<typeof tauri.sendMessageWithStatus>>);
    await sendMessage("ch-1", "hi");
    await new Promise((r) => setTimeout(r, 0));
    expect(markChannelAsRead).not.toHaveBeenCalled();
  });

  it("an edit or deletion during a forward load wins over the page", async () => {
    setMessagesState("byChannel", "ch-1", [at("m-10", 10)]);
    setMessagesState("newerAfter", "ch-1", "m-10");
    let release!: () => void;
    vi.mocked(tauri.getMessagesAfter).mockImplementationOnce(
      () =>
        new Promise((r) => {
          release = () =>
            r({
              items: [at("m-11", 11, "old text"), at("m-12", 12)],
              has_more: false,
              next_cursor: null,
            } as PaginatedMessages);
        }),
    );
    const load = loadNewerMessages("ch-1");
    await Promise.resolve();
    updateMessage(at("m-11", 11, "edited text"));
    removeMessage("ch-1", "m-12");
    release();
    await load;
    const list = messagesState.byChannel["ch-1"];
    expect(list.map((m) => m.id)).toEqual(["m-10", "m-11"]);
    expect(list[1].content).toBe("edited text");
  });

  it("an edit or deletion during a jump wins over the loaded pages", async () => {
    setMessagesState("byChannel", "ch-1", [at("m-50", 50)]);
    let release!: () => void;
    vi.mocked(tauri.getMessages).mockImplementationOnce(
      () =>
        new Promise((r) => {
          release = () =>
            r({
              items: [at("m-09", 9), at("m-10", 10, "old")],
              has_more: true,
              next_cursor: null,
            } as PaginatedMessages);
        }),
    );
    vi.mocked(tauri.getMessagesAfter).mockResolvedValueOnce({
      items: [at("m-50", 50)], has_more: false, next_cursor: null,
    } as PaginatedMessages);
    const jump = loadMessagesForJump("ch-1", "m-10");
    await Promise.resolve();
    updateMessage(at("m-10", 10, "new"));
    removeMessage("ch-1", "m-09");
    release();
    await expect(jump).resolves.toBe(true);
    const list = messagesState.byChannel["ch-1"];
    expect(list.map((m) => m.id)).toEqual(["m-10", "m-50"]);
    expect(list[0].content).toBe("new");
  });
});

