import {
  Component,
  Show,
  createMemo,
  createEffect,
  For,
  onMount,
  onCleanup,
  createSignal,
  createResource,
} from "solid-js";
import { marked } from "marked";
import DOMPurify from "dompurify";
import {
  File,
  Download,
  Copy,
  Link,
  Hash,
  Trash2,
  Flag,
  MessageSquareMore,
  Pencil,
} from "lucide-solid";
import type { Message } from "@/lib/types";
import { formatTimestamp } from "@/lib/utils";
import Avatar from "@/components/ui/Avatar";
import CodeBlock from "@/components/ui/CodeBlock";
import BlurhashPlaceholder from "@/components/ui/BlurhashPlaceholder";
import ReactionBar from "./ReactionBar";
import ThreadIndicator from "./ThreadIndicator";
import MessageActions, { QUICK_EMOJIS } from "./MessageActions";
import {
  getSignedUrl,
  addReaction,
  removeReaction,
  deleteMessage,
  editMessage,
} from "@/lib/tauri";
import {
  showContextMenu,
  type ContextMenuEntry,
} from "@/components/ui/ContextMenu";
import { currentUser } from "@/stores/auth";
import { showUserContextMenu, triggerReport } from "@/lib/contextMenuBuilders";
import { spoilerExtension } from "@/lib/markdown/spoilerExtension";
import { openThread } from "@/stores/threads";
import { removeMessage, updateMessage, editingMessageId, setEditingMessageId } from "@/stores/messages";
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
  ALLOWED_TAGS: [
    "p",
    "br",
    "strong",
    "em",
    "code",
    "pre",
    "a",
    "ul",
    "ol",
    "li",
    "blockquote",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "del",
    "s",
    "table",
    "thead",
    "tbody",
    "tr",
    "th",
    "td",
    "span",
    "mark",
  ],
  ALLOWED_ATTR: ["href", "target", "rel", "class", "data-spoiler"],
  ALLOW_DATA_ATTR: false,
  RETURN_TRUSTED_TYPE: false as const,
};

