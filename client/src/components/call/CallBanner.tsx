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
import { callState, joinCall, declineCall, endCall } from "@/stores/call";
import {
  joinDMCall,
  declineDMCall,
  leaveDMCall,
  joinVoice,
  leaveVoice,
} from "@/lib/tauri";
import { t } from "@/i18n";

interface CallBannerProps {
  channelId: string;
}

const CallBanner: Component<CallBannerProps> = (props) => {
  const [isLoading, setIsLoading] = createSignal(false);
  const [callDuration, setCallDuration] = createSignal(0);

  const isCallAlreadyGoneError = (err: unknown): boolean => {
    if (!(err instanceof Error)) return false;
    return /call not found|call_not_found/i.test(err.message);
  };

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
      joinCall(props.channelId);
      await joinDMCall(props.channelId);
      // Start voice connection after signaling join
      await joinVoice(props.channelId);
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

  const handleLeave = async () => {
    setIsLoading(true);
    try {
      // Stop voice connection first
      await leaveVoice();
      await leaveDMCall(props.channelId);
      endCall(props.channelId, "last_left");
    } catch (err) {
      if (!isCallAlreadyGoneError(err)) {
        console.error("Failed to leave call:", err);
      }
      await leaveVoice().catch(() => {}); // Best effort cleanup
      endCall(props.channelId, "last_left");
    } finally {
      setIsLoading(false);
    }
  };

  const handleCancel = async () => {
    setIsLoading(true);
    try {
      // Stop voice connection first (initiator may have started voice)
      await leaveVoice();
      await leaveDMCall(props.channelId);
      endCall(props.channelId, "cancelled");
    } catch (err) {
      if (!isCallAlreadyGoneError(err)) {
        console.error("Failed to cancel call:", err);
      }
      await leaveVoice().catch(() => {}); // Best effort cleanup
      endCall(props.channelId, "cancelled");
    } finally {
      setIsLoading(false);
    }
  };

  const currentCall = () => {
    const call = callState.currentCall;
    if (call.status === "idle") return null;
    if ("channelId" in call && call.channelId === props.channelId) {
      return call;
    }
    return null;
  };

  const endReasonText = (reason: string): string => {
    switch (reason) {
      case "cancelled":
        return t("voice.call.ended.cancelled");
      case "all_declined":
        return t("voice.call.ended.declined");
      case "no_answer":
        return t("voice.call.ended.noAnswer");
      case "last_left":
        return t("voice.call.ended.default");
      default:
        return t("voice.call.ended.default");
    }
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
                  class="p-2 rounded-lg bg-red-500/20 hover:bg-red-500/30 transition-colors flex items-center justify-center w-10 h-10 disabled:opacity-50"
                  title={t("voice.call.decline")}
                >
                  <div style={{ "background-image": "var(--icon-leave)" }} class="w-5 h-5 bg-contain bg-center bg-no-repeat opacity-90" />
                </button>
                <button
                  type="button"
                  onClick={handleAccept}
                  disabled={isLoading()}
                  class="p-2 rounded-lg bg-green-500/20 hover:bg-green-500/30 transition-colors flex items-center justify-center w-10 h-10 disabled:opacity-50"
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
                class="p-2 rounded-lg bg-red-500/20 hover:bg-red-500/30 transition-colors flex items-center justify-center w-10 h-10 disabled:opacity-50"
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
                class="p-2 rounded-lg bg-red-500/20 hover:bg-red-500/30 transition-colors flex items-center justify-center w-10 h-10 disabled:opacity-50"
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
