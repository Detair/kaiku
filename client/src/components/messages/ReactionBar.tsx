import { Component, For, Show, createSignal } from "solid-js";
import PositionedEmojiPicker from "@/components/emoji/PositionedEmojiPicker";
import type { Reaction } from "@/lib/types";
import { plural, t } from "@/i18n";

/** "👍, 3 people: Oma, Tom" for a screen reader; names when the server sent them. */
export function reactionLabel(reaction: Reaction): string {
  const users = reaction.users ?? [];
  if (users.length > 0) {
    return t("chat.reaction.labelNames", { emoji: reaction.emoji, names: users.join(", ") });
  }
  return plural(reaction.count, {
    one: "chat.reaction.label.one",
    other: "chat.reaction.label.other",
  }, { emoji: reaction.emoji });
}

interface ReactionBarProps {
  reactions: Reaction[];
  onAddReaction: (emoji: string) => void;
  onRemoveReaction: (emoji: string) => void;
  guildId?: string;
}

const ReactionBar: Component<ReactionBarProps> = (props) => {
  const [showPicker, setShowPicker] = createSignal(false);
  let addReactionButtonRef: HTMLButtonElement | undefined;

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
    <div data-testid="reaction-bar" class="flex flex-wrap items-center gap-1 mt-1">
      <For each={props.reactions}>
        {(reaction) => (
          <button
            class={`inline-flex items-center gap-1 px-2 py-0.5 rounded text-sm transition-all duration-150 ${
              reaction.me
                ? "bg-accent-primary/20 border border-accent-primary/50 hover:bg-accent-primary/30"
                : "bg-surface-layer2 border border-transparent hover:bg-white/10"
            }`}
            onClick={() => handleReactionClick(reaction)}
            aria-pressed={!!reaction.me}
            aria-label={reactionLabel(reaction)}
            title={
              (reaction.users ?? []).length > 0
                ? (reaction.users ?? []).join(", ")
                : undefined
            }
          >
            <span class="text-base">{reaction.emoji}</span>
            <span class="text-xs text-text-secondary">{reaction.count}</span>
          </button>
        )}
      </For>

      {/* Add reaction button */}
      <div>
        <button
          ref={addReactionButtonRef}
          class="w-6 h-6 flex items-center justify-center rounded hover:bg-white/10 text-text-secondary hover:text-text-primary transition-colors"
          onClick={() => setShowPicker(!showPicker())}
          title={t("chat.message.addReaction")}
          aria-label={t("chat.message.addReaction")}
          aria-expanded={showPicker()}
        >
          <svg
            class="w-4 h-4"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
          >
            <circle cx="12" cy="12" r="10" />
            <path d="M8 14s1.5 2 4 2 4-2 4-2" />
            <line x1="9" y1="9" x2="9.01" y2="9" />
            <line x1="15" y1="9" x2="15.01" y2="9" />
          </svg>
        </button>

        <Show when={showPicker() && addReactionButtonRef}>
          <PositionedEmojiPicker
            anchorEl={addReactionButtonRef!}
            onSelect={handleAddReaction}
            onClose={() => setShowPicker(false)}
            guildId={props.guildId}
          />
        </Show>
      </div>
    </div>
  );
};

export default ReactionBar;
