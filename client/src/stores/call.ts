/**
 * Call Store
 *
 * Manages DM voice call state including incoming calls, active calls, and call UI.
 */

import { createStore, produce } from "solid-js/store";
import { startRinging, stopRinging } from "@/lib/sound/ring";
import { showToast } from "@/components/ui/Toast";
import { t } from "@/i18n";
import type { CallStateResponse } from "@/lib/types";
import { isTauri } from "@/lib/tauri/common";
import {
  bumpVoiceEpoch,
  captureCallJoinEpoch,
  isVoiceEpochStale,
} from "@/lib/webrtc/voiceSession";
import {
  VoiceRecovery,
  RECOVERY_ATTEMPT_TIMEOUT_MS,
  type RecoverySession,
} from "./voiceRecovery";

// Constants
const CALL_ENDED_DISPLAY_MS = 3000;
/**
 * C33: a ring ends locally this long after it started, whatever the server
 * said (the server's own ring timeout is 90 s). An incoming one is missed.
 */
export const RING_CAP_MS = 100_000;

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
  /** Server call_id per DM channel, once known (HTTP response or events). */
  callIds: Record<string, string>;
  /** Recently ended call_ids per DM channel; their late events are ignored. */
  endedCallIds: Record<string, string[]>;
}

const [callState, setCallState] = createStore<CallStoreState>({
  currentCall: { status: "idle" },
  activeCallsByChannel: {},
  callIds: {},
  endedCallIds: {},
});

const MAX_ENDED_IDS = 5;
const NIL_UUID = "00000000-0000-0000-0000-000000000000";

/** Older servers send the nil UUID for "no call id": treat it as unknown. */
function knownCallId(callId: string | undefined | null): string | undefined {
  return callId && callId !== NIL_UUID ? callId : undefined;
}

/** Remember the server's call_id for a DM (ignored if that call already ended). */
export function setChannelCallId(
  channelId: string,
  rawCallId: string | undefined,
): void {
  const callId = knownCallId(rawCallId);
  if (!callId || isEndedCallId(channelId, callId)) return;
  const known = callState.callIds[channelId];
  if (known && known !== callId) return;
  setCallState("callIds", channelId, callId);
}

function isEndedCallId(channelId: string, callId: string): boolean {
  return (callState.endedCallIds[channelId] ?? []).includes(callId);
}

/** Move a channel's tracked call_id (and `extra`) into the ended set. */
function rememberEnded(channelId: string, extra?: string): void {
  const ids = [callState.callIds[channelId], extra].filter(
    (id): id is string => !!id,
  );
  setCallState(
    produce((state) => {
      const ended = state.endedCallIds[channelId] ?? [];
      for (const id of ids) if (!ended.includes(id)) ended.push(id);
      state.endedCallIds[channelId] = ended.slice(-MAX_ENDED_IDS);
      delete state.callIds[channelId];
    }),
  );
}

/**
 * Does an event with this call_id belong to a different (older) call in the
 * channel? Unknown ids are applied unless they are a recently ended call.
 */
export function isStaleCallEvent(channelId: string, rawCallId?: string): boolean {
  const callId = knownCallId(rawCallId);
  if (!callId) return false;
  if (isEndedCallId(channelId, callId)) return true;
  const known = callState.callIds[channelId];
  return !!known && known !== callId;
}

/**
 * Does a call event belong to the call this client is in? Same channel and
 * not a stale call_id.
 */
export function callEventMatches(channelId: string, callId?: string): boolean {
  const current = callState.currentCall;
  if (current.status === "idle" || !("channelId" in current)) return false;
  if (current.channelId !== channelId) return false;
  return !isStaleCallEvent(channelId, callId);
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
    setCallState("currentCall", { status: "idle" });
    setCallState(
      produce((state) => {
        delete state.callIds[channelId];
      }),
    );
  } else {
    endCall(channelId, "cancelled");
  }
}

// Ring cap and missed calls (C33)

let ringCapTimer: ReturnType<typeof setTimeout> | null = null;
let ringToken = 0;

/** (Re)start the local ring cap for the ring that just began. */
function armRingCap(channelId: string): void {
  if (ringCapTimer) clearTimeout(ringCapTimer);
  const token = ++ringToken;
  ringCapTimer = setTimeout(() => {
    ringCapTimer = null;
    if (token === ringToken) ringCapReached(channelId);
  }, RING_CAP_MS);
}

