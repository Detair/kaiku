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
import {
  Phone,
  PhoneOff,
  PhoneIncoming,
  PhoneOutgoing,
  Users,
} from "lucide-solid";
import { callState, joinCall, declineCall, endCall } from "@/stores/call";
import {
  joinDMCall,
  declineDMCall,
  leaveDMCall,
  joinVoice,
  leaveVoice,
} from "@/lib/tauri";

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
      console.error("Failed to leave call:", err);
      // If call not found (404) or conflict (409), just clean up local state
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
      console.error("Failed to cancel call:", err);
      // If call not found (404) or already ended, just clean up local state
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
        return "Call cancelled";
      case "all_declined":
        return "Call declined";
      case "no_answer":
        return "No answer";
      case "last_left":
        return "Call ended";
      default:
        return "Call ended";
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
                  <PhoneIncoming class="w-5 h-5 text-green-400" />
                </div>
                <div>
                  <p class="text-text-primary font-medium">
                    {(call() as { initiatorName: string }).initiatorName} is
                    calling
                  </p>
                  <p class="text-sm text-text-secondary">Incoming voice call</p>
                </div>
              </div>
              <div class="flex items-center gap-2">
                <button
                  type="button"
                  onClick={handleDecline}
                  disabled={isLoading()}
                  class="px-4 py-2 rounded-lg bg-red-500/20 text-red-400 hover:bg-red-500/30 transition-colors flex items-center gap-2 disabled:opacity-50"
                >
                  <PhoneOff class="w-4 h-4" />
                  <span>Decline</span>
                </button>
                <button
                  type="button"
                  onClick={handleAccept}
                  disabled={isLoading()}
                  class="px-4 py-2 rounded-lg bg-green-500/20 text-green-400 hover:bg-green-500/30 transition-colors flex items-center gap-2 disabled:opacity-50"
                >
                  <Phone class="w-4 h-4" />
                  <span>Accept</span>
                </button>
              </div>
            </div>
          </Show>

          {/* Outgoing Call (Ringing) */}
          <Show when={call().status === "outgoing_ringing"}>
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-3">
                <div class="w-10 h-10 rounded-full bg-accent-primary/20 flex items-center justify-center">
                  <PhoneOutgoing class="w-5 h-5 text-accent-primary animate-pulse" />
                </div>
                <div>
                  <p class="text-text-primary font-medium">Calling...</p>
                  <p class="text-sm text-text-secondary">Waiting for answer</p>
                </div>
              </div>
              <button
                type="button"
                onClick={handleCancel}
                disabled={isLoading()}
                class="px-4 py-2 rounded-lg bg-red-500/20 text-red-400 hover:bg-red-500/30 transition-colors flex items-center gap-2 disabled:opacity-50"
              >
                <PhoneOff class="w-4 h-4" />
                <span>Cancel</span>
              </button>
            </div>
          </Show>

          {/* Connecting */}
          <Show when={call().status === "connecting"}>
            <div class="flex items-center gap-3">
              <div class="w-10 h-10 rounded-full bg-accent-primary/20 flex items-center justify-center">
                <Phone class="w-5 h-5 text-accent-primary animate-pulse" />
              </div>
              <div>
                <p class="text-text-primary font-medium">Connecting...</p>
                <p class="text-sm text-text-secondary">
                  Setting up voice connection
                </p>
              </div>
            </div>
          </Show>

          {/* Connected */}
          <Show when={call().status === "connected"}>
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-3">
                <div class="w-10 h-10 rounded-full bg-green-500/20 flex items-center justify-center">
                  <Phone class="w-5 h-5 text-green-400" />
                </div>
                <div>
                  <p class="text-text-primary font-medium">In call</p>
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
                class="px-4 py-2 rounded-lg bg-red-500/20 text-red-400 hover:bg-red-500/30 transition-colors flex items-center gap-2 disabled:opacity-50"
              >
                <PhoneOff class="w-4 h-4" />
                <span>Leave</span>
              </button>
            </div>
          </Show>

          {/* Reconnecting */}
          <Show when={call().status === "reconnecting"}>
            <div class="flex items-center gap-3">
              <div class="w-10 h-10 rounded-full bg-yellow-500/20 flex items-center justify-center">
                <Phone class="w-5 h-5 text-yellow-400 animate-pulse" />
              </div>
              <div>
                <p class="text-text-primary font-medium">Reconnecting...</p>
                <p class="text-sm text-text-secondary">
                  Connection lost, retrying (
                  {(call() as { countdown: number }).countdown}s)
                </p>
              </div>
            </div>
          </Show>

          {/* Ended */}
          <Show when={call().status === "ended"}>
            <div class="flex items-center gap-3">
              <div class="w-10 h-10 rounded-full bg-surface-layer1 flex items-center justify-center">
                <PhoneOff class="w-5 h-5 text-text-secondary" />
              </div>
              <div>
                <p class="text-text-primary font-medium">
                  {endReasonText((call() as { reason: string }).reason)}
                </p>
                <Show when={(call() as { duration?: number }).duration}>
                  <p class="text-sm text-text-secondary">
                    Duration:{" "}
                    {formatDuration((call() as { duration: number }).duration)}
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
