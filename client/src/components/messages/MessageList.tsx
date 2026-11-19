import { lastMarkableMessage as lastMarkableMessageOf } from "@/lib/readCursor";
import {
  Component,
  For,
  Show,
  createEffect,
  on,
  createMemo,
  createSignal,
  onCleanup,
  onMount,
  untrack,
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
import { countAppended } from "./newMessages";
import {
  messagesState,
  setMessagesState,
  loadInitialMessages,
  loadMessages,
  loadMessagesForJump,
  loadNewerMessages,
  hasMoreMessages,
  hasNewerMessages,
} from "@/stores/messages";
import {
  channelsState,
  markChannelAsRead,
  getChannelLastReadMessageId,
  isChannelUnread,
  getUnreadCount,
} from "@/stores/channels";
import { areThreadsEnabled } from "@/stores/guilds";
import { dmsState } from "@/stores/dms";
import { currentUser } from "@/stores/auth";
import { showToast } from "@/components/ui/Toast";
import { shouldGroupWithPrevious } from "@/lib/utils";
import { isEscapeForOverlay } from "@/lib/a11y/dialog";
import { plural, t } from "@/i18n";
import type { Message } from "@/lib/types";
import { announcementFor, dayKey, dayLabel, threadsEnabledFor } from "./listHelpers";

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
  /** Channel the list last (re)loaded; a highlight for another one waits for its load. */
  let loadedChannelId: string | undefined;

  // --- Screen reader announcements for new messages from others ---
  const [announcement, setAnnouncement] = createSignal("");
  let announceQueue: Message[] = [];
  let announceTimer: ReturnType<typeof setTimeout> | null = null;
  /** Collect arrivals for a moment so a burst is one short sentence. */
  const queueAnnouncement = (arrived: Message[]) => {
    const me = currentUser()?.id;
    const channelId = props.channelId;
    const fromOthers = arrived.filter(
      (m) =>
        m.channel_id === channelId &&
        !m.id.startsWith("pending:") &&
        m.author.id !== me &&
        m.message_type !== "system",
    );
    if (fromOthers.length === 0) return;
    announceQueue.push(...fromOthers);
    if (announceTimer) return;
    announceTimer = setTimeout(() => {
      announceTimer = null;
      // Only what belongs to the channel on screen now.
      const queued = announceQueue.filter((m) => m.channel_id === props.channelId);
      announceQueue = [];
      const text = announcementFor(queued);
      if (text) setAnnouncement(text);
    }, 1500);
  };
  /** A new channel starts silent: nothing queued or shown from the last one. */
  const resetAnnouncements = () => {
    if (announceTimer) clearTimeout(announceTimer);
    announceTimer = null;
    announceQueue = [];
    setAnnouncement("");
  };
  onCleanup(() => {
    if (announceTimer) clearTimeout(announceTimer);
  });

  // --- Direct message or server channel ---
  const isDirectMessage = createMemo(() => dmsState.dms.some((dm) => dm.id === props.channelId));
  const guildIdForChannel = createMemo(
    () =>
      props.guildId ??
      channelsState.channels.find((c) => c.id === props.channelId)?.guild_id ??
      undefined,
  );
  const threadsEnabled = createMemo(() =>
    threadsEnabledFor(isDirectMessage(), guildIdForChannel(), areThreadsEnabled),
  );

  // Use createMemo for proper reactive tracking of store values
  const messages = createMemo(() => {
    return messagesState.byChannel[props.channelId] || [];
  });

  const lastMarkableMessage = () => lastMarkableMessageOf(messages());

  // Compute messages with compact flag and first-unread marker
  const messagesWithCompact = createMemo(() => {
    const msgs = messages();
    const lastReadId = getChannelLastReadMessageId(props.channelId);
    let foundLastRead = lastReadId == null; // if null, no divider needed

    return msgs.map((message, idx) => {
      const prev = idx > 0 ? msgs[idx - 1] : null;
      const isNewDay = !prev || dayKey(prev.created_at) !== dayKey(message.created_at);
      const isCompact = prev && !isNewDay
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

      return { message, isCompact, isFirstUnread, isNewDay };
    });
  });

  const loading = createMemo(
    () => !!messagesState.loadingChannels[props.channelId],
  );

  type CompactItem = ReturnType<typeof messagesWithCompact>[number];
  const itemsById = createMemo(
    () => new Map(messagesWithCompact().map((item) => [item.message.id, item])),
  );

  // Message ids by index. A new array (and so a new getItemKey) only when the
  // ids change, e.g. a pending message confirmed, not on every edit or reaction.
  const messageKeys = createMemo(
    () => messagesWithCompact().map((item) => item.message.id),
    undefined,
    { equals: (a, b) => a.length === b.length && a.every((id, i) => id === b[i]) },
  );

  // --- Virtualizer ---
  const virtualizer = createVirtualizer({
    get count() {
      return messageKeys().length;
    },
    get getItemKey() {
      const keys = messageKeys();
      return (index: number) => keys[index] ?? index;
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

      // "New Messages" divider and day separator (~32px each)
      if (item?.isFirstUnread) estimate += 32;
      if (item?.isNewDay) estimate += 32;

      return estimate + 16; // padding
    },
    overscan: 5,
  });

  let sweepQueued = false;
  const scheduleElementSweep = () => {
    if (sweepQueued) return;
    sweepQueued = true;
    queueMicrotask(() => {
      sweepQueued = false;
      virtualizer.measureElement(null);
    });
  };

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
    const channelId = props.channelId;
    markAsReadTimer = setTimeout(() => {
      markAsReadTimer = null;
      // The timer belongs to the channel it was started in: after a switch
      // the new channel has not been on screen for three seconds yet.
      if (props.channelId !== channelId) return;
      const lastMsg = lastMarkableMessage();
      // An older window (after a far jump) must never move the read marker
      // back: the server stores whatever id it gets.
      if (lastMsg && isAtBottom() && checkIfAtBottom() && !hasNewerMessages(channelId)) {
        markChannelAsRead(channelId, lastMsg.id);
      }
    }, 3000);
  };

  // When the newest markable message changes while the list sits at the
  // bottom (a send confirmed after the timer ran, a live message in place),
  // start the guarded timer again so the marker reaches it. Not on channel
  // switches or the first load: those have their own paths.
  createEffect(
    on(
      () => [props.channelId, lastMarkableMessage()?.id] as const,
      ([channelId, id], prev) => {
        if (!prev || prev[0] !== channelId || !prev[1] || !id || id === prev[1]) return;
        if (isAtBottom()) scheduleMarkAsRead();
      },
    ),
  );

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
    if (atBottom && hasNewerMessages(props.channelId)) void loadNewer();
    if (atBottom) {
      setHasNewMessages(false);
      setNewMessageCount(0);
      scheduleMarkAsRead();
    } else {
      cancelMarkAsRead();
    }
  };

  // --- Newer messages after an older window ---
  /** True while a page of newer messages is merged: not "new" arrivals. */
  let isLoadingNewer = false;
  const loadNewer = async () => {
    if (isLoadingNewer) return;
    isLoadingNewer = true;
    try {
      await loadNewerMessages(props.channelId);
    } finally {
      isLoadingNewer = false;
    }
    // The page went in below the view without a scroll event: the person
    // has not seen it yet, so the list is no longer "at the bottom".
    const atBottom = checkIfAtBottom();
    setIsAtBottom(atBottom);
    if (!atBottom) cancelMarkAsRead();
  };

  /** Go to the newest message, reloading when the list shows an older window. */
  let bottomAfterLoad = false;
  const jumpToNewest = () => {
    if (hasNewerMessages(props.channelId)) {
      bottomAfterLoad = true;
      setHasNewMessages(false);
      setNewMessageCount(0);
      void loadInitialMessages(props.channelId);
      return;
    }
    scrollToBottom(true);
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
      // Leaving the bottom: keep the bottom pin from pulling the list back.
      if (index < msgs.length - 1) setIsAtBottom(false);

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

  /**
   * Scroll to `messageId`, loading the messages around it first when it is
   * not loaded (an old search result or pinned message).
   */
  const jumpTo = async (messageId: string) => {
    pendingHighlightId = messageId;
    if (scrollToMessage(messageId)) return;
    const channelId = props.channelId;
    const loaded = await loadMessagesForJump(channelId, messageId);
    if (pendingHighlightId !== messageId || props.channelId !== channelId) return;
    requestAnimationFrame(() => {
      if (pendingHighlightId !== messageId) return;
      if (loaded && scrollToMessage(messageId)) return;
      pendingHighlightId = null;
      showToast({ type: "info", title: t("chat.list.jumpFailed") });
    });
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
      if (isEscapeForOverlay(e)) return;

      const lastMsg = lastMarkableMessage();
      if (lastMsg && isChannelUnread(props.channelId) && !hasNewerMessages(props.channelId)) {
        markChannelAsRead(props.channelId, lastMsg.id);
        scrollToBottom(true);
      }
    }
  };

  // Stay at the bottom when the list gets shorter (window resized, composer
  // grown by a multi-line draft) or the content below grows (an image in the
  // last message finished loading): neither fires a scroll event.
  const bottomPin = new ResizeObserver(() => {
    if (containerRef && isAtBottom()) {
      containerRef.scrollTop = containerRef.scrollHeight;
    }
  });
  onCleanup(() => bottomPin.disconnect());
  onMount(() => {
    if (containerRef) bottomPin.observe(containerRef);
  });

  onMount(() => document.addEventListener("keydown", handleKeyDown));
  onCleanup(() => document.removeEventListener("keydown", handleKeyDown));

  // Track message count for auto-scroll / new-message indicator
  let prevMessageCount = 0;
  /** Newest message id at the last check; older pages load in front of it. */
  let prevLastId: string | undefined;

  // --- Watch for ?highlight= search param ---
  createEffect(() => {
    const highlightId = searchParams.highlight;
    if (highlightId && typeof highlightId === "string") {
      // Store as pending and consume the param
      pendingHighlightId = highlightId;
      setSearchParams({ highlight: undefined }, { replace: true });

      // Channel already shown: jump now (loading the message if needed).
      // A channel that is about to load picks the pending ID up in the
      // message-count tracking effect below.
      if (untrack(() => props.channelId) === loadedChannelId && !untrack(loading)) {
        void jumpTo(highlightId);
      }
    }
  });

  // --- Load messages when channelId changes ---
  createEffect(
    on(
      () => props.channelId,
      (channelId, prevChannelId) => {
        if (channelId && channelId !== prevChannelId) {
          loadedChannelId = channelId;
          resetAnnouncements();
          bottomAfterLoad = false;
          setIsAtBottom(true);
          setHasNewMessages(false);
          setNewMessageCount(0);
          setPaginationError(null);
          // Sizes are keyed by message id; drop the previous channel's.
          virtualizer.measure();
          prevMessageCount = 0;
          prevLastId = undefined;

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

    if (isLoadingNewer && currentCount > prevMessageCount && prevMessageCount > 0) {
      // A page of older-but-newer history merged in, not new arrivals.
    } else if (currentCount > prevMessageCount && prevMessageCount > 0) {
      // Check for pending highlight before auto-scrolling
      if (pendingHighlightId) {
        const found = scrollToMessage(pendingHighlightId);
        if (found) {
          // Highlight handled; skip normal scroll behavior
          prevMessageCount = currentCount;
          prevLastId = messages()[currentCount - 1]?.id;
          return;
        }
      }

      // Only messages after the previous newest one are new; a loaded page
      // of older messages grows the list at the front.
      const msgs = messages();
      const lastMsg = msgs[msgs.length - 1];
      const appended = countAppended(msgs, prevLastId, prevMessageCount);

      // Always scroll when the user sends a message (optimistic messages have pending: prefix)
      const isOwnSend = lastMsg?.id?.startsWith("pending:");

      if (appended > 0) queueAnnouncement(msgs.slice(msgs.length - appended));

      if (appended > 0 && (isOwnSend || isAtBottom())) {
        // The new message is on screen: mark read through the guarded timer
        // (no scroll event fires when the list already was at the bottom).
        // This is also where a sent message marks the channel read.
        requestAnimationFrame(() => {
          scrollToBottom(true);
          scheduleMarkAsRead();
        });
      } else if (appended > 0) {
        setHasNewMessages(true);
        setNewMessageCount((count) => count + appended);
      }
    } else if (currentCount > 0 && prevMessageCount === 0) {
      // Initial load complete — sticky to absorb measurement reflows
      if (bottomAfterLoad) {
        bottomAfterLoad = false;
        requestAnimationFrame(() => {
          scrollToBottom(true);
          scheduleMarkAsRead();
        });
      } else if (pendingHighlightId) {
        requestAnimationFrame(() => {
          if (pendingHighlightId) {
            const id = pendingHighlightId;
            if (!scrollToMessage(id)) {
              scrollToBottom(true);
              void jumpTo(id);
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
              // The scroll event comes a frame later; until then the bottom
              // pin would still see "at bottom" and undo the jump.
              setIsAtBottom(checkIfAtBottom());
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
    prevLastId = messages()[currentCount - 1]?.id;
  });

  return (
    <>
    <div
      ref={containerRef}
      class="flex-1 overflow-y-auto relative"
      role="list"
      aria-label={t("chat.list.label")}
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
            {t("chat.list.start.title")}
          </h2>
          <p class="text-sm text-text-secondary">
            {t("chat.list.start.hint")}
          </p>
        </div>
      </Show>

      {/* Pagination error indicator */}
      <Show when={paginationError() && messages().length > 0}>
        <div class="flex items-center justify-center gap-2 py-3 px-4 text-sm text-accent-danger">
          <AlertCircle class="w-4 h-4 flex-shrink-0" />
          <span>{t("chat.list.olderFailed")}</span>
          <button
            onClick={() => triggerLoadMore().catch(() => {})}
            class="ml-1 text-text-link hover:underline inline-flex items-center gap-1"
          >
            <RefreshCw class="w-3 h-3" />
            {t("common.retry")}
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
          <p class="text-text-secondary">{t("chat.list.loading")}</p>
        </div>
      </Show>

      {/* Error state */}
      <Show when={!loading() && messages().length === 0 && messagesState.loadError}>
        <div class="flex flex-col items-center justify-center h-full text-center px-4">
          <AlertCircle class="w-10 h-10 text-accent-danger mb-4" />
          <h3 class="text-lg font-semibold text-text-primary mb-2">
            {t("chat.list.loadFailedTitle")}
          </h3>
          <p class="text-text-secondary max-w-sm mb-4">{messagesState.loadError}</p>
          <button
            onClick={() => loadInitialMessages(props.channelId)}
            class="px-4 py-2 bg-accent-primary text-on-accent rounded-lg font-medium hover:opacity-90 transition-opacity"
          >
            {t("common.retry")}
          </button>
        </div>
      </Show>

      {/* Empty state */}
      <Show
        when={!loading() && messages().length === 0 && !messagesState.loadError}
      >
        <div class="flex flex-col items-center justify-center h-full text-center px-4">
          <img src={flokiHappy} alt={""} class="w-16 h-16 object-contain mb-4" loading="lazy" />
          <h3 class="text-lg font-semibold text-text-primary mb-2">
            {t("chat.list.empty.title")}
          </h3>
          <p class="text-text-secondary max-w-sm">
            {isDirectMessage() ? t("chat.list.empty.hintDm") : t("chat.list.empty.hint")}
          </p>
        </div>
      </Show>

      {/* Virtualized messages. Always mounted: the rows are disposed by
          <For> alone (an outer <Show> on the count disposed them a second
          time in the same update when a channel emptied). */}
      <div
        ref={(el) => bottomPin.observe(el)}
        style={{
          height: `${virtualizer.getTotalSize()}px`,
          position: "relative",
        }}
      >
        <For each={virtualizer.getVirtualItems()}>
          {(virtualItem) => {
            // One row per message id: the element, its measured size and the
            // MessageItem stay with the message while rows shift around it.
            // Looked up by key, not index: while a page is prepended the
            // index can lag behind the data. Keeps the last value while the
            // row is being removed, so MessageItem's cleanup never reads a
            // disposed <Show> accessor.
            const item = createMemo<CompactItem | undefined>(
              (prev) => itemsById().get(String(virtualItem.key)) ?? prev,
              undefined,
              {
                // messagesWithCompact() rebuilds its wrappers on every list
                // change; only a different message or flag is an update.
                equals: (a, b) =>
                  a?.message === b?.message &&
                  a?.isCompact === b?.isCompact &&
                  a?.isFirstUnread === b?.isFirstUnread &&
                  a?.isNewDay === b?.isNewDay,
              },
            );
            // virtual-core caches each measured element by key and only
            // drops detached ones on measureElement(null), which Solid never
            // calls by itself. Sweep once this row's element is removed.
            onCleanup(scheduleElementSweep);
            const isHighlighted = () =>
              item()?.message.id != null &&
              highlightedId() === item()?.message.id;
            return (
              <div
                role="listitem"
                data-index={virtualItem.index}
                ref={(el) => {
                  queueMicrotask(() => virtualizer.measureElement(el));
                }}
                class={isHighlighted() ? "message-highlight" : undefined}
                style={{
                  position: "absolute",
                  top: 0,
                  left: 0,
                  width: "100%",
                  transform: `translateY(${virtualItem.start}px)`,
                }}
              >
                <Show when={item()}>
                  {(data) => (
                    <>
                      <Show when={data().isNewDay}>
                        <div
                          role="separator"
                          aria-label={dayLabel(data().message.created_at)}
                          class="flex items-center gap-2 px-4 py-1 my-1"
                        >
                          <div class="flex-1 h-px bg-white/10" />
                          <span class="text-xs font-semibold text-text-secondary">
                            {dayLabel(data().message.created_at)}
                          </span>
                          <div class="flex-1 h-px bg-white/10" />
                        </div>
                      </Show>
                      <Show when={data().isFirstUnread}>
                        <div class="flex items-center gap-2 px-4 py-1 my-1">
                          <div class="flex-1 h-px bg-accent-danger" />
                          <span class="text-xs font-semibold text-accent-danger uppercase tracking-wide">
                            {t("chat.list.newMessages")}
                          </span>
                          <div class="flex-1 h-px bg-accent-danger" />
                        </div>
                      </Show>
                      <MessageItem
                        message={data().message}
                        compact={data().isCompact}
                        guildId={props.guildId}
                        threadsEnabled={threadsEnabled()}
                      />
                    </>
                  )}
                </Show>
              </div>
            );
          }}
        </For>
      </div>

      {/* Back to the newest message: shown whenever the list is scrolled up.
          A zero-height sticky strip keeps it at the bottom of the chat (not
          of the window), centred over the messages. */}
      <Show
        when={
          hasNewMessages() ||
          (messages().length > 0 && (!isAtBottom() || hasNewerMessages(props.channelId)))
        }
      >
        <div class="sticky bottom-0 h-0 z-10 flex justify-center pointer-events-none">
          <button
            data-testid="jump-to-latest"
            onClick={jumpToNewest}
            class={`absolute bottom-4 pointer-events-auto rounded-full shadow-2xl flex items-center gap-2 transition-all font-medium ${
              hasNewMessages()
                ? "bg-accent-primary hover:bg-accent-primary/90 text-on-accent px-5 py-2.5"
                : "bg-surface-layer2 hover:bg-surface-layer1 text-text-primary border border-white/10 px-4 py-2 text-sm"
            }`}
          >
            <ChevronDown class="w-4 h-4" />
            <span>
              {hasNewMessages()
                ? plural(newMessageCount(), {
                    one: "chat.list.newCount.one",
                    other: "chat.list.newCount.other",
                  })
                : t("chat.list.jumpToLatest")}
            </span>
          </button>
        </div>
      </Show>

      {/* Image lightbox (rendered via Portal) */}
      <MessageImageLightbox />
    </div>

    {/* Screen reader: new messages from others, one short sentence at a time.
        Outside the list so the list holds only its rows. */}
    <div class="sr-only" role="status" aria-live="polite" aria-atomic="true" data-testid="new-message-announcer">
      {announcement()}
    </div>
    </>
  );
};

export default MessageList;
