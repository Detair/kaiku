import { Component, createSignal, Show, onCleanup } from "solid-js";
import { PlusCircle, Send, UploadCloud } from "lucide-solid";
import { sendMessage, messagesState, addMessage } from "@/stores/messages";
import { stopTyping, sendTyping } from "@/stores/websocket";
import { uploadMessageWithFile } from "@/lib/tauri";

interface MessageInputProps {
  channelId: string;
  channelName: string;
}

const MessageInput: Component<MessageInputProps> = (props) => {
  const [content, setContent] = createSignal("");
  const [isSending, setIsSending] = createSignal(false);
  const [isDragging, setIsDragging] = createSignal(false);
  let typingTimeout: NodeJS.Timeout | undefined;
  let inputRef: HTMLInputElement | undefined;

  // Cleanup typing timeout on unmount
  onCleanup(() => {
    if (typingTimeout) {
      clearTimeout(typingTimeout);
      stopTyping(props.channelId);
    }
  });

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
    if (!text || isSending()) return;

    // Stop typing indicator
    if (typingTimeout) {
      clearTimeout(typingTimeout);
    }
    stopTyping(props.channelId);

    setIsSending(true);
    try {
      await sendMessage(props.channelId, text);
      setContent("");
    } finally {
      setIsSending(false);
      // Refocus the input after sending
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

  const handleDrop = async (e: DragEvent) => {
    e.preventDefault();
    setIsDragging(false);

    if (e.dataTransfer?.files && e.dataTransfer.files.length > 0) {
      const file = e.dataTransfer.files[0];
      await handleFileUpload(file);
    }
  };

  const handleFileUpload = async (file: File) => {
    if (isSending()) return;
    setIsSending(true);

    // Stop typing indicator
    if (typingTimeout) {
      clearTimeout(typingTimeout);
    }
    stopTyping(props.channelId);

    try {
      // Upload file with optional text content in a single request
      const textContent = content().trim() || undefined;
      const message = await uploadMessageWithFile(props.channelId, file, textContent);

      // Add message to local store
      addMessage(message);
      setContent("");
    } catch (err) {
      console.error("Upload failed:", err);
      // Ideally show a toast or error in the UI
    } finally {
      setIsSending(false);
      inputRef?.focus();
    }
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
        <div class="absolute inset-0 z-50 bg-background-tertiary/90 backdrop-blur-sm rounded-lg border-2 border-dashed border-accent-primary flex flex-col items-center justify-center pointer-events-none m-4">
          <UploadCloud class="w-12 h-12 text-accent-primary mb-2" />
          <p class="text-text-primary font-medium">Drop file to upload</p>
        </div>
      </Show>

      <div class="relative flex items-center bg-background-tertiary rounded-lg border border-transparent focus-within:border-white/10 transition-colors">
        {/* Attachment button placeholder (future: trigger file picker) */}
        <button
          type="button"
          class="p-3 text-text-muted hover:text-text-secondary transition-colors"
          title="Drag & Drop files to upload"
          onClick={() => {
            // Future: File picker
            const input = document.createElement('input');
            input.type = 'file';
            input.onchange = (e) => {
              const file = (e.target as HTMLInputElement).files?.[0];
              if (file) handleFileUpload(file);
            };
            input.click();
          }}
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
          class="flex-1 bg-transparent py-3 text-text-primary placeholder-text-muted focus:outline-none"
          placeholder={`Message #${props.channelName}`}
          disabled={isSending()}
        />

        {/* Send button */}
        <Show when={content().trim()}>
          <button
            type="submit"
            class="p-3 text-primary hover:text-primary/80 transition-colors disabled:opacity-50"
            disabled={isSending()}
          >
            <Send class="w-5 h-5" />
          </button>
        </Show>
      </div>

      {/* Error display */}
      <Show when={messagesState.error}>
        <div class="mt-2 text-sm text-danger">
          Failed to send: {messagesState.error}
        </div>
      </Show>
    </form>
  );
};

export default MessageInput;
