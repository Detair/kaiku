/**
 * Message Actions Toolbar
 *
 * Displays quick emoji reactions and action buttons on message hover.
 * Appears at the top-right of messages.
 */

import { Component, createSignal, Show } from "solid-js";
import { SmilePlus, MoreHorizontal, MessageSquareMore } from "lucide-solid";
import PositionedEmojiPicker from "@/components/emoji/PositionedEmojiPicker";

interface MessageActionsProps {
  /** Callback when a quick emoji is clicked */
  onAddReaction: (emoji: string) => void;
  /** Callback when context menu is shown */
  onShowContextMenu: (e: MouseEvent) => void;
  /** Optional guild ID for custom emojis */
  guildId?: string;
  /** Whether this message is a thread reply (hides thread button) */
  isThreadReply?: boolean;
  /** Callback to open thread for this message */
  onReplyInThread?: () => void;
}

// Quick reaction emojis
const QUICK_EMOJIS = ["👍", "❤️", "😂", "😮"];

const MessageActions: Component<MessageActionsProps> = (props) => {
  const [showEmojiPicker, setShowEmojiPicker] = createSignal(false);
  let emojiButtonRef: HTMLButtonElement | undefined;

  const handleQuickReaction = (emoji: string) => {
    props.onAddReaction(emoji);
  };

  const handleEmojiSelect = (emoji: string) => {
    props.onAddReaction(emoji);
    setShowEmojiPicker(false);
  };

  return (
    <div class="absolute top-0 right-4 -translate-y-1/2 flex items-center gap-1 bg-surface-layer2 border border-white/10 rounded-lg shadow-xl px-1 py-1 opacity-0 group-hover:opacity-100 transition-opacity">
      {/* Quick emoji reactions */}
      {QUICK_EMOJIS.map((emoji) => (
        <button
          class="w-7 h-7 flex items-center justify-center rounded hover:bg-white/10 transition-colors"
          onClick={() => handleQuickReaction(emoji)}
          title={`React with ${emoji}`}
          aria-label={`React with ${emoji}`}
        >
          <span class="text-base leading-none">{emoji}</span>
        </button>
      ))}

      {/* Emoji picker button */}
      <button
        ref={emojiButtonRef}
        class="w-7 h-7 flex items-center justify-center rounded hover:bg-white/10 text-text-secondary hover:text-text-primary transition-colors"
        onClick={() => setShowEmojiPicker(!showEmojiPicker())}
        title="More reactions"
        aria-label="More reactions"
        aria-expanded={showEmojiPicker()}
      >
        <SmilePlus class="w-4 h-4" />
      </button>

      <Show when={showEmojiPicker() && emojiButtonRef}>
        <PositionedEmojiPicker
          anchorEl={emojiButtonRef!}
          onSelect={handleEmojiSelect}
          onClose={() => setShowEmojiPicker(false)}
          guildId={props.guildId}
        />
      </Show>

      {/* Reply in thread button (hidden for thread replies) */}
      <Show when={!props.isThreadReply && props.onReplyInThread}>
        <button
          class="w-7 h-7 flex items-center justify-center rounded hover:bg-white/10 text-text-secondary hover:text-text-primary transition-colors"
          onClick={() => props.onReplyInThread?.()}
          title="Reply in Thread"
          aria-label="Reply in Thread"
        >
          <MessageSquareMore class="w-4 h-4" />
        </button>
      </Show>

      {/* Divider */}
      <div class="w-px h-5 bg-white/10 mx-0.5" />

      {/* Context menu button */}
      <button
        class="w-7 h-7 flex items-center justify-center rounded hover:bg-white/10 text-text-secondary hover:text-text-primary transition-colors"
        onClick={props.onShowContextMenu}
        title="More actions"
        aria-label="More actions"
      >
        <MoreHorizontal class="w-4 h-4" />
      </button>
    </div>
  );
};

export default MessageActions;
