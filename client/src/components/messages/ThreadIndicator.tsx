/**
 * Thread Indicator
 *
 * Displayed on parent messages that have thread replies.
 * Shows participant avatars, reply count, last reply time, and unread indicator.
 * Clickable to open the thread sidebar.
 */

import { Component, Show, For } from "solid-js";
import type { Message, ThreadInfo } from "@/lib/types";
import { openThread } from "@/stores/threads";
import { threadsState } from "@/stores/threads";
import { formatRelativeTime } from "@/lib/utils";
import Avatar from "@/components/ui/Avatar";

interface ThreadIndicatorProps {
  message: Message;
}

const ThreadIndicator: Component<ThreadIndicatorProps> = (props) => {
  const replyCount = () => props.message.thread_reply_count;
  const lastReplyAt = () => props.message.thread_last_reply_at;

  // Cache (updated by WebSocket events) takes priority over initial message response
  const threadInfo = (): ThreadInfo | undefined =>
    threadsState.threadInfoCache[props.message.id] ?? props.message.thread_info;

  const participants = () => threadInfo()?.participant_ids ?? [];
  const avatars = () => threadInfo()?.participant_avatars ?? [];

  const hasUnread = (): boolean => {
    const info = threadInfo();
    return info?.has_unread === true;
  };

  const handleClick = () => {
    openThread(props.message);
  };

  return (
    <Show when={replyCount() > 0}>
      <button
        onClick={handleClick}
        class="mt-1 flex items-center gap-2 px-2 py-1 rounded-md hover:bg-white/5 transition-colors cursor-pointer group/thread"
      >
        {/* Participant avatars (up to 3, overlapping) */}
        <Show when={participants().length > 0}>
          <div class="flex items-center -space-x-1.5">
            <For each={participants().slice(0, 3)}>
              {(participantId, index) => (
                <div class="ring-2 ring-surface-base rounded-full">
                  <Avatar
                    src={avatars()[index()]}
                    alt={participantId}
                    size="xs"
                  />
                </div>
              )}
            </For>
          </div>
        </Show>

        {/* Unread dot */}
        <Show when={hasUnread()}>
          <span class="w-2 h-2 rounded-full bg-accent-primary flex-shrink-0" />
        </Show>

        <span
          class={`text-sm text-accent-primary group-hover/thread:underline ${hasUnread() ? "font-bold" : "font-medium"}`}
        >
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
