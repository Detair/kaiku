import { Component, createSignal, Show, For, onCleanup, createEffect, createMemo } from "solid-js";
import { PlusCircle, Send, UploadCloud, X, File as FileIcon } from "lucide-solid";
import { sendMessage, messagesState, addMessage } from "@/stores/messages";
import { stopTyping, sendTyping } from "@/stores/websocket";
import { uploadMessageWithFile, validateFileSize, getUploadLimitText } from "@/lib/tauri";
import { showToast } from "@/components/ui/Toast";
import { getDraft, saveDraft, clearDraft } from "@/stores/drafts";
import AutocompletePopup from "./AutocompletePopup";
import { guildsState } from "@/stores/guilds";
import { channelsState } from "@/stores/channels";
import { listGuildCommands, type GuildCommand } from "@/lib/api/bots";

interface MessageInputProps {
  channelId: string;
  channelName: string;
  /** Guild ID - used to determine if channel is E2EE (undefined = DM) */
  guildId?: string;
  /** Is E2EE enabled for this channel (DMs only) */
  isE2EE?: boolean;
  /** DM participants (for @user autocomplete in DMs) */
  dmParticipants?: Array<{ user_id: string; username: string; display_name: string; avatar_url: string | null }>;
}

interface PendingFile {
  file: File;
  previewUrl: string | null;
}

