import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/sound/ring", () => ({
  startRinging: vi.fn(),
  stopRinging: vi.fn(),
}));

vi.mock("@/components/ui/Toast", () => ({
  showToast: vi.fn(),
  dismissToast: vi.fn(),
}));

vi.mock("@/lib/tauri/common", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/tauri/common")>()),
  isTauri: vi.fn(() => false),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock("@/lib/tauri", () => ({
  leaveVoice: vi.fn().mockResolvedValue(undefined),
  leaveVoiceSilently: vi.fn().mockResolvedValue(undefined),
  joinVoice: vi.fn().mockResolvedValue(undefined),
  leaveDMCall: vi.fn().mockResolvedValue(undefined),
}));

import {
  callState,
  setCallState,
  startCall,
  receiveIncomingCall,
  callEndedExternally,
  callEventMatches,
  setChannelCallId,
  isStaleCallEvent,
  participantJoined,
  participantLeft,
  declineCall,
  endCall,
  abortOutgoingCall,
  connectOnFirstJoin,
  answeredElsewhere,
  declinedElsewhere,
  resyncCallAfterReconnect,
  handleCallVoiceRemoved,
  noteCallSignallingLost,
  onNativeCallVoiceState,
  isCallRecoveryRunning,
  RING_CAP_MS,
} from "../call";
import { isVoiceEpochStale } from "@/lib/webrtc/voiceSession";
import { RECOVERY_OFFLINE_CAP_MS } from "../voiceRecovery";
import { showToast } from "@/components/ui/Toast";
import type { CallStateResponse } from "@/lib/types";
import { stopRinging } from "@/lib/sound/ring";

function reset() {
  setCallState({ currentCall: { status: "idle" }, activeCallsByChannel: {}, callIds: {}, endedCallIds: {} });
}

describe("call store event matching", () => {
  beforeEach(reset);

  it("callEventMatches requires the same channel", () => {
    startCall("dm-x");
    expect(callEventMatches("dm-x")).toBe(true);
    expect(callEventMatches("dm-y")).toBe(false);
  });

  it("callEventMatches compares call_id when both are known", () => {
    startCall("dm-x");
    setChannelCallId("dm-x", "call-1");
    expect(callEventMatches("dm-x", "call-1")).toBe(true);
    expect(callEventMatches("dm-x", "call-2")).toBe(false);
    expect(callEventMatches("dm-x")).toBe(true);
  });

  it("call_ended with a different call_id is a no-op", () => {
    startCall("dm-x");
    setChannelCallId("dm-x", "call-1");
    callEndedExternally("dm-x", "no_answer", undefined, "call-0");
    expect(callState.currentCall.status).toBe("outgoing_ringing");
    callEndedExternally("dm-x", "no_answer", undefined, "call-1");
    expect(callState.currentCall.status).toBe("ended");
  });

  it("incoming_call stores its call_id", () => {
    receiveIncomingCall("dm-x", "u2", "Bob", "call-9");
    expect(callState.callIds["dm-x"]).toBe("call-9");
  });
});

describe("call glare", () => {
  beforeEach(reset);

  it("an incoming call from the other side replaces our outgoing ring in that DM", () => {
    startCall("dm-x");
    receiveIncomingCall("dm-x", "u2", "Bob", "call-b");
    expect(callState.currentCall.status).toBe("incoming_ringing");
  });

  it("an incoming call in another DM does not interrupt our outgoing ring", () => {
    startCall("dm-x");
    receiveIncomingCall("dm-y", "u2", "Bob", "call-b");
    expect(callState.currentCall.status).toBe("outgoing_ringing");
  });

  it("after a start conflict the state is idle so a later incoming call is shown", () => {
    startCall("dm-x");
    abortOutgoingCall("dm-x", "conflict");
    expect(callState.currentCall.status).toBe("idle");
    receiveIncomingCall("dm-x", "u2", "Bob", "call-b");
    expect(callState.currentCall.status).toBe("incoming_ringing");
  });

  it("a conflict does not clobber an incoming call that already won", () => {
    startCall("dm-x");
    receiveIncomingCall("dm-x", "u2", "Bob", "call-b");
    abortOutgoingCall("dm-x", "conflict");
    expect(callState.currentCall.status).toBe("incoming_ringing");
  });
});

describe("connectOnFirstJoin", () => {
  beforeEach(reset);

  it("a join in another DM does not connect a call ringing in this one", () => {
    startCall("dm-x");
    connectOnFirstJoin("dm-y", "u2");
    expect(callState.currentCall.status).toBe("outgoing_ringing");
  });

  it("a join in the same DM connects, a join with a foreign call_id does not", () => {
    startCall("dm-x");
    setChannelCallId("dm-x", "c1");
    connectOnFirstJoin("dm-x", "u2", "c2");
    expect(callState.currentCall.status).toBe("outgoing_ringing");
    connectOnFirstJoin("dm-x", "u2", "c1");
    expect(callState.currentCall.status).toBe("connected");
  });
});

describe("per-channel call ids", () => {
  beforeEach(reset);

  it("call_ended for DM Y clears Y's banner and leaves our call in X untouched", () => {
    startCall("dm-x");
    setChannelCallId("dm-x", "cx");
    receiveIncomingCall("dm-y", "u2", "Bob", "cy");
    expect(callState.activeCallsByChannel["dm-y"]).toBeDefined();

    callEndedExternally("dm-y", "last_left", undefined, "cy");

    expect(callState.activeCallsByChannel["dm-y"]).toBeUndefined();
    expect(callState.currentCall.status).toBe("outgoing_ringing");
  });

  it("a stale call_ended for Y (other id) does not clear Y's current call", () => {
    receiveIncomingCall("dm-y", "u2", "Bob", "cy2");
    callEndedExternally("dm-y", "last_left", undefined, "cy1");
    expect(callState.activeCallsByChannel["dm-y"]).toBeDefined();
  });

  it("declineCall forgets the id of that channel", () => {
    receiveIncomingCall("dm-x", "u2", "Bob", "c1");
    declineCall("dm-x");
    expect(callState.callIds["dm-x"]).toBeUndefined();
  });

  it("late events of the previous call in the same DM are ignored once the new id is known", () => {
    startCall("dm-x");
    setChannelCallId("dm-x", "new");
    participantLeft("dm-x", "u2", "old");
    expect(callState.currentCall.status).toBe("outgoing_ringing");
    connectOnFirstJoin("dm-x", "u2", "old");
    expect(callState.currentCall.status).toBe("outgoing_ringing");
    callEndedExternally("dm-x", "last_left", undefined, "old");
    expect(callState.currentCall.status).toBe("outgoing_ringing");
  });

  it("before the start response, events of a recently ended call id are ignored", () => {
    startCall("dm-x");
    setChannelCallId("dm-x", "old");
    callEndedExternally("dm-x", "last_left", undefined, "old");
    // new call in same DM; its id is not known yet
    startCall("dm-x");
    participantLeft("dm-x", "u2", "old");
    connectOnFirstJoin("dm-x", "u2", "old");
    callEndedExternally("dm-x", "last_left", undefined, "old");
    expect(callState.currentCall.status).toBe("outgoing_ringing");
    // an event with a fresh id is applied
    connectOnFirstJoin("dm-x", "u2", "fresh");
    expect(callState.currentCall.status).toBe("connected");
  });

  it("participantJoined with a stale call_id does not add a participant", () => {
    startCall("dm-x");
    setChannelCallId("dm-x", "new");
    connectOnFirstJoin("dm-x", "u2", "new");
    participantJoined("dm-x", "u3", "old");
    const cur = callState.currentCall;
    expect(cur.status === "connected" && cur.participants).toEqual(["u2"]);
  });

  it("a local end remembers the id as ended", () => {
    startCall("dm-x");
    setChannelCallId("dm-x", "c1");
    endCall("dm-x", "cancelled");
    expect(callState.endedCallIds["dm-x"]).toContain("c1");
  });
});

describe("nil-UUID call ids (older server)", () => {
  beforeEach(reset);
  const NIL = "00000000-0000-0000-0000-000000000000";

  it("are treated as unknown: stored as nothing, never stale", () => {
    startCall("dm-x");
    setChannelCallId("dm-x", NIL);
    expect(callState.callIds["dm-x"]).toBeUndefined();
    setChannelCallId("dm-x", "real");
    expect(isStaleCallEvent("dm-x", NIL)).toBe(false);
    callEndedExternally("dm-x", "last_left", undefined, NIL);
    expect(callState.currentCall.status).toBe("ended");
  });

  it("incoming_call with a nil id is shown and stores no id", () => {
    receiveIncomingCall("dm-x", "u2", "Bob", NIL);
    expect(callState.currentCall.status).toBe("incoming_ringing");
    expect(callState.callIds["dm-x"]).toBeUndefined();
  });
});

describe("call ids per DM across calls", () => {
  beforeEach(reset);

  it("a banner call that ended in another DM lets that DM learn the next call id", () => {
    receiveIncomingCall("dm-y", "u2", "Bob", "y1");
    callEndedExternally("dm-y", "last_left", undefined, "y1");
    receiveIncomingCall("dm-y", "u2", "Bob", "y2");
    expect(callState.callIds["dm-y"]).toBe("y2");
  });

  it("the first known id wins over a different one from call_started", () => {
    startCall("dm-x");
    setChannelCallId("dm-x", "a");
    setChannelCallId("dm-x", "b");
    expect(callState.callIds["dm-x"]).toBe("a");
  });
});

describe("call_ended after our own decline or end", () => {
  beforeEach(reset);

  it("incoming -> decline -> call_ended (same id) clears the indicator", () => {
    receiveIncomingCall("dm-x", "u2", "Bob", "c1");
    expect(callState.activeCallsByChannel["dm-x"]).toBeDefined();
    declineCall("dm-x");
    callEndedExternally("dm-x", "all_declined", undefined, "c1");
    expect(callState.activeCallsByChannel["dm-x"]).toBeUndefined();
    expect(callState.currentCall.status).toBe("idle");
  });

  it("outgoing -> end -> call_ended clears the indicator and records the server reason/duration", () => {
    startCall("dm-x");
    setChannelCallId("dm-x", "c1");
    setCallState("activeCallsByChannel", "dm-x", { initiator: "me", initiatorName: "Me", participants: ["me"] });
    endCall("dm-x", "cancelled");
    setCallState("activeCallsByChannel", "dm-x", { initiator: "me", initiatorName: "Me", participants: ["me"] });
    callEndedExternally("dm-x", "no_answer", 12, "c1");
    expect(callState.activeCallsByChannel["dm-x"]).toBeUndefined();
    const cur = callState.currentCall;
    expect(cur.status).toBe("ended");
    expect(cur.status === "ended" && cur.reason).toBe("no_answer");
    expect(cur.status === "ended" && cur.duration).toBe(12);
  });

  it("a late call_ended of an older call does not end a new ringing call in the same DM", () => {
    startCall("dm-x");
    setChannelCallId("dm-x", "old");
    endCall("dm-x", "cancelled");
    startCall("dm-x");
    setChannelCallId("dm-x", "new");
    callEndedExternally("dm-x", "last_left", undefined, "old");
    expect(callState.currentCall.status).toBe("outgoing_ringing");
  });

  it("same, while the new call's id is still unknown", () => {
    startCall("dm-x");
    setChannelCallId("dm-x", "old");
    endCall("dm-x", "cancelled");
    startCall("dm-x");
    callEndedExternally("dm-x", "last_left", undefined, "old");
    expect(callState.currentCall.status).toBe("outgoing_ringing");
  });

  it("a late call_ended of an older call keeps a newer incoming call's indicator", () => {
    receiveIncomingCall("dm-x", "u2", "Bob", "old");
    declineCall("dm-x");
    receiveIncomingCall("dm-x", "u2", "Bob", "new");
    callEndedExternally("dm-x", "all_declined", undefined, "old");
    expect(callState.activeCallsByChannel["dm-x"]).toBeDefined();
    expect(callState.currentCall.status).toBe("incoming_ringing");
  });
});

describe("answered on another device (C2)", () => {
  beforeEach(() => {
    reset();
    vi.clearAllMocks();
  });

  it("stops the ring when the own user joins elsewhere", () => {
    receiveIncomingCall("dm-x", "u2", "Bob", "call-1");
    answeredElsewhere("dm-x", "call-1");
    expect(stopRinging).toHaveBeenCalled();
    expect(callState.currentCall.status).toBe("idle");
    // The call itself goes on (sidebar indicator stays).
    expect(callState.activeCallsByChannel["dm-x"]).toBeDefined();
  });

  it("ignores another DM or an older call", () => {
    receiveIncomingCall("dm-x", "u2", "Bob", "call-1");
    answeredElsewhere("dm-y", "call-1");
    answeredElsewhere("dm-x", "call-0");
    expect(callState.currentCall.status).toBe("incoming_ringing");
  });

  it("leaves a call this device answered alone", () => {
    receiveIncomingCall("dm-x", "u2", "Bob", "call-1");
    setCallState("currentCall", { status: "connecting", channelId: "dm-x" });
    answeredElsewhere("dm-x", "call-1");
    expect(callState.currentCall.status).toBe("connecting");
  });
});

describe("local ring cap (C33)", () => {
  beforeEach(() => {
    reset();
    vi.clearAllMocks();
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("an incoming ring ends after 100 s and is shown as missed", () => {
    receiveIncomingCall("dm-x", "u2", "Bob", "call-1");
    vi.advanceTimersByTime(RING_CAP_MS - 1);
    expect(callState.currentCall.status).toBe("incoming_ringing");
    vi.advanceTimersByTime(1);
    expect(callState.currentCall.status).toBe("idle");
    expect(stopRinging).toHaveBeenCalled();
    expect(callState.activeCallsByChannel["dm-x"]).toBeUndefined();
    expect(showToast).toHaveBeenCalledWith(
      expect.objectContaining({ title: "Missed call from Bob" }),
    );
    // Late events of that call are ignored now.
    expect(isStaleCallEvent("dm-x", "call-1")).toBe(true);
  });

  it("an outgoing ring ends as no answer after 100 s", async () => {
    const tauri = await import("@/lib/tauri");
    startCall("dm-x");
    vi.advanceTimersByTime(RING_CAP_MS);
    expect(callState.currentCall).toMatchObject({ status: "ended", reason: "no_answer" });
    await vi.runOnlyPendingTimersAsync();
    expect(tauri.leaveDMCall).toHaveBeenCalledWith("dm-x");
    expect(showToast).not.toHaveBeenCalled();
  });

  it("an answered ring is not capped", () => {
    receiveIncomingCall("dm-x", "u2", "Bob", "call-1");
    setCallState("currentCall", { status: "connecting", channelId: "dm-x" });
    vi.advanceTimersByTime(RING_CAP_MS);
    expect(callState.currentCall.status).toBe("connecting");
    expect(showToast).not.toHaveBeenCalled();
  });

  it("a new ring restarts the cap", () => {
    receiveIncomingCall("dm-x", "u2", "Bob", "call-1");
    vi.advanceTimersByTime(60_000);
    declineCall("dm-x");
    receiveIncomingCall("dm-x", "u2", "Bob", "call-2");
    vi.advanceTimersByTime(60_000);
    expect(callState.currentCall.status).toBe("incoming_ringing");
    vi.advanceTimersByTime(40_000);
    expect(callState.currentCall.status).toBe("idle");
  });
});

describe("call resync after reconnect (C33)", () => {
  beforeEach(() => {
    reset();
    vi.clearAllMocks();
  });

  const active = (participants: string[], callId = "call-1"): CallStateResponse => ({
    channel_id: "dm-x",
    call_id: callId,
    status: "active",
    participants,
  });

  it("a vanished incoming ring is cleared and shown as missed", async () => {
    receiveIncomingCall("dm-x", "u2", "Bob", "call-1");
    await resyncCallAfterReconnect("me", async () => null);
    expect(callState.currentCall.status).toBe("idle");
    expect(stopRinging).toHaveBeenCalled();
    expect(showToast).toHaveBeenCalledWith(
      expect.objectContaining({ title: "Missed call from Bob" }),
    );
  });

  it("a vanished outgoing ring clears silently", async () => {
    startCall("dm-x");
    await resyncCallAfterReconnect("me", async () => null);
    expect(callState.currentCall.status).toBe("idle");
    expect(showToast).not.toHaveBeenCalled();
  });

  it("an ended call clears a connected call and its voice", async () => {
    const tauri = await import("@/lib/tauri");
    setCallState("currentCall", { status: "connected", channelId: "dm-x", participants: ["u2"], startedAt: 1 });
    await resyncCallAfterReconnect("me", async () => ({
      channel_id: "dm-x",
      call_id: "call-1",
      status: "ended",
    }));
    expect(callState.currentCall.status).toBe("idle");
    await vi.waitFor(() => expect(tauri.leaveVoiceSilently).toHaveBeenCalled());
    expect(showToast).not.toHaveBeenCalled();
  });

  it("an incoming ring answered elsewhere in the gap stops ringing, not missed", async () => {
    receiveIncomingCall("dm-x", "u2", "Bob", "call-1");
    await resyncCallAfterReconnect("me", async () => active(["u2", "me"]));
    expect(callState.currentCall.status).toBe("idle");
    expect(showToast).not.toHaveBeenCalled();
  });

  it("a still-ringing incoming call keeps ringing", async () => {
    receiveIncomingCall("dm-x", "u2", "Bob", "call-1");
    await resyncCallAfterReconnect("me", async () => ({
      channel_id: "dm-x",
      call_id: "call-1",
      status: "ringing",
    }));
    expect(callState.currentCall.status).toBe("incoming_ringing");
  });

  it("an outgoing ring that was answered becomes connected", async () => {
    startCall("dm-x");
    await resyncCallAfterReconnect("me", async () => active(["me", "u2"]));
    expect(callState.currentCall).toMatchObject({ status: "connected", participants: ["u2"] });
  });

  it("a connected call without this user as participant ends locally", async () => {
    setCallState("currentCall", { status: "connected", channelId: "dm-x", participants: ["u2"], startedAt: 1 });
    await resyncCallAfterReconnect("me", async () => active(["u2"]));
    expect(callState.currentCall.status).toBe("idle");
  });

  it("a different call id in the DM counts as gone", async () => {
    receiveIncomingCall("dm-x", "u2", "Bob", "call-1");
    await resyncCallAfterReconnect("me", async () => active(["u2"], "call-2"));
    expect(callState.currentCall.status).toBe("idle");
    expect(showToast).toHaveBeenCalled();
  });

  it("does nothing without a ringing or connected call", async () => {
    const fetchState = vi.fn();
    await resyncCallAfterReconnect("me", fetchState);
    expect(fetchState).not.toHaveBeenCalled();
  });

  it("keeps the local state when the request fails", async () => {
    receiveIncomingCall("dm-x", "u2", "Bob", "call-1");
    await resyncCallAfterReconnect("me", async () => {
      throw new Error("offline");
    });
    expect(callState.currentCall.status).toBe("incoming_ringing");
  });
});

describe("DM call voice rejoin after reconnect (C32, native)", () => {
  beforeEach(async () => {
    reset();
    vi.clearAllMocks();
    vi.useFakeTimers();
    const common = await import("@/lib/tauri/common");
    vi.mocked(common.isTauri).mockReturnValue(true);
  });
  afterEach(async () => {
    endCall("dm-x", "last_left"); // cancels any recovery left running
    const common = await import("@/lib/tauri/common");
    vi.mocked(common.isTauri).mockReturnValue(false);
    const tauri = await import("@/lib/tauri");
    vi.mocked(tauri.joinVoice).mockReset().mockResolvedValue(undefined);
    vi.useRealTimers();
  });

  const activeWithMe = async (): Promise<CallStateResponse> => ({
    channel_id: "dm-x",
    call_id: "call-1",
    status: "active",
    participants: ["u2", "me"],
  });

  /** The next native join reaches publisher "Connected". */
  async function nativeJoinConnects(): Promise<void> {
    const tauri = await import("@/lib/tauri");
    vi.mocked(tauri.joinVoice).mockImplementationOnce(async () => {
      queueMicrotask(() => onNativeCallVoiceState("Connected"));
    });
  }

  function connectedCall(): void {
    setCallState("currentCall", { status: "connected", channelId: "dm-x", participants: ["u2"], startedAt: 1 });
  }

  it("rejoins the call voice silently while the server lists this user", async () => {
    const tauri = await import("@/lib/tauri");
    connectedCall();
    await nativeJoinConnects();
    const done = resyncCallAfterReconnect("me", activeWithMe);
    await vi.advanceTimersByTimeAsync(1000);
    await done;
    expect(tauri.leaveVoiceSilently).toHaveBeenCalledTimes(1);
    expect(tauri.joinVoice).toHaveBeenCalledWith("dm-x", expect.any(Number));
    expect(tauri.leaveVoice).not.toHaveBeenCalled();
    expect(callState.currentCall.status).toBe("connected");
    expect(isCallRecoveryRunning()).toBe(false);
  });

  it("a join that returns without publisher Connected is not success (15 s per attempt)", async () => {
    const tauri = await import("@/lib/tauri");
    connectedCall();
    const done = resyncCallAfterReconnect("me", activeWithMe);
    await vi.advanceTimersByTimeAsync(1000 + 15_000 - 1);
    expect(tauri.joinVoice).toHaveBeenCalledTimes(1);
    expect(isCallRecoveryRunning()).toBe(true);
    await nativeJoinConnects();
    await vi.advanceTimersByTimeAsync(1 + 3000);
    await done;
    expect(tauri.joinVoice).toHaveBeenCalledTimes(2);
    expect(callState.currentCall.status).toBe("connected");
  });

  it("ends the call after three failed tries (shared 1/3/9 s flow)", async () => {
    const tauri = await import("@/lib/tauri");
    vi.mocked(tauri.joinVoice).mockRejectedValue(new Error("no"));
    connectedCall();
    const done = resyncCallAfterReconnect("me", activeWithMe);
    await vi.advanceTimersByTimeAsync(13_000);
    await done;
    expect(tauri.joinVoice).toHaveBeenCalledTimes(3);
    expect(callState.currentCall.status).toBe("ended");
    expect(tauri.leaveDMCall).toHaveBeenCalledWith("dm-x");
  });

  it("socket loss during an attempt is not charged; 60 s offline ends the call", async () => {
    const tauri = await import("@/lib/tauri");
    connectedCall();
    let joinEpoch: number | undefined;
    vi.mocked(tauri.joinVoice).mockImplementationOnce(async (_ch, epoch) => {
      joinEpoch = epoch;
      await new Promise(() => {}); // never returns
    });
    void resyncCallAfterReconnect("me", activeWithMe);
    await vi.advanceTimersByTimeAsync(1000);
    expect(tauri.joinVoice).toHaveBeenCalledTimes(1);

    noteCallSignallingLost();
    await vi.advanceTimersByTimeAsync(0);
    expect(isVoiceEpochStale(joinEpoch)).toBe(true);
    expect(isCallRecoveryRunning()).toBe(true);
    await vi.advanceTimersByTimeAsync(RECOVERY_OFFLINE_CAP_MS);
    expect(isCallRecoveryRunning()).toBe(false);
    expect(callState.currentCall.status).toBe("ended");
    expect(tauri.joinVoice).toHaveBeenCalledTimes(1);
  });

  it("a native ICE failure in a connected call starts the recovery", async () => {
    const tauri = await import("@/lib/tauri");
    connectedCall();
    await nativeJoinConnects();
    onNativeCallVoiceState("Failed");
    expect(isCallRecoveryRunning()).toBe(true);
    await vi.advanceTimersByTimeAsync(1000);
    await vi.waitFor(() => expect(isCallRecoveryRunning()).toBe(false));
    expect(tauri.joinVoice).toHaveBeenCalledTimes(1);
  });

  it("hanging up during a pending attempt cancels it", async () => {
    const tauri = await import("@/lib/tauri");
    connectedCall();
    let joinEpoch: number | undefined;
    vi.mocked(tauri.joinVoice).mockImplementationOnce(async (_ch, epoch) => {
      joinEpoch = epoch;
      await new Promise(() => {});
    });
    void resyncCallAfterReconnect("me", activeWithMe);
    await vi.advanceTimersByTimeAsync(1000);
    endCall("dm-x", "last_left");
    expect(isVoiceEpochStale(joinEpoch)).toBe(true);
    expect(isCallRecoveryRunning()).toBe(false);
    await vi.advanceTimersByTimeAsync(30_000);
    expect(tauri.joinVoice).toHaveBeenCalledTimes(1);
  });

  it("voice_removed during a native DM call tears it down and never rejoins", async () => {
    const tauri = await import("@/lib/tauri");
    connectedCall();
    let joinEpoch: number | undefined;
    vi.mocked(tauri.joinVoice).mockImplementationOnce(async (_ch, epoch) => {
      joinEpoch = epoch;
      await new Promise(() => {});
    });
    void resyncCallAfterReconnect("me", activeWithMe);
    await vi.advanceTimersByTimeAsync(1000);

    expect(await handleCallVoiceRemoved("dm-x", "Removed")).toBe(true);
    expect(isVoiceEpochStale(joinEpoch)).toBe(true);
    expect(tauri.leaveVoiceSilently).toHaveBeenCalled();
    expect(tauri.leaveVoice).not.toHaveBeenCalled();
    expect(tauri.leaveDMCall).not.toHaveBeenCalled();
    expect(callState.currentCall.status).toBe("idle");
    expect(showToast).toHaveBeenCalledWith(expect.objectContaining({ title: "Removed" }));

    // The next resync has nothing to rejoin.
    await resyncCallAfterReconnect("me", activeWithMe);
    await vi.advanceTimersByTimeAsync(30_000);
    expect(tauri.joinVoice).toHaveBeenCalledTimes(1);
  });

  it("voice_removed for another channel is not for the call", async () => {
    connectedCall();
    expect(await handleCallVoiceRemoved("ch-other", "Removed")).toBe(false);
    expect(callState.currentCall.status).toBe("connected");
  });

  it("an outgoing ring answered in the gap becomes connected and recovers voice (R4)", async () => {
    const tauri = await import("@/lib/tauri");
    startCall("dm-x");
    await nativeJoinConnects();
    const done = resyncCallAfterReconnect("me", async () => ({
      channel_id: "dm-x",
      call_id: "call-1",
      status: "active",
      participants: ["me", "u2"],
    }));
    await vi.advanceTimersByTimeAsync(1000);
    await done;
    expect(callState.currentCall).toMatchObject({ status: "connected", participants: ["u2"] });
    expect(tauri.joinVoice).toHaveBeenCalledWith("dm-x", expect.any(Number));
  });
});

describe("declined on another device (R4)", () => {
  beforeEach(() => {
    reset();
    vi.clearAllMocks();
  });

  it("stops the ring, not a missed call, and the cap stays quiet", () => {
    vi.useFakeTimers();
    receiveIncomingCall("dm-x", "u2", "Bob", "call-1");
    declinedElsewhere("dm-x", "call-1");
    expect(callState.currentCall.status).toBe("idle");
    expect(stopRinging).toHaveBeenCalled();
    vi.advanceTimersByTime(RING_CAP_MS + 1);
    expect(showToast).not.toHaveBeenCalled();
    vi.useRealTimers();
  });

  it("resync with declined_by naming me clears the ring without a missed call", async () => {
    receiveIncomingCall("dm-x", "u2", "Bob", "call-1");
    await resyncCallAfterReconnect("me", async () => ({
      channel_id: "dm-x",
      call_id: "call-1",
      status: "ringing",
      participants: ["u2"],
      declined_by: ["me"],
    }));
    expect(callState.currentCall.status).toBe("idle");
    expect(showToast).not.toHaveBeenCalled();
  });

  it("an outgoing ring whose active call does not list me is cleared", async () => {
    startCall("dm-x");
    await resyncCallAfterReconnect("me", async () => ({
      channel_id: "dm-x",
      call_id: "call-1",
      status: "active",
      participants: ["u2", "u3"],
    }));
    expect(callState.currentCall.status).toBe("idle");
  });
});
