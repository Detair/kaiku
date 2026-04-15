/**
 * Unit tests for ICE candidate buffering in BrowserVoiceAdapter.
 *
 * Covers the buffer-until-setRemoteDescription logic added in
 * `fix(client): buffer ICE candidates until remote description is set`.
 *
 * Restrictive-NAT deployments can deliver trickle ICE candidates before the
 * SDP answer/offer is applied; adding them pre-SRD throws InvalidStateError
 * and silently breaks connectivity. These tests verify the buffer + drain
 * behaviour closes that race correctly.
 *
 * Race model under test: handlePublisher{Answer,Offer} resets the buffer,
 * awaits setRemoteDescription, then drains. Candidates that arrive *during*
 * the SRD await land in the (freshly reset) buffer and must drain afterwards.
 * The fakes use a deferred promise so tests can interleave candidate arrivals
 * at exactly that moment.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { BrowserVoiceAdapter } from "../browser";

/** Identity constructor for RTCIceCandidate — stores init dict verbatim. */
class FakeRtcIceCandidate {
  constructor(public readonly init: RTCIceCandidateInit) {}
}

/** Passthrough for RTCSessionDescription. */
class FakeRtcSessionDescription {
  constructor(public readonly init: RTCSessionDescriptionInit) {}
}

/** Simple deferred/resolver for gating async steps in tests. */
function deferred<T = void>(): {
  promise: Promise<T>;
  resolve: (value: T | PromiseLike<T>) => void;
  reject: (reason?: unknown) => void;
} {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

/**
 * Fake RTCPeerConnection. `setRemoteDescription` awaits `srdGate.promise` so
 * tests can queue ICE candidates between the handler's buffer-reset and its
 * drain step. No state machine is modelled beyond what the buffer logic
 * inspects, because the adapter gates `addIceCandidate` on its own
 * `remoteDescriptionSet` flag.
 */
class FakeRtcPc {
  remoteDesc: RTCSessionDescriptionInit | null = null;
  addedCandidates: RTCIceCandidateInit[] = [];
  setRemoteDescriptionCalls = 0;
  srdGate = deferred<void>();

  async setRemoteDescription(desc: FakeRtcSessionDescription): Promise<void> {
    this.setRemoteDescriptionCalls += 1;
    this.remoteDesc = desc.init;
    await this.srdGate.promise;
  }

  async addIceCandidate(c: FakeRtcIceCandidate): Promise<void> {
    this.addedCandidates.push(c.init);
  }

  async createAnswer(): Promise<RTCSessionDescriptionInit> {
    return { type: "answer", sdp: "v=0\r\n" };
  }

  async setLocalDescription(_: RTCSessionDescriptionInit): Promise<void> {}

  close(): void {}
  addEventListener(): void {}
  removeEventListener(): void {}
  dispatchEvent(_: Event): boolean {
    return false;
  }

  /** Allow the in-flight setRemoteDescription to resolve and arm a fresh gate. */
  openSrd(): void {
    this.srdGate.resolve();
    this.srdGate = deferred<void>();
  }
}

/** Construct a fresh PcState-like object. */
function makePcState(pc: FakeRtcPc) {
  return {
    pc,
    remoteDescriptionSet: false,
    pendingCandidates: [] as RTCIceCandidateInit[],
  };
}

/** Build a candidate JSON string as the adapter expects (it parses JSON). */
function candidateJson(idx: number): string {
  return JSON.stringify({
    candidate: `candidate:${idx} 1 udp 2122260223 192.0.2.${idx % 256} ${40000 + idx} typ host`,
    sdpMid: "0",
    sdpMLineIndex: 0,
  });
}

/** Yield to the microtask queue so awaited promises can progress. */
async function flushMicrotasks(): Promise<void> {
  // Two turns is plenty — the handler has at most 1 await between
  // buffer-reset and the drain loop.
  await Promise.resolve();
  await Promise.resolve();
}

type AdapterPrivate = {
  channelId: string;
  publisherState: ReturnType<typeof makePcState>;
  subscriberState: ReturnType<typeof makePcState>;
};

describe("BrowserVoiceAdapter ICE buffering", () => {
  const CHANNEL_ID = "test-channel";
  let adapter: BrowserVoiceAdapter;
  let publisherPc: FakeRtcPc;
  let subscriberPc: FakeRtcPc;
  let warnSpy: ReturnType<typeof vi.spyOn>;
  // Silence noisy info logs from the adapter during tests.
  let logSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    vi.stubGlobal("RTCPeerConnection", FakeRtcPc);
    vi.stubGlobal("RTCIceCandidate", FakeRtcIceCandidate);
    vi.stubGlobal("RTCSessionDescription", FakeRtcSessionDescription);

    adapter = new BrowserVoiceAdapter();
    publisherPc = new FakeRtcPc();
    subscriberPc = new FakeRtcPc();

    // Inject fake state directly. `private` is compile-time-only in TS, and
    // exercising the buffer via `join()` would drag in getUserMedia, ICE
    // fetches, and WebSocket wiring — overkill for a narrow buffer test.
    const a = adapter as unknown as AdapterPrivate;
    a.channelId = CHANNEL_ID;
    a.publisherState = makePcState(publisherPc);
    a.subscriberState = makePcState(subscriberPc);

    warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
    logSpy = vi.spyOn(console, "log").mockImplementation(() => {});
  });

  afterEach(() => {
    warnSpy.mockRestore();
    logSpy.mockRestore();
    vi.unstubAllGlobals();
  });

  it("buffers candidates before remote description is set on publisher PC", async () => {
    // Kick off the answer handler. It resets the buffer and then awaits SRD;
    // our FakeRtcPc holds SRD open via srdGate.
    const answerPromise = adapter.handlePublisherAnswer(
      CHANNEL_ID,
      "v=0\r\nanswer-sdp",
    );
    await flushMicrotasks();

    // SRD is in-flight. remoteDescriptionSet is false, so incoming candidates
    // buffer rather than throwing InvalidStateError.
    const publisherState = (adapter as unknown as AdapterPrivate)
      .publisherState;
    expect(publisherState.remoteDescriptionSet).toBe(false);
    expect(publisherPc.setRemoteDescriptionCalls).toBe(1);

    for (let i = 0; i < 5; i += 1) {
      const result = await adapter.handleIceCandidate(
        CHANNEL_ID,
        candidateJson(i),
        "publisher",
      );
      expect(result.ok).toBe(true);
    }

    // Nothing applied yet — everything is in the buffer.
    expect(publisherPc.addedCandidates).toHaveLength(0);
    expect(publisherState.pendingCandidates).toHaveLength(5);

    // Release SRD; the handler flips the flag and drains.
    publisherPc.openSrd();
    const result = await answerPromise;
    expect(result.ok).toBe(true);

    // All 5 candidates applied in order.
    expect(publisherPc.addedCandidates).toHaveLength(5);
    for (let i = 0; i < 5; i += 1) {
      expect(publisherPc.addedCandidates[i]).toMatchObject({
        sdpMid: "0",
        sdpMLineIndex: 0,
      });
      expect(publisherPc.addedCandidates[i].candidate).toContain(
        `candidate:${i} `,
      );
    }

    // Buffer drained.
    expect(publisherState.pendingCandidates).toHaveLength(0);
    expect(publisherState.remoteDescriptionSet).toBe(true);
  });

  it("buffers candidates before remote description is set on subscriber PC", async () => {
    const offerPromise = adapter.handleSubscriberOffer(
      CHANNEL_ID,
      "v=0\r\noffer-sdp",
    );
    await flushMicrotasks();

    const subscriberState = (adapter as unknown as AdapterPrivate)
      .subscriberState;
    expect(subscriberState.remoteDescriptionSet).toBe(false);
    expect(subscriberPc.setRemoteDescriptionCalls).toBe(1);

    for (let i = 0; i < 5; i += 1) {
      const result = await adapter.handleIceCandidate(
        CHANNEL_ID,
        candidateJson(i),
        "subscriber",
      );
      expect(result.ok).toBe(true);
    }

    expect(subscriberPc.addedCandidates).toHaveLength(0);
    expect(subscriberState.pendingCandidates).toHaveLength(5);

    subscriberPc.openSrd();
    const result = await offerPromise;
    expect(result.ok).toBe(true);
    if (result.ok) {
      // handleSubscriberOffer returns the answer SDP.
      expect(typeof result.value).toBe("string");
      expect(result.value.length).toBeGreaterThan(0);
    }

    expect(subscriberPc.addedCandidates).toHaveLength(5);
    expect(subscriberState.pendingCandidates).toHaveLength(0);
    expect(subscriberState.remoteDescriptionSet).toBe(true);
  });

  it("drops candidates exceeding MAX_PENDING_CANDIDATES (100)", async () => {
    const answerPromise = adapter.handlePublisherAnswer(
      CHANNEL_ID,
      "v=0\r\nanswer-sdp",
    );
    await flushMicrotasks();

    // Queue 105 candidates while SRD is still in-flight. Entries 100..104
    // should be dropped with a console.warn each; return values stay ok:true
    // because the drop is deliberate, not an error condition for the caller.
    for (let i = 0; i < 105; i += 1) {
      const result = await adapter.handleIceCandidate(
        CHANNEL_ID,
        candidateJson(i),
        "publisher",
      );
      expect(result.ok).toBe(true);
    }

    const publisherState = (adapter as unknown as AdapterPrivate)
      .publisherState;
    expect(publisherState.pendingCandidates).toHaveLength(100);

    // Exactly 5 buffer-full warnings were logged.
    const bufferFullWarns = warnSpy.mock.calls.filter(
      (args: unknown[]) =>
        typeof args[0] === "string" &&
        args[0].includes("ICE candidate buffer full"),
    );
    expect(bufferFullWarns).toHaveLength(5);

    // Drain: only the first 100 survive.
    publisherPc.openSrd();
    const result = await answerPromise;
    expect(result.ok).toBe(true);
    expect(publisherPc.addedCandidates).toHaveLength(100);
    expect(publisherPc.addedCandidates[0].candidate).toContain(
      "candidate:0 ",
    );
    expect(publisherPc.addedCandidates[99].candidate).toContain(
      "candidate:99 ",
    );
  });

  it("resets buffer on renegotiation", async () => {
    // Round 1: 3 candidates arrive while the first SRD is in-flight.
    const firstAnswer = adapter.handlePublisherAnswer(
      CHANNEL_ID,
      "v=0\r\nanswer-1",
    );
    await flushMicrotasks();

    for (let i = 0; i < 3; i += 1) {
      const result = await adapter.handleIceCandidate(
        CHANNEL_ID,
        candidateJson(i),
        "publisher",
      );
      expect(result.ok).toBe(true);
    }

    const publisherState = (adapter as unknown as AdapterPrivate)
      .publisherState;
    expect(publisherState.pendingCandidates).toHaveLength(3);

    publisherPc.openSrd();
    const firstResult = await firstAnswer;
    expect(firstResult.ok).toBe(true);
    expect(publisherPc.addedCandidates).toHaveLength(3);
    expect(publisherPc.setRemoteDescriptionCalls).toBe(1);
    expect(publisherState.remoteDescriptionSet).toBe(true);
    expect(publisherState.pendingCandidates).toHaveLength(0);

    // Post-SRD, any new candidate applies directly (no buffering).
    const directResult = await adapter.handleIceCandidate(
      CHANNEL_ID,
      candidateJson(100),
      "publisher",
    );
    expect(directResult.ok).toBe(true);
    expect(publisherPc.addedCandidates).toHaveLength(4);
    expect(publisherPc.addedCandidates[3].candidate).toContain(
      "candidate:100 ",
    );
    expect(publisherState.pendingCandidates).toHaveLength(0);

    // Round 2: renegotiation. A second answer resets the buffer and awaits
    // SRD again; any candidates arriving during that window must re-buffer
    // under the new session and drain cleanly. Stale candidates from round 1
    // must not reappear.
    const secondAnswer = adapter.handlePublisherAnswer(
      CHANNEL_ID,
      "v=0\r\nanswer-2",
    );
    await flushMicrotasks();
    expect(publisherState.remoteDescriptionSet).toBe(false);
    expect(publisherState.pendingCandidates).toHaveLength(0);
    expect(publisherPc.setRemoteDescriptionCalls).toBe(2);

    for (let i = 200; i < 202; i += 1) {
      const result = await adapter.handleIceCandidate(
        CHANNEL_ID,
        candidateJson(i),
        "publisher",
      );
      expect(result.ok).toBe(true);
    }
    expect(publisherState.pendingCandidates).toHaveLength(2);
    // No new addIceCandidate calls yet — still 4 from the direct post-SRD-1 add.
    expect(publisherPc.addedCandidates).toHaveLength(4);

    publisherPc.openSrd();
    const secondResult = await secondAnswer;
    expect(secondResult.ok).toBe(true);

    // The 2 re-buffered candidates drained after the second SRD.
    expect(publisherPc.addedCandidates).toHaveLength(6);
    expect(publisherPc.addedCandidates[4].candidate).toContain(
      "candidate:200 ",
    );
    expect(publisherPc.addedCandidates[5].candidate).toContain(
      "candidate:201 ",
    );
    expect(publisherState.pendingCandidates).toHaveLength(0);
    expect(publisherState.remoteDescriptionSet).toBe(true);
  });
});
