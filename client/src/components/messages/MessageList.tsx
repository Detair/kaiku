import {
  Component,
  Index,
  Show,
  createEffect,
  on,
  createMemo,
  createSignal,
  onCleanup,
  onMount,
} from "solid-js";
import { useSearchParams } from "@solidjs/router";
import { createVirtualizer } from "@/lib/virtualizer";
import {
  Loader2,
  ChevronDown,
  AlertCircle,
  MessageSquare,
  RefreshCw,
} from "lucide-solid";
import flokiHappy from "@/assets/emotes/floki_emote_1.webp";
import MessageItem, { MessageImageLightbox } from "./MessageItem";
import {
  messagesState,
  setMessagesState,
  loadInitialMessages,
  loadMessages,
  hasMoreMessages,
} from "@/stores/messages";
import {
  markChannelAsRead,
  getChannelLastReadMessageId,
  isChannelUnread,
  getUnreadCount,
} from "@/stores/channels";
import { areThreadsEnabled } from "@/stores/guilds";
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

  const [searchParams, setSearchParams] = useSearchParams();

  // Track scroll state
  const [isAtBottom, setIsAtBottom] = createSignal(true);
  /** Timestamp until which we force-stick to bottom (handles measurement reflow) */
  let stickyBottomUntil = 0;
  const [hasNewMessages, setHasNewMessages] = createSignal(false);
  const [newMessageCount, setNewMessageCount] = createSignal(0);
  const [paginationError, setPaginationError] = createSignal<string | null>(
    null,
  );

  // --- Highlight support (for search result navigation) ---
  const [highlightedId, setHighlightedId] = createSignal<string | null>(null);
  /** Message ID we want to scroll to once messages finish loading */
  let pendingHighlightId: string | null = null;
  let highlightTimer: ReturnType<typeof setTimeout> | null = null;

  // Use createMemo for proper reactive tracking of store values
  const messages = createMemo(() => {
    return messagesState.byChannel[props.channelId] || [];
  });

  // Compute messages with compact flag and first-unread marker
  const messagesWithCompact = createMemo(() => {
    const msgs = messages();
    const lastReadId = getChannelLastReadMessageId(props.channelId);
    let foundLastRead = lastReadId == null; // if null, no divider needed

    return msgs.map((message, idx) => {
      const prev = idx > 0 ? msgs[idx - 1] : null;
      const isCompact = prev
        ? shouldGroupWithPrevious(
            message.created_at,
            prev.created_at,
            message.author.id,
            prev.author.id,
          )
        : false;

      let isFirstUnread = false;
      if (!foundLastRead && prev && prev.id === lastReadId) {
        isFirstUnread = true;
        foundLastRead = true;
      }

      return { message, isCompact, isFirstUnread };
    });
  });

  const loading = createMemo(
    () => !!messagesState.loadingChannels[props.channelId],
  );

  // --- Virtualizer ---
  const virtualizer = createVirtualizer({
    get count() {
      return messagesWithCompact().length;
    },
    getScrollElement: () => containerRef ?? null,
    estimateSize: (index: number) => {
      const item = messagesWithCompact()[index];
      if (!item?.message) return 96;
      const msg = item.message;
      const content = msg.content || "";
      const attachments = msg.attachments || [];

      // Base: header (avatar + name + timestamp) or compact (no header)
      let estimate = item.isCompact ? 8 : 52;

      // Content lines (~22px each), with code blocks handled separately
      let contentHeight = 0;
      const codeBlockRegex = /```[\s\S]*?```/g;
      const codeBlocks = content.match(codeBlockRegex) || [];
      const textOnly = content.replace(codeBlockRegex, "");

      for (const block of codeBlocks) {
        contentHeight += block.split("\n").length * 20 + 32;
      }
      contentHeight += textOnly.split("\n").length * 22;

      estimate += Math.max(contentHeight, 22);

      // Attachments
      for (const a of attachments) {
        estimate += a.mime_type?.startsWith("image/") ? 320 : 48;
      }

      // Reactions (~36px) and thread indicator (~28px)
      if (msg.reactions?.length) estimate += 36;
      if (msg.thread_reply_count) estimate += 28;

      // "New Messages" divider (~32px)
      if (item?.isFirstUnread) estimate += 32;

      return estimate + 16; // padding
    },
    overscan: 5,
  });

  // --- Check if at bottom ---
  const checkIfAtBottom = () => {
    if (!containerRef) return true;
    const { scrollTop, scrollHeight, clientHeight } = containerRef;
    return scrollHeight - scrollTop - clientHeight < 100;
  };

  // --- Debounced mark-as-read on scroll-to-bottom ---
  let markAsReadTimer: ReturnType<typeof setTimeout> | null = null;

  const scheduleMarkAsRead = () => {
    if (markAsReadTimer) clearTimeout(markAsReadTimer);
    markAsReadTimer = setTimeout(() => {
      const msgs = messages();
      const lastMsg = msgs[msgs.length - 1];
      if (lastMsg && isAtBottom()) {
        markChannelAsRead(props.channelId, lastMsg.id);
      }
      markAsReadTimer = null;
    }, 3000);
  };

  const cancelMarkAsRead = () => {
    if (markAsReadTimer) {
      clearTimeout(markAsReadTimer);
      markAsReadTimer = null;
    }
  };

  onCleanup(() => cancelMarkAsRead());

  // --- Handle scroll ---
  const handleScroll = () => {
    // If in sticky-bottom mode (after sending), force re-scroll on any reflow
    if (Date.now() < stickyBottomUntil && containerRef) {
      if (!checkIfAtBottom()) {
        containerRef.scrollTo({ top: containerRef.scrollHeight, behavior: "auto" });
        return; // Don't update isAtBottom while sticky
      }
    }

    const atBottom = checkIfAtBottom();
    setIsAtBottom(atBottom);
    if (atBottom) {
      setHasNewMessages(false);
      setNewMessageCount(0);
      scheduleMarkAsRead();
    } else {
      cancelMarkAsRead();
    }
  };

  // --- Scroll to bottom ---
  const scrollToBottom = (sticky = false) => {
    if (!containerRef) return;
    const count = messagesWithCompact().length;
    if (count === 0) return;

    // Activate sticky mode — keeps re-scrolling for 500ms to handle
    // virtualizer measurement reflows that shrink scrollHeight
    if (sticky) {
      stickyBottomUntil = Date.now() + 1500;
    }

    virtualizer.scrollToIndex(count - 1, {
      align: "end",
      behavior: "auto",
    });
    setHasNewMessages(false);
    setNewMessageCount(0);
  };

  // --- Scroll to a specific message and highlight it ---
  const scrollToMessage = (messageId: string) => {
    const msgs = messagesWithCompact();
    const index = msgs.findIndex((m) => m.message.id === messageId);

    if (index !== -1) {
      // Message found in current buffer — scroll to it
      virtualizer.scrollToIndex(index, { align: "center", behavior: "smooth" });

      // Apply highlight with fade-out
      setHighlightedId(messageId);
      if (highlightTimer) clearTimeout(highlightTimer);
      highlightTimer = setTimeout(() => {
        setHighlightedId(null);
        highlightTimer = null;
      }, 2000);

      pendingHighlightId = null;
      return true;
    }

    return false;
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
      console.warn(
        "[MessageList] Eviction would remove all messages, skipping",
      );
      return;
    }

    setMessagesState("byChannel", props.channelId, kept);
    // Re-enable hasMore for evicted directions
    if (keepStart > 0) {
      setMessagesState("hasMore", props.channelId, true);
    }
  }

  // --- IntersectionObserver for upward pagination ---
  createEffect(
    on(
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
                console.error("[MessageList] Unhandled pagination error:", err),
              );
            }
          },
          { root: containerRef, rootMargin: "200px 0px 0px 0px" },
        );
        observer.observe(sentinelRef);
        onCleanup(() => observer.disconnect());
      },
    ),
  );

  // Clean up highlight timer on unmount
  onCleanup(() => {
    if (highlightTimer) clearTimeout(highlightTimer);
  });

  // --- Escape key: mark channel as read ---
  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      // Don't trigger if a modal, context menu, or search overlay is open
      const hasOverlay = document.querySelector("[role='dialog'], [data-context-menu], [data-search-overlay]");
      if (hasOverlay) return;

      const msgs = messages();
      const lastMsg = msgs[msgs.length - 1];
      if (lastMsg && isChannelUnread(props.channelId)) {
        markChannelAsRead(props.channelId, lastMsg.id);
        scrollToBottom(true);
      }
    }
  };

  onMount(() => document.addEventListener("keydown", handleKeyDown));
  onCleanup(() => document.removeEventListener("keydown", handleKeyDown));

  // Track message count for auto-scroll / new-message indicator
  let prevMessageCount = 0;

  // --- Watch for ?highlight= search param ---
  createEffect(() => {
    const highlightId = searchParams.highlight;
    if (highlightId && typeof highlightId === "string") {
      // Store as pending and consume the param
      pendingHighlightId = highlightId;
      setSearchParams({ highlight: undefined }, { replace: true });

      // Try to scroll immediately if messages are already loaded
      const found = scrollToMessage(highlightId);
      if (!found) {
        // Messages may not be loaded yet; the pending ID will be
        // picked up by the message-count tracking effect below.
      }
    }
  });

  // --- Load messages when channelId changes ---
  createEffect(
    on(
      () => props.channelId,
      (channelId, prevChannelId) => {
        if (channelId && channelId !== prevChannelId) {
          setIsAtBottom(true);
          setHasNewMessages(false);
          setNewMessageCount(0);
          setPaginationError(null);
          prevMessageCount = 0;

          const lastReadId = getChannelLastReadMessageId(channelId);
          const unread = isChannelUnread(channelId);
          const unreadCnt = getUnreadCount(channelId);
          loadInitialMessages(channelId, unread ? lastReadId : undefined, unreadCnt);
        }
      },
      { defer: false },
    ),
  );

  // --- Track new messages for auto-scroll / indicator ---
  createEffect(() => {
    const currentCount = messages().length;

    if (currentCount > prevMessageCount && prevMessageCount > 0) {
      // Check for pending highlight before auto-scrolling
      if (pendingHighlightId) {
        const found = scrollToMessage(pendingHighlightId);
        if (found) {
          // Highlight handled; skip normal scroll behavior
          prevMessageCount = currentCount;
          return;
        }
      }

      // Always scroll when the user sends a message (optimistic messages have pending: prefix)
      const msgs = messages();
      const lastMsg = msgs[msgs.length - 1];
      const isOwnSend = lastMsg?.id?.startsWith("pending:");

      if (isOwnSend || isAtBottom()) {
        requestAnimationFrame(() => scrollToBottom(true));
      } else {
        setHasNewMessages(true);
        setNewMessageCount(
          (count) => count + (currentCount - prevMessageCount),
        );
      }
    } else if (currentCount > 0 && prevMessageCount === 0) {
      // Initial load complete — sticky to absorb measurement reflows
      if (pendingHighlightId) {
        requestAnimationFrame(() => {
          if (pendingHighlightId) {
            const found = scrollToMessage(pendingHighlightId);
            if (!found) {
              pendingHighlightId = null;
              scrollToBottom(true);
            }
          }
        });
      } else {
        const lastReadId = getChannelLastReadMessageId(props.channelId);
        if (lastReadId && isChannelUnread(props.channelId)) {
          // Scroll to first unread (the message after lastReadId)
          const msgs = messages();
          const readIdx = msgs.findIndex((m) => m.id === lastReadId);
          if (readIdx !== -1 && readIdx < msgs.length - 1) {
            requestAnimationFrame(() => {
              virtualizer.scrollToIndex(readIdx + 1, { align: "start", behavior: "auto" });
              scheduleMarkAsRead();
            });
          } else {
            // lastReadId not in loaded range — fallback to bottom
            requestAnimationFrame(() => {
              scrollToBottom(true);
              scheduleMarkAsRead();
            });
          }
        } else {
          requestAnimationFrame(() => {
            scrollToBottom(true);
            if (isChannelUnread(props.channelId)) {
              scheduleMarkAsRead();
            }
          });
        }
      }
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
          <h2 class="text-lg font-semibold text-text-primary mb-1">
            Beginning of conversation
          </h2>
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
          <p class="text-text-secondary max-w-sm mb-4">{messagesState.error}</p>
          <button
            onClick={() => loadInitialMessages(props.channelId)}
            class="px-4 py-2 bg-accent-primary text-on-accent rounded-lg font-medium hover:opacity-90 transition-opacity"
          >
            Retry
          </button>
        </div>
      </Show>

      {/* Empty state */}
      <Show
        when={!loading() && messages().length === 0 && !messagesState.error}
      >
        <div class="flex flex-col items-center justify-center h-full text-center px-4">
          <img src={flokiHappy} alt="" class="w-16 h-16 object-contain mb-4" loading="lazy" />
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
          style={{
            height: `${virtualizer.getTotalSize()}px`,
            position: "relative",
          }}
        >
          <Index each={virtualizer.getVirtualItems()}>
            {(virtualItem) => {
              const item = () => messagesWithCompact()[virtualItem().index];
              const isHighlighted = () =>
                item()?.message.id != null &&
                highlightedId() === item()?.message.id;
              return (
                <div
                  role="listitem"
                  data-index={virtualItem().index}
                  ref={(el) => {
                    queueMicrotask(() => virtualizer.measureElement(el));
                  }}
                  class={isHighlighted() ? "message-highlight" : undefined}
                  style={{
                    position: "absolute",
                    top: 0,
                    left: 0,
                    width: "100%",
                    transform: `translateY(${virtualItem().start}px)`,
                  }}
                >
                  {(() => {
                    const data = item();
                    return data ? (
                      <>
                        <Show when={data.isFirstUnread}>
                          <div class="flex items-center gap-2 px-4 py-1 my-1">
                            <div class="flex-1 h-px bg-accent-danger" />
                            <span class="text-xs font-semibold text-accent-danger uppercase tracking-wide">
                              New Messages
                            </span>
                            <div class="flex-1 h-px bg-accent-danger" />
                          </div>
                        </Show>
                        <MessageItem
                          message={data.message}
                          compact={data.isCompact}
                          guildId={props.guildId}
                          threadsEnabled={areThreadsEnabled(props.guildId)}
                        />
                      </>
                    ) : null;
                  })()}
                </div>
              );
            }}
          </Index>
        </div>
      </Show>

      {/* New messages indicator */}
      <Show when={hasNewMessages()}>
        <button
          onClick={() => scrollToBottom(true)}
          class="fixed bottom-24 left-1/2 transform -translate-x-1/2 bg-accent-primary hover:bg-accent-primary/90 text-on-accent px-5 py-2.5 rounded-full shadow-2xl flex items-center gap-2 transition-all z-10 font-medium"
        >
          <ChevronDown class="w-4 h-4" />
          <span>
            {newMessageCount() === 1
              ? "1 new message"
              : `${newMessageCount()} new messages`}
          </span>
        </button>
      </Show>

      {/* Image lightbox (rendered via Portal) */}
      <MessageImageLightbox />
    </div>
  );
};

export default MessageList;
