/**
 * Browser adapter: a screen share or camera is captured first and only
 * sent (track added to the publisher connection) when published, after the
 * server accepted it (C30). A capture that ends by itself is reported once.
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

const pcs: FakePc[] = [];
class FakePc {
  closed = false;
  signalingState = "stable";
  tracks: unknown[] = [];
  removed: unknown[] = [];
  transceivers: unknown[] = [];
  constructor() {
    pcs.push(this);
  }
  addTrack(t: unknown) {
    this.tracks.push(t);
    return { track: t };
  }
  removeTrack(sender: unknown) {
    this.removed.push(sender);
  }
  addTransceiver(t: unknown) {
    this.transceivers.push(t);
    return { sender: { track: t } };
  }
  close() {
    this.closed = true;
    this.signalingState = "closed";
  }
  addEventListener() {}
  removeEventListener() {}
}

function fakeTrack(kind: string, label = "") {
  const listeners: Array<() => void> = [];
  return {
    kind,
    label,
    readyState: "live",
    enabled: true,
    stop: vi.fn(),
    addEventListener: (_: string, fn: () => void) => listeners.push(fn),
    end: () => listeners.forEach((fn) => fn()),
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

describe("BrowserVoiceAdapter capture then publish", () => {
  let gdm: ReturnType<typeof vi.fn>;
  let gum: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    pcs.length = 0;
    gum = vi.fn().mockResolvedValue(streamOf(fakeTrack("audio")));
    gdm = vi.fn();
    vi.stubGlobal("RTCPeerConnection", FakePc);
    vi.stubGlobal("navigator", { mediaDevices: { getUserMedia: gum, getDisplayMedia: gdm } });
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({ ok: true, json: async () => ({ ice_servers: [] }) }),
    );
    vi.spyOn(console, "log").mockImplementation(() => {});
    vi.spyOn(console, "error").mockImplementation(() => {});
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  async function joined() {
    const adapter = new BrowserVoiceAdapter();
    await adapter.join(A);
    return { adapter, publisher: pcs[0] };
  }

  it("captureScreen adds no track; publishScreenShare adds video and audio", async () => {
    const { adapter, publisher } = await joined();
    const video = fakeTrack("video", "Window A");
    const audio = fakeTrack("audio");
    gdm.mockResolvedValue(streamOf(video, audio));
    const before = publisher.tracks.length;

    const captured = await adapter.captureScreen({ streamId: "s1", withAudio: true });
    expect(captured).toEqual({ ok: true, value: { hasAudio: true, sourceLabel: "Window A" } });
    expect(publisher.tracks.length).toBe(before);

    expect(await adapter.publishScreenShare("s1")).toEqual({ ok: true, value: undefined });
    expect(publisher.tracks.slice(before)).toEqual([video, audio]);
  });

  it("stop of a captured, never published share stops the capture and removes nothing", async () => {
    const { adapter, publisher } = await joined();
    const video = fakeTrack("video");
    gdm.mockResolvedValue(streamOf(video));
    await adapter.captureScreen({ streamId: "s1" });

    await adapter.stopScreenShare("s1");
    expect(video.stop).toHaveBeenCalled();
    expect(publisher.removed).toEqual([]);
    expect(adapter.isScreenSharing()).toBe(false);
  });

  it("stop of a published share removes its senders", async () => {
    const { adapter, publisher } = await joined();
    const video = fakeTrack("video");
    gdm.mockResolvedValue(streamOf(video));
    await adapter.captureScreen({ streamId: "s1" });
    await adapter.publishScreenShare("s1");

    await adapter.stopScreenShare("s1");
    expect(publisher.removed).toEqual([{ track: video }]);
    expect(video.stop).toHaveBeenCalled();
  });

  it("the browser's own Stop sharing (track ended) reports onLocalMediaEnded once", async () => {
    const { adapter } = await joined();
    const ended = vi.fn();
    adapter.setEventHandlers({ onLocalMediaEnded: ended });
    const video = fakeTrack("video");
    gdm.mockResolvedValue(streamOf(video));
    await adapter.captureScreen({ streamId: "s1" });
    await adapter.publishScreenShare("s1");

    video.end();
    expect(ended).toHaveBeenCalledWith("screen", "s1");

    // After the owner's stop the same track ending again is not news.
    await adapter.stopScreenShare("s1");
    video.end();
    expect(ended).toHaveBeenCalledTimes(1);
  });

  it("cancel in the browser picker (NotAllowedError) is a cancel, not a permission error", async () => {
    const { adapter } = await joined();
    gdm.mockRejectedValue(new DOMException("Permission denied", "NotAllowedError"));
    const result = await adapter.captureScreen({ streamId: "s1" });
    expect(result).toEqual({ ok: false, error: expect.objectContaining({ type: "cancelled" }) });
  });

  it("a block by the system is a permission error", async () => {
    const { adapter } = await joined();
    gdm.mockRejectedValue(new DOMException("Permission denied by system", "NotAllowedError"));
    const result = await adapter.captureScreen({ streamId: "s1" });
    expect(result).toEqual({
      ok: false,
      error: expect.objectContaining({ type: "permission_denied" }),
    });
  });

  it("webcam: capture adds nothing, publish adds the transceiver, ended is reported", async () => {
    const { adapter, publisher } = await joined();
    const ended = vi.fn();
    adapter.setEventHandlers({ onLocalMediaEnded: ended });
    const cam = fakeTrack("video");
    gum.mockResolvedValueOnce(streamOf(cam));

    expect(await adapter.captureWebcam({})).toEqual({ ok: true, value: undefined });
    expect(publisher.transceivers).toEqual([]);
    await adapter.publishWebcam();
    expect(publisher.transceivers).toEqual([cam]);
    cam.end();
    expect(ended).toHaveBeenCalledWith("webcam", null);
  });
});
