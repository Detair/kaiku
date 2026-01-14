/**
 * Call Store
 *
 * Manages DM voice call state including incoming calls, active calls, and call UI.
 */

import { createStore, produce } from "solid-js/store";

// Call state types matching backend
export type EndReason = "cancelled" | "all_declined" | "no_answer" | "last_left";

export type CallState =
  | { status: "idle" }
  | { status: "outgoing_ringing"; channelId: string; startedAt: number }
  | { status: "incoming_ringing"; channelId: string; initiator: string; initiatorName: string }
  | { status: "connecting"; channelId: string }
  | { status: "connected"; channelId: string; participants: string[]; startedAt: number }
  | { status: "reconnecting"; channelId: string; countdown: number }
  | { status: "ended"; channelId: string; reason: EndReason; duration?: number };

// Store state
interface CallStoreState {
  currentCall: CallState;
  activeCallsByChannel: Record<
    string,
    { initiator: string; initiatorName: string; participants: string[] }
  >;
}

const [callState, setCallState] = createStore<CallStoreState>({
  currentCall: { status: "idle" },
  activeCallsByChannel: {},
});

// Actions

/**
 * Start an outgoing call.
 */
export function startCall(channelId: string): void {
  setCallState("currentCall", {
    status: "outgoing_ringing",
    channelId,
    startedAt: Date.now(),
  });
}

/**
 * Receive an incoming call notification.
 */
export function receiveIncomingCall(
  channelId: string,
  initiator: string,
  initiatorName: string
): void {
  // Only update if we're idle (don't interrupt an existing call)
  if (callState.currentCall.status === "idle") {
    setCallState("currentCall", {
      status: "incoming_ringing",
      channelId,
      initiator,
      initiatorName,
    });
  }
  // Always track in activeCallsByChannel for sidebar indicator
  setCallState("activeCallsByChannel", channelId, {
    initiator,
    initiatorName,
    participants: [initiator],
  });
}

/**
 * Transition to connecting state when joining a call.
 */
export function joinCall(channelId: string): void {
  setCallState("currentCall", {
    status: "connecting",
    channelId,
  });
}

/**
 * Call is now connected.
 */
export function callConnected(channelId: string, participants: string[]): void {
  setCallState("currentCall", {
    status: "connected",
    channelId,
    participants,
    startedAt: Date.now(),
  });
}

/**
 * A participant joined the call.
 */
export function participantJoined(channelId: string, userId: string): void {
  const current = callState.currentCall;
  if (current.status === "connected" && current.channelId === channelId) {
    setCallState(
      produce((state) => {
        if (state.currentCall.status === "connected") {
          state.currentCall.participants = [...state.currentCall.participants, userId];
        }
      })
    );
  }
  // Update active calls
  setCallState(
    produce((state) => {
      if (state.activeCallsByChannel[channelId]) {
        state.activeCallsByChannel[channelId].participants.push(userId);
      }
    })
  );
}

/**
 * A participant left the call.
 */
export function participantLeft(channelId: string, userId: string): void {
  const current = callState.currentCall;
  if (current.status === "connected" && current.channelId === channelId) {
    setCallState(
      produce((state) => {
        if (state.currentCall.status === "connected") {
          state.currentCall.participants = state.currentCall.participants.filter(
            (id) => id !== userId
          );
        }
      })
    );
  }
  // Update active calls
  setCallState(
    produce((state) => {
      if (state.activeCallsByChannel[channelId]) {
        state.activeCallsByChannel[channelId].participants = state.activeCallsByChannel[
          channelId
        ].participants.filter((id) => id !== userId);
      }
    })
  );
}

/**
 * Decline an incoming call.
 */
export function declineCall(channelId: string): void {
  const current = callState.currentCall;
  if (current.status === "incoming_ringing" && current.channelId === channelId) {
    setCallState("currentCall", { status: "idle" });
  }
}

/**
 * End the current call (local action).
 */
export function endCall(channelId: string, reason: EndReason, duration?: number): void {
  setCallState("currentCall", {
    status: "ended",
    channelId,
    reason,
    duration,
  });
  // Remove from active calls
  setCallState(
    produce((state) => {
      delete state.activeCallsByChannel[channelId];
    })
  );

  // Reset to idle after showing ended state briefly
  setTimeout(() => {
    const current = callState.currentCall;
    if (current.status === "ended" && current.channelId === channelId) {
      setCallState("currentCall", { status: "idle" });
    }
  }, 3000);
}

/**
 * Call ended externally (by server event).
 */
export function callEndedExternally(
  channelId: string,
  reason: EndReason,
  duration?: number
): void {
  // Remove from active calls
  setCallState(
    produce((state) => {
      delete state.activeCallsByChannel[channelId];
    })
  );

  // Update current call if it's the one that ended
  const current = callState.currentCall;
  if (current.status !== "idle" && "channelId" in current && current.channelId === channelId) {
    endCall(channelId, reason, duration);
  }
}

// Selectors

/**
 * Get the current call state.
 */
export function getCurrentCall(): CallState {
  return callState.currentCall;
}

/**
 * Get active call info for a specific channel.
 */
export function getActiveCallForChannel(
  channelId: string
): { initiator: string; initiatorName: string; participants: string[] } | undefined {
  return callState.activeCallsByChannel[channelId];
}

/**
 * Check if currently in any call.
 */
export function isInCall(): boolean {
  const status = callState.currentCall.status;
  return status !== "idle" && status !== "ended";
}

/**
 * Check if in a call for a specific channel.
 */
export function isInCallForChannel(channelId: string): boolean {
  const current = callState.currentCall;
  return (
    current.status !== "idle" &&
    current.status !== "ended" &&
    "channelId" in current &&
    current.channelId === channelId
  );
}

/**
 * Check if there's an active call in a channel (for sidebar indicator).
 */
export function hasActiveCallInChannel(channelId: string): boolean {
  return !!callState.activeCallsByChannel[channelId];
}

// Export store for reactive access
export { callState, setCallState };
