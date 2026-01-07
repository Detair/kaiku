import { Component, Show, For } from "solid-js";
import { PhoneOff, Signal } from "lucide-solid";
import { voiceState, leaveVoice, getParticipants } from "@/stores/voice";
import { getChannel } from "@/stores/channels";
import VoiceControls from "./VoiceControls";
import Avatar from "@/components/ui/Avatar";

/**
 * Voice panel shown when connected to a voice channel.
 * Displays current channel, participants, and controls.
 */
const VoicePanel: Component = () => {
  const channel = () => {
    if (!voiceState.channelId) return null;
    return getChannel(voiceState.channelId);
  };

  const participants = () => getParticipants();

  const handleDisconnect = () => {
    leaveVoice();
  };

  return (
    <Show when={voiceState.state === "connected" && channel()}>
      <div class="bg-background-tertiary border-t border-background-secondary">
        {/* Connection info */}
        <div class="px-3 py-2 flex items-center justify-between">
          <div class="flex items-center gap-2 min-w-0">
            <Signal class="w-4 h-4 text-success flex-shrink-0" />
            <div class="min-w-0">
              <div class="text-xs font-medium text-success">Voice Connected</div>
              <div class="text-xs text-text-muted truncate">{channel()?.name}</div>
            </div>
          </div>
          <button
            onClick={handleDisconnect}
            class="p-1.5 text-text-muted hover:text-danger hover:bg-danger/10 rounded transition-colors"
            title="Disconnect"
          >
            <PhoneOff class="w-4 h-4" />
          </button>
        </div>

        {/* Participants list */}
        <Show when={participants().length > 0}>
          <div class="px-3 pb-2">
            <div class="flex flex-wrap gap-1">
              <For each={participants()}>
                {(participant) => (
                  <div
                    class={`flex items-center gap-1 px-2 py-1 rounded text-xs ${
                      participant.speaking
                        ? "bg-success/20 text-success"
                        : "bg-background-secondary text-text-secondary"
                    } ${participant.muted ? "opacity-50" : ""}`}
                    title={participant.muted ? "Muted" : undefined}
                  >
                    <div class="w-4 h-4 rounded-full bg-primary/50" />
                    <span class="truncate max-w-20">{participant.user_id.slice(0, 8)}</span>
                  </div>
                )}
              </For>
            </div>
          </div>
        </Show>

        {/* Voice controls */}
        <VoiceControls />
      </div>
    </Show>
  );
};

export default VoicePanel;
