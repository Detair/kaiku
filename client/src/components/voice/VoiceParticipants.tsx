import { Component, For, Show, createSignal } from "solid-js";
import { User, MicOff, Volume2, Monitor, Camera } from "lucide-solid";
import {
  voiceState,
  getLocalMetrics,
  getParticipantMetrics,
} from "@/stores/voice";
import { authState } from "@/stores/auth";
import { QualityIndicator } from "./QualityIndicator";
import { QualityTooltip } from "./QualityTooltip";
import type { ConnectionMetrics } from "@/lib/webrtc/types";

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
      (p) =>
        voiceState.channelId === props.channelId && !isCurrentUser(p.user_id),
    );
  };

  const isCurrentUser = (userId: string) => {
    return authState.user?.id === userId;
  };

  const getUserDisplay = (participant: {
    user_id: string;
    display_name?: string;
    username?: string;
  }) => {
    if (isCurrentUser(participant.user_id)) {
      return authState.user?.display_name || authState.user?.username || "You";
    }
    return (
      participant.display_name ||
      participant.username ||
      participant.user_id.slice(0, 8)
    );
  };

  // Tooltip state for local user
  const [showLocalTooltip, setShowLocalTooltip] = createSignal(false);

  return (
    <Show when={isActiveInChannel()}>
      <div class="ml-6 mt-1 space-y-1">
        {/* Local user (always show first when connected) */}
        <div class="flex items-center gap-2 px-2 py-1 text-xs">
          <User class="w-3 h-3 text-accent-primary" />
          <span class="flex-1 truncate text-accent-primary font-medium">
            {authState.user?.display_name || authState.user?.username || "You"}
          </span>

          <div
            class="relative"
            onMouseEnter={() => setShowLocalTooltip(true)}
            onMouseLeave={() => setShowLocalTooltip(false)}
          >
            <QualityIndicator metrics={getLocalMetrics()} mode="circle" />
            <Show
              when={showLocalTooltip() && typeof getLocalMetrics() === "object"}
            >
              <div class="absolute bottom-full right-0 mb-2 z-50">
                <QualityTooltip
                  metrics={getLocalMetrics() as ConnectionMetrics}
                />
              </div>
            </Show>
          </div>

          <Show when={voiceState.webcamActive}>
            <div title="Camera On">
              <Camera class="w-3 h-3 text-accent-primary" />
            </div>
          </Show>
          <Show when={voiceState.screenSharing}>
            <div title="Screen Sharing">
              <Monitor class="w-3 h-3 text-accent-primary" />
            </div>
          </Show>
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
          {(participant) => {
            const [showTooltip, setShowTooltip] = createSignal(false);
            const metrics = () => getParticipantMetrics(participant.user_id);

            // Convert ParticipantMetrics to ConnectionMetrics format for tooltip
            const metricsForTooltip = (): ConnectionMetrics | null => {
              const m = metrics();
              if (!m) return null;
              return {
                latency: m.latency,
                packetLoss: m.packetLoss,
                jitter: m.jitter,
                quality: m.quality,
                timestamp: Date.now(),
              };
            };

            return (
              <div class="flex items-center gap-2 px-2 py-1 text-xs">
                <User class="w-3 h-3 text-text-secondary" />
                <span class="flex-1 truncate text-text-secondary">
                  {getUserDisplay(participant)}
                </span>

                <div
                  class="relative"
                  onMouseEnter={() => setShowTooltip(true)}
                  onMouseLeave={() => setShowTooltip(false)}
                >
                  <QualityIndicator
                    metrics={metricsForTooltip()}
                    mode="circle"
                  />
                  <Show when={showTooltip() && metricsForTooltip()}>
                    <div class="absolute bottom-full right-0 mb-2 z-50">
                      <QualityTooltip metrics={metricsForTooltip()!} />
                    </div>
                  </Show>
                </div>

                <Show when={participant.webcam_active}>
                  <div title="Camera On">
                    <Camera class="w-3 h-3 text-accent-primary" />
                  </div>
                </Show>
                <Show when={participant.screen_sharing}>
                  <div title="Screen Sharing">
                    <Monitor class="w-3 h-3 text-accent-primary" />
                  </div>
                </Show>
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
            );
          }}
        </For>
      </div>
    </Show>
  );
};

export default VoiceParticipants;
