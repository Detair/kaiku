/**
 * Thread Indicator
 *
 * Displayed on parent messages that have thread replies.
 * Shows reply count and last reply time.
 * Clickable to open the thread sidebar.
 */

import { Component, Show } from "solid-js";
import type { Message } from "@/lib/types";
import { openThread } from "@/stores/threads";
import { formatRelativeTime } from "@/lib/utils";

interface ThreadIndicatorProps {
  message: Message;
}

const ThreadIndicator: Component<ThreadIndicatorProps> = (props) => {
  const replyCount = () => props.message.thread_reply_count;
  const lastReplyAt = () => props.message.thread_last_reply_at;

  const handleClick = () => {
    openThread(props.message);
  };

  return (
    <Show when={replyCount() > 0}>
      <button
        onClick={handleClick}
        class="mt-1 flex items-center gap-2 px-2 py-1 rounded-md hover:bg-white/5 transition-colors cursor-pointer group/thread"
      >
        <span class="text-sm font-medium text-accent-primary group-hover/thread:underline">
          {replyCount()} {replyCount() === 1 ? "reply" : "replies"}
        </span>
        <Show when={lastReplyAt()}>
          <span class="text-xs text-text-secondary">
            Last reply {formatRelativeTime(lastReplyAt()!)}
          </span>
        </Show>
      </button>
    </Show>
  );
};

export default ThreadIndicator;
