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
import { nextVoiceToken } from "@/lib/webrtc/voiceSession";
import { voiceErrorText } from "@/lib/webrtc/voiceErrorText";
import { voiceOwner, type VoiceHooks, type VoiceOutcome, type VoiceWish } from "./voiceOwner";

/** `@/lib/tauri`, loaded once and shared (lazy: avoids an import cycle). */
let tauriModule: Promise<typeof import("@/lib/tauri")> | null = null;
function tauriApi(): Promise<typeof import("@/lib/tauri")> {
  tauriModule ??= import("@/lib/tauri");
  return tauriModule;
}

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
  /**
   * The voice of the connected call is being restored after a connection
   * loss (C32): the banner says so; the call itself is still the same.
   */
  voiceReconnecting: boolean;
}

const [callState, setCallState] = createStore<CallStoreState>({
  currentCall: { status: "idle" },
  activeCallsByChannel: {},
  callIds: {},
  endedCallIds: {},
  voiceReconnecting: false,
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
    void dropCallVoice(channelId, { leaveCall: true });
    endCall(channelId, "no_answer");
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
 * The call's voice is no longer wanted (and, with `leaveCall`, the call
 * itself is left on the server, best effort). The intent changes before
 * this returns; the voice owner ends the session in the background.
 * `silent`: without `voice_leave` (the server already dropped the peer).
 * Browser DM calls carry no voice, so only the call leave applies there.
 */
async function dropCallVoice(
  channelId: string,
  opts: { leaveCall: boolean; silent?: boolean },
): Promise<void> {
  // The server is told without waiting for the voice session to be gone.
  const released = releaseCallVoice({ silent: opts.silent ?? false });
  if (opts.leaveCall) await leaveCallOnServer(channelId);
  await released;
}

/** Tell the server this client left the call (best effort). */
async function leaveCallOnServer(channelId: string): Promise<void> {
  try {
    const tauri = await tauriApi();
    await tauri.leaveDMCall(channelId);
  } catch {
    // Already gone on the server (it ends unanswered rings at 90 s).
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
  // A recovery the resync does not confirm (request failed) keeps waiting
  // and ends at the owner's 60 s deadline.
  await resyncCall(myUserId, fetchState);
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
  // The same call (object identity), not just "a call in this channel
  // with this status": a new ring in the gap is not this answer's call.
  if (callState.currentCall !== before) return;

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
    // Declined on another of my devices: handled, not missed.
    else if (!!myUserId && (server!.declined_by ?? []).includes(myUserId)) {
      declinedElsewhere(channelId);
    }
    return;
  }
  if (server!.status !== "active") {
    // Still ringing. The caller's own voice went with the old socket: it
    // comes back now, so the call has audio once it is answered.
    if (meIn) await rejoinCallVoice();
    return;
  }
  if (!meIn) {
    // An active call that does not list me is no call for me.
    stopRinging();
    rememberEnded(channelId);
    setCallState("currentCall", { status: "idle" });
    void dropCallVoice(channelId, { leaveCall: false, silent: true });
    return;
  }
  if (status === "outgoing_ringing") {
    // Answered during the gap: connected, and the voice that went with the
    // old socket comes back through the recovery.
    callConnected(
      channelId,
      participants.filter((id) => id !== myUserId),
    );
  }
  await rejoinCallVoice();
}

// DM-call voice (desktop app): a wish of the voice owner, like channel
// voice. The call store only says what it wants; joining, ending and
// recovering (C32) the session is the owner's work (`voiceOwner.ts`).

/** The voice session of the call this client is in (desktop app only). */
let callVoice: VoiceWish | null = null;
/** Token of the latest start/accept: what its continuations belong to. */
let callToken: number | null = null;

/** The call started or accepted under `token` is still the call here. */
function callIsLive(token: number, channelId: string): boolean {
  return callToken === token && isInCallForChannel(channelId);
}

/** Whether the DM-call voice is being recovered (tests). */
export function isCallRecoveryRunning(): boolean {
  return voiceOwner.isRecovering(callVoice);
}

/**
 * The call wants its voice: the session of this start/accept becomes the
 * owner's intent at once; it is joined after `ready` (the server's answer
 * to the start or accept). Browser DM calls carry no voice.
 */
function requestCallVoice(
  channelId: string,
  token: number,
  ready: Promise<unknown>,
): Promise<VoiceOutcome> {
  if (!isTauri()) return Promise.resolve({ type: "established" });
  const hooks: VoiceHooks = {
    ready,
    // C32/C33: rejoin only once the resync says the call is still active
    // and this user is in it.
    needsConfirmation: true,
    recovering: () => setCallState("voiceReconnecting", true),
    recovered: () => {
      setCallState("voiceReconnecting", false);
      console.info("[Call] Rejoined call voice:", channelId);
    },
    gaveUp: () => {
      // Third failure or 60 s without the rejoin: the call ends here and
      // is left on the server (C33).
      console.warn("[Call] Call voice rejoin gave up:", channelId);
      setCallState("voiceReconnecting", false);
      if (callVoice === wish) callVoice = null;
      if (callIsLive(token, channelId) && stillCall(channelId, "connected")) {
        endCall(channelId, "last_left");
        void leaveCallOnServer(channelId);
      }
    },
    superseded: () => {
      // The user joined a voice channel instead: that leaves this call.
      // (A newer call has its own token and is not touched.)
      if (callVoice === wish) {
        callVoice = null;
        setCallState("voiceReconnecting", false);
      }
      if (callIsLive(token, channelId)) {
        endCall(channelId, "last_left");
        void leaveCallOnServer(channelId);
      }
    },
  };
  const wish = voiceOwner.wish("dm-call", channelId, hooks, token);
  callVoice = wish;
  return voiceOwner.request(wish);
}

/**
 * The call's voice is no longer wanted. Synchronous for the intent: from
 * this instant no join and no recovery attempt runs for it; the promise
 * resolves when the voice owner has ended the session.
 */
function releaseCallVoice(opts: { silent: boolean }): Promise<void> {
  const wish = callVoice;
  callVoice = null;
  setCallState("voiceReconnecting", false);
  return voiceOwner.drop(wish, opts);
}

// The signed-in session ends: no call, no ring and no call voice of it is
// left for whoever signs in next (the owner has already ended the session).
voiceOwner.onShutdown(() => {
  callVoice = null;
  callToken = null;
  if (ringCapTimer) clearTimeout(ringCapTimer);
  ringCapTimer = null;
  if (callEndedTimeoutId) clearTimeout(callEndedTimeoutId);
  callEndedTimeoutId = null;
  stopRinging();
  setCallState({
    currentCall: { status: "idle" },
    activeCallsByChannel: {},
    callIds: {},
    endedCallIds: {},
    voiceReconnecting: false,
  });
});

/**
 * Hang-up / cancel, step 1: the voice session of the call is over
 * BEFORE the server is told, so nothing can rejoin behind the hang-up.
 * Call it synchronously, before any await.
 */
export function leaveCallVoice(): Promise<void> {
  return releaseCallVoice({ silent: false });
}

/**
 * Native DM-call voice after a reconnect the server confirmed (C32/C33):
 * the recovery may run on the new socket.
 */
function rejoinCallVoice(): Promise<void> {
  // The answer came over the new socket. If that socket has dropped again
  // meanwhile, the next one brings its own resync.
  if (voiceOwner.signallingDown) return Promise.resolve();
  voiceOwner.socketReady();
  voiceOwner.confirm(callVoice);
  return isCallRecoveryRunning() ? voiceOwner.whenCalm() : Promise.resolve();
}

/**
 * C31 `voice_removed` for the DM call's voice: the voice
 * session is over without `voice_leave` and is never rejoined, and the
 * call is left here without ending it on the server (with
 * `session_replaced` it goes on on the other device). The call goes idle,
 * so the resync never rejoins it. Returns whether the event was for the
 * current DM call.
 */
export async function handleCallVoiceRemoved(
  channelId: string,
  text: string,
): Promise<boolean> {
  const cur = callState.currentCall;
  if (cur.status === "idle" || !("channelId" in cur) || cur.channelId !== channelId) {
    return false;
  }
  voiceOwner.serverRemoved(channelId);
  stopRinging();
  setCallState("currentCall", { status: "idle" });
  const ended = releaseCallVoice({ silent: true });
  showToast({ type: "warning", title: text, duration: 8000, id: "voice-removed" });
  await ended;
  return true;
}

// Actions

/**
 * Start an outgoing call. Returns the token of this start: what happens
 * later for this call (the server's answer, its voice session) carries
 * exactly this value and never looks one up (`startOutgoingCall` does all
 * of it).
 */
export function startCall(channelId: string): number {
  const token = nextVoiceToken();
  callToken = token;
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
  return token;
}

/** A failed voice join as the error the caller shows. */
function throwIfVoiceFailed(outcome: VoiceOutcome): void {
  if (outcome.type === "failed") throw new Error(voiceErrorText(outcome.error));
}

/**
 * Start an outgoing call and its voice: local state and the voice wish at
 * once, then the server start; the voice owner joins the voice after the
 * server accepted, under the token taken when THIS call started (never
 * looked up later, so a cancelled call's late response cannot join a newer
 * call). Throws what the server start throws; the caller shows it.
 */
export async function startOutgoingCall(channelId: string): Promise<void> {
  const token = startCall(channelId);
  const started = (async () => {
    const tauri = await tauriApi();
    const res = await tauri.startDMCall(channelId);
    if (callIsLive(token, channelId)) setChannelCallId(channelId, res.call_id);
  })();
  const voice = requestCallVoice(channelId, token, started);
  await started;
  throwIfVoiceFailed(await voice);
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
 * Transition to connecting state when joining a call. Returns the token of
 * this accept (see `startCall`).
 */
export function joinCall(channelId: string): number {
  const token = nextVoiceToken();
  callToken = token;
  // Stop ringing when answering the call
  stopRinging();
  setCallState("currentCall", {
    status: "connecting",
    channelId,
  });
  return token;
}

/**
 * Accept an incoming call and join its voice under the token taken when it
 * was accepted. Throws what the server join throws; the caller cleans up.
 */
export async function acceptCall(channelId: string): Promise<void> {
  const token = joinCall(channelId);
  const accepted = (async () => {
    const tauri = await tauriApi();
    const res = await tauri.joinDMCall(channelId);
    if (callIsLive(token, channelId)) setChannelCallId(channelId, res.call_id);
  })();
  const voice = requestCallVoice(channelId, token, accepted);
  await accepted;
  throwIfVoiceFailed(await voice);
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
  // The call is over here: so is its voice session, if it still has one.
  void releaseCallVoice({ silent: false });
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
