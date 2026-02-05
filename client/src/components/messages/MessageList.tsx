import { Component, For, Show, createEffect, on, createMemo, createSignal, onCleanup } from "solid-js";
import { createVirtualizer } from "@tanstack/solid-virtual";
import { Loader2, ChevronDown, AlertCircle, MessageSquare, RefreshCw } from "lucide-solid";
import MessageItem from "./MessageItem";
import {
  messagesState,
  setMessagesState,
  loadInitialMessages,
  loadMessages,
  hasMoreMessages,
} from "@/stores/messages";
import { shouldGroupWithPrevious } from "@/lib/utils";

interface MessageListProps {
  channelId: string;
  /** Guild ID for custom emoji support in reactions */
  guildId?: string;
}

/** Max messages per channel before eviction kicks in */
const MAX_MESSAGES_PER_CHANNEL = 2000;
/** Messages to keep around viewport when evicting */
const EVICTION_KEEP_WINDOW = 500;

const MessageList: Component<MessageListProps> = (props) => {
  let containerRef: HTMLDivElement | undefined;
  let sentinelRef: HTMLDivElement | undefined;
  /** Synchronous guard against double-firing IntersectionObserver */
  let isLoadingMore = false;

  // Track scroll state
  const [isAtBottom, setIsAtBottom] = createSignal(true);
  const [hasNewMessages, setHasNewMessages] = createSignal(false);
  const [newMessageCount, setNewMessageCount] = createSignal(0);
  const [paginationError, setPaginationError] = createSignal<string | null>(null);

  // Use createMemo for proper reactive tracking of store values
  const messages = createMemo(() => {
    return messagesState.byChannel[props.channelId] || [];
  });

  // Compute messages with compact flag
  const messagesWithCompact = createMemo(() => {
    const msgs = messages();
    return msgs.map((message, idx) => {
      const prev = idx > 0 ? msgs[idx - 1] : null;
      const isCompact = prev
        ? shouldGroupWithPrevious(
            message.created_at,
            prev.created_at,
            message.author.id,
            prev.author.id
          )
        : false;
      return { message, isCompact };
    });
  });

  const loading = createMemo(() => !!messagesState.loadingChannels[props.channelId]);

  // --- Virtualizer ---
  const virtualizer = createVirtualizer({
    get count() { return messagesWithCompact().length; },
    getScrollElement: () => containerRef ?? null,
    estimateSize: (index) => {
      const item = messagesWithCompact()[index];
      if (!item) return 96;
      const msg = item.message;

      let estimate = item.isCompact ? 48 : 96;

      // Images are tall (~320px from max-h-80)
      const hasImage = msg.attachments?.some((a) =>
        a.mime_type?.startsWith("image/")
      );
      if (hasImage) estimate = 400;

      // Code blocks add height
      if (msg.content.includes("```")) estimate = Math.max(estimate, 200);

      // Reactions add ~36px
      if (msg.reactions && msg.reactions.length > 0) estimate += 36;

      return estimate;
    },
    overscan: 5,
  });

  // --- Check if at bottom ---
  const checkIfAtBottom = () => {
    if (!containerRef) return true;
    const { scrollTop, scrollHeight, clientHeight } = containerRef;
    return scrollHeight - scrollTop - clientHeight < 100;
  };

  // --- Handle scroll ---
  const handleScroll = () => {
    const atBottom = checkIfAtBottom();
    setIsAtBottom(atBottom);
    if (atBottom) {
      setHasNewMessages(false);
      setNewMessageCount(0);
    }
  };

  // --- Scroll to bottom ---
  const scrollToBottom = (smooth = true) => {
    const count = messagesWithCompact().length;
    if (count > 0) {
      virtualizer.scrollToIndex(count - 1, {
        align: "end",
        behavior: smooth ? "smooth" : "auto",
      });
      setHasNewMessages(false);
      setNewMessageCount(0);
    }
  };

  // --- Infinite scroll: load older messages ---
  async function triggerLoadMore() {
    isLoadingMore = true;
    setPaginationError(null);

    try {
      // Remember what the user is looking at
      const topItem = virtualizer.getVirtualItems()[0];
      const topIndex = topItem?.index ?? 0;
      const topOffset = (containerRef?.scrollTop ?? 0) - (topItem?.start ?? 0);

      const prevCount = messagesWithCompact().length;
      await loadMessages(props.channelId);
      const addedCount = messagesWithCompact().length - prevCount;

      // Restore scroll position in index-space
      if (addedCount > 0) {
        virtualizer.scrollToIndex(topIndex + addedCount, { align: "start" });

        // Fine-adjust by pixel offset, then evict, then release guard
        requestAnimationFrame(() => {
          if (containerRef) {
            containerRef.scrollTop += topOffset;
          }

          requestAnimationFrame(() => {
            evictIfNeeded();
            isLoadingMore = false;
          });
        });
        // Early return — isLoadingMore is cleared inside the rAF chain
        return;
      }
    } catch (err) {
      const error = err instanceof Error ? err.message : String(err);
      console.error("[MessageList] Pagination failed:", error);
      setPaginationError(error);
    }

    isLoadingMore = false;
  }

  // --- Memory eviction ---
  function evictIfNeeded() {
    const msgs = messages();
    if (msgs.length <= MAX_MESSAGES_PER_CHANNEL) return;

    const items = virtualizer.getVirtualItems();
    if (items.length === 0) return;

    const centerIndex = items[Math.floor(items.length / 2)]?.index ?? 0;
    const halfWindow = Math.floor(EVICTION_KEEP_WINDOW / 2);
    const keepStart = Math.max(0, centerIndex - halfWindow);
    const keepEnd = Math.min(msgs.length, centerIndex + halfWindow);

    const kept = msgs.slice(keepStart, keepEnd);

    // Guard against evicting everything
    if (kept.length === 0) {
      console.warn("[MessageList] Eviction would remove all messages, skipping");
      return;
    }

    setMessagesState("byChannel", props.channelId, kept);
    // Re-enable hasMore for evicted directions
    if (keepStart > 0) {
      setMessagesState("hasMore", props.channelId, true);
    }
  }

  // --- IntersectionObserver for upward pagination ---
  createEffect(on(
    () => props.channelId,
    () => {
      if (!sentinelRef || !containerRef) return;

      const observer = new IntersectionObserver(
        ([entry]) => {
          if (
            entry.isIntersecting &&
            hasMoreMessages(props.channelId) &&
            !loading() &&
            !isLoadingMore
          ) {
            triggerLoadMore().catch((err) =>
              console.error("[MessageList] Unhandled pagination error:", err)
            );
          }
        },
        { root: containerRef, rootMargin: "200px 0px 0px 0px" }
      );
      observer.observe(sentinelRef);
      onCleanup(() => observer.disconnect());
    }
  ));

  // --- Load messages when channelId changes ---
  createEffect(on(
    () => props.channelId,
    (channelId, prevChannelId) => {
      if (channelId && channelId !== prevChannelId) {
        setIsAtBottom(true);
        setHasNewMessages(false);
        setNewMessageCount(0);
        setPaginationError(null);
        prevMessageCount = 0;
        loadInitialMessages(channelId);
      }
    },
    { defer: false }
  ));

  // --- Track new messages for auto-scroll / indicator ---
  let prevMessageCount = 0;
  createEffect(() => {
    const currentCount = messages().length;

    if (currentCount > prevMessageCount && prevMessageCount > 0) {
      if (isAtBottom()) {
        setTimeout(() => scrollToBottom(true), 50);
      } else {
        setHasNewMessages(true);
        setNewMessageCount(count => count + (currentCount - prevMessageCount));
      }
    } else if (currentCount > 0 && prevMessageCount === 0) {
      // Initial load — scroll to bottom instantly
      setTimeout(() => scrollToBottom(false), 50);
    }

    prevMessageCount = currentCount;
  });

  return (
    <div
      ref={containerRef}
      class="flex-1 overflow-y-auto relative"
      role="list"
      aria-label="Messages"
      onScroll={handleScroll}
    >
      {/* Sentinel for infinite scroll (top) */}
      <div ref={sentinelRef} class="h-1" />

      {/* Beginning of conversation marker */}
      <Show when={!hasMoreMessages(props.channelId) && messages().length > 0}>
        <div class="flex flex-col items-center py-8 px-4 text-center">
          <div class="w-16 h-16 bg-surface-layer2 rounded-full flex items-center justify-center mb-3">
            <MessageSquare class="w-8 h-8 text-text-secondary" />
          </div>
          <h3 class="text-lg font-semibold text-text-primary mb-1">
            Beginning of conversation
          </h3>
          <p class="text-sm text-text-secondary">
            This is the start of the message history.
          </p>
        </div>
      </Show>

      {/* Pagination error indicator */}
      <Show when={paginationError() && messages().length > 0}>
        <div class="flex items-center justify-center gap-2 py-3 px-4 text-sm text-accent-danger">
          <AlertCircle class="w-4 h-4 flex-shrink-0" />
          <span>Failed to load older messages</span>
          <button
            onClick={() => triggerLoadMore().catch(() => {})}
            class="ml-1 text-text-link hover:underline inline-flex items-center gap-1"
          >
            <RefreshCw class="w-3 h-3" />
            Retry
          </button>
        </div>
      </Show>

      {/* Loading indicator at top (pagination) */}
      <Show when={loading() && messages().length > 0}>
        <div class="flex justify-center py-4 sticky top-0 z-10">
          <Loader2 class="w-5 h-5 text-text-secondary animate-spin" />
        </div>
      </Show>

      {/* Initial loading state */}
      <Show when={loading() && messages().length === 0}>
        <div class="flex flex-col items-center justify-center h-full">
          <Loader2 class="w-8 h-8 text-text-secondary animate-spin mb-4" />
          <p class="text-text-secondary">Loading messages...</p>
        </div>
      </Show>

      {/* Error state */}
      <Show when={!loading() && messages().length === 0 && messagesState.error}>
        <div class="flex flex-col items-center justify-center h-full text-center px-4">
          <AlertCircle class="w-10 h-10 text-accent-danger mb-4" />
          <h3 class="text-lg font-semibold text-text-primary mb-2">
            Failed to load messages
          </h3>
          <p class="text-text-secondary max-w-sm mb-4">
            {messagesState.error}
          </p>
          <button
            onClick={() => loadInitialMessages(props.channelId)}
            class="px-4 py-2 bg-accent-primary text-white rounded-lg font-medium hover:opacity-90 transition-opacity"
          >
            Retry
          </button>
        </div>
      </Show>

      {/* Empty state */}
      <Show when={!loading() && messages().length === 0 && !messagesState.error}>
        <div class="flex flex-col items-center justify-center h-full text-center px-4">
          <div class="w-20 h-20 bg-surface-layer2 rounded-full flex items-center justify-center mb-4">
            <span class="text-4xl">👋</span>
          </div>
          <h3 class="text-lg font-semibold text-text-primary mb-2">
            No messages yet
          </h3>
          <p class="text-text-secondary max-w-sm">
            Be the first to send a message in this channel!
          </p>
        </div>
      </Show>

      {/* Virtualized messages */}
      <Show when={messagesWithCompact().length > 0}>
        <div
          style={{ height: `${virtualizer.getTotalSize()}px`, position: "relative" }}
        >
          <For each={virtualizer.getVirtualItems()}>
            {(virtualItem) => {
              const item = () => messagesWithCompact()[virtualItem.index];
              return (
                <div
                  role="listitem"
                  data-index={virtualItem.index}
                  ref={(el) => virtualizer.measureElement(el)}
                  style={{
                    position: "absolute",
                    top: `${virtualItem.start}px`,
                    width: "100%",
                  }}
                >
                  {(() => {
                    const data = item();
                    return data ? (
                      <MessageItem
                        message={data.message}
                        compact={data.isCompact}
                        guildId={props.guildId}
                      />
                    ) : null;
                  })()}
                </div>
              );
            }}
          </For>
        </div>
      </Show>

      {/* New messages indicator */}
      <Show when={hasNewMessages()}>
        <button
          onClick={() => scrollToBottom(true)}
          class="fixed bottom-24 left-1/2 transform -translate-x-1/2 bg-accent-primary hover:bg-accent-primary/90 text-white px-5 py-2.5 rounded-full shadow-2xl flex items-center gap-2 transition-all z-10 font-medium"
        >
          <ChevronDown class="w-4 h-4" />
          <span>
            {newMessageCount() === 1
              ? "1 new message"
              : `${newMessageCount()} new messages`}
          </span>
        </button>
      </Show>
    </div>
  );
};

export default MessageList;
