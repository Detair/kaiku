/**
 * Threads Store
 *
 * Manages thread state for message threads: opening/closing the sidebar,
 * loading replies, sending replies, and tracking thread info.
 */

import { createStore } from "solid-js/store";
import type { Message, ThreadInfo } from "@/lib/types";
import * as tauri from "@/lib/tauri";
import { showToast } from "@/components/ui/Toast";
import { messagesState, setMessagesState } from "./messages";

// ============================================================================
// State
// ============================================================================

interface ThreadsState {
  activeThreadId: string | null;
  activeThreadParent: Message | null;
  repliesByThread: Record<string, Message[]>;
  loadingThreads: Record<string, boolean>;
  hasMore: Record<string, boolean>;
  lastReadMessageByThread: Record<string, string | null>;
  threadInfoCache: Record<string, ThreadInfo>;
  error: string | null;
}

const [threadsState, setThreadsState] = createStore<ThreadsState>({
  activeThreadId: null,
  activeThreadParent: null,
  repliesByThread: {},
  loadingThreads: {},
  hasMore: {},
  lastReadMessageByThread: {},
  threadInfoCache: {},
  error: null,
});

const THREAD_REPLY_LIMIT = 50;

// ============================================================================
// Actions
// ============================================================================

/**
 * Open the thread sidebar for a parent message.
 */
export async function openThread(parentMessage: Message): Promise<void> {
  setThreadsState({
    activeThreadId: parentMessage.id,
    activeThreadParent: parentMessage,
    error: null,
  });

  // Clear unread indicator immediately
  clearThreadUnread(parentMessage.id);

  // Load replies if not already loaded
  if (!threadsState.repliesByThread[parentMessage.id]) {
    await loadThreadReplies(parentMessage.id);
  }

  // Mark thread as read
  markThreadRead(parentMessage.id);
}

/**
 * Close the thread sidebar.
 */
export function closeThread(): void {
  setThreadsState({
    activeThreadId: null,
    activeThreadParent: null,
  });
}

/**
 * Load thread replies for a parent message.
 */
export async function loadThreadReplies(parentId: string): Promise<void> {
  if (threadsState.loadingThreads[parentId]) return;

  setThreadsState("loadingThreads", parentId, true);
  setThreadsState("error", null);

  try {
    const response = await tauri.getThreadReplies(
      parentId,
      undefined,
      THREAD_REPLY_LIMIT,
    );

    setThreadsState("repliesByThread", parentId, response.items);
    setThreadsState("hasMore", parentId, response.has_more);
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to load thread replies:", error);
    showToast({
      type: "error",
      title: "Thread Load Failed",
      message: "Could not load thread replies.",
      duration: 8000,
    });
    setThreadsState("error", error);
  } finally {
    setThreadsState("loadingThreads", parentId, false);
  }
}

/**
 * Load more thread replies after the newest loaded message.
 */
export async function loadMoreThreadReplies(parentId: string): Promise<void> {
  if (threadsState.loadingThreads[parentId]) return;
  if (!threadsState.hasMore[parentId]) return;

  const existing = threadsState.repliesByThread[parentId] || [];
  const after =
    existing.length > 0 ? existing[existing.length - 1].id : undefined;

  setThreadsState("loadingThreads", parentId, true);

  try {
    const response = await tauri.getThreadReplies(
      parentId,
      after,
      THREAD_REPLY_LIMIT,
    );

    const current = threadsState.repliesByThread[parentId] || [];
    setThreadsState("repliesByThread", parentId, [
      ...current,
      ...response.items,
    ]);
    setThreadsState("hasMore", parentId, response.has_more);
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to load more thread replies:", error);
    setThreadsState("error", error);
  } finally {
    setThreadsState("loadingThreads", parentId, false);
  }
}

/**
 * Send a reply in a thread.
 */
export async function sendThreadReply(
  parentId: string,
  channelId: string,
  content: string,
): Promise<Message | null> {
  if (!content.trim()) return null;

  setThreadsState("error", null);

  try {
    const message = await tauri.sendThreadReply(
      parentId,
      channelId,
      content.trim(),
    );

    // Add to local store (may already be added by WebSocket)
    const existing = threadsState.repliesByThread[parentId] || [];
    if (!existing.some((m) => m.id === message.id)) {
      setThreadsState("repliesByThread", parentId, [...existing, message]);
    }

    return message;
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to send thread reply:", error);
    showToast({
      type: "error",
      title: "Reply Failed",
      message: "Could not send thread reply. Please try again.",
      duration: 8000,
    });
    setThreadsState("error", error);
    return null;
  }
}

