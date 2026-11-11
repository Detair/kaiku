import { Component, Show, createMemo } from "solid-js";
import { typingState } from "@/stores/websocket";
import { messagesState } from "@/stores/messages";
import { guildsState } from "@/stores/guilds";
import { dmsState } from "@/stores/dms";
import { plural, t } from "@/i18n";

interface TypingIndicatorProps {
  channelId: string;
}

/** Display name for a user id, from what this client already knows. */
function nameFor(channelId: string, userId: string): string | null {
  const messages = messagesState.byChannel[channelId] ?? [];
  for (let i = messages.length - 1; i >= 0; i--) {
    const author = messages[i].author;
    if (author.id === userId) return author.display_name || author.username;
  }
  for (const members of Object.values(guildsState.members)) {
    const member = members?.find((m) => m.user_id === userId);
    if (member) return member.nickname || member.display_name || member.username;
  }
  for (const dm of dmsState.dms) {
    const p = dm.participants.find((x) => x.user_id === userId);
    if (p) return p.display_name || p.username;
  }
  return null;
}

/** "Oma is typing…", "Oma and Tom are typing…", "3 people are typing…". */
export function typingTextFor(names: (string | null)[]): string | null {
  if (names.length === 0) return null;
  const known = names.filter((n): n is string => !!n);
  if (names.length === 1 && known.length === 1) {
    return t("chat.typing.name", { name: known[0] });
  }
  if (names.length === 2 && known.length === 2) {
    return t("chat.typing.twoNames", { first: known[0], second: known[1] });
  }
  return plural(names.length, { one: "chat.typing.one", other: "chat.typing.other" });
}

const TypingIndicator: Component<TypingIndicatorProps> = (props) => {
  // Get typing users reactively
  const typingUsers = createMemo(() => {
    // Access the store to create reactivity
    const users = typingState.byChannel[props.channelId];
    return users ? Array.from(users) : [];
  });

  const typingText = createMemo(() =>
    typingTextFor(typingUsers().map((id) => nameFor(props.channelId, id))),
  );

  // The live region stays mounted so screen readers hear the text when it
  // appears; it only changes when someone starts or stops typing.
  return (
    <div role="status" aria-live="polite" aria-atomic="true">
      <Show when={typingUsers().length > 0}>
        <div data-testid="typing-indicator" class="h-6 px-4 flex items-center text-sm text-text-muted">
          <div class="flex items-center gap-2">
            {/* Animated typing dots */}
            <div class="flex gap-0.5" aria-hidden="true">
              <span
                class="w-1.5 h-1.5 bg-text-muted rounded-full animate-bounce"
                style={{"animation-delay":"0ms"}}
              />
              <span
                class="w-1.5 h-1.5 bg-text-muted rounded-full animate-bounce"
                style={{"animation-delay":"150ms"}}
              />
              <span
                class="w-1.5 h-1.5 bg-text-muted rounded-full animate-bounce"
                style={{"animation-delay":"300ms"}}
              />
            </div>
            <span>{typingText()}</span>
          </div>
        </div>
      </Show>
    </div>
  );
};

export default TypingIndicator;
