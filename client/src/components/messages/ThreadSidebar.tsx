/**
 * Thread Sidebar
 *
 * Side panel showing thread replies for a parent message.
 * Displays the parent message, scrollable replies, and a reply input.
 * ~400px wide, positioned as a flex sibling to the main message area.
 */

import { Component, Show, For, createSignal, createEffect } from "solid-js";
import { X, Send } from "lucide-solid";
import MessageItem from "./MessageItem";
import {
  threadsState,
  closeThread,
  loadMoreThreadReplies,
  sendThreadReply,
} from "@/stores/threads";
import { areThreadsEnabled } from "@/stores/guilds";

interface ThreadSidebarProps {
  channelId: string;
  guildId?: string;
}

const ThreadSidebar: Component<ThreadSidebarProps> = (props) => {
  const [replyContent, setReplyContent] = createSignal("");
  const [sending, setSending] = createSignal(false);
  let repliesEndRef: HTMLDivElement | undefined;
  let scrollContainerRef: HTMLDivElement | undefined;
  let textareaRef: HTMLTextAreaElement | undefined;
  let prevReplyCount = 0;
  let prevLastId: string | undefined;

  const parentMessage = () => threadsState.activeThreadParent;
  const threadId = () => threadsState.activeThreadId;
  const replies = () =>
    threadId() ? threadsState.repliesByThread[threadId()!] || [] : [];
  const isLoading = () =>
    threadId() ? threadsState.loadingThreads[threadId()!] : false;
  const hasMore = () =>
    threadId() ? threadsState.hasMore[threadId()!] : false;

  // Auto-scroll to bottom only when a new reply is appended (not on pagination)
  createEffect(() => {
    const currentReplies = replies();
    const currentCount = currentReplies.length;
    const currentLastId =
      currentCount > 0 ? currentReplies[currentCount - 1].id : undefined;

    // Scroll if: new reply appended at the end (last ID changed and count grew)
    // or initial load (prev was 0)
    const isNewReplyAppended =
      currentCount > prevReplyCount && currentLastId !== prevLastId;

    if (isNewReplyAppended && repliesEndRef) {
      setTimeout(
        () => repliesEndRef?.scrollIntoView({ behavior: "smooth" }),
        50,
      );
    }

    prevReplyCount = currentCount;
    prevLastId = currentLastId;
  });

  const handleSend = async () => {
    const content = replyContent().trim();
    const parentId = threadId();
    if (!content || !parentId || sending()) return;

    setSending(true);
    try {
      await sendThreadReply(parentId, props.channelId, content);
      setReplyContent("");
      // Reset textarea height
      if (textareaRef) {
        textareaRef.style.height = "auto";
      }
    } finally {
      setSending(false);
    }
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  // Auto-resize textarea
  const handleInput = (e: Event) => {
    const target = e.target as HTMLTextAreaElement;
    setReplyContent(target.value);
    target.style.height = "auto";
    target.style.height = Math.min(target.scrollHeight, 120) + "px";
  };

  // Load more when reaching the bottom of the loaded reply list
  const handleScroll = () => {
    if (!scrollContainerRef || !threadId()) return;

    const distanceFromBottom =
      scrollContainerRef.scrollHeight -
      scrollContainerRef.scrollTop -
      scrollContainerRef.clientHeight;

    if (distanceFromBottom <= 40 && hasMore() && !isLoading()) {
      loadMoreThreadReplies(threadId()!);
    }
  };

  return (
    <div class="w-[400px] flex-shrink-0 flex flex-col border-l border-white/5 bg-surface-layer1">
      {/* Header */}
      <header class="h-12 px-4 flex items-center justify-between border-b border-white/5 shadow-sm">
        <span class="font-semibold text-text-primary">Thread</span>
        <button
          onClick={closeThread}
          class="w-7 h-7 flex items-center justify-center rounded hover:bg-white/10 text-text-secondary hover:text-text-primary transition-colors"
          title="Close thread"
          aria-label="Close thread"
        >
          <X class="w-4 h-4" />
        </button>
      </header>

      {/* Scrollable content */}
      <div
        ref={scrollContainerRef}
        class="flex-1 overflow-y-auto"
        onScroll={handleScroll}
      >
        {/* Parent message */}
        <Show when={parentMessage()}>
          <div class="border-b border-white/5 pb-2">
            <MessageItem
              message={parentMessage()!}
              guildId={props.guildId}
              isInsideThread={true}
              threadsEnabled={areThreadsEnabled(props.guildId)}
            />
          </div>
        </Show>

        {/* Loading indicator for older replies */}
        <Show when={isLoading()}>
          <div class="flex justify-center py-3">
            <div class="w-5 h-5 border-2 border-accent-primary border-t-transparent rounded-full animate-spin" />
          </div>
        </Show>

        {/* Replies */}
        <Show
          when={replies().length > 0}
          fallback={
            <Show when={!isLoading()}>
              <div class="flex items-center justify-center py-8 text-text-secondary text-sm">
                No replies yet. Start the conversation!
              </div>
            </Show>
          }
        >
          <div class="py-1">
            <For each={replies()}>
              {(reply) => (
                <MessageItem
                  message={reply}
                  compact={false}
                  guildId={props.guildId}
                  isInsideThread={true}
                  threadsEnabled={areThreadsEnabled(props.guildId)}
                />
              )}
            </For>
          </div>
        </Show>

        {/* Scroll anchor */}
        <div ref={repliesEndRef} />
      </div>

      {/* Reply input (hidden when threads are disabled — existing threads are read-only) */}
      <Show when={areThreadsEnabled(props.guildId)}>
        <div class="px-4 pb-4 pt-2 border-t border-white/5">
          <div class="flex items-end gap-2 bg-surface-layer2 rounded-lg px-3 py-2 border border-white/5 focus-within:border-accent-primary/50 transition-colors">
            <textarea
              ref={textareaRef}
              value={replyContent()}
              onInput={handleInput}
              onKeyDown={handleKeyDown}
              placeholder="Reply in thread..."
              rows={1}
              class="flex-1 bg-transparent text-text-primary text-sm placeholder-text-secondary/50 resize-none outline-none max-h-[120px]"
            />
            <button
              onClick={handleSend}
              disabled={!replyContent().trim() || sending()}
              class="w-8 h-8 flex items-center justify-center rounded-lg text-text-secondary hover:text-accent-primary hover:bg-white/5 transition-colors disabled:opacity-30 disabled:cursor-not-allowed flex-shrink-0"
              title="Send reply"
              aria-label="Send reply"
            >
              <Send class="w-4 h-4" />
            </button>
          </div>
        </div>
      </Show>
    </div>
  );
};

export default ThreadSidebar;
