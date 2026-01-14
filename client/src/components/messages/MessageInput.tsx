import { Component, createSignal, Show, For, onCleanup } from "solid-js";
import { PlusCircle, Send, UploadCloud, X, File as FileIcon } from "lucide-solid";
import { sendMessage, messagesState, addMessage } from "@/stores/messages";
import { stopTyping, sendTyping } from "@/stores/websocket";
import { uploadMessageWithFile } from "@/lib/tauri";

interface MessageInputProps {
  channelId: string;
  channelName: string;
}

interface PendingFile {
  file: File;
  previewUrl: string | null;
}

const MessageInput: Component<MessageInputProps> = (props) => {
  const [content, setContent] = createSignal("");
  const [isSending, setIsSending] = createSignal(false);
  const [isDragging, setIsDragging] = createSignal(false);
  const [pendingFiles, setPendingFiles] = createSignal<PendingFile[]>([]);
  let typingTimeout: NodeJS.Timeout | undefined;
  let inputRef: HTMLInputElement | undefined;

  // Cleanup on unmount
  onCleanup(() => {
    if (typingTimeout) {
      clearTimeout(typingTimeout);
      stopTyping(props.channelId);
    }
    // Revoke object URLs to prevent memory leaks
    pendingFiles().forEach((pf) => {
      if (pf.previewUrl) URL.revokeObjectURL(pf.previewUrl);
    });
  });

  // Add file to pending list with preview
  const addPendingFile = (file: File) => {
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

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    const text = content().trim();
    const files = pendingFiles();

    // Need either text or files to send
    if ((!text && files.length === 0) || isSending()) return;

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
          addMessage(message);
        }
        clearPendingFiles();
      } else {
        // Text-only message
        await sendMessage(props.channelId, text);
      }
      setContent("");
    } catch (err) {
      console.error("Send failed:", err);
    } finally {
      setIsSending(false);
      inputRef?.focus();
    }
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    // Send on Enter (without Shift)
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSubmit(e);
    }
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
        <input
          ref={inputRef}
          type="text"
          value={content()}
          onInput={(e) => handleInput(e.currentTarget.value)}
          onKeyDown={handleKeyDown}
          class="flex-1 bg-transparent py-3 text-text-input placeholder-text-secondary focus:outline-none"
          placeholder={`Message #${props.channelName}`}
          disabled={isSending()}
        />

        {/* Send button - show when there's content OR pending files */}
        <Show when={content().trim() || pendingFiles().length > 0}>
          <button
            type="submit"
            class="p-3 text-accent-primary hover:text-accent-primary/80 transition-colors disabled:opacity-50"
            disabled={isSending()}
            title={pendingFiles().length > 0 ? `Send ${pendingFiles().length} file(s)` : "Send message"}
          >
            <Send class="w-5 h-5" />
          </button>
        </Show>
      </div>

      {/* Error display */}
      <Show when={messagesState.error}>
        <div class="mt-2 text-sm" style="color: var(--color-error-text)">
          Failed to send: {messagesState.error}
        </div>
      </Show>
    </form>
  );
};

export default MessageInput;