// Restrict class attribute values to prevent CSS-based UI spoofing
const ALLOWED_CLASSES = new Set([
  "mention-everyone",
  "mention-user",
  "spoiler",
]);
DOMPurify.addHook("uponSanitizeAttribute", (_node, data) => {
  if (data.attrName === "class") {
    const filtered = data.attrValue
      .split(/\s+/)
      .filter((cls) => ALLOWED_CLASSES.has(cls))
      .join(" ");
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
    match.replace(/</g, "&lt;").replace(/>/g, "&gt;"),
  );

  // @everyone and @here -- high-visibility (only after whitespace or start of string)
  processed = processed.replace(
    /(?<=\s|^)@(everyone|here)\b/g,
    '<mark class="mention-everyone">@$1</mark>',
  );

  // @username -- normal mention (2-32 chars, alphanumeric + underscore only)
  processed = processed.replace(
    /(?<=\s|^)@([a-zA-Z0-9_]{2,32})\b/g,
    (match, username) => {
      if (username === "everyone" || username === "here") return match;
      return `<mark class="mention-user">@${username}</mark>`;
    },
  );

  // Restore inline code spans
  processed = processed.replace(
    /\x00CODE(\d+)\x00/g,
    (_, idx) => codeSpans[parseInt(idx)],
  );
  return processed;
}

interface CodeBlockData {
  type: "code";
  language: string;
  code: string;
}

interface TextBlock {
  type: "text";
  html: string;
}

type ContentBlock = CodeBlockData | TextBlock;

// ---- Module-level signed URL cache ----
// Caches presigned S3 URLs to avoid redundant API calls.
// Key: `${attachmentId}:${variant ?? "original"}`
const signedUrlCache = new Map<
  string,
  { url: string; expiresAt: number }
>();

async function fetchSignedUrl(
  attachmentId: string,
  variant?: string,
): Promise<string> {
  const cacheKey = `${attachmentId}:${variant ?? "original"}`;
  const cached = signedUrlCache.get(cacheKey);
  if (cached && cached.expiresAt > Date.now()) {
    return cached.url;
  }

  const result = await getSignedUrl(attachmentId, variant);

  // Evict expired entries when cache grows too large
  if (signedUrlCache.size > 500) {
    const now = Date.now();
    for (const [key, entry] of signedUrlCache) {
      if (entry.expiresAt <= now) signedUrlCache.delete(key);
    }
    // Hard cap: if still over limit, evict entries closest to expiry
    if (signedUrlCache.size > 500) {
      const sorted = [...signedUrlCache.entries()].sort(
        (a, b) => a[1].expiresAt - b[1].expiresAt,
      );
      for (let i = 0; i < sorted.length - 500; i++) {
        signedUrlCache.delete(sorted[i][0]);
      }
    }
  }

  // Cache with safety margin (at most 300s, at most half the TTL) to avoid negative expiry
  const safetyMargin = Math.min(300, Math.floor(result.expires_in / 2));
  signedUrlCache.set(cacheKey, {
    url: result.url,
    expiresAt: Date.now() + (result.expires_in - safetyMargin) * 1000,
  });
  return result.url;
}

// ---- Module-level spoiler reveal state ----
// Persists revealed spoilers across component remounts (e.g. virtual scroll).
// Key format: `${messageId}:${spoilerIndex}` (0-based index within a message).
const [revealedSpoilers, setRevealedSpoilers] = createSignal<Set<string>>(
  new Set(),
);

/**
 * Mark a spoiler as revealed and persist it in the session-scoped set.
 */
function revealSpoiler(messageId: string, spoilerIndex: number): void {
  const key = `${messageId}:${spoilerIndex}`;
  setRevealedSpoilers((prev) => {
    const next = new Set(prev);
    next.add(key);
    return next;
  });
}

/**
 * Check whether a spoiler has been revealed in this session.
 */
function isSpoilerRevealed(messageId: string, spoilerIndex: number): boolean {
  return revealedSpoilers().has(`${messageId}:${spoilerIndex}`);
}

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
    if (editingMessageId()) return;

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
  const isOwn = () => currentUser()?.id === props.message.author.id;
  const isBeingEdited = () => editingMessageId() === props.message.id;
  const [editContent, setEditContent] = createSignal("");
  const [isSavingEdit, setIsSavingEdit] = createSignal(false);
  let editTextareaRef: HTMLTextAreaElement | undefined;

  const startEdit = () => {
    if (props.message.encrypted) return;
    setEditContent(props.message.content);
    setEditingMessageId(props.message.id);
  };

  const cancelEdit = () => {
    if (editingMessageId() === props.message.id) {
      setEditingMessageId(null);
    }
  };

  const saveEdit = async () => {
    const newContent = editContent().trim();
    if (!newContent || newContent === props.message.content) {
      cancelEdit();
      return;
    }

    setIsSavingEdit(true);
    try {
      const updated = await editMessage(props.message.id, newContent);
      updateMessage(updated);
      setEditingMessageId(null);
    } catch (err) {
      console.error("Failed to edit message:", err);
      showToast({
        type: "error",
        title: "Edit Failed",
        message: err instanceof Error ? err.message : "Could not edit message.",
        duration: 5000,
      });
    } finally {
      setIsSavingEdit(false);
    }
  };

  const handleEditKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      cancelEdit();
    } else if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void saveEdit();
    }
  };

  const resizeEditTextarea = () => {
    if (!editTextareaRef) return;
    editTextareaRef.style.height = "auto";
    const newHeight = Math.min(Math.max(editTextareaRef.scrollHeight, 24), 192);
    editTextareaRef.style.height = `${newHeight}px`;
  };

  // Reset editing state if this component unmounts mid-edit (e.g., channel navigation)
  onCleanup(() => {
    if (editingMessageId() === props.message.id) {
      setEditingMessageId(null);
    }
  });

  const hasReactions = () =>
    props.message.reactions && props.message.reactions.length > 0;

  // Register the singleton keydown listener once
  onMount(() => ensureReactionShortcutListener());

  // Setup spoiler click-to-reveal functionality.
  // Uses createEffect so listeners are re-attached when the content div
  // remounts after exiting edit mode (Show unmounts/remounts children).
  createEffect(() => {
    // Only attach when NOT editing (content div is mounted)
    if (isBeingEdited()) return;

    // Wait for next frame so contentRef is populated after Show remount
    requestAnimationFrame(() => {
      if (!contentRef) return;

      const messageId = props.message.id;
      const spoilerEls = contentRef.querySelectorAll(
        '.spoiler[data-spoiler="true"]',
      );

      spoilerEls.forEach((spoiler, index) => {
        // Restore previously revealed state
        if (isSpoilerRevealed(messageId, index)) {
          (spoiler as HTMLElement).classList.add("revealed");
        }

        const handler: EventListener = function (this: HTMLElement) {
          this.classList.add("revealed");
          revealSpoiler(messageId, index);
        };
        spoiler.addEventListener("click", handler);
      });
    });
  });

  const handleAddReaction = async (emoji: string) => {
    try {
      await addReaction(props.message.channel_id, props.message.id, emoji);
    } catch (err) {
      console.error("Failed to add reaction:", err);
      showToast({
        type: "error",
        title: "Reaction Failed",
        message: "Could not add reaction.",
        duration: 8000,
      });
    }
  };

  const handleRemoveReaction = async (emoji: string) => {
    try {
      await removeReaction(props.message.channel_id, props.message.id, emoji);
    } catch (err) {
      console.error("Failed to remove reaction:", err);
      showToast({
        type: "error",
        title: "Reaction Failed",
        message: "Could not remove reaction.",
        duration: 8000,
      });
    }
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
            const html = sanitizeHtml(
              marked.parse(highlightMentions(text), { async: false }) as string,
            );
            blocks.push({ type: "text", html });
          }
        }

        blocks.push({
          type: "code",
          language: match[1] || "plaintext",
          code: match[2].trim(),
        });

        lastIndex = match.index + match[0].length;
      }

      if (lastIndex < content.length) {
        const text = content.substring(lastIndex);
        if (text.trim()) {
          const html = sanitizeHtml(
            marked.parse(highlightMentions(text), { async: false }) as string,
          );
          blocks.push({ type: "text", html });
        }
      }

      if (blocks.length === 0) {
        const html = sanitizeHtml(
          marked.parse(highlightMentions(content), { async: false }) as string,
        );
        blocks.push({ type: "text", html });
      }

      return blocks;
    } catch (err) {
      console.error("Failed to parse message content:", err);
      // Fallback: render plain text safely
      const safeText = DOMPurify.sanitize(content, {
        ALLOWED_TAGS: [],
        ALLOWED_ATTR: [],
      }) as string;
      return [{ type: "text", html: safeText }];
    }
  });

  const handleContextMenu = (e: MouseEvent) => {
    const msg = props.message;

    const items: ContextMenuEntry[] = [
      {
        label: "Copy Text",
        icon: Copy,
        action: () => navigator.clipboard.writeText(msg.content),
      },
      {
        label: "Copy Message Link",
        icon: Link,
        action: () =>
          navigator.clipboard.writeText(
            `${window.location.origin}/channels/${msg.channel_id}/${msg.id}`,
          ),
      },
      {
        label: "Copy ID",
        icon: Hash,
        action: () => navigator.clipboard.writeText(msg.id),
      },
    ];

    // Only show "Reply in Thread" for top-level messages, not inside ThreadSidebar, and only when threads are enabled
    if (
      !msg.parent_id &&
      !props.isInsideThread &&
      props.threadsEnabled !== false
    ) {
      items.push(
        { separator: true },
        {
          label: "Reply in Thread",
          icon: MessageSquareMore,
          action: () => openThread(msg),
        },
      );
    }

    if (!isOwn()) {
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

    if (isOwn()) {
      if (!msg.encrypted) {
        items.push(
          { separator: true },
          {
            label: "Edit Message",
            icon: Pencil,
            action: () => startEdit(),
          },
        );
      }
      items.push(
        ...(!msg.encrypted ? [] : [{ separator: true } as ContextMenuEntry]),
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
      data-testid="message-item"
      onContextMenu={handleContextMenu}
      onMouseEnter={() => {
        reactionShortcutHandler = handleAddReaction;
      }}
      onMouseLeave={() => {
        reactionShortcutHandler = null;
      }}
      class={`group relative flex gap-4 px-4 py-0.5 hover:bg-white/3 transition-colors ${
        props.compact ? "mt-0" : "mt-4"
      } ${isBeingEdited() ? "bg-accent-primary/5 ring-1 ring-accent-primary/20 rounded-lg" : ""}`}
    >
      {/* Avatar column */}
      <div class="w-10 flex-shrink-0">
        <Show when={!props.compact}>
          <div
            onContextMenu={(e: MouseEvent) => {
              e.stopPropagation();
              showUserContextMenu(e, {
                id: author().id,
                username: author().username,
                display_name: author().display_name,
              });
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
        onReplyInThread={
          props.isInsideThread ? undefined : () => openThread(props.message)
        }
        threadsEnabled={props.threadsEnabled}
        isOwn={isOwn()}
        onEdit={props.message.encrypted ? undefined : startEdit}
      />

      {/* Content column */}
      <div class="flex-1 min-w-0">
        <Show when={!props.compact}>
          <div class="flex items-baseline gap-2 mb-0.5">
            <span
              class="font-semibold text-text-primary hover:underline cursor-pointer transition-colors"
              onContextMenu={(e: MouseEvent) => {
                e.stopPropagation();
                showUserContextMenu(e, {
                  id: author().id,
                  username: author().username,
                  display_name: author().display_name,
                });
              }}
            >
              {author().display_name}
            </span>
            <span class="text-xs text-text-secondary">
              {formatTimestamp(props.message.created_at)}
            </span>
          </div>
        </Show>

        <Show
          when={!isBeingEdited()}
          fallback={
            <div class="mt-1">
              <textarea
                ref={(el) => {
                  editTextareaRef = el;
                  requestAnimationFrame(() => {
                    el.focus();
                    el.selectionStart = el.value.length;
                    resizeEditTextarea();
                  });
                }}
                class="w-full bg-surface-base border border-accent-primary/50 rounded-lg px-3 py-2 text-text-primary text-sm resize-none focus:outline-none focus:border-accent-primary transition-colors"
                value={editContent()}
                onInput={(e) => {
                  setEditContent(e.currentTarget.value);
                  resizeEditTextarea();
                }}
                onKeyDown={handleEditKeyDown}
                disabled={isSavingEdit()}
                rows={1}
              />
              <div class="text-xs text-text-secondary mt-1">
                escape to cancel · enter to save
              </div>
            </div>
          }
        >
          <div
            ref={contentRef}
            class="text-text-primary break-words leading-relaxed prose prose-invert max-w-none"
          >
            <For each={contentBlocks()}>
              {(block) => (
                <Show
                  when={block.type === "code"}
                  fallback={<div innerHTML={(block as TextBlock).html} />}
                >
                  <CodeBlock language={(block as CodeBlockData).language}>
                    {(block as CodeBlockData).code}
                  </CodeBlock>
                </Show>
              )}
            </For>
            <Show when={isEdited()}>
              <span
                class="text-xs text-text-secondary/70 ml-1.5 align-super"
                title={`Edited ${formatTimestamp(props.message.edited_at!)}`}
              >
                (edited)
              </span>
            </Show>
          </div>
        </Show>

        {/* Attachments */}
        <Show when={props.message.attachments?.length > 0}>
          <div class="mt-2 flex flex-wrap gap-3">
            {props.message.attachments.map((attachment) => (
              <div class="group/attachment relative">
                <Show
                  when={isImage(attachment.mime_type)}
                  fallback={
                    <button
                      type="button"
                      class="flex items-center gap-3 px-4 py-3 bg-surface-layer2 rounded-xl hover:bg-surface-highlight transition-all duration-200 border border-white/5 max-w-sm cursor-pointer text-left"
                      onClick={async () => {
                        try {
                          const url = await fetchSignedUrl(attachment.id);
                          window.open(url, "_blank");
                        } catch (err) {
                          console.error("Failed to get signed URL:", err);
                          showToast({
                            type: "error",
                            title: "Download Failed",
                            message: "Could not get download link.",
                            duration: 8000,
                          });
                        }
                      }}
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
                    </button>
                  }
                >
                  {(() => {
                    const variant = attachment.thumbnail_url
                      ? "thumbnail"
                      : undefined;
                    const [imgSrc] = createResource(
                      () => attachment.id,
                      async (id) => {
                        try {
                          return await fetchSignedUrl(id, variant);
                        } catch (err) {
                          console.error("Failed to load image attachment:", id, err);
                          throw err;
                        }
                      },
                    );

                    return (
                      <div
                        class="relative rounded-xl overflow-hidden border border-white/5 bg-surface-layer2 max-w-md"
                        style={
                          attachment.width && attachment.height
                            ? {
                                "aspect-ratio": `${attachment.width} / ${attachment.height}`,
                                "max-height": "320px",
                              }
                            : { "max-height": "320px" }
                        }
                      >
                        {/* Blurhash placeholder (visible while image loads) */}
                        <Show when={attachment.blurhash}>
                          <BlurhashPlaceholder
                            hash={attachment.blurhash!}
                            width={attachment.width ?? 32}
                            height={attachment.height ?? 32}
                            class="absolute inset-0 w-full h-full"
                          />
                        </Show>

                        {/* Error fallback when signed URL fetch fails */}
                        <Show when={imgSrc.error}>
                          <div class="absolute inset-0 flex items-center justify-center text-text-secondary text-sm">
                            Failed to load image
                          </div>
                        </Show>

                        {/* Actual image — loads signed URL, fades in over placeholder */}
                        <Show when={imgSrc()}>
                          <img
                            src={imgSrc()}
                            alt={attachment.filename}
                            class="relative w-full h-full object-contain block opacity-0 transition-opacity duration-300"
                            loading="lazy"
                            onLoad={(e) => {
                              (
                                e.target as HTMLImageElement
                              ).classList.remove("opacity-0");
                              (e.target as HTMLImageElement).classList.add(
                                "opacity-100",
                              );
                            }}
                            onClick={async () => {
                              try {
                                const url = await fetchSignedUrl(
                                  attachment.id,
                                );
                                window.open(url, "_blank");
                              } catch (err) {
                                console.error("Failed to get signed URL:", err);
                                showToast({
                                  type: "error",
                                  title: "Download Failed",
                                  message: "Could not get download link.",
                                  duration: 8000,
                                });
                              }
                            }}
                            style={{ cursor: "pointer" }}
                          />
                        </Show>
                        <button
                          type="button"
                          class="absolute top-2 right-2 p-1.5 bg-black/50 hover:bg-black/70 rounded-lg text-white opacity-0 group-hover/attachment:opacity-100 transition-opacity backdrop-blur-sm cursor-pointer"
                          title="Download original"
                          onClick={async () => {
                            try {
                              const url = await fetchSignedUrl(attachment.id);
                              window.open(url, "_blank");
                            } catch (err) {
                              console.error("Failed to get signed URL:", err);
                              showToast({
                                type: "error",
                                title: "Download Failed",
                                message: "Could not get download link.",
                                duration: 8000,
                              });
                            }
                          }}
                        >
                          <Download class="w-4 h-4" />
                        </button>
                      </div>
                    );
                  })()}
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
        <Show
          when={
            !props.isInsideThread &&
            !props.message.parent_id &&
            props.message.thread_reply_count > 0
          }
        >
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
  if (bytes < 1024 * 1024 * 1024)
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

export default MessageItem;