function ringCapReached(channelId: string): void {
  const current = callState.currentCall;
  if (!("channelId" in current) || current.channelId !== channelId) return;
  if (current.status === "incoming_ringing") {
    console.info("[Call] Ring cap reached, call missed:", channelId);
    endRingAsMissed(channelId, current.initiatorName);
  } else if (current.status === "outgoing_ringing") {
    console.info("[Call] Ring cap reached, nobody answered:", channelId);
    endCall(channelId, "no_answer");
    void dropCallVoice(channelId, { leaveCall: true });
  }
}

/** An incoming ring that ends without an answer here: shown as missed. */
function endRingAsMissed(channelId: string, initiatorName: string): void {
  stopRinging();
  rememberEnded(channelId);
  setCallState("currentCall", { status: "idle" });
  setCallState(
    produce((state) => {
      delete state.activeCallsByChannel[channelId];
    }),
  );
  showToast({
    type: "info",
    title: t("voice.call.missed", { name: initiatorName }),
    duration: 0,
    id: `missed-call-${channelId}`,
  });
}

/**
 * Best effort: leave the DM call's native voice (and the call itself).
 * Browser DM calls carry no voice, so only the call leave applies there.
 */
async function dropCallVoice(
  channelId: string,
  opts: { leaveCall: boolean; silent?: boolean },
): Promise<void> {
  try {
    const tauri = await import("@/lib/tauri");
    await (opts.silent ? tauri.leaveVoiceSilently() : tauri.leaveVoice()).catch(
      (err: unknown) => console.warn("[Call] Leaving call voice failed:", err),
    );
    if (opts.leaveCall) {
      await tauri.leaveDMCall(channelId).catch(() => {
        // Already gone on the server (it ends unanswered rings at 90 s).
      });
    }
  } catch (err) {
    console.warn("[Call] Dropping call voice failed:", err);
  }
}

/** The local call for `channelId` is still this status. */
function stillCall(channelId: string, status: CallState["status"]): boolean {
  const cur = callState.currentCall;
  return cur.status === status && "channelId" in cur && cur.channelId === channelId;
}

/**
 * C33: after every WebSocket re-establishment, ask the server about the DM
 * call this client is ringing for or in, and apply the answer. "No call"
 * clears the local state silently; a vanished incoming ring is missed.
 * Connected calls rejoin their voice only while the server lists this user
 * as a participant (C32).
 */
export async function resyncCallAfterReconnect(
  myUserId: string | undefined,
  fetchState?: (channelId: string) => Promise<CallStateResponse | null>,
): Promise<void> {
  try {
    await resyncCall(myUserId, fetchState);
  } finally {
    // The socket is back. A recovery the resync did not resume (call gone,
    // request failed) keeps waiting and ends at its 60 s cap; without one
    // a later ICE failure must not wait for a socket that is already up.
    if (!callRecovery.running) callRecovery.socketReady();
  }
}

