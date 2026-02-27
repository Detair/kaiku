import { Component, Show, createMemo } from "solid-js";
import { typingState } from "@/stores/websocket";

interface TypingIndicatorProps {
  channelId: string;
}

const TypingIndicator: Component<TypingIndicatorProps> = (props) => {
  // Get typing users reactively
  const typingUsers = createMemo(() => {
    // Access the store to create reactivity
    const users = typingState.byChannel[props.channelId];
    return users ? Array.from(users) : [];
  });

  const typingText = createMemo(() => {
    const users = typingUsers();
    if (users.length === 0) return null;
    if (users.length === 1) return `Someone is typing...`;
    if (users.length === 2) return `2 people are typing...`;
    return `${users.length} people are typing...`;
  });

  return (
    <Show when={typingUsers().length > 0}>
      <div class="h-6 px-4 flex items-center text-sm text-text-muted">
        <div class="flex items-center gap-2">
          {/* Animated typing dots */}
          <div class="flex gap-0.5">
            <span
              class="w-1.5 h-1.5 bg-text-muted rounded-full animate-bounce"
              style="animation-delay: 0ms"
            />
            <span
              class="w-1.5 h-1.5 bg-text-muted rounded-full animate-bounce"
              style="animation-delay: 150ms"
            />
            <span
              class="w-1.5 h-1.5 bg-text-muted rounded-full animate-bounce"
              style="animation-delay: 300ms"
            />
          </div>
          <span>{typingText()}</span>
        </div>
      </div>
    </Show>
  );
};

export default TypingIndicator;
