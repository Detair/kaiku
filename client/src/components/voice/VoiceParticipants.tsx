import { Component, For, Show } from "solid-js";
import { User } from "lucide-solid";
import { voiceState } from "@/stores/voice";
import { authState } from "@/stores/auth";

interface Props {
  channelId: string;
}

/**
 * Displays participants in a voice channel
 */
const VoiceParticipants: Component<Props> = (props) => {
  const participants = () => {
    return Object.values(voiceState.participants).filter(
      (_p) => voiceState.channelId === props.channelId
    );
  };

  const isCurrentUser = (userId: string) => {
    return authState.user?.id === userId;
  };

  const getUserDisplay = (participant: any) => {
    // If it's the current user, we have full info
    if (isCurrentUser(participant.user_id)) {
      return authState.user?.display_name || authState.user?.username || "You";
    }
    // Use display_name or username from participant info
    return participant.display_name || participant.username || participant.user_id.slice(0, 8);
  };

  return (
    <Show when={voiceState.channelId === props.channelId && participants().length > 0}>
      <div class="ml-6 mt-1 space-y-1">
        <For each={participants()}>
          {(participant) => (
            <div class="flex items-center gap-2 px-2 py-1 text-xs">
              <User class="w-3 h-3 text-text-muted" />
              <span class={isCurrentUser(participant.user_id) ? "text-success font-medium" : "text-text-secondary"}>
                {getUserDisplay(participant)}
              </span>
              <Show when={participant.muted}>
                <span class="text-danger text-[10px]" title="Muted">🔇</span>
              </Show>
              <Show when={participant.speaking}>
                <span class="text-success animate-pulse text-[10px]" title="Speaking">🔊</span>
              </Show>
            </div>
          )}
        </For>
      </div>
    </Show>
  );
};

export default VoiceParticipants;
