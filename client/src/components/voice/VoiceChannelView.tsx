/**
 * VoiceChannelView — Main content area for voice channels
 *
 * Shows a participant grid with speaking indicators, screen share
 * buttons, and voice controls. When not connected, shows a preview
 * with participant count and a "Join Voice" button.
 */

import { Component, Show, createMemo } from "solid-js";
import { Users } from "lucide-solid";
import { voiceState, getParticipants, joinVoice } from "@/stores/voice";
import VoiceControls from "./VoiceControls";
import VoiceTileGrid from "./VoiceTileGrid";
import { showToast } from "@/components/ui/Toast";
import { t, plural } from "@/i18n";
import { errorText } from "@/i18n/errors";
import { requestSettings } from "@/stores/settingsNav";

interface VoiceChannelViewProps {
  channelId: string;
  channelName: string;
}

const VoiceChannelView: Component<VoiceChannelViewProps> = (props) => {
  // Reconnecting is still this voice session (R2): grid, controls and no
  // Join button for this channel; VoicePanel keeps Leave.
  const isConnected = () =>
    (voiceState.state === "connected" || voiceState.state === "reconnecting") &&
    voiceState.channelId === props.channelId;
  const isReconnecting = () =>
    voiceState.state === "reconnecting" && voiceState.channelId === props.channelId;
  const isConnecting = () => voiceState.state === "connecting";
  const participants = createMemo(() => getParticipants());
  const screenShares = () => voiceState.screenShares || [];

  const handleJoin = async () => {
    try {
      await joinVoice(props.channelId);
    } catch (err) {
      // joinVoice records the adapter's error type before throwing.
      const micProblem =
        voiceState.error?.type === "permission_denied" ||
        voiceState.error?.type === "device_not_found";
      showToast({
        type: "error",
        title: t("voice.channel.joinFailed"),
        message: micProblem
          ? t("voice.join.micUnavailable")
          : errorText(err, "voice.channel.joinFailed"),
        action: micProblem
          ? {
              label: t("voice.join.openAudioSettings"),
              onClick: () => requestSettings("audio"),
            }
          : undefined,
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
                fallback={t("voice.channel.empty")}
              >
                {plural(participants().length, {
                  one: "voice.channel.connected.one",
                  other: "voice.channel.connected.other",
                })}
              </Show>
            </p>
          </div>
          <button
            onClick={handleJoin}
            disabled={isConnecting()}
            class="px-8 py-3 bg-accent-primary text-on-accent rounded-xl font-semibold text-lg hover:opacity-90 transition-opacity state-disabled"
          >
            {isConnecting() ? t("voice.status.connecting") : t("voice.channel.join")}
          </button>
        </div>
      </Show>

      {/* Connected — tile grid + controls */}
      <Show when={isConnected()}>
        <Show when={isReconnecting()}>
          <p
            data-testid="voice-channel-reconnecting"
            role="status"
            class="px-4 py-2 text-sm text-text-primary bg-surface-layer2 border-b border-white/10"
          >
            {t("voice.channel.reconnecting")}
          </p>
        </Show>
        <div class="flex-1 min-h-0">
          <VoiceTileGrid
            participants={participants()}
            screenShares={screenShares()}
          />
        </div>
        <VoiceControls />
      </Show>
    </div>
  );
};

export default VoiceChannelView;
