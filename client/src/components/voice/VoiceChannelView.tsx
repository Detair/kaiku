/**
 * VoiceChannelView — Main content area for voice channels
 *
 * Shows a participant grid with speaking indicators, screen share
 * buttons, and voice controls. When not connected, shows a preview
 * with participant count and a "Join Voice" button.
 */

import { Component, For, Show, createMemo } from "solid-js";
import { Users, MonitorPlay, MonitorOff } from "lucide-solid";
import { voiceState, getParticipants, joinVoice, getParticipantMetrics, stopScreenShare } from "@/stores/voice";
import { startViewing } from "@/stores/screenShareViewer";
import { authState } from "@/stores/auth";
import VoiceControls from "./VoiceControls";
import { QualityIndicator } from "./QualityIndicator";
import { showToast } from "@/components/ui/Toast";

interface VoiceChannelViewProps {
  channelId: string;
  channelName: string;
}

const VoiceChannelView: Component<VoiceChannelViewProps> = (props) => {
  const isConnected = () =>
    voiceState.state === "connected" && voiceState.channelId === props.channelId;
  const isConnecting = () => voiceState.state === "connecting";
  const participants = createMemo(() => getParticipants());
  const screenShares = () => voiceState.screenShares || [];

  const handleJoin = async () => {
    try {
      await joinVoice(props.channelId);
    } catch {
      showToast({
        type: "error",
        title: "Could not join voice channel",
        duration: 8000,
      });
    }
  };

  return (
    <div class="flex-1 flex flex-col min-h-0">
      {/* Not connected — preview */}
      <Show when={!isConnected()}>
        <div class="flex-1 flex flex-col items-center justify-center gap-6 px-8">
          <div class="w-20 h-20 rounded-full bg-accent-primary/20 flex items-center justify-center">
            <Users class="w-10 h-10 text-text-secondary" />
          </div>
          <div class="text-center">
            <h2 class="text-xl font-semibold text-text-primary mb-2">
              {props.channelName}
            </h2>
            <p class="text-text-secondary text-sm">
              <Show
                when={participants().length > 0}
                fallback="No one is in this channel yet."
              >
                {participants().length}{" "}
                {participants().length === 1 ? "person" : "people"} connected
              </Show>
            </p>
          </div>
          <button
            onClick={handleJoin}
            disabled={isConnecting()}
            class="px-8 py-3 bg-accent-primary text-on-accent rounded-xl font-semibold text-lg hover:opacity-90 transition-opacity disabled:opacity-50"
          >
            {isConnecting() ? "Connecting..." : "Join Voice"}
          </button>
        </div>
      </Show>

      {/* Connected — participant grid + controls */}
      <Show when={isConnected()}>
        <div class="flex-1 overflow-y-auto p-6">
          {/* Participant grid */}
          <div class="flex flex-wrap gap-4 justify-center mb-6">
            <For each={participants()}>
              {(participant) => {
                const metrics = () =>
                  getParticipantMetrics(participant.user_id);
                const isSpeaking = () => participant.speaking;
                const initial = () =>
                  (
                    participant.display_name?.charAt(0) ||
                    participant.username?.charAt(0) ||
                    "?"
                  ).toUpperCase();

                return (
                  <div
                    class="w-32 flex flex-col items-center gap-2 p-4 rounded-xl transition-all"
                    classList={{
                      "bg-accent-primary/10 ring-2 ring-accent-primary/40":
                        isSpeaking(),
                      "bg-surface-layer2": !isSpeaking(),
                    }}
                  >
                    <div class="w-16 h-16 rounded-full bg-surface-highlight flex items-center justify-center text-2xl font-bold text-text-primary">
                      {initial()}
                    </div>
                    <span class="text-sm text-text-primary font-medium truncate w-full text-center">
                      {participant.display_name || participant.username}
                    </span>
                    <div class="flex items-center gap-1.5">
                      <QualityIndicator
                        metrics={metrics() as any ?? null}
                        mode="circle"
                      />
                      <Show when={participant.muted}>
                        <span
                          class="w-4 h-4 rounded-full bg-accent-danger/20 flex items-center justify-center text-[10px]"
                          title="Muted"
                        >
                          🔇
                        </span>
                      </Show>
                      <Show when={participant.screen_sharing}>
                        <span
                          class="w-4 h-4 rounded-full bg-accent-primary/20 flex items-center justify-center text-[10px]"
                          title="Screen sharing"
                        >
                          📺
                        </span>
                      </Show>
                    </div>
                  </div>
                );
              }}
            </For>
          </div>

          {/* Screen share thumbnails */}
          <Show when={screenShares().length > 0}>
            <div class="mt-4">
              <h3 class="text-sm font-semibold text-text-secondary uppercase tracking-wide mb-3 flex items-center gap-2">
                <MonitorPlay class="w-4 h-4" />
                Screen Shares
              </h3>
              <div class="flex flex-wrap gap-3">
                <For each={screenShares()}>
                  {(share) => {
                    const isOwn = () => share.user_id === authState.user?.id;
                    return (
                      <div class="flex items-center gap-1">
                        <button
                          onClick={() => {
                            if (!isOwn()) startViewing(share.stream_id);
                          }}
                          class="flex items-center gap-2 px-4 py-2 rounded-lg bg-surface-layer2 hover:bg-surface-highlight text-text-primary text-sm transition-colors"
                          classList={{ "cursor-default": isOwn() }}
                        >
                          <MonitorPlay class="w-4 h-4 text-text-secondary" />
                          {isOwn() ? "Your screen" : `${share.username}'s screen`}
                          <Show
                            when={share.source_label && share.source_label !== "Screen"}
                          >
                            <span class="text-text-secondary">({share.source_label})</span>
                          </Show>
                        </button>
                        <Show when={isOwn()}>
                          <button
                            onClick={() => stopScreenShare(share.stream_id)}
                            class="p-2 rounded-lg bg-accent-danger/20 text-text-primary hover:bg-accent-danger/30 transition-colors"
                            title="Stop sharing"
                          >
                            <MonitorOff class="w-4 h-4" />
                          </button>
                        </Show>
                      </div>
                    );
                  }}
                </For>
              </div>
            </div>
          </Show>
        </div>

        {/* Controls bar */}
        <div class="px-6 py-3 border-t border-white/10 bg-surface-layer1 flex items-center justify-center">
          <VoiceControls />
        </div>
      </Show>
    </div>
  );
};

export default VoiceChannelView;
