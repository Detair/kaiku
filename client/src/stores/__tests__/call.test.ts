import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/sound/ring", () => ({
  startRinging: vi.fn(),
  stopRinging: vi.fn(),
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
} from "../call";

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
