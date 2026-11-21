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
  offers = 0;
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


/**
 * What a browser does around a withdrawn offer (measured in Chromium):
 * after a rollback it fires `negotiationneeded` by itself, and an answer
 * made for an earlier offer is refused once a newer offer is outstanding.
 * The fake also lets a test hold `createOffer` to look into the gap
 * between "negotiation started" and "offer set".
 */
class BrowserPc extends NegPc {
  holdOffer: Promise<void> | null = null;
  async createOffer() {
    if (this.holdOffer) await this.holdOffer;
    return super.createOffer();
  }
  async setLocalDescription(d: { type: string }) {
    await super.setLocalDescription(d);
    if (d.type === "rollback") {
      setTimeout(() => this.dispatchEvent(new Event("negotiationneeded")), 0);
    }
  }
  async setRemoteDescription(desc?: { init?: { sdp?: string } }) {
    const made = /answer-for-(\d+)/.exec(desc?.init?.sdp ?? "");
    if (made && Number(made[1]) !== this.offers && this.signalingState === "have-local-offer") {
      throw new Error("Failed to set remote answer sdp: The order of m-lines in answer doesn't match");
    }
    return super.setRemoteDescription();
  }
}

/** Offer numbers (per connection) in the order they were sent. */
const sentOfferNumbers = () =>
  wsSend.mock.calls
    .map(([m]) => m as { type: string; sdp?: string })
    .filter((m) => m.type === "voice_publisher_offer")
    .map((m) => Number(/offer-(\d+)/.exec(m.sdp ?? "")?.[1]));