async function resyncCall(
  myUserId: string | undefined,
  fetchState?: (channelId: string) => Promise<CallStateResponse | null>,
): Promise<void> {
  const before = callState.currentCall;
  if (
    before.status !== "incoming_ringing" &&
    before.status !== "outgoing_ringing" &&
    before.status !== "connected"
  ) {
    return;
  }
  const { channelId, status } = before;

  let server: CallStateResponse | null;
  try {
    const fetcher =
      fetchState ?? (await import("@/lib/tauri/dms")).getCallState;
    server = await fetcher(channelId);
  } catch (err) {
    console.warn("[Call] Call resync failed:", err);
    return;
  }
  // The call moved on while the request ran: the newer state wins.
  if (!stillCall(channelId, status)) return;

  const serverCallId = knownCallId(server?.call_id);
  const gone =
    !server ||
    server.status === "ended" ||
    (!!serverCallId && isEndedCallId(channelId, serverCallId)) ||
    (!!serverCallId &&
      !!callState.callIds[channelId] &&
      callState.callIds[channelId] !== serverCallId);

  if (gone) {
    if (status === "incoming_ringing") {
      const name = (callState.currentCall as { initiatorName: string }).initiatorName;
      endRingAsMissed(channelId, name);
      return;
    }
    cancelCallRecovery();
    stopRinging();
    rememberEnded(channelId);
    setCallState("currentCall", { status: "idle" });
    setCallState(
      produce((state) => {
        delete state.activeCallsByChannel[channelId];
      }),
    );
    void dropCallVoice(channelId, { leaveCall: false, silent: true });
    return;
  }

  setChannelCallId(channelId, serverCallId);
  const participants = server!.participants ?? [];
  const meIn = !!myUserId && participants.includes(myUserId);

  if (status === "incoming_ringing") {
    if (meIn) answeredElsewhere(channelId);
    // Declined on another of my devices (R4): handled, not missed.
    else if (!!myUserId && (server!.declined_by ?? []).includes(myUserId)) {
      declinedElsewhere(channelId);
    }
    return;
  }
  if (server!.status !== "active") return; // still ringing
  if (!meIn) {
    // An active call that does not list me is no call for me (R4).
    cancelCallRecovery();
    stopRinging();
    rememberEnded(channelId);
    setCallState("currentCall", { status: "idle" });
    void dropCallVoice(channelId, { leaveCall: false, silent: true });
    return;
  }
  if (status === "outgoing_ringing") {
    // Answered during the gap: connected, and the voice that went with the
    // old socket comes back through the recovery (R4, Codex 9).
    callConnected(
      channelId,
      participants.filter((id) => id !== myUserId),
    );
  }
  await rejoinCallVoice(channelId);
}

// DM-call voice recovery (C32, R3): the same flow as channel voice.

/** The recovery flow of native DM-call voice. */
const callRecovery = new VoiceRecovery();

let nativeWatch: Promise<void> | null = null;
let nativeConnectedWaiters: Array<(ok: boolean) => void> = [];

function settleNativeConnected(ok: boolean): void {
  const waiters = nativeConnectedWaiters;
  nativeConnectedWaiters = [];
  for (const settle of waiters) settle(ok);
}

/** Resolves true on the next native publisher "Connected", false after `ms`. */
function waitNativeConnected(ms: number): Promise<boolean> {
  return new Promise((resolve) => {
    const settle = (ok: boolean) => {
      clearTimeout(timer);
      nativeConnectedWaiters = nativeConnectedWaiters.filter((w) => w !== settle);
      resolve(ok);
    };
    const timer = setTimeout(() => settle(false), ms);
    nativeConnectedWaiters.push(settle);
  });
}

/**
 * Native publisher state of DM-call voice (`voice:state_change`, Rust
 * `ConnectionState`): "Connected" ends a recovery attempt successfully,
 * "Failed" (ICE failure) starts the recovery. Exported for tests.
 */
export function onNativeCallVoiceState(payload: string): void {
  if (payload === "Connected") settleNativeConnected(true);
  if (payload === "Failed") {
    const cur = callState.currentCall;
    if (cur.status === "connected") void startCallRecovery(cur.channelId);
  }
}

/** Native event callback (not a reactive scope). */
function onNativeVoiceStateEvent(event: { payload: string }): void {
  onNativeCallVoiceState(event.payload);
}

/** Listen to the native voice state once (DM calls bypass the voice store). */
function watchNativeCallVoice(): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  nativeWatch ??= import("@tauri-apps/api/event")
    .then(({ listen }) => listen<string>("voice:state_change", onNativeVoiceStateEvent))
    .then(() => undefined)
    .catch((err: unknown) => {
      nativeWatch = null;
      console.warn("[Call] Watching native call voice failed:", err);
    });
  return nativeWatch;
}

/** Whether the DM-call recovery runs (tests). */
export function isCallRecoveryRunning(): boolean {
  return callRecovery.running;
}

