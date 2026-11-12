/**
 * CallBanner Component
 *
 * Shows the current call state at the top of a DM conversation.
 * Displays different UI based on call status: incoming, outgoing, connecting, connected, ended.
 */

import {
  Component,
  Show,
  createSignal,
  createEffect,
  onCleanup,
} from "solid-js";
import { Users } from "lucide-solid";
import {
  callState,
  acceptCall,
  declineCall,
  endCall,
  leaveCallVoice,
} from "@/stores/call";
import { declineDMCall, leaveDMCall } from "@/lib/tauri";
import { t } from "@/i18n";

import { endReasonText, isCallAlreadyGoneError } from "@/lib/callEnd";

interface CallBannerProps {
  channelId: string;
}

const CallBanner: Component<CallBannerProps> = (props) => {
  const [isLoading, setIsLoading] = createSignal(false);
  const [callDuration, setCallDuration] = createSignal(0);

  // Duration timer for connected calls
  createEffect(() => {
    const current = callState.currentCall;
    if (
      current.status === "connected" &&
      current.channelId === props.channelId
    ) {
      const interval = setInterval(() => {
        const elapsed = Math.floor((Date.now() - current.startedAt) / 1000);
        setCallDuration(elapsed);
      }, 1000);
      onCleanup(() => clearInterval(interval));
    }
  });

  const formatDuration = (seconds: number): string => {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, "0")}`;
  };

  const handleAccept = async () => {
    setIsLoading(true);
    try {
      // Accept, server join and native voice under this accept's own
      // session token (the token never leaves the store).
      await acceptCall(props.channelId);
    } catch (err) {
      console.error("Failed to join call:", err);
      // Reset state on error
      declineCall(props.channelId);
    } finally {
      setIsLoading(false);
    }
  };

  const handleDecline = async () => {
    setIsLoading(true);
    try {
      await declineDMCall(props.channelId);
      declineCall(props.channelId);
    } catch (err) {
      console.error("Failed to decline call:", err);
      // If call not found (404) or already ended, just clean up local state
      declineCall(props.channelId);
    } finally {
      setIsLoading(false);
    }
  };

  /**
   * Hang up or cancel. The call's voice session is over from the first
   * line on (no join and no recovery attempt runs for it any more); the
   * voice owner ends it while the server is told.
   */
  const hangUp = async (reason: "last_left" | "cancelled") => {
    const voiceEnded = leaveCallVoice();
    setIsLoading(true);
    try {
      await voiceEnded;
      await leaveDMCall(props.channelId);
    } catch (err) {
      if (!isCallAlreadyGoneError(err)) {
        console.error("Failed to leave call:", err);
      }
    } finally {
      endCall(props.channelId, reason);
      setIsLoading(false);
    }
  };

  const handleLeave = () => hangUp("last_left");

  const handleCancel = () => hangUp("cancelled");

  const currentCall = () => {
    const call = callState.currentCall;
    if (call.status === "idle") return null;
    if ("channelId" in call && call.channelId === props.channelId) {
      return call;
    }
    return null;
  };

  return (
    <Show when={currentCall()}>
      {(call) => (
        <div
          class="px-4 py-3 border-b border-white/5 bg-surface-layer2"
          role="status"
          aria-live="polite"
        >
          {/* Incoming Call */}
          <Show when={call().status === "incoming_ringing"}>
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-3">
                <div class="w-10 h-10 rounded-full bg-green-500/20 flex items-center justify-center animate-pulse">
                  <div style={{ "background-image": "var(--icon-pickup)" }} class="w-5 h-5 bg-contain bg-center bg-no-repeat" />
                </div>
                <div>
                  <p class="text-text-primary font-medium">
                    {t("voice.call.isCalling", {
                      name: (call() as { initiatorName: string }).initiatorName,
                    })}
                  </p>
                  <p class="text-sm text-text-secondary">{t("voice.call.incoming")}</p>
                </div>
              </div>
              <div class="flex items-center gap-2">
                <button
                  type="button"
                  onClick={handleDecline}
                  disabled={isLoading()}
                  class="p-2 rounded-lg bg-red-500/20 hover:bg-red-500/30 transition-colors flex items-center justify-center w-10 h-10 state-disabled"
                  title={t("voice.call.decline")}
                >
                  <div style={{ "background-image": "var(--icon-leave)" }} class="w-5 h-5 bg-contain bg-center bg-no-repeat opacity-90" />
                </button>
                <button
                  type="button"
                  onClick={handleAccept}
                  disabled={isLoading()}
                  class="p-2 rounded-lg bg-green-500/20 hover:bg-green-500/30 transition-colors flex items-center justify-center w-10 h-10 state-disabled"
                  title={t("voice.call.accept")}
                >
                  <div style={{ "background-image": "var(--icon-pickup)" }} class="w-5 h-5 bg-contain bg-center bg-no-repeat opacity-90" />
                </button>
              </div>
            </div>
          </Show>

          {/* Outgoing Call (Ringing) */}
          <Show when={call().status === "outgoing_ringing"}>
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-3">
                <div class="w-10 h-10 rounded-full bg-accent-primary/20 flex items-center justify-center">
                  <div style={{ "background-image": "var(--icon-pickup)" }} class="w-5 h-5 bg-contain bg-center bg-no-repeat animate-pulse" />
                </div>
                <div>
                  <p class="text-text-primary font-medium">{t("voice.call.calling")}</p>
                  <p class="text-sm text-text-secondary">{t("voice.call.waiting")}</p>
                </div>
              </div>
              <button
                type="button"
                onClick={handleCancel}
                disabled={isLoading()}
                class="p-2 rounded-lg bg-red-500/20 hover:bg-red-500/30 transition-colors flex items-center justify-center w-10 h-10 state-disabled"
                title={t("common.cancel")}
              >
                <div style={{ "background-image": "var(--icon-leave)" }} class="w-5 h-5 bg-contain bg-center bg-no-repeat opacity-90" />
              </button>
            </div>
          </Show>

          {/* Connecting */}
          <Show when={call().status === "connecting"}>
            <div class="flex items-center gap-3">
              <div class="w-10 h-10 rounded-full bg-accent-primary/20 flex items-center justify-center">
                <div style={{ "background-image": "var(--icon-pickup)" }} class="w-5 h-5 bg-contain bg-center bg-no-repeat animate-pulse" />
              </div>
              <div>
                <p class="text-text-primary font-medium">{t("voice.status.connecting")}</p>
                <p class="text-sm text-text-secondary">
                  {t("voice.call.settingUp")}
                </p>
              </div>
            </div>
          </Show>

          {/* Connected */}
          <Show when={call().status === "connected"}>
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-3">
                <div class="w-10 h-10 rounded-full bg-green-500/20 flex items-center justify-center">
                  <div style={{ "background-image": "var(--icon-pickup)" }} class="w-5 h-5 bg-contain bg-center bg-no-repeat" />
                </div>
                <div>
                  <p class="text-text-primary font-medium">{t("voice.call.inCall")}</p>
                  <div class="flex items-center gap-2 text-sm text-text-secondary">
                    <span>{formatDuration(callDuration())}</span>
                    <Show
                      when={
                        (call() as { participants: string[] }).participants
                          .length > 0
                      }
                    >
                      <span class="flex items-center gap-1">
                        <Users class="w-3 h-3" />
                        {
                          (call() as { participants: string[] }).participants
                            .length
                        }
                      </span>
                    </Show>
                  </div>
                </div>
              </div>
              <button
                type="button"
                onClick={handleLeave}
                disabled={isLoading()}
                class="p-2 rounded-lg bg-red-500/20 hover:bg-red-500/30 transition-colors flex items-center justify-center w-10 h-10 state-disabled"
                title={t("voice.call.leave")}
              >
                <div style={{ "background-image": "var(--icon-leave)" }} class="w-5 h-5 bg-contain bg-center bg-no-repeat opacity-90" />
              </button>
            </div>
          </Show>

          {/* Reconnecting */}
          <Show when={call().status === "reconnecting"}>
            <div class="flex items-center gap-3">
              <div class="w-10 h-10 rounded-full bg-yellow-500/20 flex items-center justify-center">
                <div style={{ "background-image": "var(--icon-pickup)" }} class="w-5 h-5 bg-contain bg-center bg-no-repeat animate-pulse" />
              </div>
              <div>
                <p class="text-text-primary font-medium">{t("voice.call.reconnecting")}</p>
                <p class="text-sm text-text-secondary">
                  {t("voice.call.reconnectingHint", {
                    seconds: (call() as { countdown: number }).countdown,
                  })}
                </p>
              </div>
            </div>
          </Show>

          {/* Ended */}
          <Show when={call().status === "ended"}>
            <div class="flex items-center gap-3">
              <div class="w-10 h-10 rounded-full bg-surface-layer1 flex items-center justify-center">
                <div style={{ "background-image": "var(--icon-leave)" }} class="w-5 h-5 bg-contain bg-center bg-no-repeat opacity-50" />
              </div>
              <div>
                <p class="text-text-primary font-medium">
                  {endReasonText((call() as { reason: string }).reason)}
                </p>
                <Show when={(call() as { duration?: number }).duration}>
                  <p class="text-sm text-text-secondary">
                    {t("voice.call.duration", {
                      duration: formatDuration(
                        (call() as { duration: number }).duration,
                      ),
                    })}
                  </p>
                </Show>
              </div>
            </div>
          </Show>
        </div>
      )}
    </Show>
  );
};

export default CallBanner;
