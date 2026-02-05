import { Component, Show, createMemo, For, onMount, onCleanup } from "solid-js";
import { marked } from "marked";
import DOMPurify from "dompurify";
import { File, Download, Copy, Link, Hash, Trash2, Flag, MessageSquareMore } from "lucide-solid";
import type { Message, Attachment } from "@/lib/types";
import { formatTimestamp } from "@/lib/utils";
import Avatar from "@/components/ui/Avatar";
import CodeBlock from "@/components/ui/CodeBlock";
import ReactionBar from "./ReactionBar";
import ThreadIndicator from "./ThreadIndicator";
import MessageActions from "./MessageActions";
import { getServerUrl, getAccessToken, addReaction, removeReaction } from "@/lib/tauri";
import { showContextMenu, type ContextMenuEntry } from "@/components/ui/ContextMenu";
import { currentUser } from "@/stores/auth";
import { showUserContextMenu, triggerReport } from "@/lib/contextMenuBuilders";
import { spoilerExtension } from "@/lib/markdown/spoilerExtension";
import { openThread } from "@/stores/threads";

interface MessageItemProps {
  message: Message;
  /** If true, shows compact version without avatar/name (for grouped messages) */
  compact?: boolean;
  /** Guild ID for custom emoji support in reactions */
  guildId?: string;
  /** If true, suppresses thread indicator and "Reply in Thread" actions (when rendered inside ThreadSidebar) */
  isInsideThread?: boolean;
}

// Configure marked for GitHub Flavored Markdown
marked.setOptions({
  breaks: true,
  gfm: true,
});

marked.use({ extensions: [spoilerExtension] });

// Configure DOMPurify for safe HTML rendering (XSS prevention)
const PURIFY_CONFIG = {
  ALLOWED_TAGS: ['p', 'br', 'strong', 'em', 'code', 'pre', 'a', 'ul', 'ol', 'li', 'blockquote', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'hr', 'del', 's', 'table', 'thead', 'tbody', 'tr', 'th', 'td'],
  ALLOWED_ATTR: ['href', 'target', 'rel'],
  ALLOW_DATA_ATTR: false,
  RETURN_TRUSTED_TYPE: false as const,
};

const sanitizeHtml = (html: string): string => {
  return DOMPurify.sanitize(html, PURIFY_CONFIG) as string;
};

interface CodeBlockData {
  type: 'code';
  language: string;
  code: string;
}

interface TextBlock {
  type: 'text';
  html: string;
}

type ContentBlock = CodeBlockData | TextBlock;