describe("BrowserVoiceAdapter: an offer without an answer", () => {
  let gdm: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    pcs.length = 0;
    wsSend.mockReset();
    wsSend.mockResolvedValue(undefined);
    const gum = vi.fn().mockResolvedValue(streamOf(fakeTrack("audio")));
    gdm = vi.fn().mockResolvedValue(streamOf(fakeTrack("video")));
    vi.stubGlobal("RTCPeerConnection", BrowserPc);
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

  const rollbacks = (pc: NegPc) => pc.descriptions.filter((d) => d === "rollback").length;

  /** Let the async steps of a negotiation (offer, import, send) finish. */
  async function settle(): Promise<void> {
    for (let i = 0; i < 5; i += 1) await vi.advanceTimersByTimeAsync(0);
  }

  /**
   * A server that answers offer `n` after `delay(n)` ms (never for null),
   * counted from when the driver first sees it. Time runs in 500 ms
   * steps; the answers are applied in the order sent.
   */
  function server(adapter: BrowserVoiceAdapter, delay: (n: number) => number | null) {
    let now = 0;
    // The join's offer was answered already; every later one is the server's.
    let seen = 1;
    const due: Array<{ at: number; n: number }> = [];
    const results: Array<{ n: number; at: number; ok: boolean }> = [];
    async function run(ms: number): Promise<void> {
      for (let t = 0; t < ms; t += 500) {
        await vi.advanceTimersByTimeAsync(500);
        now += 500;
        const sent = sentOfferNumbers();
        while (sent.length > seen) {
          const n = sent[seen];
          seen += 1;
          const d = delay(n);
          if (d !== null) due.push({ at: now + d, n });
        }
        while (due.length && due[0].at <= now) {
          const { n } = due.shift()!;
          const r = await adapter.handlePublisherAnswer("ch-a", `answer-for-${n}`);
          results.push({ n, at: now, ok: r.ok });
          await settle();
        }
      }
    }
    return { run, results, now: () => now };
  }

  /** Joined, the join's offer answered, then a screen share's offer sent. */
  async function shareOfferSent() {
    const adapter = new BrowserVoiceAdapter();
    const ended = vi.fn();
    const lost = vi.fn();
    adapter.setEventHandlers({ onLocalMediaEnded: ended, onConnectionLost: lost });
    await adapter.join(A);
    const pc = pcs[0];
    await pc.onnegotiationneeded?.();
    expect((await adapter.handlePublisherAnswer("ch-a", "answer-for-1")).ok).toBe(true);
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    await adapter.captureScreen({ streamId: "s1" });
    await adapter.publishScreenShare("s1");
    await pc.onnegotiationneeded?.();
    expect(sentOfferNumbers()).toEqual([1, 2]);
    expect(pc.signalingState).toBe("have-local-offer");
    return { adapter, ended, lost, pc };
  }

  it("(a) a server that never answers: one retry, then the connection is reported lost once", async () => {
    const { adapter, ended, lost, pc } = await shareOfferSent();
    const s = server(adapter, () => null);

    await s.run(9_500);
    expect(rollbacks(pc)).toBe(0);
    await s.run(1_000); // 10 s: withdrawn, the browser offers again
    expect(rollbacks(pc)).toBe(1);
    expect(ended).toHaveBeenCalledWith("screen", "s1", "not_sent");
    expect(sentOfferNumbers()).toEqual([1, 2, 3]);

    await s.run(19_000); // 29.5 s
    expect(lost).not.toHaveBeenCalled();
    await s.run(1_000); // 30.5 s: the retry's 20 s are over
    expect(lost).toHaveBeenCalledTimes(1);
    expect(lost).toHaveBeenCalledWith(A);
    const sentAtLost = wsSend.mock.calls.length;

    await s.run(90_000);
    expect(sentOfferNumbers()).toEqual([1, 2, 3]);
    expect(wsSend.mock.calls.length).toBe(sentAtLost);
    expect(lost).toHaveBeenCalledTimes(1);
    expect(ended).toHaveBeenCalledTimes(1);
  });

  it("(b) a server that answers every offer 12 s late gets the retry through", async () => {
    const { adapter, ended, lost, pc } = await shareOfferSent();
    const s = server(adapter, () => 12_000);
    await s.run(120_000);

    expect(sentOfferNumbers()).toEqual([1, 2, 3]);
    // The first answer meets the retry and is ignored; the retry's applies.
    expect(s.results.map((r) => [r.n, r.ok])).toEqual([
      [2, true],
      [3, true],
    ]);
    expect(pc.signalingState).toBe("stable");
    expect(rollbacks(pc)).toBe(1);
    expect(ended).toHaveBeenCalledTimes(1);
    expect(lost).not.toHaveBeenCalled();
  });

  it("(c) a dropped offer: the retry is answered at once and a later share goes out", async () => {
    const { adapter, ended, lost, pc } = await shareOfferSent();
    const s = server(adapter, (n) => (n === 2 ? null : 0));
    await s.run(12_000);
    expect(sentOfferNumbers()).toEqual([1, 2, 3]);
    expect(pc.signalingState).toBe("stable");
    expect(ended).toHaveBeenCalledTimes(1);
    expect(ended).toHaveBeenCalledWith("screen", "s1", "not_sent");

    await adapter.captureScreen({ streamId: "s2" });
    await adapter.publishScreenShare("s2");
    await pc.onnegotiationneeded?.();
    await s.run(120_000);
    expect(sentOfferNumbers()).toEqual([1, 2, 3, 4]);
    expect(pc.signalingState).toBe("stable");
    expect(ended).toHaveBeenCalledTimes(1);
    expect(lost).not.toHaveBeenCalled();
  });

  it("(d) an answer in time: one offer, and its deadline is gone", async () => {
    const { adapter, ended, lost, pc } = await shareOfferSent();
    const s = server(adapter, (n) => (n === 2 ? 1_000 : null));
    await s.run(120_000);
    expect(sentOfferNumbers()).toEqual([1, 2]);
    expect(rollbacks(pc)).toBe(0);
    expect(ended).not.toHaveBeenCalled();
    expect(lost).not.toHaveBeenCalled();
  });

  it("(d2) after an answer in time, a later unanswered offer waits its full 10 s", async () => {
    const { adapter, lost, pc } = await shareOfferSent();
    const s = server(adapter, (n) => (n === 2 ? 1_000 : null));
    await s.run(30_000);
    expect(pc.signalingState).toBe("stable");

    await adapter.captureScreen({ streamId: "s2" });
    await adapter.publishScreenShare("s2");
    await pc.onnegotiationneeded?.();
    await s.run(9_500);
    expect(rollbacks(pc)).toBe(0);
    await s.run(1_000);
    expect(rollbacks(pc)).toBe(1);
    // Its retry has 20 s again: nothing is lost before that.
    await s.run(19_000);
    expect(lost).not.toHaveBeenCalled();
    await s.run(1_000);
    expect(lost).toHaveBeenCalledTimes(1);
  });

  it("(e) a second refused answer while the retry waits: lost, no further offer", async () => {
    const { adapter, lost, pc } = await shareOfferSent();
    const s = server(adapter, () => null);
    await s.run(10_500);
    expect(sentOfferNumbers()).toEqual([1, 2, 3]);

    // The first refused answer counts as the late answer of offer 2.
    await adapter.handlePublisherAnswer("ch-a", "answer-for-2");
    await settle();
    expect(pc.signalingState).toBe("have-local-offer");
    expect(lost).not.toHaveBeenCalled();
    // The lock stays held: a change now waits.
    await pc.onnegotiationneeded?.();
    await settle();
    expect(sentOfferNumbers()).toEqual([1, 2, 3]);

    // A second refused answer ends it.
    await adapter.handlePublisherAnswer("ch-a", "answer-for-99");
    await settle();
    expect(lost).toHaveBeenCalledTimes(1);
    await s.run(120_000);
    expect(sentOfferNumbers()).toEqual([1, 2, 3]);
    expect(lost).toHaveBeenCalledTimes(1);
  });

  it("(f) a send that fails withdraws the offer and ends its share", async () => {
    const adapter = new BrowserVoiceAdapter();
    const ended = vi.fn();
    const lost = vi.fn();
    adapter.setEventHandlers({ onLocalMediaEnded: ended, onConnectionLost: lost });
    await adapter.join(A);
    const pc = pcs[0];
    await pc.onnegotiationneeded?.();
    await adapter.handlePublisherAnswer("ch-a", "answer-for-1");
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    await adapter.captureScreen({ streamId: "s1" });
    await adapter.publishScreenShare("s1");
    wsSend.mockRejectedValueOnce(new Error("WebSocket not connected"));
    await pc.onnegotiationneeded?.();
    await settle();

    expect(rollbacks(pc)).toBe(1);
    expect(ended).toHaveBeenCalledWith("screen", "s1", "not_sent");
    // The lock was freed: the browser's own next negotiation went out.
    expect(sentOfferNumbers()).toEqual([1, 2, 3]);
    // Offer 2 never reached the server; the retry is answered.
    const s = server(adapter, (n) => (n === 2 ? null : 0));
    await s.run(120_000);
    expect(pc.signalingState).toBe("stable");
    expect(lost).not.toHaveBeenCalled();
  });

  it("(g) an applied answer ends the row: the next unanswered offer waits 10 s again", async () => {
    const { adapter, lost, pc } = await shareOfferSent();
    const s = server(adapter, (n) => (n === 3 ? 0 : null));
    await s.run(12_000); // 2 withdrawn at 10 s, its retry 3 answered
    expect(pc.signalingState).toBe("stable");
    expect(rollbacks(pc)).toBe(1);

    await adapter.captureScreen({ streamId: "s2" });
    await adapter.publishScreenShare("s2");
    await pc.onnegotiationneeded?.();
    await s.run(9_500);
    expect(rollbacks(pc)).toBe(1);
    await s.run(1_000);
    expect(rollbacks(pc)).toBe(2);
    expect(lost).not.toHaveBeenCalled();
    await s.run(120_000);
    expect(lost).toHaveBeenCalledTimes(1);
  });

  it("(h1) a leave while the first offer waits: nothing fires", async () => {
    const { adapter, ended, lost, pc } = await shareOfferSent();
    const s = server(adapter, () => null);
    await s.run(5_000);
    await adapter.leave(A);
    const sentAtLeave = wsSend.mock.calls.length;
    const descriptionsAtLeave = pc.descriptions.length;
    await s.run(120_000);
    expect(pc.descriptions.length).toBe(descriptionsAtLeave);
    expect(wsSend.mock.calls.length).toBe(sentAtLeave);
    expect(ended).not.toHaveBeenCalled();
    expect(lost).not.toHaveBeenCalled();
  });

  it("(h2) a leave while the retry waits: nothing fires", async () => {
    const { adapter, lost, pc } = await shareOfferSent();
    const s = server(adapter, () => null);
    await s.run(15_000);
    expect(sentOfferNumbers()).toEqual([1, 2, 3]);
    await adapter.leave(A);
    const sentAtLeave = wsSend.mock.calls.length;
    const descriptionsAtLeave = pc.descriptions.length;
    await s.run(120_000);
    expect(pc.descriptions.length).toBe(descriptionsAtLeave);
    expect(wsSend.mock.calls.length).toBe(sentAtLeave);
    expect(lost).not.toHaveBeenCalled();
  });

  it("(h3) the old session's deadline does not touch the new session's offer", async () => {
    const { adapter, ended, lost, pc } = await shareOfferSent();
    await vi.advanceTimersByTimeAsync(5_000);
    await adapter.leave(A);
    const descriptionsAtLeave = pc.descriptions.length;

    const B = { token: 2, channelId: "ch-a" };
    await adapter.join(B);
    const pc2 = pcs[2]; // [publisher, subscriber] per join
    await pc2.onnegotiationneeded?.();
    expect(pc2.signalingState).toBe("have-local-offer");

    // The old offer's 10 s pass: the new session's offer stays, its lock too.
    await vi.advanceTimersByTimeAsync(6_000);
    await settle();
    expect(pc.descriptions.length).toBe(descriptionsAtLeave);
    expect(rollbacks(pc2)).toBe(0);
    const offersOfB = sentOfferNumbers().length;
    await pc2.onnegotiationneeded?.();
    await settle();
    expect(sentOfferNumbers().length).toBe(offersOfB);
    // Its own deadline still runs: 10 s after its offer it is withdrawn.
    await vi.advanceTimersByTimeAsync(4_000);
    await settle();
    expect(rollbacks(pc2)).toBe(1);
    expect(ended).not.toHaveBeenCalledWith("screen", "s1", "not_sent");
    expect(lost).not.toHaveBeenCalled();
  });

  it("(h4) a failed send of the old session leaves the new session's lock and deadline", async () => {
    const adapter = new BrowserVoiceAdapter();
    const lost = vi.fn();
    adapter.setEventHandlers({ onConnectionLost: lost });
    await adapter.join(A);
    const pc = pcs[0];
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    // The old session's join offer is still being sent when it is left.
    let failSend!: (e: Error) => void;
    wsSend.mockImplementationOnce(() => new Promise((_, reject) => (failSend = reject)));
    const oldNegotiation = pc.onnegotiationneeded?.();
    await settle();
    await adapter.leave(A);

    const B = { token: 2, channelId: "ch-a" };
    await adapter.join(B);
    const pc2 = pcs[2];
    await pc2.onnegotiationneeded?.();
    expect(pc2.signalingState).toBe("have-local-offer");

    failSend(new Error("WebSocket not connected"));
    await oldNegotiation;
    await settle();

    // The new session's lock is still held: a change waits for its answer.
    const offersOfB = sentOfferNumbers().length;
    await pc2.onnegotiationneeded?.();
    await settle();
    expect(sentOfferNumbers().length).toBe(offersOfB);
    // And its deadline still runs.
    await vi.advanceTimersByTimeAsync(10_000);
    await settle();
    expect(rollbacks(pc2)).toBe(1);
  });

  it("an answer while a negotiation is still being made leaves the lock held", async () => {
    const { adapter, pc } = await shareOfferSent();
    await adapter.handlePublisherAnswer("ch-a", "answer-for-2");
    expect(pc.signalingState).toBe("stable");

    // A new negotiation starts; its offer is not made yet.
    let makeOffer!: () => void;
    (pc as BrowserPc).holdOffer = new Promise((r) => (makeOffer = r));
    await adapter.captureScreen({ streamId: "s2" });
    await adapter.publishScreenShare("s2");
    const negotiation = pc.onnegotiationneeded?.();
    await settle();
    // A stray answer arrives in that gap: ignored.
    await adapter.handlePublisherAnswer("ch-a", "answer-for-2");
    await settle();
    // A further change waits for the offer under way.
    void pc.onnegotiationneeded?.();
    (pc as BrowserPc).holdOffer = null;
    makeOffer();
    await negotiation;
    await settle();
    expect(sentOfferNumbers()).toEqual([1, 2, 3]);
    // Exactly one offer was made locally for the two changes.
    expect(pc.descriptions).toEqual(["offer", "offer", "offer"]);
    expect(pc.signalingState).toBe("have-local-offer");
    expect(rollbacks(pc)).toBe(0);
  });

  it("an answer in stable uses up the expected late answer: the retry's refusal ends it at once", async () => {
    const { adapter, lost, pc } = await shareOfferSent();
    // The retry is still being made when offer 2's late answer arrives.
    let makeOffer!: () => void;
    (pc as BrowserPc).holdOffer = new Promise((r) => (makeOffer = r));
    await vi.advanceTimersByTimeAsync(10_000);
    await vi.advanceTimersByTimeAsync(1); // the browser's own next negotiation starts
    await settle();
    expect(rollbacks(pc)).toBe(1);
    expect(pc.signalingState).toBe("stable");
    await adapter.handlePublisherAnswer("ch-a", "answer-for-2");
    (pc as BrowserPc).holdOffer = null;
    makeOffer();
    await settle();
    expect(sentOfferNumbers()).toEqual([1, 2, 3]);

    // The retry's own answer is refused: no late answer is owed any more.
    await adapter.handlePublisherAnswer("ch-a", "answer-for-99");
    await settle();
    expect(lost).toHaveBeenCalledTimes(1);
  });

  it("a refused answer is no expired deadline: the retry's refusal ends it at once", async () => {
    const { adapter, ended, lost, pc } = await shareOfferSent();
    await adapter.handlePublisherAnswer("ch-a", "answer-for-99");
    await settle();
    expect(rollbacks(pc)).toBe(1);
    expect(ended).toHaveBeenCalledWith("screen", "s1", "not_sent");
    expect(sentOfferNumbers()).toEqual([1, 2, 3]);

    await adapter.handlePublisherAnswer("ch-a", "answer-for-98");
    await settle();
    expect(lost).toHaveBeenCalledTimes(1);
  });

  it("a leave while the giving-up withdraw is under way raises no lost", async () => {
    const { adapter, lost, pc } = await shareOfferSent();
    await vi.advanceTimersByTimeAsync(10_500);
    await settle();
    let release!: () => void;
    const hold = new Promise<void>((r) => (release = r));
    const original = pc.setLocalDescription.bind(pc);
    pc.setLocalDescription = async (d: { type: string }) => {
      if (d.type === "rollback") await hold;
      return original(d);
    };
    await vi.advanceTimersByTimeAsync(20_000); // the retry's deadline: its withdraw waits
    await adapter.leave(A);
    release();
    await settle();
    expect(lost).not.toHaveBeenCalled();
  });
});
