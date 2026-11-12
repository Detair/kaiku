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

vi.mock("@/lib/tauri", () => ({
  leaveDMCall: vi.fn().mockResolvedValue(undefined),
  startDMCall: vi.fn().mockResolvedValue({ call_id: "call-s" }),
  joinDMCall: vi.fn().mockResolvedValue({ call_id: "call-j" }),
}));

type Session = { token: number; channelId: string };
type AdapterHandlers = {
  onStateChange: (state: string, session: Session | null) => void;
  onConnectionLost: (session: Session | null) => void;
};

/** Handlers the voice owner installed on the adapter. */
let adapterHandlers: AdapterHandlers | null = null;

/** The voice adapter as the voice owner sees it (desktop DM-call voice). */
const mockAdapter = {
  join: vi.fn(),
  leave: vi.fn(),
  teardown: vi.fn(),
  setEventHandlers: vi.fn((h: Partial<AdapterHandlers>) => {
    adapterHandlers = { ...adapterHandlers, ...h } as AdapterHandlers;
  }),
};

/** Default adapter behaviour: joins succeed, ending a session reports it. */
function resetAdapter(): void {
  const ends = async (session: Session) => {
    adapterHandlers?.onStateChange("disconnected", session);
    return { ok: true };
  };
  mockAdapter.join.mockReset().mockImplementation(async (session: Session) => ({
    ok: true,
    value: session,
  }));
  mockAdapter.leave.mockReset().mockImplementation(ends);
  mockAdapter.teardown.mockReset().mockImplementation(ends);
}

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
  isCallRecoveryRunning,
  leaveCallVoice,
  callConnected,
  startOutgoingCall,
  acceptCall,
  RING_CAP_MS,
} from "../call";
import { voiceOwner, RECOVERY_OFFLINE_CAP_MS } from "../voiceOwner";
import { showToast } from "@/components/ui/Toast";
import type { CallStateResponse } from "@/lib/types";
import { stopRinging } from "@/lib/sound/ring";

voiceOwner.useAdapter(async () => mockAdapter as never);

function reset() {
  setCallState({ currentCall: { status: "idle" }, activeCallsByChannel: {}, callIds: {}, endedCallIds: {} });
}

/** Sessions the adapter was asked to join, in order. */
function joinedSessions(): Session[] {
  return mockAdapter.join.mock.calls.map(([session]) => session as Session);
}

