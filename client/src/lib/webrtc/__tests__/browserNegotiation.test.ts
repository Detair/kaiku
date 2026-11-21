/**
 * An answer the browser refuses leaves the publisher connection usable:
 * the local offer is withdrawn (rollback to stable), the publication that
 * asked for the negotiation ends through the owner's stop path, and the
 * next start negotiates normally.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const wsSend = vi.fn().mockResolvedValue(undefined);

vi.mock("@/lib/tauri", () => ({
  wsSend: (...a: unknown[]) => wsSend(...a),
  wsStatus: () => Promise.resolve({ type: "connected" }),
  wsConnect: () => Promise.resolve(),
  getServerUrl: () => "http://test",
  getAccessToken: () => "tok",
}));

import { BrowserVoiceAdapter } from "../browser";

const A = { token: 1, channelId: "ch-a" };

const pcs: NegPc[] = [];
class NegPc {
  signalingState: RTCSignalingState = "stable";
  onnegotiationneeded: (() => Promise<void>) | null = null;
  descriptions: string[] = [];
  refuseNextAnswer = false;
  tracks: unknown[] = [];
  private offers = 0;
  constructor() {
    pcs.push(this);
  }
  addTrack(t: unknown) {
    this.tracks.push(t);
    return { track: t };
  }
  removeTrack() {}
  addTransceiver(t: unknown) {
    this.tracks.push(t);
    return { sender: { track: t } };
  }
  async createOffer() {
    this.offers += 1;
    return { type: "offer", sdp: `offer-${this.offers}` };
  }
  async setLocalDescription(d: { type: string }) {
    this.descriptions.push(d.type);
    if (d.type === "rollback") this.signalingState = "stable";
    else if (d.type === "offer") this.signalingState = "have-local-offer";
  }
  async setRemoteDescription() {
    if (this.signalingState !== "have-local-offer") throw new Error("InvalidStateError");
    if (this.refuseNextAnswer) {
      this.refuseNextAnswer = false;
      throw new Error("Failed to set remote answer sdp");
    }
    this.signalingState = "stable";
  }
  async addIceCandidate() {}
  close() {
    this.signalingState = "closed";
  }
  addEventListener() {}
  removeEventListener() {}
  dispatchEvent(e: Event) {
    if (e.type === "negotiationneeded") void this.onnegotiationneeded?.();
    return true;
  }
}

function fakeTrack(kind: string) {
  return {
    kind,
    label: "",
    readyState: "live",
    enabled: true,
    stop: vi.fn(),
    addEventListener: () => {},
  };
}

function streamOf(...tracks: ReturnType<typeof fakeTrack>[]): MediaStream {
  return {
    id: "s",
    getTracks: () => tracks,
    getAudioTracks: () => tracks.filter((t) => t.kind === "audio"),
    getVideoTracks: () => tracks.filter((t) => t.kind === "video"),
  } as unknown as MediaStream;
}

const offersSent = () =>
  wsSend.mock.calls.filter(([m]) => (m as { type: string }).type === "voice_publisher_offer").length;

describe("BrowserVoiceAdapter: a refused answer", () => {
  let gum: ReturnType<typeof vi.fn>;
  let gdm: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    pcs.length = 0;
    wsSend.mockClear();
    gum = vi.fn().mockResolvedValue(streamOf(fakeTrack("audio")));
    gdm = vi.fn().mockResolvedValue(streamOf(fakeTrack("video")));
    vi.stubGlobal("RTCPeerConnection", NegPc);
    vi.stubGlobal(
      "RTCSessionDescription",
      class {
        constructor(readonly init: unknown) {}
      },
    );
    vi.stubGlobal("navigator", { mediaDevices: { getUserMedia: gum, getDisplayMedia: gdm } });
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({ ok: true, json: async () => ({ ice_servers: [] }) }),
    );
    vi.spyOn(console, "log").mockImplementation(() => {});
    vi.spyOn(console, "error").mockImplementation(() => {});
    vi.spyOn(console, "warn").mockImplementation(() => {});
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("rolls back, ends the camera that asked for it, and a share afterwards arrives", async () => {
    const adapter = new BrowserVoiceAdapter();
    const ended = vi.fn();
    adapter.setEventHandlers({ onLocalMediaEnded: ended });
    await adapter.join(A);
    const pc = pcs[0];
    // The join's offer and its answer.
    await pc.onnegotiationneeded?.();
    expect((await adapter.handlePublisherAnswer("ch-a", "answer-1")).ok).toBe(true);

    // The camera's offer: the browser refuses the server's answer.
    gum.mockResolvedValueOnce(streamOf(fakeTrack("video")));
    await adapter.captureWebcam({});
    await adapter.publishWebcam();
    await pc.onnegotiationneeded?.();
    pc.refuseNextAnswer = true;
    expect((await adapter.handlePublisherAnswer("ch-a", "answer-2")).ok).toBe(false);
    expect(pc.signalingState).toBe("stable");
    expect(pc.descriptions[pc.descriptions.length - 1]).toBe("rollback");
    expect(ended).toHaveBeenCalledWith("webcam", null, "not_sent");

    // A screen share started afterwards negotiates normally.
    await adapter.captureScreen({ streamId: "s1" });
    await adapter.publishScreenShare("s1");
    await pc.onnegotiationneeded?.();
    expect(offersSent()).toBe(3);
    expect((await adapter.handlePublisherAnswer("ch-a", "answer-3")).ok).toBe(true);
    expect(ended).not.toHaveBeenCalledWith("screen", "s1", expect.anything());
    expect(pc.signalingState).toBe("stable");
  });

  it("a refused answer to the join's offer only rolls back", async () => {
    const adapter = new BrowserVoiceAdapter();
    const ended = vi.fn();
    adapter.setEventHandlers({ onLocalMediaEnded: ended });
    await adapter.join(A);
    const pc = pcs[0];
    await pc.onnegotiationneeded?.();
    pc.refuseNextAnswer = true;
    expect((await adapter.handlePublisherAnswer("ch-a", "answer-1")).ok).toBe(false);
    expect(pc.signalingState).toBe("stable");
    expect(ended).not.toHaveBeenCalled();
  });
});

describe("BrowserVoiceAdapter: an offer without an answer", () => {
  let gdm: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    pcs.length = 0;
    wsSend.mockClear();
    const gum = vi.fn().mockResolvedValue(streamOf(fakeTrack("audio")));
    gdm = vi.fn().mockResolvedValue(streamOf(fakeTrack("video")));
    vi.stubGlobal("RTCPeerConnection", NegPc);
    vi.stubGlobal(
      "RTCSessionDescription",
      class {
        constructor(readonly init: unknown) {}
      },
    );
    vi.stubGlobal("navigator", { mediaDevices: { getUserMedia: gum, getDisplayMedia: gdm } });
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({ ok: true, json: async () => ({ ice_servers: [] }) }),
    );
    vi.spyOn(console, "log").mockImplementation(() => {});
    vi.spyOn(console, "error").mockImplementation(() => {});
    vi.spyOn(console, "warn").mockImplementation(() => {});
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  /** Let the async steps of a negotiation (offer, import, send) finish. */
  async function settle(): Promise<void> {
    for (let i = 0; i < 5; i += 1) await vi.advanceTimersByTimeAsync(0);
  }

  /** Joined, the join's offer answered, then a screen share's offer sent. */
  async function shareOfferSent() {
    const adapter = new BrowserVoiceAdapter();
    const ended = vi.fn();
    adapter.setEventHandlers({ onLocalMediaEnded: ended });
    await adapter.join(A);
    const pc = pcs[0];
    await pc.onnegotiationneeded?.();
    expect((await adapter.handlePublisherAnswer("ch-a", "answer-1")).ok).toBe(true);
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    await adapter.captureScreen({ streamId: "s1" });
    await adapter.publishScreenShare("s1");
    await pc.onnegotiationneeded?.();
    expect(offersSent()).toBe(2);
    expect(pc.signalingState).toBe("have-local-offer");
    return { adapter, ended, pc };
  }

  it("ends like a refused one after 10 s and sends what was queued", async () => {
    const { adapter, ended, pc } = await shareOfferSent();
    // A change while the offer is out is queued behind it.
    await pc.onnegotiationneeded?.();
    expect(offersSent()).toBe(2);

    await vi.advanceTimersByTimeAsync(9_999);
    expect(ended).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    await settle();

    expect(pc.descriptions).toContain("rollback");
    expect(ended).toHaveBeenCalledWith("screen", "s1", "not_sent");
    // The queued negotiation went out as a new offer, and it completes.
    expect(offersSent()).toBe(3);
    expect((await adapter.handlePublisherAnswer("ch-a", "answer-3")).ok).toBe(true);
    expect(pc.signalingState).toBe("stable");
  });

  it("an answer in time stops the deadline; a later offer keeps its own", async () => {
    const { adapter, ended, pc } = await shareOfferSent();
    expect((await adapter.handlePublisherAnswer("ch-a", "answer-2")).ok).toBe(true);

    // A later change, 5 s on, sends its own offer.
    await vi.advanceTimersByTimeAsync(5_000);
    await pc.onnegotiationneeded?.();
    expect(offersSent()).toBe(3);

    // 10 s after the first offer: its deadline is gone, the later offer stays.
    await vi.advanceTimersByTimeAsync(7_000);
    await settle();
    expect(pc.descriptions).not.toContain("rollback");
    expect(pc.signalingState).toBe("have-local-offer");
    expect((await adapter.handlePublisherAnswer("ch-a", "answer-3")).ok).toBe(true);
    expect(pc.signalingState).toBe("stable");

    await vi.advanceTimersByTimeAsync(30_000);
    await settle();
    expect(pc.descriptions).not.toContain("rollback");
    expect(ended).not.toHaveBeenCalled();
    expect(offersSent()).toBe(3);
  });

  it("an answer after the deadline is ignored and leaves the lock free", async () => {
    const { adapter, ended, pc } = await shareOfferSent();
    await vi.advanceTimersByTimeAsync(10_000);
    await settle();
    expect(ended).toHaveBeenCalledTimes(1);
    expect(offersSent()).toBe(2);

    // The late answer finds no offer outstanding: nothing changes.
    await adapter.handlePublisherAnswer("ch-a", "answer-2");
    await settle();
    expect(ended).toHaveBeenCalledTimes(1);
    expect(offersSent()).toBe(2);
    expect(pc.signalingState).toBe("stable");

    // The lock is free: the next change sends an offer at once.
    await pc.onnegotiationneeded?.();
    expect(offersSent()).toBe(3);
  });

  it("an answer after the deadline that the next offer refuses ends that offer once", async () => {
    const { adapter, ended, pc } = await shareOfferSent();
    await pc.onnegotiationneeded?.(); // queued
    await vi.advanceTimersByTimeAsync(10_000);
    await settle();
    expect(offersSent()).toBe(3);

    // The late answer meets the newer offer, and the browser refuses it.
    pc.refuseNextAnswer = true;
    await adapter.handlePublisherAnswer("ch-a", "answer-2");
    await settle();
    expect(pc.signalingState).toBe("stable");
    expect(offersSent()).toBe(3);

    // Its own deadline is gone with it: nothing more happens.
    await vi.advanceTimersByTimeAsync(30_000);
    await settle();
    expect(offersSent()).toBe(3);
    expect(ended).toHaveBeenCalledTimes(1);
    await pc.onnegotiationneeded?.();
    expect(offersSent()).toBe(4);
  });

  it("a leave before the deadline leaves nothing to fire", async () => {
    const { adapter, ended, pc } = await shareOfferSent();
    await vi.advanceTimersByTimeAsync(5_000);
    await adapter.leave(A);
    const descriptionsAtLeave = pc.descriptions.length;

    // A new session, its join offer still waiting for an answer.
    const B = { token: 2, channelId: "ch-a" };
    await adapter.join(B);
    const pc2 = pcs[2]; // [publisher, subscriber] per join
    await pc2.onnegotiationneeded?.();
    const offersOfB = offersSent();

    // The old offer's deadline (10 s) passes: nothing of the new session moves.
    await vi.advanceTimersByTimeAsync(6_000);
    await settle();
    expect(pc.descriptions.length).toBe(descriptionsAtLeave);
    expect(pc2.descriptions).not.toContain("rollback");
    expect(ended).not.toHaveBeenCalledWith("screen", "s1", "not_sent");
    // The new session's lock is still held: a change waits for its answer.
    await pc2.onnegotiationneeded?.();
    expect(offersSent()).toBe(offersOfB);
    expect((await adapter.handlePublisherAnswer("ch-a", "answer-b")).ok).toBe(true);
    await settle();
    expect(offersSent()).toBe(offersOfB + 1);
  });
});