const MessageItem: Component<MessageItemProps> = (props) => {
  let contentRef: HTMLDivElement | undefined;

  const author = () => props.message.author;
  const isEdited = () => !!props.message.edited_at;
  const hasReactions = () => props.message.reactions && props.message.reactions.length > 0;

  // Setup spoiler click-to-reveal functionality
  onMount(() => {
    if (contentRef) {
      const spoilers = contentRef.querySelectorAll('.spoiler[data-spoiler="true"]');
      const listeners: Array<{ element: Element; handler: EventListener }> = [];

      spoilers.forEach((spoiler) => {
        const handler: EventListener = function(this: HTMLElement) {
          this.classList.add('revealed');
        };
        spoiler.addEventListener('click', handler);
        listeners.push({ element: spoiler, handler });
      });

      onCleanup(() => {
        listeners.forEach(({ element, handler }) => {
          element.removeEventListener('click', handler);
        });
      });
    }
  });

  const handleAddReaction = async (emoji: string) => {
    try {
      await addReaction(props.message.channel_id, props.message.id, emoji);
    } catch (err) {
      console.error("Failed to add reaction:", err);
    }
  };

  const handleRemoveReaction = async (emoji: string) => {
    try {
      await removeReaction(props.message.channel_id, props.message.id, emoji);
    } catch (err) {
      console.error("Failed to remove reaction:", err);
    }
  };

  const getDownloadUrl = (attachment: Attachment) => {
    // Construct absolute URL for the attachment with token for browser requests
    const baseUrl = getServerUrl().replace(/\/+$/, "");
    const token = getAccessToken();
    const url = `${baseUrl}/api/messages/attachments/${attachment.id}/download`;
    // Include token as query param since <img> and <a> can't set Authorization headers
    return token ? `${url}?token=${encodeURIComponent(token)}` : url;
  };

  const isImage = (mimeType: string) => mimeType.startsWith("image/");

  // Parse markdown and extract code blocks for separate rendering
  const contentBlocks = createMemo(() => {
    const content = props.message.content;
    const blocks: ContentBlock[] = [];

    // Split content by code blocks
    const codeBlockRegex = /```(\w+)?\n([\s\S]*?)```/g;
    let lastIndex = 0;
    let match;

    while ((match = codeBlockRegex.exec(content)) !== null) {
      // Add text before code block
      if (match.index > lastIndex) {
        const text = content.substring(lastIndex, match.index);
        if (text.trim()) {
          const html = sanitizeHtml(marked.parse(text, { async: false }) as string);
          blocks.push({ type: 'text', html });
        }
      }

      // Add code block
      blocks.push({
        type: 'code',
        language: match[1] || 'plaintext',
        code: match[2].trim(),
      });

      lastIndex = match.index + match[0].length;
    }

    // Add remaining text
    if (lastIndex < content.length) {
      const text = content.substring(lastIndex);
      if (text.trim()) {
        const html = sanitizeHtml(marked.parse(text, { async: false }) as string);
        blocks.push({ type: 'text', html });
      }
    }

    // If no code blocks found, just parse the whole content
    if (blocks.length === 0) {
      const html = sanitizeHtml(marked.parse(content, { async: false }) as string);
      blocks.push({ type: 'text', html });
    }

    return blocks;
  });

  const handleContextMenu = (e: MouseEvent) => {
    const msg = props.message;
    const me = currentUser();
    const isOwn = me?.id === msg.author.id;

    const items: ContextMenuEntry[] = [
      {
        label: "Copy Text",
        icon: Copy,
        action: () => navigator.clipboard.writeText(msg.content),
      },
      {
        label: "Copy Message Link",
        icon: Link,
        action: () => navigator.clipboard.writeText(`${window.location.origin}/channels/${msg.channel_id}/${msg.id}`),
      },
      {
        label: "Copy ID",
        icon: Hash,
        action: () => navigator.clipboard.writeText(msg.id),
      },
    ];

    // Only show "Reply in Thread" for top-level messages, not inside ThreadSidebar
    if (!msg.parent_id && !props.isInsideThread) {
      items.push(
        { separator: true },
        {
          label: "Reply in Thread",
          icon: MessageSquareMore,
          action: () => openThread(msg),
        },
      );
    }

    if (!isOwn) {
      items.push(
        { separator: true },
        {
          label: "Report Message",
          icon: Flag,
          danger: true,
          action: () => {
            triggerReport({
              userId: msg.author.id,
              username: msg.author.username,
              messageId: msg.id,
            });
          },
        },
      );
    }

    if (isOwn) {
      items.push(
        { separator: true },
        {
          label: "Delete Message",
          icon: Trash2,
          danger: true,
          action: () => {
            // TODO: trigger delete confirmation
            console.log("Delete message:", msg.id);
          },
        },
      );
    }

    showContextMenu(e, items);
  };

  return (
    <div
      onContextMenu={handleContextMenu}
      class={`group relative flex gap-4 px-4 py-0.5 hover:bg-white/3 transition-colors ${
        props.compact ? "mt-0" : "mt-4"
      }`}
    >
      {/* Avatar column */}
      <div class="w-10 flex-shrink-0">
        <Show when={!props.compact}>
          <div
            onContextMenu={(e: MouseEvent) => {
              e.stopPropagation();
              showUserContextMenu(e, { id: author().id, username: author().username, display_name: author().display_name });
            }}
          >
            <Avatar
              src={author().avatar_url}
              alt={author().display_name}
              status={author().status}
              size="md"
            />
          </div>
        </Show>
        <Show when={props.compact}>
          {/* Show timestamp on hover for compact messages */}
          <span class="text-xs text-text-secondary opacity-0 group-hover:opacity-100 transition-opacity select-none leading-6">
            {new Date(props.message.created_at).toLocaleTimeString([], {
              hour: "2-digit",
              minute: "2-digit",
            })}
          </span>
        </Show>
      </div>

      {/* Message Actions Toolbar (shown on hover) */}
      <MessageActions
        onAddReaction={handleAddReaction}
        onShowContextMenu={handleContextMenu}
        guildId={props.guildId}
        isThreadReply={!!props.message.parent_id || !!props.isInsideThread}
        onReplyInThread={props.isInsideThread ? undefined : () => openThread(props.message)}
      />

      {/* Content column */}
      <div class="flex-1 min-w-0">
        <Show when={!props.compact}>
          <div class="flex items-baseline gap-2 mb-0.5">
            <span
              class="font-semibold text-text-primary hover:underline cursor-pointer transition-colors"
              onContextMenu={(e: MouseEvent) => {
                e.stopPropagation();
                showUserContextMenu(e, { id: author().id, username: author().username, display_name: author().display_name });
              }}
            >
              {author().display_name}
            </span>
            <span class="text-xs text-text-secondary">
              {formatTimestamp(props.message.created_at)}
            </span>
          </div>
        </Show>

        <div ref={contentRef} class="text-text-primary break-words leading-relaxed prose prose-invert max-w-none">
          <For each={contentBlocks()}>
            {(block) => (
              <Show
                when={block.type === 'code'}
                fallback={<div innerHTML={(block as TextBlock).html} />}
              >
                <CodeBlock language={(block as CodeBlockData).language}>
                  {(block as CodeBlockData).code}
                </CodeBlock>
              </Show>
            )}
          </For>
          <Show when={isEdited()}>
            <span class="text-xs text-text-secondary/70 ml-1.5 align-super" title={`Edited ${formatTimestamp(props.message.edited_at!)}`}>
              (edited)
            </span>
          </Show>
        </div>

        {/* Attachments */}
        <Show when={props.message.attachments?.length > 0}>
          <div class="mt-2 flex flex-wrap gap-3">
            {props.message.attachments.map((attachment) => (
              <div class="group/attachment relative">
                <Show
                  when={isImage(attachment.mime_type)}
                  fallback={
                    <a
                      href={getDownloadUrl(attachment)}
                      target="_blank"
                      rel="noopener noreferrer"
                      class="flex items-center gap-3 px-4 py-3 bg-surface-layer2 rounded-xl hover:bg-surface-highlight transition-all duration-200 border border-white/5 max-w-sm"
                    >
                      <div class="p-2 bg-surface-base rounded-lg text-accent-primary">
                        <File class="w-6 h-6" />
                      </div>
                      <div class="flex-1 min-w-0">
                        <div class="text-sm font-medium text-text-primary truncate">
                          {attachment.filename}
                        </div>
                        <div class="text-xs text-text-secondary">
                          {formatFileSize(attachment.size)}
                        </div>
                      </div>
                      <Download class="w-4 h-4 text-text-secondary opacity-0 group-hover/attachment:opacity-100 transition-opacity" />
                    </a>
                  }
                >
                  <div class="relative rounded-xl overflow-hidden border border-white/5 bg-surface-layer2 max-w-md">
                    <img
                      src={getDownloadUrl(attachment)}
                      alt={attachment.filename}
                      class="max-h-80 w-auto object-contain block"
                      loading="lazy"
                    />
                    <a
                      href={getDownloadUrl(attachment)}
                      target="_blank"
                      rel="noopener noreferrer"
                      class="absolute top-2 right-2 p-1.5 bg-black/50 hover:bg-black/70 rounded-lg text-white opacity-0 group-hover/attachment:opacity-100 transition-opacity backdrop-blur-sm"
                      title="Download"
                    >
                      <Download class="w-4 h-4" />
                    </a>
                  </div>
                </Show>
              </div>
            ))}
          </div>
        </Show>

        {/* Reactions */}
        <Show when={hasReactions()}>
          <ReactionBar
            reactions={props.message.reactions!}
            onAddReaction={handleAddReaction}
            onRemoveReaction={handleRemoveReaction}
            guildId={props.guildId}
          />
        </Show>

        {/* Thread indicator (only on top-level messages with replies, not inside ThreadSidebar) */}
        <Show when={!props.isInsideThread && !props.message.parent_id && props.message.thread_reply_count > 0}>
          <ThreadIndicator message={props.message} />
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
