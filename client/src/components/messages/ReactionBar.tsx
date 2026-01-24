import { Component, For, Show, createSignal } from "solid-js";
import EmojiPicker from "@/components/emoji/EmojiPicker";
import type { Reaction } from "@/lib/types";

interface ReactionBarProps {
  reactions: Reaction[];
  onAddReaction: (emoji: string) => void;
  onRemoveReaction: (emoji: string) => void;
  guildId?: string;
}

const ReactionBar: Component<ReactionBarProps> = (props) => {
  const [showPicker, setShowPicker] = createSignal(false);

  const handleReactionClick = (reaction: Reaction) => {
    if (reaction.me) {
      props.onRemoveReaction(reaction.emoji);
    } else {
      props.onAddReaction(reaction.emoji);
    }
  };

  const handleAddReaction = (emoji: string) => {
    props.onAddReaction(emoji);
    setShowPicker(false);
  };

  return (
    <div class="flex flex-wrap items-center gap-1 mt-1">
      <For each={props.reactions}>
        {(reaction) => (
          <button
            class={`inline-flex items-center gap-1 px-2 py-0.5 rounded text-sm transition-all duration-150 ${
              reaction.me
                ? "bg-accent-primary/20 border border-accent-primary/50 hover:bg-accent-primary/30"
                : "bg-surface-layer2 border border-transparent hover:bg-white/10"
            }`}
            onClick={() => handleReactionClick(reaction)}
            title={reaction.users.length > 0 ? reaction.users.join(", ") : undefined}
          >
            <span class="text-base">{reaction.emoji}</span>
            <span class="text-xs text-text-secondary">{reaction.count}</span>
          </button>
        )}
      </For>

      {/* Add reaction button */}
      <div class="relative">
        <button
          class="w-6 h-6 flex items-center justify-center rounded hover:bg-white/10 text-text-secondary hover:text-text-primary transition-colors"
          onClick={() => setShowPicker(!showPicker())}
          title="Add reaction"
        >
          <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="12" cy="12" r="10" />
            <path d="M8 14s1.5 2 4 2 4-2 4-2" />
            <line x1="9" y1="9" x2="9.01" y2="9" />
            <line x1="15" y1="9" x2="15.01" y2="9" />
          </svg>
        </button>

        <Show when={showPicker()}>
          <div class="absolute bottom-full left-0 mb-2 z-50">
            <EmojiPicker
              onSelect={handleAddReaction}
              onClose={() => setShowPicker(false)}
              guildId={props.guildId}
            />
          </div>
        </Show>
      </div>
    </div>
  );
};

export default ReactionBar;
