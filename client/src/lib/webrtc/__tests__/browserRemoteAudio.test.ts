/**
 * Received audio is played from a stream of its own that holds exactly the
 * received track, not from the stream object of the track event (two
 * received tracks can share that object and its ids for a moment).
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/tauri", () => ({
  wsSend: vi.fn().mockResolvedValue(undefined),
  wsStatus: () => Promise.resolve({ type: "connected" }),
  wsConnect: () => Promise.resolve(),
  getServerUrl: () => "http://test",
  getAccessToken: () => "tok",
}));

import { BrowserVoiceAdapter } from "../browser";
import type { RemoteTrack } from "../types";

const pcs: FakePc[] = [];
class FakePc {
  signalingState: RTCSignalingState = "stable";
  onnegotiationneeded: (() => Promise<void>) | null = null;
  ontrack: ((e: { track: unknown; streams: unknown[] }) => void) | null = null;
  constructor() {
    pcs.push(this);
  }
  addTrack() {
    return {};
  }
  removeTrack() {}
  async createOffer() {
    return { type: "offer", sdp: "offer" };
  }
  async setLocalDescription() {}
  async setRemoteDescription() {}
  async addIceCandidate() {}
  close() {}
  addEventListener() {}
  removeEventListener() {}
  dispatchEvent() {
    return true;
  }
}

let nextStream = 0;
class FakeMediaStream {
  readonly id: string;
  private readonly tracks: unknown[];
  constructor(tracks: unknown[] = [], id?: string) {
    nextStream += 1;
    this.id = id ?? `own-${nextStream}`;
    this.tracks = [...tracks];
  }
  getTracks() {
    return this.tracks;
  }
  getAudioTracks() {
    return this.tracks.filter((t) => (t as { kind: string }).kind === "audio");
  }
  getVideoTracks() {
    return this.tracks.filter((t) => (t as { kind: string }).kind === "video");
  }
}

function audioTrack(id: string) {
  return {
    id,
    kind: "audio",
    label: "",
    readyState: "live",
    enabled: true,
    stop: vi.fn(),
    onended: null as null | (() => void),
    addEventListener: () => {},
  };
}

describe("BrowserVoiceAdapter: received audio", () => {
  beforeEach(() => {
    pcs.length = 0;
    vi.stubGlobal("RTCPeerConnection", FakePc);
    vi.stubGlobal("MediaStream", FakeMediaStream);
    vi.stubGlobal("navigator", {
      mediaDevices: {
        getUserMedia: vi.fn().mockResolvedValue(new FakeMediaStream([audioTrack("mic")], "local")),
      },
    });
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({ ok: true, json: async () => ({ ice_servers: [] }) }),
    );
    vi.spyOn(console, "log").mockImplementation(() => {});
    vi.spyOn(console, "warn").mockImplementation(() => {});
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  async function joined() {
    const adapter = new BrowserVoiceAdapter();
    const played: RemoteTrack[] = [];
    const removed = vi.fn();
    adapter.setEventHandlers({
      onRemoteTrack: (t) => played.push(t),
      onRemoteTrackRemoved: removed,
    });
    await adapter.join({ token: 1, channelId: "ch-a" });
    const subscriber = pcs[1]; // [publisher, subscriber]
    return { adapter, played, removed, subscriber };
  }

  it("plays a stream of its own holding exactly the received track", async () => {
    const { played, subscriber } = await joined();
    const t1 = audioTrack("u2:microphone");
    const other = audioTrack("other");
    const eventStream = new FakeMediaStream([t1, other], "u2:microphone");
    subscriber.ontrack?.({ track: t1, streams: [eventStream] });

    expect(played).toHaveLength(1);
    expect(played[0].userId).toBe("u2");
    expect(played[0].stream).not.toBe(eventStream);
    expect(played[0].stream?.getTracks()).toEqual([t1]);
  });

  it("a second track of the same person replaces the first; the first ending later removes nothing", async () => {
    const { adapter, played, removed, subscriber } = await joined();
    // Both tracks arrive with the same stream object and ids.
    const shared = new FakeMediaStream([], "u2:microphone");
    const t1 = audioTrack("u2:microphone");
    const t2 = audioTrack("u2:microphone");
    subscriber.ontrack?.({ track: t1, streams: [shared] });
    subscriber.ontrack?.({ track: t2, streams: [shared] });

    expect(played).toHaveLength(2);
    expect(played[1].stream?.getTracks()).toEqual([t2]);
    expect(played[1].stream).not.toBe(played[0].stream);

    // Deafening reaches the track that plays now.
    await adapter.setDeafen(true);
    expect(t2.enabled).toBe(false);

    t1.onended?.();
    expect(removed).not.toHaveBeenCalled();
    t2.onended?.();
    expect(removed).toHaveBeenCalledWith("u2");
  });
});
