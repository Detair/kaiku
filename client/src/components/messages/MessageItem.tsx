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
import MessageActions, { QUICK_EMOJIS } from "./MessageActions";
import { getServerUrl, getAccessToken, addReaction, removeReaction, deleteMessage } from "@/lib/tauri";
import { showContextMenu, type ContextMenuEntry } from "@/components/ui/ContextMenu";
import { currentUser } from "@/stores/auth";
import { showUserContextMenu, triggerReport } from "@/lib/contextMenuBuilders";
import { spoilerExtension } from "@/lib/markdown/spoilerExtension";
import { openThread } from "@/stores/threads";
import { removeMessage } from "@/stores/messages";
import { showToast } from "@/components/ui/Toast";

interface MessageItemProps {
  message: Message;
  /** If true, shows compact version without avatar/name (for grouped messages) */
  compact?: boolean;
  /** Guild ID for custom emoji support in reactions */
  guildId?: string;
  /** If true, suppresses thread indicator and "Reply in Thread" actions (when rendered inside ThreadSidebar) */
  isInsideThread?: boolean;
  /** Whether threads are enabled for this guild (default true) */
  threadsEnabled?: boolean;
}

// Configure marked for GitHub Flavored Markdown
marked.setOptions({
  breaks: true,
  gfm: true,
});

marked.use({ extensions: [spoilerExtension] });

// Configure DOMPurify for safe HTML rendering (XSS prevention)
const PURIFY_CONFIG = {
  ALLOWED_TAGS: ['p', 'br', 'strong', 'em', 'code', 'pre', 'a', 'ul', 'ol', 'li', 'blockquote', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'hr', 'del', 's', 'table', 'thead', 'tbody', 'tr', 'th', 'td', 'span', 'mark'],
  ALLOWED_ATTR: ['href', 'target', 'rel', 'class', 'data-spoiler'],
  ALLOW_DATA_ATTR: false,
  RETURN_TRUSTED_TYPE: false as const,
};

// Restrict class attribute values to prevent CSS-based UI spoofing
const ALLOWED_CLASSES = new Set(['mention-everyone', 'mention-user', 'spoiler']);
DOMPurify.addHook('uponSanitizeAttribute', (_node, data) => {
  if (data.attrName === 'class') {
    const filtered = data.attrValue.split(/\s+/).filter(cls => ALLOWED_CLASSES.has(cls)).join(' ');
    data.attrValue = filtered;
    if (!filtered) data.keepAttr = false;
  }
});

const sanitizeHtml = (html: string): string => {
  return DOMPurify.sanitize(html, PURIFY_CONFIG) as string;
};

/**
 * Highlight @mentions in text before markdown parsing.
 * Protects inline code spans from modification, then wraps
 * @everyone, @here, and @username in styled <mark> tags.
 */