/**
 * Add a thread reply received from WebSocket.
 */
export function addThreadReply(parentId: string, message: Message): void {
  const existing = threadsState.repliesByThread[parentId] || [];
  if (existing.some((m) => m.id === message.id)) return;
  setThreadsState("repliesByThread", parentId, [...existing, message]);
}

/**
 * Remove a thread reply (when deleted).
 */
export function removeThreadReply(parentId: string, messageId: string): void {
  const existing = threadsState.repliesByThread[parentId];
  if (existing) {
    setThreadsState(
      "repliesByThread",
      parentId,
      existing.filter((m) => m.id !== messageId),
    );
  }
}

/**
 * Update thread info cache for a parent message.
 * Preserves the existing `has_unread` flag if the incoming data does not provide one,
 * since unread state is managed separately by markThreadUnread/clearThreadUnread.
 */
export function updateThreadInfo(
  parentId: string,
  threadInfo: ThreadInfo,
): void {
  const existing = threadsState.threadInfoCache[parentId];
  if (
    existing?.has_unread !== undefined &&
    threadInfo.has_unread === undefined
  ) {
    setThreadsState("threadInfoCache", parentId, {
      ...threadInfo,
      has_unread: existing.has_unread,
    });
  } else {
    setThreadsState("threadInfoCache", parentId, threadInfo);
  }
}

/**
 * Mark a thread as having unread replies (e.g. from WebSocket event).
 * Only updates if a cache entry already exists — callers should ensure
 * updateThreadInfo is called first to populate the entry with real data.
 */
export function markThreadUnread(parentId: string): void {
  const cached = threadsState.threadInfoCache[parentId];
  if (cached) {
    setThreadsState("threadInfoCache", parentId, {
      ...cached,
      has_unread: true,
    });
  }
}

/**
 * Clear unread state for a thread (e.g. when opening or marking as read).
 */
export function clearThreadUnread(parentId: string): void {
  const cached = threadsState.threadInfoCache[parentId];
  if (cached && cached.has_unread) {
    setThreadsState("threadInfoCache", parentId, {
      ...cached,
      has_unread: false,
    });
  }
}

/**
 * Update the parent message's thread counters in the main messages store.
 */
export function updateParentThreadIndicator(
  channelId: string,
  parentId: string,
  threadInfo: ThreadInfo,
): void {
  const messages = messagesState.byChannel[channelId];
  if (!messages) return;

  const index = messages.findIndex((m) => m.id === parentId);
  if (index === -1) return;

  setMessagesState(
    "byChannel",
    channelId,
    index,
    "thread_reply_count",
    threadInfo.reply_count,
  );
  setMessagesState(
    "byChannel",
    channelId,
    index,
    "thread_last_reply_at",
    threadInfo.last_reply_at,
  );

  // Also update the active thread parent if it matches
  if (
    threadsState.activeThreadId === parentId &&
    threadsState.activeThreadParent
  ) {
    setThreadsState("activeThreadParent", {
      ...threadsState.activeThreadParent,
      thread_reply_count: threadInfo.reply_count,
      thread_last_reply_at: threadInfo.last_reply_at,
    });
  }
}

/**
 * Mark a thread as read.
 */
export async function markThreadRead(parentId: string): Promise<void> {
  try {
    await tauri.markThreadRead(parentId);

    const replies = threadsState.repliesByThread[parentId] || [];
    const lastReadMessageId =
      replies.length > 0 ? replies[replies.length - 1].id : null;
    setThreadsState("lastReadMessageByThread", parentId, lastReadMessageId);
    clearThreadUnread(parentId);
  } catch (err) {
    console.warn("Failed to mark thread as read:", err);
    // Restore unread indicator since the server didn't persist the read state
    markThreadUnread(parentId);
  }
}

/**
 * Sync thread read position from WebSocket events.
 */
export function setThreadReadState(
  parentId: string,
  lastReadMessageId: string | null,
): void {
  setThreadsState("lastReadMessageByThread", parentId, lastReadMessageId);
}

// ============================================================================
// Exports
// ============================================================================

export { threadsState, setThreadsState };