const MessageInput: Component<MessageInputProps> = (props) => {
  const [content, setContent] = createSignal("");
  const [isSending, setIsSending] = createSignal(false);
  const [isDragging, setIsDragging] = createSignal(false);
  const [uploadError, setUploadError] = createSignal<string | null>(null);
  const [pendingFiles, setPendingFiles] = createSignal<PendingFile[]>([]);
  const [isComposing, setIsComposing] = createSignal(false);
  // Autocomplete state
  const [autocompleteType, setAutocompleteType] = createSignal<"user" | "emoji" | "channel" | "command" | null>(null);
  const [autocompleteQuery, setAutocompleteQuery] = createSignal("");
  const [autocompleteIndex, setAutocompleteIndex] = createSignal(0);
  const [autocompleteStart, setAutocompleteStart] = createSignal(0);
  const [guildCommands, setGuildCommands] = createSignal<GuildCommand[]>([]);
  const [commandsFetched, setCommandsFetched] = createSignal(false);
  let typingTimeout: NodeJS.Timeout | undefined;
  let textareaRef: HTMLTextAreaElement | undefined;
  let resizeFrame: number | undefined;

  // Load draft when channel changes (handles both initial mount and channel switches)
  createEffect(() => {
    const channelId = props.channelId;
    const draft = getDraft(channelId);
    setContent(draft);
    // Resize after setting content
    setTimeout(() => resizeTextarea(), 0);
  });

  // Auto-resize textarea with RAF batching
  const resizeTextarea = () => {
    if (!textareaRef) return;

    if (resizeFrame) cancelAnimationFrame(resizeFrame);

    resizeFrame = requestAnimationFrame(() => {
      if (!textareaRef) return;

      // Reset height to auto to get correct scrollHeight
      textareaRef.style.height = 'auto';

      // Calculate new height (min 24px = 1 line, max 192px = 8 lines)
      const newHeight = Math.min(Math.max(textareaRef.scrollHeight, 24), 192);
      textareaRef.style.height = `${newHeight}px`;
    });
  };

  // Calculate character length properties separating code blocks and regular text
  const lengthStats = createMemo(() => {
    const text = content();
    // Use spreader to count unicode code points instead of UTF-16 code units (matching backend)
    const totalLength = [...text].length;

    // Remove code blocks to get regular text length
    // Using a regex similar to backend: matches ``` followed by anything until next ```
    const textWithoutBlocks = text.replace(/```[\s\S]*?```/g, '');
    const regularLength = [...textWithoutBlocks].length;

    return {
      totalLength,
      regularLength,
      totalLimit: 10000,
      regularLimit: 4000
    };
  });

  const isNearLimit = () => {
    const stats = lengthStats();
    return stats.regularLength > stats.regularLimit * 0.8 || stats.totalLength > stats.totalLimit * 0.8;
  };

  const isOverLimit = () => {
    const stats = lengthStats();
    return stats.regularLength > stats.regularLimit || stats.totalLength > stats.totalLimit;
  };

  // Cleanup on unmount
  onCleanup(() => {
    if (typingTimeout) {
      clearTimeout(typingTimeout);
      stopTyping(props.channelId);
    }
    if (resizeFrame) {
      cancelAnimationFrame(resizeFrame);
    }
    // Revoke object URLs to prevent memory leaks
    pendingFiles().forEach((pf) => {
      if (pf.previewUrl) URL.revokeObjectURL(pf.previewUrl);
    });
  });

  // Add file to pending list with preview
  const addPendingFile = (file: File) => {
    // Frontend validation before adding to pending list
    const validationError = validateFileSize(file, 'attachment');
    if (validationError) {
      setUploadError(validationError);
      setTimeout(() => setUploadError(null), 5000); // Clear after 5 seconds
      return;
    }

    setUploadError(null);
    const isImage = file.type.startsWith("image/");
    const previewUrl = isImage ? URL.createObjectURL(file) : null;
    setPendingFiles((prev) => [...prev, { file, previewUrl }]);
  };

  // Remove file from pending list
  const removePendingFile = (index: number) => {
    setPendingFiles((prev) => {
      const removed = prev[index];
      if (removed?.previewUrl) URL.revokeObjectURL(removed.previewUrl);
      return prev.filter((_, i) => i !== index);
    });
  };

  // Clear all pending files
  const clearPendingFiles = () => {
    pendingFiles().forEach((pf) => {
      if (pf.previewUrl) URL.revokeObjectURL(pf.previewUrl);
    });
    setPendingFiles([]);
  };

  const handleInput = (value: string) => {
    setContent(value);
    resizeTextarea();

    // Detect autocomplete triggers
    detectAutocomplete(value);

    // Save draft (debounced, skips E2EE channels)
    const isE2EE = props.isE2EE ?? false;
    saveDraft(props.channelId, value, isE2EE);

    // Send typing indicator
    if (value.trim()) {
      sendTyping(props.channelId);

      // Clear existing timeout
      if (typingTimeout) {
        clearTimeout(typingTimeout);
      }

      // Stop typing after 3 seconds of inactivity
      typingTimeout = setTimeout(() => {
        stopTyping(props.channelId);
      }, 3000);
    }
  };

  // Reset command cache when guild changes (must run before fetch effect)
  createEffect(() => {
    void props.guildId;
    setCommandsFetched(false);
    setGuildCommands([]);
  });

  // Lazy-fetch guild commands when command autocomplete is activated
  createEffect(() => {
    if (autocompleteType() === "command" && !commandsFetched() && props.guildId) {
      setCommandsFetched(true);
      listGuildCommands(props.guildId!).then(setGuildCommands).catch(() => {
        setGuildCommands([]);
        setCommandsFetched(false); // Allow retry on next / keystroke
      });
    }
  });

  // Detect @user, :emoji:, #channel, or /command triggers
  const detectAutocomplete = (value: string) => {
    if (!textareaRef) return;

    const cursorPos = textareaRef.selectionStart;
    const textBeforeCursor = value.substring(0, cursorPos);

    // Check for /command (only at start of message, only in guilds)
    if (props.guildId) {
      const commandMatch = textBeforeCursor.match(/^\/([a-z0-9_-]*)$/);
      if (commandMatch) {
        setAutocompleteType("command");
        setAutocompleteQuery(commandMatch[1]);
        setAutocompleteStart(0);
        setAutocompleteIndex(0);
        return;
      }
    }

    // Check for @user mentions
    const userMatch = textBeforeCursor.match(/@(\w*)$/);
    if (userMatch) {
      // Prevent false positives like email addresses or URLs
      const beforeAt = textBeforeCursor.substring(0, textBeforeCursor.length - userMatch[0].length);
      if (beforeAt.match(/[a-zA-Z0-9]$/)) {
        setAutocompleteType(null);
        return;
      }
      setAutocompleteType("user");
      setAutocompleteQuery(userMatch[1]);
      setAutocompleteStart(cursorPos - userMatch[0].length);
      setAutocompleteIndex(0);
      return;
    }

    // Check for #channel mentions (only in guilds)
    if (props.guildId) {
      const channelMatch = textBeforeCursor.match(/#(\w*)$/);
      if (channelMatch) {
        // Prevent false positives like URL fragments or hex colors
        const beforeHash = textBeforeCursor.substring(0, textBeforeCursor.length - channelMatch[0].length);
        if (beforeHash.match(/[a-zA-Z0-9]$/)) {
          setAutocompleteType(null);
          return;
        }
        setAutocompleteType("channel");
        setAutocompleteQuery(channelMatch[1]);
        setAutocompleteStart(cursorPos - channelMatch[0].length);
        setAutocompleteIndex(0);
        return;
      }
    }

    // Check for :emoji: triggers (min 2 chars to avoid false positives)
    const emojiMatch = textBeforeCursor.match(/:(\w{2,})$/);
    if (emojiMatch) {
      // Prevent false positives like times (12:30) or URLs
      const beforeColon = textBeforeCursor.substring(0, textBeforeCursor.length - emojiMatch[0].length);
      if (beforeColon.match(/[0-9]$/)) {
        setAutocompleteType(null);
        return;
      }
      setAutocompleteType("emoji");
      setAutocompleteQuery(emojiMatch[1]);
      setAutocompleteStart(cursorPos - emojiMatch[0].length);
      setAutocompleteIndex(0);
      return;
    }

    // No match, close autocomplete
    setAutocompleteType(null);
  };

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    const text = content().trim();
    const files = pendingFiles();

    // Need either text or files to send, and text must be within the limit
    if ((!text && files.length === 0) || isSending() || isOverLimit()) return;

    // Stop typing indicator
    if (typingTimeout) {
      clearTimeout(typingTimeout);
    }
    stopTyping(props.channelId);

    setIsSending(true);
    try {
      if (files.length > 0) {
        // Upload files one at a time (first file gets the text, rest are separate)
        for (let i = 0; i < files.length; i++) {
          const messageText = i === 0 ? text || undefined : undefined;
          const message = await uploadMessageWithFile(props.channelId, files[i].file, messageText);
          await addMessage(message);
        }
        clearPendingFiles();
      } else {
        // Text-only message
        await sendMessage(props.channelId, text);
      }
      setContent("");
      // Clear draft after successful send
      clearDraft(props.channelId);
      // Reset textarea height after successful send
      if (textareaRef) {
        textareaRef.style.height = 'auto';
      }
    } catch (err) {
      console.error("Send failed:", err);
      showToast({
        type: "error",
        title: "Failed to Send Message",
        message: "Could not send your message. Please try again.",
        duration: 8000,
      });
    } finally {
      setIsSending(false);
      textareaRef?.focus();
    }
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    // Don't send during IME composition (CJK input)
    if (isComposing()) return;

    // Handle autocomplete keyboard navigation
    if (autocompleteType()) {
      // Let AutocompletePopup handle these keys
      if (["ArrowDown", "ArrowUp", "Escape"].includes(e.key)) {
        return; // Handled by PopupList
      }

      // Enter or Tab to accept suggestion (handled by PopupList via onSelect)
      if (e.key === "Enter" || e.key === "Tab") {
        return; // Handled by PopupList
      }
    }

    // Send on Enter (without Shift), allow Shift+Enter for newlines
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSubmit(e);
    }
  };

  // Handle textarea click - close autocomplete if cursor moved away from trigger
  const handleTextareaClick = () => {
    // Re-detect autocomplete at current cursor position
    // This will close autocomplete if user clicked away from trigger
    detectAutocomplete(content());
  };

  // Insert autocomplete selection at cursor
  const handleAutocompleteSelect = (value: string) => {
    if (!textareaRef) return;

    const currentContent = content();
    const start = autocompleteStart();
    const cursorPos = textareaRef.selectionStart;

    // Replace the trigger and query with the selected value
    const before = currentContent.substring(0, start);
    const after = currentContent.substring(cursorPos);
    const newContent = before + value + after;

    setContent(newContent);
    setAutocompleteType(null);

    // Set cursor position after insertion
    const newCursorPos = start + value.length;
    setTimeout(() => {
      if (textareaRef) {
        textareaRef.focus();
        textareaRef.setSelectionRange(newCursorPos, newCursorPos);
      }
    }, 0);
  };

  // Drag & Drop Handlers
  const handleDragOver = (e: DragEvent) => {
    e.preventDefault();
    setIsDragging(true);
  };

  const handleDragLeave = (e: DragEvent) => {
    e.preventDefault();
    setIsDragging(false);
  };

  const handleDrop = (e: DragEvent) => {
    e.preventDefault();
    setIsDragging(false);

    if (e.dataTransfer?.files && e.dataTransfer.files.length > 0) {
      // Add all dropped files to pending
      Array.from(e.dataTransfer.files).forEach(addPendingFile);
    }
  };

  const handleFileSelect = () => {
    const input = document.createElement("input");
    input.type = "file";
    input.multiple = true;
    input.onchange = (e) => {
      const files = (e.target as HTMLInputElement).files;
      if (files) {
        Array.from(files).forEach(addPendingFile);
      }
    };
    input.click();
  };

  return (
    <form
      onSubmit={handleSubmit}
      class="px-4 pb-4 relative"
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
    >
      {/* Drag Overlay */}
      <Show when={isDragging()}>
        <div class="absolute inset-0 z-50 bg-surface-base/90 backdrop-blur-sm rounded-lg border-2 border-dashed border-accent-primary flex flex-col items-center justify-center pointer-events-none m-4">
          <UploadCloud class="w-12 h-12 text-accent-primary mb-2" />
          <p class="text-text-primary font-medium">Drop files to upload</p>
          <p class="text-xs text-text-secondary mt-1">Maximum size: {getUploadLimitText('attachment')}</p>
        </div>
      </Show>

      {/* Upload Error */}
      <Show when={uploadError()}>
        <div class="mb-2 p-3 bg-error-bg border border-error-border rounded-lg text-sm text-error-text flex items-center justify-between">
          <span>{uploadError()}</span>
          <button onClick={() => setUploadError(null)}>
            <X class="w-4 h-4" />
          </button>
        </div>
      </Show>

      {/* Upload Preview Tray */}
      <Show when={pendingFiles().length > 0}>
        <div class="mb-2 p-3 bg-surface-layer2 rounded-xl border border-white/5">
          <div class="flex items-center justify-between mb-2">
            <span class="text-xs text-text-secondary font-medium">
              {pendingFiles().length} file{pendingFiles().length > 1 ? "s" : ""} ready to send
            </span>
            <button
              type="button"
              onClick={clearPendingFiles}
              class="text-xs text-text-secondary hover:text-accent-danger transition-colors"
            >
              Clear all
            </button>
          </div>
          <div class="flex gap-2 overflow-x-auto pb-1">
            <For each={pendingFiles()}>
              {(pf, index) => (
                <div class="relative group flex-shrink-0">
                  <Show
                    when={pf.previewUrl}
                    fallback={
                      <div class="w-20 h-20 bg-surface-layer1 rounded-lg flex flex-col items-center justify-center border border-white/5">
                        <FileIcon class="w-6 h-6 text-text-secondary mb-1" />
                        <span class="text-[10px] text-text-secondary truncate max-w-[70px] px-1">
                          {pf.file.name}
                        </span>
                      </div>
                    }
                  >
                    <img
                      src={pf.previewUrl!}
                      alt={pf.file.name}
                      class="w-20 h-20 object-cover rounded-lg border border-white/5"
                    />
                  </Show>
                  <button
                    type="button"
                    onClick={() => removePendingFile(index())}
                    class="absolute -top-1.5 -right-1.5 p-0.5 bg-accent-danger rounded-full text-white opacity-0 group-hover:opacity-100 transition-opacity shadow-lg"
                    title="Remove file"
                  >
                    <X class="w-3 h-3" />
                  </button>
                </div>
              )}
            </For>
          </div>
        </div>
      </Show>

      <div class="relative flex items-center rounded-xl border border-white/5 focus-within:border-accent-primary/30 transition-colors" style="background-color: var(--color-surface-layer2)">
        {/* Attachment button */}
        <button
          type="button"
          class="p-3 text-text-secondary hover:text-text-primary transition-colors"
          title="Add files"
          onClick={handleFileSelect}
        >
          <PlusCircle class="w-5 h-5" />
        </button>

        {/* Text input */}
        <textarea
          ref={textareaRef}
          value={content()}
          onInput={(e) => handleInput(e.currentTarget.value)}
          onKeyDown={handleKeyDown}
          onClick={handleTextareaClick}
          onCompositionStart={() => setIsComposing(true)}
          onCompositionEnd={() => setIsComposing(false)}
          class="flex-1 bg-transparent py-3 text-text-input placeholder-text-secondary focus:outline-none resize-none overflow-y-auto"
          style={{ "min-height": "24px", "max-height": "192px" }}
          placeholder={`Message #${props.channelName}`}
          disabled={isSending()}
          rows={1}
        />

        {/* Send button and character counter container */}
        <div class="flex items-center gap-1 pr-1">
          {/* Character counter - show when nearing limit */}
          <Show when={isNearLimit()}>
            <div class="flex flex-col items-end pr-2 gap-0.5 pointer-events-none">
              <Show when={lengthStats().totalLength > lengthStats().regularLength}>
                <span class={`text-[9px] ${lengthStats().totalLength > lengthStats().totalLimit ? 'text-accent-danger font-bold' : 'text-text-secondary'}`}>
                  Total: {lengthStats().totalLength}/{lengthStats().totalLimit}
                </span>
              </Show>
              <span class={`text-[10px] leading-tight ${lengthStats().regularLength > lengthStats().regularLimit ? 'text-accent-danger font-bold' : 'text-text-secondary'}`}>
                Text: {lengthStats().regularLength}/{lengthStats().regularLimit}
              </span>
            </div>
          </Show>

          {/* Send button - show when there's content OR pending files */}
          <Show when={content().trim() || pendingFiles().length > 0}>
            <button
              type="submit"
              class="p-2 text-accent-primary hover:text-accent-primary/80 transition-colors disabled:opacity-50"
              disabled={isSending() || isOverLimit()}
              title={pendingFiles().length > 0 ? `Send ${pendingFiles().length} file(s)` : "Send message"}
            >
              <Send class="w-5 h-5" />
            </button>
          </Show>
        </div>
      </div>

      {/* Error display */}
      <Show when={messagesState.error}>
        <div class="mt-2 text-sm" style="color: var(--color-error-text)">
          Failed to send: {messagesState.error}
        </div>
      </Show>

      {/* Autocomplete Popup */}
      <Show when={autocompleteType() && textareaRef}>
        <AutocompletePopup
          anchorEl={textareaRef!}
          type={autocompleteType()!}
          query={autocompleteQuery()}
          selectedIndex={autocompleteIndex()}
          guildMembers={props.guildId ? guildsState.members[props.guildId] : undefined}
          dmParticipants={props.dmParticipants}
          guildId={props.guildId}
          channels={channelsState.channels}
          commands={guildCommands()}
          onSelect={handleAutocompleteSelect}
          onClose={() => setAutocompleteType(null)}
          onSelectionChange={setAutocompleteIndex}
        />
      </Show>
    </form>
  );
};

export default MessageInput;
