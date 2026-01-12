import { Component, Show } from "solid-js";
import { SolidMarkdown } from "solid-markdown";
import { File, Download } from "lucide-solid";
import type { Message, Attachment } from "@/lib/types";
import { formatTimestamp } from "@/lib/utils";
import Avatar from "@/components/ui/Avatar";
import CodeBlock from "@/components/ui/CodeBlock";
import { getServerUrl } from "@/lib/tauri";

interface MessageItemProps {
  message: Message;
  /** If true, shows compact version without avatar/name (for grouped messages) */
  compact?: boolean;
}

const MessageItem: Component<MessageItemProps> = (props) => {
  const author = () => props.message.author;
  const isEdited = () => !!props.message.edited_at;

  const getDownloadUrl = (attachment: Attachment) => {
    // Construct absolute URL for the attachment
    const baseUrl = getServerUrl().replace(/\/+$/, "");
    return `${baseUrl}/api/messages/attachments/${attachment.id}/download`;
  };

  const isImage = (mimeType: string) => mimeType.startsWith("image/");

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
          <SolidMarkdown
            children={props.message.content}
            components={{
              code: (codeProps: any) => {
                // Inline code (no language class)
                if (!codeProps.class) {
                  return (
                    <code class="px-1.5 py-0.5 bg-surface-layer2 rounded text-sm">
                      {codeProps.children}
                    </code>
                  );
                }
                // Block code with language
                const language = codeProps.class?.replace("language-", "");
                return (
                  <CodeBlock language={language}>{codeProps.children}</CodeBlock>
                );
              },
            }}
          />
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