function highlightMentions(text: string): string {
  // Protect inline code spans from mention processing
  const codeSpans: string[] = [];
  let processed = text.replace(/`[^`]+`/g, (match) => {
    codeSpans.push(match);
    return `\x00CODE${codeSpans.length - 1}\x00`;
  });

  // Escape any existing mark/span tags to prevent injection via user HTML
  processed = processed.replace(/<\/?(?:mark|span)\b[^>]*>/gi, (match) =>
    match.replace(/</g, '&lt;').replace(/>/g, '&gt;')
  );

  // @everyone and @here -- high-visibility (only after whitespace or start of string)
  processed = processed.replace(
    /(?<=\s|^)@(everyone|here)\b/g,
    '<mark class="mention-everyone">@$1</mark>'
  );

  // @username -- normal mention (2-32 chars, alphanumeric + underscore only)
  processed = processed.replace(
    /(?<=\s|^)@([a-zA-Z0-9_]{2,32})\b/g,
    (match, username) => {
      if (username === "everyone" || username === "here") return match;
      return `<mark class="mention-user">@${username}</mark>`;
    }
  );

  // Restore inline code spans
  processed = processed.replace(/\x00CODE(\d+)\x00/g, (_, idx) => codeSpans[parseInt(idx)]);
  return processed;
}

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

// ---- Module-level singleton for Alt+1..4 reaction shortcuts ----
// One global listener instead of one per rendered MessageItem.
let reactionShortcutHandler: ((emoji: string) => void) | null = null;
let reactionListenerRegistered = false;

function ensureReactionShortcutListener() {
  if (reactionListenerRegistered) return;
  reactionListenerRegistered = true;
  document.addEventListener("keydown", (e: KeyboardEvent) => {
    if (!e.altKey || !reactionShortcutHandler) return;
    const tag = document.activeElement?.tagName;
    if (tag === "TEXTAREA" || tag === "INPUT") return;

    const index = parseInt(e.key, 10) - 1;
    if (index >= 0 && index < QUICK_EMOJIS.length) {
      e.preventDefault();
      reactionShortcutHandler(QUICK_EMOJIS[index]);
    }
  });
}

const MessageItem: Component<MessageItemProps> = (props) => {
  let contentRef: HTMLDivElement | undefined;

  const author = () => props.message.author;
  const isEdited = () => !!props.message.edited_at;
  const hasReactions = () => props.message.reactions && props.message.reactions.length > 0;

  // Register the singleton keydown listener once
  onMount(() => ensureReactionShortcutListener());

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
      showToast({ type: "error", title: "Reaction Failed", message: "Could not add reaction." });
    }
  };

  const handleRemoveReaction = async (emoji: string) => {
    try {
      await removeReaction(props.message.channel_id, props.message.id, emoji);
    } catch (err) {
      console.error("Failed to remove reaction:", err);
      showToast({ type: "error", title: "Reaction Failed", message: "Could not remove reaction." });
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
  const contentBlocks = createMemo((): ContentBlock[] => {
    const content = props.message.content;

    try {
      const blocks: ContentBlock[] = [];

      // Split content by fenced code blocks
      const codeBlockRegex = /```(\w+)?\n([\s\S]*?)```/g;
      let lastIndex = 0;
      let match;

      while ((match = codeBlockRegex.exec(content)) !== null) {
        if (match.index > lastIndex) {
          const text = content.substring(lastIndex, match.index);
          if (text.trim()) {
            const html = sanitizeHtml(marked.parse(highlightMentions(text), { async: false }) as string);
            blocks.push({ type: 'text', html });
          }
        }

        blocks.push({
          type: 'code',
          language: match[1] || 'plaintext',
          code: match[2].trim(),
        });

        lastIndex = match.index + match[0].length;
      }

      if (lastIndex < content.length) {
        const text = content.substring(lastIndex);
        if (text.trim()) {
          const html = sanitizeHtml(marked.parse(highlightMentions(text), { async: false }) as string);
          blocks.push({ type: 'text', html });
        }
      }

      if (blocks.length === 0) {
        const html = sanitizeHtml(marked.parse(highlightMentions(content), { async: false }) as string);
        blocks.push({ type: 'text', html });
      }

      return blocks;
    } catch (err) {
      console.error("Failed to parse message content:", err);
      // Fallback: render plain text safely
      const safeText = DOMPurify.sanitize(content, { ALLOWED_TAGS: [], ALLOWED_ATTR: [] }) as string;
      return [{ type: 'text', html: safeText }];
    }
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

    // Only show "Reply in Thread" for top-level messages, not inside ThreadSidebar, and only when threads are enabled
    if (!msg.parent_id && !props.isInsideThread && props.threadsEnabled !== false) {
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
          action: async () => {
            if (confirm("Delete this message? This cannot be undone.")) {
              try {
                await deleteMessage(msg.id);
                removeMessage(msg.channel_id, msg.id);
              } catch (e) {
                console.error("Failed to delete message:", e);
              }
            }
          },
        },
      );
    }

    showContextMenu(e, items);
  };

  return (
    <div
      onContextMenu={handleContextMenu}
      onMouseEnter={() => { reactionShortcutHandler = handleAddReaction; }}
      onMouseLeave={() => { reactionShortcutHandler = null; }}
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
        threadsEnabled={props.threadsEnabled}
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
