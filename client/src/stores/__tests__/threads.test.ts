import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/tauri", () => ({
  getThreadReplies: vi.fn(),
  sendThreadReply: vi.fn(),
  markThreadRead: vi.fn(),
}));

import * as tauri from "@/lib/tauri";
import type { Message } from "@/lib/types";
import {
  loadMoreThreadReplies,
  markThreadRead,
  setThreadReadState,
  setThreadsState,
  threadsState,
} from "../threads";

function createMessage(id: string): Message {
  return {
    id,
    channel_id: "channel-1",
    author: {
      id: "user-1",
      username: "user",
      display_name: "User",
      avatar_url: null,
      status: "offline",
    },
    content: `message-${id}`,
    encrypted: false,
    attachments: [],
    reply_to: null,
    parent_id: "parent-1",
    thread_reply_count: 0,
    thread_last_reply_at: null,
    edited_at: null,
    created_at: new Date().toISOString(),
    mention_type: null,
    reactions: [],
  };
}

describe("threads store", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setThreadsState({
      activeThreadId: null,
      activeThreadParent: null,
      repliesByThread: {},
      loadingThreads: {},
      hasMore: {},
      lastReadMessageByThread: {},
      threadInfoCache: {},
      error: null,
    });
  });

  it("paginates with last loaded reply cursor and appends newer replies", async () => {
    const parentId = "parent-1";
    const first = createMessage("m1");
    const second = createMessage("m2");
    const third = createMessage("m3");

    setThreadsState("repliesByThread", parentId, [first, second]);
    setThreadsState("hasMore", parentId, true);

    vi.mocked(tauri.getThreadReplies).mockResolvedValueOnce({
      items: [third],
      has_more: false,
      next_cursor: null,
    });

    await loadMoreThreadReplies(parentId);

    expect(tauri.getThreadReplies).toHaveBeenCalledWith(parentId, "m2", 50);
    expect(threadsState.repliesByThread[parentId].map((m) => m.id)).toEqual(["m1", "m2", "m3"]);
    expect(threadsState.hasMore[parentId]).toBe(false);
  });

  it("stores thread read cursor from websocket sync", () => {
    setThreadReadState("parent-1", "m9");
    expect(threadsState.lastReadMessageByThread["parent-1"]).toBe("m9");

    setThreadReadState("parent-1", null);
    expect(threadsState.lastReadMessageByThread["parent-1"]).toBeNull();
  });

  it("updates local read cursor to latest loaded reply when marking read", async () => {
    const parentId = "parent-1";
    setThreadsState("repliesByThread", parentId, [createMessage("m1"), createMessage("m2")]);
    vi.mocked(tauri.markThreadRead).mockResolvedValueOnce(undefined);

    await markThreadRead(parentId);

    expect(tauri.markThreadRead).toHaveBeenCalledWith(parentId);
    expect(threadsState.lastReadMessageByThread[parentId]).toBe("m2");
  });
});
