import { Component, For, Show } from "solid-js";
import { User, MicOff, Volume2 } from "lucide-solid";
import { voiceState } from "@/stores/voice";
import { authState } from "@/stores/auth";

interface Props {
  channelId: string;
}

/**
 * Displays participants in a voice channel
 */
const VoiceParticipants: Component<Props> = (props) => {
  // Check if we're connected or connecting to this channel
  const isActiveInChannel = () =>
    voiceState.channelId === props.channelId &&
    (voiceState.state === "connected" || voiceState.state === "connecting");

  // Remote participants from server (exclude current user, they're shown separately)
  const remoteParticipants = () => {
    return Object.values(voiceState.participants).filter(
      (p) => voiceState.channelId === props.channelId && !isCurrentUser(p.user_id)
    );
  };

  const isCurrentUser = (userId: string) => {
    return authState.user?.id === userId;
  };

  const getUserDisplay = (participant: { user_id: string; display_name?: string; username?: string }) => {
    if (isCurrentUser(participant.user_id)) {
      return authState.user?.display_name || authState.user?.username || "You";
    }
    return participant.display_name || participant.username || participant.user_id.slice(0, 8);
  };

  return (
    <Show when={isActiveInChannel()}>
      <div class="ml-6 mt-1 space-y-1">
        {/* Local user (always show first when connected) */}
        <div class="flex items-center gap-2 px-2 py-1 text-xs">
          <User class="w-3 h-3 text-accent-primary" />
          <span class="text-accent-primary font-medium">
            {authState.user?.display_name || authState.user?.username || "You"}
          </span>
          <Show when={voiceState.muted}>
            <div title="Muted">
              <MicOff class="w-3 h-3 text-accent-danger" />
            </div>
          </Show>
          <Show when={voiceState.speaking}>
            <div title="Speaking">
              <Volume2 class="w-3 h-3 text-accent-primary animate-pulse" />
            </div>
          </Show>
        </div>

        {/* Remote participants */}
        <For each={remoteParticipants()}>
          {(participant) => (
            <div class="flex items-center gap-2 px-2 py-1 text-xs">
              <User class="w-3 h-3 text-text-secondary" />
              <span class="text-text-secondary">
                {getUserDisplay(participant)}
              </span>
              <Show when={participant.muted}>
                <div title="Muted">
                  <MicOff class="w-3 h-3 text-accent-danger" />
                </div>
              </Show>
              <Show when={participant.speaking}>
                <div title="Speaking">
                  <Volume2 class="w-3 h-3 text-accent-primary animate-pulse" />
                </div>
              </Show>
            </div>
          )}
        </For>
      </div>
    </Show>
  );
};

export default VoiceParticipants;
