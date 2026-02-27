import { Component, Show, For } from "solid-js";
import { PhoneOff, Signal, MonitorUp } from "lucide-solid";
import { voiceState, leaveVoice, getParticipants } from "@/stores/voice";
import { getChannel } from "@/stores/channels";
import { viewUserShare } from "@/stores/screenShareViewer";
import VoiceControls from "./VoiceControls";

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
              <div class="text-xs font-medium text-success">
                Voice Connected
              </div>
              <div class="text-xs text-text-muted truncate">
                {channel()?.name}
              </div>
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
                    <span class="truncate max-w-20">
                      {participant.user_id.slice(0, 8)}
                    </span>
                    {participant.screen_sharing && (
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          viewUserShare(participant.user_id);
                        }}
                        class="p-0.5 hover:bg-success/30 rounded transition-colors"
                        title="View screen share"
                      >
                        <MonitorUp class="w-3 h-3 text-success" />
                      </button>
                    )}
                  </div>
                )}
              </For>
            </div>
          </div>
        </Show>

        {/* Active screen shares */}
        <Show when={voiceState.screenShares.length > 0}>
          <div class="px-3 pb-2 border-t border-background-secondary pt-2">
            <div class="text-xs text-text-muted mb-1">Screen Shares</div>
            <For each={voiceState.screenShares}>
              {(share) => (
                <div
                  class="flex items-center gap-2 px-2 py-1.5 rounded bg-background-primary hover:bg-background-tertiary cursor-pointer transition-colors"
                  onClick={() => viewUserShare(share.user_id)}
                >
                  <MonitorUp class="w-4 h-4 text-success" />
                  <div class="flex-1 min-w-0">
                    <div class="text-sm text-text-primary truncate">
                      {share.username || share.user_id.slice(0, 8)}
                    </div>
                    <div class="text-xs text-text-muted">
                      {share.quality} •{" "}
                      {share.has_audio ? "With audio" : "No audio"}
                    </div>
                  </div>
                </div>
              )}
            </For>
          </div>
        </Show>

        {/* Voice controls */}
        <VoiceControls />
      </div>
    </Show>
  );
};

export default VoicePanel;
