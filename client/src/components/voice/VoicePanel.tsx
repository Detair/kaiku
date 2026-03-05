import {
  Component,
  Show,
  For,
  createSignal,
  onMount,
  onCleanup,
} from "solid-js";
import { PhoneOff, Signal, MonitorUp } from "lucide-solid";
import {
  voiceState,
  leaveVoice,
  getParticipants,
  getLocalMetrics,
} from "@/stores/voice";
import { getChannel } from "@/stores/channels";
import { viewUserShare } from "@/stores/screenShareViewer";
import { formatElapsedTime } from "@/lib/utils";
import { QualityIndicator } from "./QualityIndicator";
import { QualityTooltip } from "./QualityTooltip";
import type { ConnectionMetrics } from "@/lib/webrtc/types";
import VoiceControls from "./VoiceControls";

/**
 * Voice panel shown when connected or connecting to a voice channel.
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

  const isConnected = () => voiceState.state === "connected";
  const isConnecting = () => voiceState.state === "connecting";

  // Elapsed timer — uses the store's authoritative connectedAt timestamp
  const [elapsedTime, setElapsedTime] = createSignal("00:00");
  const [showQualityTooltip, setShowQualityTooltip] = createSignal(false);

  onMount(() => {
    const interval = setInterval(() => {
      const start = voiceState.connectedAt;
      setElapsedTime(start ? formatElapsedTime(start) : "00:00");
    }, 1000);
    onCleanup(() => clearInterval(interval));
  });

  const metrics = (): ConnectionMetrics | "unknown" | null => getLocalMetrics();
  const metricsObj = (): ConnectionMetrics | null => {
    const m = metrics();
    return typeof m === "object" && m !== null ? m : null;
  };

  return (
    <Show when={(isConnected() || isConnecting()) && voiceState.channelId}>
      <div
        data-testid="voice-panel"
        class="bg-surface-base/50 border-t relative transition-all duration-200"
        classList={{
          "border-accent-success/50 shadow-[0_0_12px_rgba(163,190,140,0.3)]":
            voiceState.speaking,
          "border-white/10": !voiceState.speaking,
        }}
      >
        {/* Connection info */}
        <div class="px-3 py-2 flex items-center justify-between">
          <div class="flex items-center gap-2 min-w-0">
            <Signal
              class="w-4 h-4 flex-shrink-0"
              classList={{
                "text-accent-success": isConnected(),
                "text-amber-400 animate-pulse": isConnecting(),
              }}
            />
            <div class="min-w-0">
              <div
                class="text-xs font-semibold"
                classList={{
                  "text-accent-success": isConnected(),
                  "text-amber-400 animate-pulse": isConnecting(),
                }}
              >
                {isConnected() ? "Voice Connected" : "Connecting..."}
              </div>
              <div class="text-xs text-text-secondary truncate flex items-center gap-1.5">
                <span>{channel()?.name || "Voice Channel"}</span>
                <Show when={isConnected()}>
                  <span class="text-text-secondary/50">&middot;</span>
                  <span class="font-mono">{elapsedTime()}</span>
                  <div
                    class="relative"
                    onMouseEnter={() => setShowQualityTooltip(true)}
                    onMouseLeave={() => setShowQualityTooltip(false)}
                  >
                    <QualityIndicator
                      metrics={metrics()}
                      mode="circle"
                      class="cursor-help"
                    />
                    <Show when={showQualityTooltip() && metricsObj()}>
                      <div class="absolute bottom-full left-1/2 -translate-x-1/2 mb-2 z-50">
                        <QualityTooltip metrics={metricsObj()!} />
                      </div>
                    </Show>
                  </div>
                </Show>
              </div>
            </div>
          </div>
          <button
            data-testid="voice-disconnect"
            onClick={handleDisconnect}
            class="p-1.5 text-text-secondary hover:text-accent-danger hover:bg-white/10 rounded transition-colors"
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
                    class={`flex items-center gap-1 px-2 py-1 rounded text-xs ${participant.speaking
                        ? "bg-accent-success/20 text-accent-success border border-accent-success/30 shadow-[0_0_8px_rgba(163,190,140,0.3)]"
                        : "bg-white/5 text-text-secondary border border-transparent"
                      } ${participant.muted ? "opacity-50" : "transition-all duration-200"}`}
                    title={participant.muted ? "Muted" : undefined}
                  >
                    <div class="w-4 h-4 rounded-full bg-accent-primary/20 flex items-center justify-center text-accent-primary">
                      {/* Using first letter as avatar fallback for simplicity */}
                      {participant.user_id.charAt(0).toUpperCase()}
                    </div>
                    <span class="truncate max-w-20">
                      {participant.user_id.slice(0, 8)}
                    </span>
                    {participant.screen_sharing && (
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          viewUserShare(participant.user_id);
                        }}
                        class="p-0.5 hover:bg-accent-success/30 rounded transition-colors"
                        title="View screen share"
                      >
                        <MonitorUp class="w-3 h-3 text-accent-success" />
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
          <div class="px-3 pb-2 border-t border-white/5 pt-2">
            <div class="text-xs text-text-secondary/70 mb-1">Screen Shares</div>
            <For each={voiceState.screenShares}>
              {(share) => (
                <div
                  class="flex items-center gap-2 px-2 py-1.5 rounded bg-white/5 hover:bg-white/10 cursor-pointer transition-colors"
                  onClick={() => viewUserShare(share.user_id)}
                >
                  <MonitorUp class="w-4 h-4 text-accent-success" />
                  <div class="flex-1 min-w-0">
                    <div class="text-sm text-text-primary font-medium truncate">
                      {share.username || share.user_id.slice(0, 8)}
                    </div>
                    <div class="text-xs text-text-secondary">
                      {share.quality} •{" "}
                      {share.has_audio ? "With audio" : "No audio"}
                    </div>
                  </div>
                </div>
              )}
            </For>
          </div>
        </Show>

        {/* Voice controls - only when fully connected */}
        <Show when={isConnected()}>
          <VoiceControls />
        </Show>
      </div>
    </Show>
  );
};

export default VoicePanel;
