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
  setCurrentCallId,
  abortOutgoingCall,
  connectOnFirstJoin,
} from "../call";

function reset() {
  setCallState({ currentCall: { status: "idle" }, activeCallsByChannel: {}, callId: null });
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
    setCurrentCallId("call-1");
    expect(callEventMatches("dm-x", "call-1")).toBe(true);
    expect(callEventMatches("dm-x", "call-2")).toBe(false);
    expect(callEventMatches("dm-x")).toBe(true);
  });

  it("call_ended with a different call_id is a no-op", () => {
    startCall("dm-x");
    setCurrentCallId("call-1");
    callEndedExternally("dm-x", "no_answer", undefined, "call-0");
    expect(callState.currentCall.status).toBe("outgoing_ringing");
    callEndedExternally("dm-x", "no_answer", undefined, "call-1");
    expect(callState.currentCall.status).toBe("ended");
  });

  it("incoming_call stores its call_id", () => {
    receiveIncomingCall("dm-x", "u2", "Bob", "call-9");
    expect(callState.callId).toBe("call-9");
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
    setCurrentCallId("c1");
    connectOnFirstJoin("dm-x", "u2", "c2");
    expect(callState.currentCall.status).toBe("outgoing_ringing");
    connectOnFirstJoin("dm-x", "u2", "c1");
    expect(callState.currentCall.status).toBe("connected");
  });
});
