import { Component, For, Show, createEffect, on, createMemo, createSignal, onMount, onCleanup } from "solid-js";
import { Loader2, ChevronDown } from "lucide-solid";
import MessageItem from "./MessageItem";
import {
  messagesState,
  loadInitialMessages,
} from "@/stores/messages";
import { shouldGroupWithPrevious } from "@/lib/utils";

interface MessageListProps {
  channelId: string;
  /** Guild ID for custom emoji support in reactions */
  guildId?: string;
}

const MessageList: Component<MessageListProps> = (props) => {
  let containerRef: HTMLDivElement | undefined;
  let bottomRef: HTMLDivElement | undefined;

  // Track scroll state
  const [isAtBottom, setIsAtBottom] = createSignal(true);
  const [hasNewMessages, setHasNewMessages] = createSignal(false);
  const [newMessageCount, setNewMessageCount] = createSignal(0);

  // Check if user is scrolled to bottom (within 100px threshold)
  const checkIfAtBottom = () => {
    if (!containerRef) return true;
    const { scrollTop, scrollHeight, clientHeight } = containerRef;
    return scrollHeight - scrollTop - clientHeight < 100;
  };

  // Handle scroll events
  const handleScroll = () => {
    const atBottom = checkIfAtBottom();
    setIsAtBottom(atBottom);

    // Clear new message indicator when user scrolls to bottom
    if (atBottom) {
      setHasNewMessages(false);
      setNewMessageCount(0);
    }
  };

  // Scroll to bottom
  const scrollToBottom = (smooth = true) => {
    if (bottomRef) {
      bottomRef.scrollIntoView({ behavior: smooth ? "smooth" : "instant" });
      setHasNewMessages(false);
      setNewMessageCount(0);
    }
  };

  // Load messages when channelId changes
  createEffect(on(
    () => props.channelId,
    (channelId, prevChannelId) => {
      if (channelId && channelId !== prevChannelId) {
        // Reset scroll state for new channel
        setIsAtBottom(true);
        setHasNewMessages(false);
        setNewMessageCount(0);
        loadInitialMessages(channelId);
      }
    },
    { defer: false }
  ));

  // Use createMemo for proper reactive tracking of store values
  const messages = createMemo(() => {
    const msgs = messagesState.byChannel[props.channelId];
    return msgs || [];
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

  // Track message count changes for auto-scroll and new message indicator
  let prevMessageCount = 0;
  createEffect(() => {
    const currentCount = messages().length;

    if (currentCount > prevMessageCount && prevMessageCount > 0) {
      // New messages arrived
      if (isAtBottom()) {
        // Auto-scroll to bottom
        setTimeout(() => scrollToBottom(true), 50);
      } else {
        // Show new message indicator
        setHasNewMessages(true);
        setNewMessageCount(count => count + (currentCount - prevMessageCount));
      }
    } else if (currentCount > 0 && prevMessageCount === 0) {
      // Initial load - scroll to bottom instantly
      setTimeout(() => scrollToBottom(false), 50);
    }

    prevMessageCount = currentCount;
  });

  // Setup scroll listener
  onMount(() => {
    if (containerRef) {
      containerRef.addEventListener("scroll", handleScroll, { passive: true });
    }
  });

  onCleanup(() => {
    if (containerRef) {
      containerRef.removeEventListener("scroll", handleScroll);
    }
  });

  return (
    <div
      ref={containerRef}
      class="flex-1 overflow-y-auto relative"
    >
      {/* Loading indicator at top */}
      <Show when={loading() && messages().length > 0}>
        <div class="flex justify-center py-4">
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

      {/* Empty state */}
      <Show when={!loading() && messages().length === 0}>
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

      {/* Messages */}
      <Show when={messagesWithCompact().length > 0}>
        <div class="py-4">
          <For each={messagesWithCompact()}>
            {(item) => (
              <MessageItem message={item.message} compact={item.isCompact} guildId={props.guildId} />
            )}
          </For>
        </div>
      </Show>

      {/* Scroll anchor at bottom */}
      <div ref={bottomRef} />

      {/* New messages indicator */}
      <Show when={hasNewMessages()}>
        <button
          onClick={() => scrollToBottom(true)}
          class="fixed bottom-24 left-1/2 transform -translate-x-1/2 bg-accent-primary hover:bg-accent-primary/90 text-surface-base px-5 py-2.5 rounded-full shadow-2xl flex items-center gap-2 transition-all z-10 font-medium"
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