/** Native DM-call voice as a recovery session (browser DM calls carry none). */
function callSession(channelId: string): RecoverySession {
  return {
    label: `call ${channelId}`,
    attempt: async (epoch) => {
      const tauri = await import("@/lib/tauri");
      await watchNativeCallVoice();
      if (isVoiceEpochStale(epoch) || !stillCall(channelId, "connected")) return false;
      // Fresh peer connections without `voice_leave` (C32).
      await tauri.leaveVoiceSilently();
      if (isVoiceEpochStale(epoch)) return false;
      const connected = waitNativeConnected(RECOVERY_ATTEMPT_TIMEOUT_MS);
      try {
        await tauri.joinVoice(channelId, epoch);
      } catch (err) {
        console.warn("[Call] Call voice rejoin attempt failed:", err);
        settleNativeConnected(false);
        return false;
      }
      return (await connected) && !isVoiceEpochStale(epoch);
    },
    abandon: async () => {
      settleNativeConnected(false);
      const tauri = await import("@/lib/tauri");
      await tauri.leaveVoiceSilently().catch(() => {});
    },
    restore: async () => {},
    commit: () => {
      console.info("[Call] Rejoined call voice:", channelId);
    },
    giveUp: async () => {
      settleNativeConnected(false);
      console.warn("[Call] Call voice rejoin gave up:", channelId);
      if (stillCall(channelId, "connected")) {
        endCall(channelId, "last_left");
        await dropCallVoice(channelId, { leaveCall: true, silent: true });
      } else {
        await dropCallVoice(channelId, { leaveCall: false, silent: true });
      }
    },
  };
}

/** Start (or keep) the recovery of the connected DM call's native voice. */
function startCallRecovery(channelId: string): Promise<void> {
  if (!isTauri() || !stillCall(channelId, "connected")) return Promise.resolve();
  return callRecovery.start(callSession(channelId), {
    socketDown: callRecovery.signallingDown,
  });
}

/**
 * Native DM-call voice after a reconnect the server confirmed (C32/C33):
 * resumes the recovery on the new socket. Browser DM calls carry no voice.
 */
function rejoinCallVoice(channelId: string): Promise<void> {
  callRecovery.socketReady();
  return startCallRecovery(channelId);
}

/**
 * The signalling socket dropped: the native call voice went with it. The
 * recovery waits for the C33 resync on the next socket (at most 60 s).
 */
export function noteCallSignallingLost(): void {
  callRecovery.socketLost();
  const cur = callState.currentCall;
  if (cur.status === "connected" && isTauri()) {
    void watchNativeCallVoice();
    void callRecovery.start(callSession(cur.channelId), { socketDown: true });
  }
}

/** The socket is open again (before re-subscription), see `socketOpen`. */
export function noteCallSignallingUp(): void {
  callRecovery.socketOpen();
}

/** Stop the DM-call recovery and any join it has pending (hang-up, end). */
function cancelCallRecovery(): boolean {
  if (!callRecovery.running) return false;
  callRecovery.cancel();
  bumpVoiceEpoch();
  settleNativeConnected(false);
  // An attempt may already have joined natively: drop it without a leave
  // message (the hang-up or the server's end owns the signalling).
  void import("@/lib/tauri")
    .then((tauri) => tauri.leaveVoiceSilently())
    .catch((err: unknown) => console.warn("[Call] Dropping recovery voice failed:", err));
  return true;
}

/**
 * Hang-up / cancel, step 1 (W1-N3): stop the DM-call recovery and start a
 * new voice epoch BEFORE the native voice is torn down and the server is
 * told, so a recovery attempt cannot rejoin behind the hang-up. Call it
 * synchronously, before any await.
 */
export function stopCallVoiceRecovery(): void {
  cancelCallRecovery();
  bumpVoiceEpoch();
}

/**
 * C31 `voice_removed` for the DM call's voice (R3, W1-I4): drop the native
 * voice without `voice_leave`, cancel any recovery, and leave the call here
 * without ending it on the server (with `session_replaced` it goes on on
 * the other device). The call goes idle, so the resync never rejoins it.
 * Returns whether the event was for the current DM call.
 */
export async function handleCallVoiceRemoved(
  channelId: string,
  text: string,
): Promise<boolean> {
  const cur = callState.currentCall;
  if (cur.status === "idle" || !("channelId" in cur) || cur.channelId !== channelId) {
    return false;
  }
  callRecovery.cancel();
  bumpVoiceEpoch();
  settleNativeConnected(false);
  stopRinging();
  setCallState("currentCall", { status: "idle" });
  await dropCallVoice(channelId, { leaveCall: false, silent: true });
  showToast({ type: "warning", title: text, duration: 8000, id: "voice-removed" });
  return true;
}

