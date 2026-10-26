/**
 * Call Store
 *
 * Manages DM voice call state including incoming calls, active calls, and call UI.
 */

import { createStore, produce } from "solid-js/store";
import { startRinging, stopRinging } from "@/lib/sound/ring";

// Constants
const CALL_ENDED_DISPLAY_MS = 3000;

// Module-level timeout tracking for cleanup
let callEndedTimeoutId: ReturnType<typeof setTimeout> | null = null;

// Call state types matching backend
export type EndReason =
  | "cancelled"
  | "all_declined"
  | "no_answer"
  | "last_left";

export type CallState =
  | { status: "idle" }
  | { status: "outgoing_ringing"; channelId: string; startedAt: number }
  | {
      status: "incoming_ringing";
      channelId: string;
      initiator: string;
      initiatorName: string;
    }
  | { status: "connecting"; channelId: string }
  | {
      status: "connected";
      channelId: string;
      participants: string[];
      startedAt: number;
    }
  | { status: "reconnecting"; channelId: string; countdown: number }
  | {
      status: "ended";
      channelId: string;
      reason: EndReason;
      duration?: number;
    };

// Store state
interface CallStoreState {
  currentCall: CallState;
  activeCallsByChannel: Record<
    string,
    { initiator: string; initiatorName: string; participants: string[] }
  >;
  /** Server call_id of the current call once known (from incoming_call/call_started). */
  callId: string | null;
}

const [callState, setCallState] = createStore<CallStoreState>({
  currentCall: { status: "idle" },
  activeCallsByChannel: {},
  callId: null,
});

/** Remember the server's call_id for the current call. */
export function setCurrentCallId(callId: string | undefined): void {
  if (callId) setCallState("callId", callId);
}

/**
 * Does a call event belong to the call this client is in? Same channel, and
 * the same call_id where both sides know one.
 */
export function callEventMatches(channelId: string, callId?: string): boolean {
  const current = callState.currentCall;
  if (current.status === "idle" || !("channelId" in current)) return false;
  if (current.channelId !== channelId) return false;
  if (callId && callState.callId && callId !== callState.callId) return false;
  return true;
}

/**
 * Starting an outgoing call failed. Return to idle (not "ended") so the other
 * side's incoming call can still be shown (glare); leave an incoming call that
 * already replaced our outgoing state untouched.
 */
export function abortOutgoingCall(
  channelId: string,
  why: "conflict" | "failed",
): void {
  const current = callState.currentCall;
  const stillOurs =
    (current.status === "outgoing_ringing" || current.status === "connecting") &&
    current.channelId === channelId;
  if (!stillOurs) return;
  if (why === "conflict") {
    stopRinging();
    setCallState({ currentCall: { status: "idle" }, callId: null });
  } else {
    endCall(channelId, "cancelled");
  }
}

// Actions

/**
 * Start an outgoing call.
 */
export function startCall(channelId: string): void {
  stopRinging();
  setCallState("callId", null);
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
  initiatorName: string,
  callId?: string,
): void {
  // Glare: the other side called this DM while ours was still ringing, so
  // theirs wins (the server rejected or will reject our start).
  const cur = callState.currentCall;
  const glare =
    cur.status === "outgoing_ringing" && cur.channelId === channelId;
  // Only update if we're idle (don't interrupt an existing call)
  if (cur.status === "idle" || glare) {
    setCallState("callId", callId ?? null);
    setCallState("currentCall", {
      status: "incoming_ringing",
      channelId,
      initiator,
      initiatorName,
    });
    // Start ringing for incoming call
    startRinging();
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
  // Stop ringing when answering the call
  stopRinging();
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
 * First participant joined: move our ringing/connecting call to connected.
 * Ignored for any other channel or call_id, and once already connected.
 */
export function connectOnFirstJoin(
  channelId: string,
  userId: string,
  callId?: string,
): void {
  if (!callEventMatches(channelId, callId)) return;
  const status = callState.currentCall.status;
  if (status === "outgoing_ringing" || status === "connecting") {
    callConnected(channelId, [userId]);
  }
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
          // Prevent duplicate participants
          if (!state.currentCall.participants.includes(userId)) {
            state.currentCall.participants = [
              ...state.currentCall.participants,
              userId,
            ];
          }
        }
      }),
    );
  }
  // Update active calls
  setCallState(
    produce((state) => {
      if (state.activeCallsByChannel[channelId]) {
        // Prevent duplicate participants
        if (
          !state.activeCallsByChannel[channelId].participants.includes(userId)
        ) {
          state.activeCallsByChannel[channelId].participants.push(userId);
        }
      }
    }),
  );
}

/**
 * A participant left the call.
 */
