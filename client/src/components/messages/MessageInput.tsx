import { Component, createSignal, Show, onCleanup } from "solid-js";
import { PlusCircle, Send } from "lucide-solid";
import { sendMessage, messagesState } from "@/stores/messages";
import { sendTyping, stopTyping } from "@/stores/websocket";

interface MessageInputProps {
  channelId: string;
  channelName: string;
}

const MessageInput: Component<MessageInputProps> = (props) => {
  const [content, setContent] = createSignal("");
  const [isSending, setIsSending] = createSignal(false);
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

  return (
    <form onSubmit={handleSubmit} class="px-4 pb-4">
      <div class="relative flex items-center bg-background-tertiary rounded-lg">
        {/* Attachment button placeholder */}
        <button
          type="button"
          class="p-3 text-text-muted hover:text-text-secondary transition-colors"
          title="Add attachment"
          disabled
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
