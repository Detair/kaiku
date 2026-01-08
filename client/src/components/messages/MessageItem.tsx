import { Component, Show } from "solid-js";
import type { Message } from "@/lib/types";
import { formatTimestamp } from "@/lib/utils";
import Avatar from "@/components/ui/Avatar";

interface MessageItemProps {
  message: Message;
  /** If true, shows compact version without avatar/name (for grouped messages) */
  compact?: boolean;
}

const MessageItem: Component<MessageItemProps> = (props) => {
  const author = () => props.message.author;
  const isEdited = () => !!props.message.edited_at;

  return (
    <div
      class={`group flex gap-4 px-4 py-0.5 hover:bg-background-secondary/50 ${
        props.compact ? "mt-0" : "mt-4"
      }`}
    >
      {/* Avatar column */}
      <div class="w-10 flex-shrink-0">
        <Show when={!props.compact}>
          <Avatar
            src={author().avatar_url}
            name={author().display_name}
            status={author().status}
            size="md"
          />
        </Show>
        <Show when={props.compact}>
          {/* Show timestamp on hover for compact messages */}
          <span class="text-xs text-text-muted opacity-0 group-hover:opacity-100 transition-opacity select-none leading-6">
            {new Date(props.message.created_at).toLocaleTimeString([], {
              hour: "2-digit",
              minute: "2-digit",
            })}
          </span>
        </Show>
      </div>

      {/* Content column */}
      <div class="flex-1 min-w-0">
        <Show when={!props.compact}>
          <div class="flex items-baseline gap-2">
            <span class="font-medium text-text-primary hover:underline cursor-pointer">
              {author().display_name}
            </span>
            <span class="text-xs text-text-muted">
              {formatTimestamp(props.message.created_at)}
            </span>
          </div>
        </Show>

        <div class="text-text-primary break-words">
          {props.message.content}
          <Show when={isEdited()}>
            <span class="text-xs text-text-muted ml-1" title={`Edited ${formatTimestamp(props.message.edited_at!)}`}>
              (edited)
            </span>
          </Show>
        </div>

        {/* Attachments */}
        <Show when={props.message.attachments?.length > 0}>
          <div class="mt-2 flex flex-wrap gap-2">
            {props.message.attachments.map((attachment) => (
              <a
                href={attachment.url}
                target="_blank"
                rel="noopener noreferrer"
                class="flex items-center gap-2 px-3 py-2 bg-background-tertiary rounded-lg hover:bg-background-secondary transition-colors"
              >
                <span class="text-sm text-text-primary truncate max-w-48">
                  {attachment.filename}
                </span>
                <span class="text-xs text-text-muted">
                  {formatFileSize(attachment.size)}
                </span>
              </a>
            ))}
          </div>
        </Show>
      </div>
    </div>
  );
};

/**
 * Format file size in human-readable format.
 */
function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

export default MessageItem;
