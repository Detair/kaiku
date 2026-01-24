import { Component, Show, createMemo, For, createSignal } from "solid-js";
import { marked } from "marked";
import { File, Download, SmilePlus } from "lucide-solid";
import type { Message, Attachment } from "@/lib/types";
import { formatTimestamp } from "@/lib/utils";
import Avatar from "@/components/ui/Avatar";
import CodeBlock from "@/components/ui/CodeBlock";
import ReactionBar from "./ReactionBar";
import EmojiPicker from "@/components/emoji/EmojiPicker";
import { getServerUrl, getAccessToken, addReaction, removeReaction } from "@/lib/tauri";

interface MessageItemProps {
  message: Message;
  /** If true, shows compact version without avatar/name (for grouped messages) */
  compact?: boolean;
  /** Guild ID for custom emoji support in reactions */
  guildId?: string;
}

// Configure marked for GitHub Flavored Markdown
marked.setOptions({
  breaks: true,
  gfm: true,
});

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
  const [showReactionPicker, setShowReactionPicker] = createSignal(false);

  const author = () => props.message.author;
  const isEdited = () => !!props.message.edited_at;
  const hasReactions = () => props.message.reactions && props.message.reactions.length > 0;

  const handleAddReaction = async (emoji: string) => {
    try {
      await addReaction(props.message.channel_id, props.message.id, emoji);
    } catch (err) {
      console.error("Failed to add reaction:", err);
    }
    setShowReactionPicker(false);
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
          const html = marked.parse(text, { async: false }) as string;
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
        const html = marked.parse(text, { async: false }) as string;
        blocks.push({ type: 'text', html });
      }
    }

    // If no code blocks found, just parse the whole content
    if (blocks.length === 0) {
      const html = marked.parse(content, { async: false }) as string;
      blocks.push({ type: 'text', html });
    }

    return blocks;
  });

  return (
    <div
      class={`group flex gap-4 px-4 py-0.5 hover:bg-white/3 transition-colors ${
        props.compact ? "mt-0" : "mt-4"
      }`}
    >
      {/* Avatar column */}
      <div class="w-10 flex-shrink-0">
        <Show when={!props.compact}>
          <Avatar
            src={author().avatar_url}
            alt={author().display_name}
            status={author().status}
            size="md"
          />
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

      {/* Content column */}
      <div class="flex-1 min-w-0">
        <Show when={!props.compact}>
          <div class="flex items-baseline gap-2 mb-0.5">
            <span class="font-semibold text-text-primary hover:underline cursor-pointer transition-colors">
              {author().display_name}
            </span>
            <span class="text-xs text-text-secondary">
              {formatTimestamp(props.message.created_at)}
            </span>
          </div>
        </Show>

        <div class="text-text-primary break-words leading-relaxed prose prose-invert max-w-none">
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

        {/* Add reaction button (shown on hover when no reactions exist) */}
        <Show when={!hasReactions()}>
          <div class="relative mt-1 opacity-0 group-hover:opacity-100 transition-opacity">
            <button
              class="w-6 h-6 flex items-center justify-center rounded hover:bg-white/10 text-text-secondary hover:text-text-primary transition-colors"
              onClick={() => setShowReactionPicker(!showReactionPicker())}
              title="Add reaction"
            >
              <SmilePlus class="w-4 h-4" />
            </button>

            <Show when={showReactionPicker()}>
              <div class="absolute bottom-full left-0 mb-2 z-50">
                <EmojiPicker
                  onSelect={handleAddReaction}
                  onClose={() => setShowReactionPicker(false)}
                  guildId={props.guildId}
                />
              </div>
            </Show>
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