/** Whether the adapter was told to end exactly this session. */
function endedWith(method: "leave" | "teardown", session: Session | undefined): boolean {
  return (
    !!session &&
    mockAdapter[method].mock.calls.some(([s]) => (s as Session).token === session.token)
  );
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

describe("DM call voice is a session of the voice owner (C32, desktop)", () => {
  beforeEach(async () => {
    reset();
    vi.clearAllMocks();
    resetAdapter();
    vi.useFakeTimers();
    voiceOwner.socketReady(); // socket known up
    const common = await import("@/lib/tauri/common");
    vi.mocked(common.isTauri).mockReturnValue(true);
  });
  afterEach(async () => {
    endCall("dm-x", "last_left"); // ends the call and whatever voice it has
    await vi.advanceTimersByTimeAsync(0);
    const common = await import("@/lib/tauri/common");
    vi.mocked(common.isTauri).mockReturnValue(false);
    vi.useRealTimers();
  });

  const activeWithMe = async (): Promise<CallStateResponse> => ({
    channel_id: "dm-x",
    call_id: "call-1",
    status: "active",
    participants: ["u2", "me"],
  });

  /** The next join reaches publisher "connected". */
  function joinConnects(): void {
    mockAdapter.join.mockImplementationOnce(async (session: Session) => {
      queueMicrotask(() => adapterHandlers!.onStateChange("connected", session));
      return { ok: true, value: session };
    });
  }

  /** The next join never returns; gives the session it was asked to join. */
  function joinHangs(): () => Session | undefined {
    let session: Session | undefined;
    mockAdapter.join.mockImplementationOnce((joined: Session) => {
      session = joined;
      return new Promise(() => {});
    });
    return () => session;
  }

  /** An accepted, connected call with its voice session; returns that session. */
  async function connectedCall(): Promise<Session> {
    receiveIncomingCall("dm-x", "u2", "Bob", "call-1");
    await acceptCall("dm-x");
    callConnected("dm-x", ["u2"]);
    const session = joinedSessions()[0]!;
    adapterHandlers!.onStateChange("connected", session);
    mockAdapter.join.mockClear();
    return session;
  }

  it("rejoins the call voice silently while the server lists this user", async () => {
    const first = await connectedCall();
    joinConnects();
    const done = resyncCallAfterReconnect("me", activeWithMe);
    await vi.advanceTimersByTimeAsync(1000);
    await done;
    // The old connection went without voice_leave; a new session was joined.
    expect(endedWith("teardown", first)).toBe(true);
    expect(mockAdapter.leave).not.toHaveBeenCalled();
    expect(mockAdapter.join).toHaveBeenCalledWith({ token: expect.any(Number), channelId: "dm-x" });
    expect(callState.currentCall.status).toBe("connected");
    expect(isCallRecoveryRunning()).toBe(false);
  });

  it("no rejoin before the resync has confirmed the call (C33)", async () => {
    await connectedCall();
    voiceOwner.socketLost();
    voiceOwner.socketReady(); // the socket is back, the resync has not answered
    expect(isCallRecoveryRunning()).toBe(true);
    await vi.advanceTimersByTimeAsync(30_000);
    expect(mockAdapter.join).not.toHaveBeenCalled();

    joinConnects();
    const done = resyncCallAfterReconnect("me", activeWithMe);
    await vi.advanceTimersByTimeAsync(1000);
    await done;
    expect(mockAdapter.join).toHaveBeenCalledTimes(1);
    expect(isCallRecoveryRunning()).toBe(false);
  });

  it("a join that returns without publisher Connected is not success (15 s per attempt)", async () => {
    await connectedCall();
    const done = resyncCallAfterReconnect("me", activeWithMe);
    await vi.advanceTimersByTimeAsync(1000 + 15_000 - 1);
    expect(mockAdapter.join).toHaveBeenCalledTimes(1);
    expect(isCallRecoveryRunning()).toBe(true);
    joinConnects();
    await vi.advanceTimersByTimeAsync(1 + 3000);
    await done;
    expect(mockAdapter.join).toHaveBeenCalledTimes(2);
    expect(callState.currentCall.status).toBe("connected");
  });

  it("ends the call after three failed tries (shared 1/3/9 s flow)", async () => {
    const tauri = await import("@/lib/tauri");
    await connectedCall();
    mockAdapter.join.mockResolvedValue({ ok: false, error: { type: "unknown", message: "no" } });
    const done = resyncCallAfterReconnect("me", activeWithMe);
    await vi.advanceTimersByTimeAsync(13_000);
    await done;
    expect(mockAdapter.join).toHaveBeenCalledTimes(3);
    expect(callState.currentCall.status).toBe("ended");
    expect(tauri.leaveDMCall).toHaveBeenCalledWith("dm-x");
  });

  it("socket loss during an attempt is not charged; 60 s without the rejoin ends the call", async () => {
    await connectedCall();
    const hanging = joinHangs();
    void resyncCallAfterReconnect("me", activeWithMe);
    await vi.advanceTimersByTimeAsync(1000);
    expect(mockAdapter.join).toHaveBeenCalledTimes(1);

    voiceOwner.socketLost();
    await vi.advanceTimersByTimeAsync(0);
    // The attempt's own session was ended, by its handle, without a leave.
    expect(endedWith("teardown", hanging())).toBe(true);
    expect(isCallRecoveryRunning()).toBe(true);
    expect(voiceOwner.attemptsUsed).toBe(0);
    await vi.advanceTimersByTimeAsync(RECOVERY_OFFLINE_CAP_MS);
    expect(isCallRecoveryRunning()).toBe(false);
    expect(callState.currentCall.status).toBe("ended");
    expect(mockAdapter.join).toHaveBeenCalledTimes(1);
  });

  it("an ICE failure in a connected call starts the recovery", async () => {
    const first = await connectedCall();
    joinConnects();
    adapterHandlers!.onConnectionLost(first);
    expect(isCallRecoveryRunning()).toBe(true);
    await vi.advanceTimersByTimeAsync(1000);
    await vi.waitFor(() => expect(isCallRecoveryRunning()).toBe(false));
    expect(mockAdapter.join).toHaveBeenCalledTimes(1);
  });

  it("an ICE failure of a session the call no longer holds starts nothing", async () => {
    await connectedCall();
    adapterHandlers!.onConnectionLost({ token: -1, channelId: "dm-x" });
    adapterHandlers!.onConnectionLost(null);
    expect(isCallRecoveryRunning()).toBe(false);
  });

  it("hanging up during a pending attempt ends that attempt's session", async () => {
    await connectedCall();
    const hanging = joinHangs();
    void resyncCallAfterReconnect("me", activeWithMe);
    await vi.advanceTimersByTimeAsync(1000);
    endCall("dm-x", "last_left");
    expect(isCallRecoveryRunning()).toBe(false);
    await vi.advanceTimersByTimeAsync(30_000);
    expect(endedWith("leave", hanging())).toBe(true);
    expect(mockAdapter.join).toHaveBeenCalledTimes(1);
  });

  it("voice_removed during a DM call tears it down and never rejoins", async () => {
    const tauri = await import("@/lib/tauri");
    await connectedCall();
    const hanging = joinHangs();
    void resyncCallAfterReconnect("me", activeWithMe);
    await vi.advanceTimersByTimeAsync(1000);

    expect(await handleCallVoiceRemoved("dm-x", "Removed")).toBe(true);
    expect(endedWith("teardown", hanging())).toBe(true);
    expect(mockAdapter.leave).not.toHaveBeenCalled();
    expect(tauri.leaveDMCall).not.toHaveBeenCalled();
    expect(callState.currentCall.status).toBe("idle");
    expect(showToast).toHaveBeenCalledWith(expect.objectContaining({ title: "Removed" }));

    // The next resync has nothing to rejoin.
    await resyncCallAfterReconnect("me", activeWithMe);
    await vi.advanceTimersByTimeAsync(30_000);
    expect(mockAdapter.join).toHaveBeenCalledTimes(1);
  });

  it("voice_removed for a connected call ends its session without voice_leave", async () => {
    const first = await connectedCall();
    expect(await handleCallVoiceRemoved("dm-x", "Removed")).toBe(true);
    expect(endedWith("teardown", first)).toBe(true);
    expect(mockAdapter.leave).not.toHaveBeenCalled();
    voiceOwner.socketLost();
    voiceOwner.socketReady();
    await vi.advanceTimersByTimeAsync(30_000);
    expect(mockAdapter.join).not.toHaveBeenCalled();
  });

  it("hang-up during the recovery pause: no rejoin, even with a slow server leave (W1-N3)", async () => {
    const tauri = await import("@/lib/tauri");
    const first = await connectedCall();
    void resyncCallAfterReconnect("me", activeWithMe);
    await vi.advanceTimersByTimeAsync(500); // recovery sleeps (1 s)
    expect(isCallRecoveryRunning()).toBe(true);

    // CallBanner hang-up: the voice is no longer wanted, then the server.
    const voiceEnded = leaveCallVoice();
    expect(isCallRecoveryRunning()).toBe(false); // at once, before any await
    let serverLeft!: () => void;
    vi.mocked(tauri.leaveDMCall).mockImplementationOnce(
      () =>
        new Promise((r) => (serverLeft = () => r(undefined as never))) as ReturnType<
          typeof tauri.leaveDMCall
        >,
    );
    await voiceEnded;
    expect(endedWith("leave", first)).toBe(true);
    const leaving = tauri.leaveDMCall("dm-x");
    await vi.advanceTimersByTimeAsync(30_000); // the slow server leave
    expect(mockAdapter.join).not.toHaveBeenCalled();
    serverLeft();
    await leaving;
    endCall("dm-x", "last_left");
    await vi.advanceTimersByTimeAsync(30_000);
    expect(mockAdapter.join).not.toHaveBeenCalled();
  });

  it("a call that ends during a recovery ends its voice session", async () => {
    const first = await connectedCall();
    void resyncCallAfterReconnect("me", activeWithMe);
    await vi.advanceTimersByTimeAsync(500);
    callEndedExternally("dm-x", "last_left", undefined, "call-1");
    await vi.advanceTimersByTimeAsync(0);
    expect(isCallRecoveryRunning()).toBe(false);
    expect(endedWith("leave", first)).toBe(true);
    await vi.advanceTimersByTimeAsync(30_000);
    expect(mockAdapter.join).not.toHaveBeenCalled();
  });

  it("an ended call clears a connected call and its voice, without voice_leave", async () => {
    const first = await connectedCall();
    await resyncCallAfterReconnect("me", async () => ({
      channel_id: "dm-x",
      call_id: "call-1",
      status: "ended",
    }));
    await vi.advanceTimersByTimeAsync(0);
    expect(callState.currentCall.status).toBe("idle");
    expect(endedWith("teardown", first)).toBe(true);
    expect(mockAdapter.leave).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
  });

  it("voice_removed for another channel is not for the call", async () => {
    await connectedCall();
    expect(await handleCallVoiceRemoved("ch-other", "Removed")).toBe(false);
    expect(callState.currentCall.status).toBe("connected");
    expect(mockAdapter.teardown).not.toHaveBeenCalled();
  });

  it("an outgoing ring answered in the gap becomes connected and recovers voice (R4)", async () => {
    const starting = startOutgoingCall("dm-x");
    await starting;
    const first = joinedSessions()[0]!;
    adapterHandlers!.onStateChange("connected", first);
    joinConnects();
    const done = resyncCallAfterReconnect("me", async () => ({
      channel_id: "dm-x",
      call_id: "call-s",
      status: "active",
      participants: ["me", "u2"],
    }));
    await vi.advanceTimersByTimeAsync(1000);
    await done;
    expect(callState.currentCall).toMatchObject({ status: "connected", participants: ["u2"] });
    expect(joinedSessions()).toHaveLength(2);
    expect(joinedSessions()[1]).toEqual({ token: expect.any(Number), channelId: "dm-x" });
    expect(endedWith("teardown", first)).toBe(true);
  });

  it("an outgoing ring that is still ringing after a reconnect gets its voice back", async () => {
    await startOutgoingCall("dm-x");
    const first = joinedSessions()[0]!;
    adapterHandlers!.onStateChange("connected", first);
    voiceOwner.socketLost();
    joinConnects();
    const done = resyncCallAfterReconnect("me", async () => ({
      channel_id: "dm-x",
      call_id: "call-s",
      status: "ringing",
      participants: ["me"],
    }));
    await vi.advanceTimersByTimeAsync(1000);
    await done;
    expect(callState.currentCall.status).toBe("outgoing_ringing");
    expect(joinedSessions()).toHaveLength(2);
    expect(isCallRecoveryRunning()).toBe(false);
    // Long after the 60 s deadline the ring's voice is still there.
    await vi.advanceTimersByTimeAsync(70_000);
    expect(endedWith("teardown", joinedSessions()[1])).toBe(false);
  });

  it("another voice session taking over ends the call here", async () => {
    const tauri = await import("@/lib/tauri");
    const first = await connectedCall();
    // The user joins a voice channel: that is the voice they want now.
    const channel = voiceOwner.wish("channel", "ch-1", {});
    const joined = voiceOwner.request(channel);
    expect(callState.currentCall.status).toBe("ended"); // at once
    await joined;
    expect(endedWith("leave", first)).toBe(true);
    expect(tauri.leaveDMCall).toHaveBeenCalledWith("dm-x");
    // The call's session was ended completely before the channel was joined.
    expect(mockAdapter.leave.mock.invocationCallOrder[0]!).toBeLessThan(
      mockAdapter.join.mock.invocationCallOrder[0]!,
    );
    expect(joinedSessions()).toEqual([{ token: channel.token, channelId: "ch-1" }]);
    await voiceOwner.drop(channel);
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

describe("call voice tokens are carried, not looked up (Codex W1c-1)", () => {
  beforeEach(async () => {
    reset();
    vi.clearAllMocks();
    resetAdapter();
    voiceOwner.socketReady();
    const common = await import("@/lib/tauri/common");
    vi.mocked(common.isTauri).mockReturnValue(true);
  });
  afterEach(async () => {
    endCall("dm-x", "cancelled");
    await Promise.resolve();
    const common = await import("@/lib/tauri/common");
    vi.mocked(common.isTauri).mockReturnValue(false);
  });

  it("call A cancelled while starting, call B started: A never joins, B joins with its own token", async () => {
    const tauri = await import("@/lib/tauri");
    let resolveA!: (v: { call_id: string }) => void;
    vi.mocked(tauri.startDMCall)
      .mockImplementationOnce(() => new Promise((r) => (resolveA = r)) as never)
      .mockResolvedValueOnce({ call_id: "call-b" } as never);

    const a = startOutgoingCall("dm-x");
    // A is cancelled (CallBanner: voice no longer wanted, then ended).
    void leaveCallVoice();
    endCall("dm-x", "cancelled");
    const b = startOutgoingCall("dm-x");
    await b;
    // A's late answer from the server belongs to a call that is over.
    resolveA({ call_id: "call-a" });
    await a;

    expect(mockAdapter.join).toHaveBeenCalledTimes(1);
    expect(joinedSessions()[0]).toEqual({ token: expect.any(Number), channelId: "dm-x" });
    expect(callState.callIds["dm-x"]).toBe("call-b");
    expect(callState.currentCall.status).toBe("outgoing_ringing");
  });

  it("the voice is joined only after the server accepted the start", async () => {
    const tauri = await import("@/lib/tauri");
    let accept!: (v: { call_id: string }) => void;
    vi.mocked(tauri.startDMCall).mockImplementationOnce(
      () => new Promise((r) => (accept = r)) as never,
    );
    const starting = startOutgoingCall("dm-x");
    await vi.waitFor(() => expect(accept).toBeTypeOf("function"));
    expect(mockAdapter.join).not.toHaveBeenCalled();
    accept({ call_id: "call-1" });
    await starting;
    expect(mockAdapter.join).toHaveBeenCalledTimes(1);
  });

  it("a start the server refuses joins nothing and throws the server's error", async () => {
    const tauri = await import("@/lib/tauri");
    vi.mocked(tauri.startDMCall).mockRejectedValueOnce(new Error("CALL_CONFLICT"));
    await expect(startOutgoingCall("dm-x")).rejects.toThrow("CALL_CONFLICT");
    await Promise.resolve();
    expect(mockAdapter.join).not.toHaveBeenCalled();
  });

  it("accepting a call joins with the token taken when it was accepted", async () => {
    const tauri = await import("@/lib/tauri");
    receiveIncomingCall("dm-x", "u2", "Bob", "call-1");
    await acceptCall("dm-x");
    expect(tauri.joinDMCall).toHaveBeenCalledWith("dm-x");
    expect(mockAdapter.join).toHaveBeenCalledWith({ token: expect.any(Number), channelId: "dm-x" });
  });

  it("a hang-up while accepting: no voice join", async () => {
    const tauri = await import("@/lib/tauri");
    let resolveJoin!: (v: { call_id: string }) => void;
    vi.mocked(tauri.joinDMCall).mockImplementationOnce(
      () => new Promise((r) => (resolveJoin = r)) as never,
    );
    receiveIncomingCall("dm-x", "u2", "Bob", "call-1");
    const accepting = acceptCall("dm-x");
    await vi.waitFor(() => expect(resolveJoin).toBeTypeOf("function"));
    void leaveCallVoice();
    resolveJoin({ call_id: "call-1" });
    await accepting;
    await Promise.resolve();
    expect(mockAdapter.join).not.toHaveBeenCalled();
  });

  it("in the browser a DM call asks for no voice session", async () => {
    const common = await import("@/lib/tauri/common");
    vi.mocked(common.isTauri).mockReturnValue(false);
    await startOutgoingCall("dm-x");
    expect(mockAdapter.join).not.toHaveBeenCalled();
    expect(voiceOwner.current).toBeNull();
  });
});