// Actions

/**
 * Start an outgoing call.
 */
export function startCall(channelId: string): void {
  // The voice session of this call starts now: a leave before the native
  // join makes it stale (the join after the HTTP start does nothing).
  captureCallJoinEpoch(channelId);
  stopRinging();
  setCallState(
    produce((state) => {
      delete state.callIds[channelId];
    }),
  );
  setCallState("currentCall", {
    status: "outgoing_ringing",
    channelId,
    startedAt: Date.now(),
  });
  armRingCap(channelId);
}

/**
 * Receive an incoming call notification.
 */
export function receiveIncomingCall(
  channelId: string,
  initiator: string,
  initiatorName: string,
  rawCallId?: string,
): void {
  const callId = knownCallId(rawCallId);
  // Glare: the other side called this DM while ours was still ringing, so
  // theirs wins (the server rejected or will reject our start).
  const cur = callState.currentCall;
  const glare =
    cur.status === "outgoing_ringing" && cur.channelId === channelId;
  // Only update if we're idle (don't interrupt an existing call)
  if (callId && isEndedCallId(channelId, callId)) return;
  if (cur.status === "idle" || glare) {
    setCallState(
      produce((state) => {
        if (callId) state.callIds[channelId] = callId;
        else delete state.callIds[channelId];
      }),
    );
    setCallState("currentCall", {
      status: "incoming_ringing",
      channelId,
      initiator,
      initiatorName,
    });
    // Start ringing for incoming call
    startRinging();
    armRingCap(channelId);
  }
  // Always track in activeCallsByChannel for sidebar indicator
  if (callId && !(cur.status === "idle" || glare)) {
    setChannelCallId(channelId, callId);
  }
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
  captureCallJoinEpoch(channelId);
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
  void watchNativeCallVoice();
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
 * C2/C33: our own user joined this DM's call on another device. A ring here
 * stops at once (answered elsewhere, not missed); nothing else changes.
 */
export function answeredElsewhere(channelId: string, callId?: string): void {
  if (isStaleCallEvent(channelId, callId)) return;
  const current = callState.currentCall;
  if (current.status !== "incoming_ringing" || current.channelId !== channelId) {
    return;
  }
  stopRinging();
  setCallState("currentCall", { status: "idle" });
}

/**
 * R4: our own user declined this ring on another device. The ring stops
 * here and it is not a missed call.
 */
export function declinedElsewhere(channelId: string, callId?: string): void {
  if (isStaleCallEvent(channelId, callId)) return;
  const current = callState.currentCall;
  if (current.status !== "incoming_ringing" || current.channelId !== channelId) {
    return;
  }
  stopRinging();
  rememberEnded(channelId);
  setCallState("currentCall", { status: "idle" });
}

/**
 * A participant joined the call.
 */
export function participantJoined(
  channelId: string,
  userId: string,
  callId?: string,
): void {
  if (isStaleCallEvent(channelId, callId)) return;
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
export function participantLeft(
  channelId: string,
  userId: string,
  callId?: string,
): void {
  if (isStaleCallEvent(channelId, callId)) return;
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
    rememberEnded(channelId);
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
  cancelCallRecovery();
  // Clear any existing timeout to prevent race conditions
  if (callEndedTimeoutId) {
    clearTimeout(callEndedTimeoutId);
    callEndedTimeoutId = null;
  }

  rememberEnded(channelId);
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
  const id = knownCallId(callId);
  if (id && isEndedCallId(channelId, id)) {
    // The server's end of a call we already left/ended/declined locally. Clear
    // the DM's indicator unless a newer call (known id) owns it, and take the
    // server's reason/duration for the "ended" display. Never touch a newer
    // call that is now current.
    if (callState.callIds[channelId]) return;
    setCallState(
      produce((state) => {
        delete state.activeCallsByChannel[channelId];
      }),
    );
    const cur = callState.currentCall;
    if (cur.status === "ended" && cur.channelId === channelId) {
      setCallState("currentCall", { reason, duration });
    }
    return;
  }
  // A different, unknown-to-us id than the DM's current call is stale.
  if (isStaleCallEvent(channelId, id)) return;
  rememberEnded(channelId, id);
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
