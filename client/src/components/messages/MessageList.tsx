import { Component, For, Show, createEffect, onCleanup } from "solid-js";
import { Loader2 } from "lucide-solid";
import MessageItem from "./MessageItem";
import {
  messagesState,
  loadInitialMessages,
  loadMessages,
  getChannelMessages,
  isLoadingMessages,
  hasMoreMessages,
} from "@/stores/messages";
import { shouldGroupWithPrevious } from "@/lib/utils";

interface MessageListProps {
  channelId: string;
}

const MessageList: Component<MessageListProps> = (props) => {
  let containerRef: HTMLDivElement | undefined;
  let isAtBottom = true;

  // Load initial messages when channel changes
  createEffect(() => {
    const channelId = props.channelId;
    if (channelId) {
      loadInitialMessages(channelId);
    }
  });

  // Scroll to bottom when new messages arrive (if already at bottom)
  createEffect(() => {
    const messages = getChannelMessages(props.channelId);
    if (messages.length > 0 && isAtBottom && containerRef) {
      scrollToBottom();
    }
  });

  const scrollToBottom = () => {
    if (containerRef) {
      containerRef.scrollTop = containerRef.scrollHeight;
    }
  };

  const handleScroll = () => {
    if (!containerRef) return;

    // Check if at bottom
    const { scrollTop, scrollHeight, clientHeight } = containerRef;
    isAtBottom = scrollHeight - scrollTop - clientHeight < 50;

    // Load more when scrolled to top
    if (scrollTop < 100 && hasMoreMessages(props.channelId) && !isLoadingMessages(props.channelId)) {
      const oldScrollHeight = containerRef.scrollHeight;
      loadMessages(props.channelId).then(() => {
        // Maintain scroll position after loading older messages
        if (containerRef) {
          const newScrollHeight = containerRef.scrollHeight;
          containerRef.scrollTop = newScrollHeight - oldScrollHeight;
        }
      });
    }
  };

  const messages = () => getChannelMessages(props.channelId);
  const loading = () => isLoadingMessages(props.channelId);

  return (
    <div
      ref={containerRef}
      class="flex-1 overflow-y-auto"
      onScroll={handleScroll}
    >
      {/* Loading indicator at top */}
      <Show when={loading() && messages().length > 0}>
        <div class="flex justify-center py-4">
          <Loader2 class="w-5 h-5 text-text-muted animate-spin" />
        </div>
      </Show>

      {/* Initial loading state */}
      <Show when={loading() && messages().length === 0}>
        <div class="flex flex-col items-center justify-center h-full">
          <Loader2 class="w-8 h-8 text-text-muted animate-spin mb-4" />
          <p class="text-text-muted">Loading messages...</p>
        </div>
      </Show>

      {/* Empty state */}
      <Show when={!loading() && messages().length === 0}>
        <div class="flex flex-col items-center justify-center h-full text-center px-4">
          <div class="w-16 h-16 bg-background-secondary rounded-full flex items-center justify-center mb-4">
            <span class="text-2xl">👋</span>
          </div>
          <h3 class="text-lg font-semibold text-text-primary mb-2">
            No messages yet
          </h3>
          <p class="text-text-muted max-w-sm">
            Be the first to send a message in this channel!
          </p>
        </div>
      </Show>

      {/* Messages */}
      <Show when={messages().length > 0}>
        <div class="py-4">
          <For each={messages()}>
            {(message, index) => {
              const prev = () => messages()[index() - 1];
              const isCompact = () =>
                prev() &&
                shouldGroupWithPrevious(
                  message.created_at,
                  prev().created_at,
                  message.author.id,
                  prev().author.id
                );

              return <MessageItem message={message} compact={isCompact()} />;
            }}
          </For>
        </div>
      </Show>
    </div>
  );
};

export default MessageList;