export function participantLeft(channelId: string, userId: string): void {
  const current = callState.currentCall;
  if (current.status === "connected" && current.channelId === channelId) {
    const remaining = current.participants.filter((id) => id !== userId);
    if (remaining.length === 0) {
      endCall(channelId, "last_left");
      return;
    }
    setCallState(
      produce((state) => {
        if (state.currentCall.status === "connected") {
          state.currentCall.participants =
            state.currentCall.participants.filter((id) => id !== userId);
        }
      }),
    );
  } else if (
    (current.status === "outgoing_ringing" || current.status === "connecting") &&
    current.channelId === channelId
  ) {
    endCall(channelId, "last_left");
    return;
  }
  // Update active calls
  setCallState(
    produce((state) => {
      if (state.activeCallsByChannel[channelId]) {
        state.activeCallsByChannel[channelId].participants =
          state.activeCallsByChannel[channelId].participants.filter(
            (id) => id !== userId,
          );
        if (state.activeCallsByChannel[channelId].participants.length === 0) {
          delete state.activeCallsByChannel[channelId];
        }
      }
    }),
  );
}

/**
 * Decline an incoming call.
 */
export function declineCall(channelId: string): void {
  const current = callState.currentCall;
  if (
    current.status === "incoming_ringing" &&
    current.channelId === channelId
  ) {
    // Stop ringing when declining the call
    stopRinging();
    setCallState("currentCall", { status: "idle" });
  }
}

/**
 * End the current call (local action).
 */
export function endCall(
  channelId: string,
  reason: EndReason,
  duration?: number,
): void {
  // Clear any existing timeout to prevent race conditions
  if (callEndedTimeoutId) {
    clearTimeout(callEndedTimeoutId);
    callEndedTimeoutId = null;
  }

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
    }),
  );

  // Reset to idle after showing ended state briefly
  callEndedTimeoutId = setTimeout(() => {
    if (callState.currentCall.status === "ended") {
      setCallState("currentCall", { status: "idle" });
      setCallState("callId", null);
    }
    callEndedTimeoutId = null;
  }, CALL_ENDED_DISPLAY_MS);
}

/**
 * Call ended externally (by server event).
 */
export function callEndedExternally(
  channelId: string,
  reason: EndReason,
  duration?: number,
  callId?: string,
): void {
  // An end for a different call (e.g. the previous one in this DM) is stale.
  if (callId && callState.callId && callId !== callState.callId) return;
  // Remove from active calls
  setCallState(
    produce((state) => {
      delete state.activeCallsByChannel[channelId];
    }),
  );

  // Update current call if it's the one that ended
  const current = callState.currentCall;
  if (
    current.status !== "idle" &&
    "channelId" in current &&
    current.channelId === channelId
  ) {
    // Stop ringing if we were being called
    if (current.status === "incoming_ringing") {
      stopRinging();
    }
    endCall(channelId, reason, duration);
  }
}

// Selectors
// NOTE: These selectors access Solid.js store state. For reactivity to work,
// they MUST be called inside a reactive context (createEffect, createMemo, JSX).
// When called inside reactive contexts, Solid.js automatically tracks store reads.

/**
 * Get the current call state.
 *
 * @remarks Must be called inside a reactive context (createEffect, createMemo, JSX)
 * for reactivity to work. Store reads are automatically tracked by Solid.js.
 */
export function getCurrentCall(): CallState {
  return callState.currentCall;
}

/**
 * Get active call info for a specific channel.
 *
 * @remarks Must be called inside a reactive context (createEffect, createMemo, JSX)
 * for reactivity to work. Store reads are automatically tracked by Solid.js.
 */
export function getActiveCallForChannel(
  channelId: string,
):
  | { initiator: string; initiatorName: string; participants: string[] }
  | undefined {
  return callState.activeCallsByChannel[channelId];
}

/**
 * Check if currently in any call.
 *
 * @remarks Must be called inside a reactive context (createEffect, createMemo, JSX)
 * for reactivity to work. Store reads are automatically tracked by Solid.js.
 */
export function isInCall(): boolean {
  const status = callState.currentCall.status;
  return status !== "idle" && status !== "ended";
}

/**
 * Check if in a call for a specific channel.
 *
 * @remarks Must be called inside a reactive context (createEffect, createMemo, JSX)
 * for reactivity to work. Store reads are automatically tracked by Solid.js.
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
 *
 * @remarks Must be called inside a reactive context (createEffect, createMemo, JSX)
 * for reactivity to work. Store reads are automatically tracked by Solid.js.
 */
export function hasActiveCallInChannel(channelId: string): boolean {
  return !!callState.activeCallsByChannel[channelId];
}

// Export store for reactive access
export { callState, setCallState };
