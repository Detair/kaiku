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
