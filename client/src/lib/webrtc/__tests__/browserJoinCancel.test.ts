/**
 * C32 amendment "leave and voice_removed win" (W1-C1, Codex 2): a browser
 * join whose voice session epoch went stale releases what it acquired and
 * never sends `voice_join`. Each adapter step is suspended on a deferred so
 * the cancel lands exactly inside it.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const wsSend = vi.fn().mockResolvedValue(undefined);
const wsStatus = vi.fn();
const wsConnect = vi.fn();

vi.mock("@/lib/tauri", () => ({
  wsSend: (...a: unknown[]) => wsSend(...a),
  wsStatus: () => wsStatus(),
  wsConnect: () => wsConnect(),
  getServerUrl: () => "http://test",
  getAccessToken: () => "tok",
}));

import { BrowserVoiceAdapter } from "../browser";
import { bumpVoiceEpoch, currentVoiceEpoch } from "../voiceSession";

function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

const pcs: FakePc[] = [];
class FakePc {
  closed = false;
  tracks: unknown[] = [];
  constructor() {
    pcs.push(this);
  }
  addTrack(t: unknown) {
    this.tracks.push(t);
  }
  close() {
    this.closed = true;
  }
  addEventListener() {}
  removeEventListener() {}
}

function fakeStream() {
  const track = { stop: vi.fn(), enabled: true, kind: "audio" };
  return {
    track,
    stream: {
      id: "s",
      getTracks: () => [track],
      getAudioTracks: () => [track],
    } as unknown as MediaStream,
  };
}

const voiceJoins = () =>
  wsSend.mock.calls.filter(([m]) => (m as { type: string }).type === "voice_join");

async function flush() {
  for (let i = 0; i < 20; i++) await Promise.resolve();
}

describe("BrowserVoiceAdapter join cancellation (epoch)", () => {
  let gum: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    pcs.length = 0;
    wsSend.mockClear();
    wsStatus.mockReset().mockResolvedValue({ type: "connected" });
    gum = vi.fn();
    vi.stubGlobal("RTCPeerConnection", FakePc);
    vi.stubGlobal("navigator", { mediaDevices: { getUserMedia: gum } });
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({ ok: true, json: async () => ({ ice_servers: [] }) }),
    );
    vi.spyOn(console, "log").mockImplementation(() => {});
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("leave during getUserMedia: mic released, no PC, no voice_join", async () => {
    const mic = deferred<MediaStream>();
    gum.mockReturnValue(mic.promise);
    const adapter = new BrowserVoiceAdapter();
    const join = adapter.join("ch", { epoch: currentVoiceEpoch() });
    await flush();

    bumpVoiceEpoch();
    await adapter.leave();
    const { stream, track } = fakeStream();
    mic.resolve(stream);
    const result = await join;

    expect(result).toEqual({ ok: false, error: expect.objectContaining({ type: "cancelled" }) });
    expect(track.stop).toHaveBeenCalled();
    expect(pcs).toHaveLength(0);
    expect(voiceJoins()).toHaveLength(0);
  });

  it("voice_removed during the ICE config fetch: PCs never created, mic stopped", async () => {
    const { stream, track } = fakeStream();
    gum.mockResolvedValue(stream);
    const ice = deferred<unknown>();
    vi.stubGlobal("fetch", vi.fn().mockReturnValue(ice.promise));
    const adapter = new BrowserVoiceAdapter();
    const join = adapter.join("ch", { epoch: currentVoiceEpoch() });
    await flush();

    bumpVoiceEpoch();
    await adapter.teardown();
    ice.resolve({ ok: true, json: async () => ({ ice_servers: [] }) });
    const result = await join;

    expect(result.ok).toBe(false);
    expect(track.stop).toHaveBeenCalled();
    expect(pcs).toHaveLength(0);
    expect(voiceJoins()).toHaveLength(0);
  });

  it("cancel during the WebSocket status check: PCs closed, no voice_join", async () => {
    const { stream, track } = fakeStream();
    gum.mockResolvedValue(stream);
    const status = deferred<{ type: string }>();
    wsStatus.mockReturnValueOnce(status.promise);
    const adapter = new BrowserVoiceAdapter();
    const join = adapter.join("ch", { epoch: currentVoiceEpoch() });
    await vi.waitFor(() => expect(wsStatus).toHaveBeenCalled());
    expect(pcs).toHaveLength(2);

    bumpVoiceEpoch();
    status.resolve({ type: "connected" });
    const result = await join;

    expect(result.ok).toBe(false);
    expect(pcs.every((pc) => pc.closed)).toBe(true);
    expect(track.stop).toHaveBeenCalled();
    expect(voiceJoins()).toHaveLength(0);
  });

  it("cancel while voice_join is being sent: no tracks published, PCs closed", async () => {
    const { stream, track } = fakeStream();
    gum.mockResolvedValue(stream);
    const sent = deferred<void>();
    wsSend.mockReturnValueOnce(sent.promise);
    const adapter = new BrowserVoiceAdapter();
    const join = adapter.join("ch", { epoch: currentVoiceEpoch() });
    await vi.waitFor(() => expect(voiceJoins()).toHaveLength(1));

    bumpVoiceEpoch();
    await adapter.leave();
    sent.resolve();
    const result = await join;

    expect(result.ok).toBe(false);
    expect(pcs.every((pc) => pc.closed && pc.tracks.length === 0)).toBe(true);
    expect(track.stop).toHaveBeenCalled();
    expect(voiceJoins()).toHaveLength(1);
    // The leave went out after the join on the same socket.
    expect(wsSend.mock.calls[wsSend.mock.calls.length - 1]?.[0]).toMatchObject({ type: "voice_leave" });
  });

  it("a newer join does not lose its mic to an older cancelled one", async () => {
    const oldMic = deferred<MediaStream>();
    const older = fakeStream();
    const newer = fakeStream();
    gum.mockReturnValueOnce(oldMic.promise).mockResolvedValueOnce(newer.stream);
    const adapter = new BrowserVoiceAdapter();
    const first = adapter.join("ch-a", { epoch: currentVoiceEpoch() });
    await flush();

    const second = adapter.join("ch-b", { epoch: bumpVoiceEpoch() });
    await flush();
    oldMic.resolve(older.stream);
    expect((await first).ok).toBe(false);
    expect((await second).ok).toBe(true);

    expect(older.track.stop).toHaveBeenCalled();
    expect(newer.track.stop).not.toHaveBeenCalled();
    expect(voiceJoins()).toEqual([[{ type: "voice_join", channel_id: "ch-b" }]]);
    expect(pcs.filter((pc) => !pc.closed)).toHaveLength(2);
  });

  it("a join without an epoch is never cancelled (old callers)", async () => {
    const { stream } = fakeStream();
    gum.mockResolvedValue(stream);
    const adapter = new BrowserVoiceAdapter();
    const join = adapter.join("ch");
    bumpVoiceEpoch();
    expect((await join).ok).toBe(true);
    expect(voiceJoins()).toHaveLength(1);
  });
});

describe("BrowserVoiceAdapter leave with the socket down (W1-N1)", () => {
  beforeEach(() => {
    pcs.length = 0;
    wsSend.mockReset().mockResolvedValue(undefined);
    wsStatus.mockReset().mockResolvedValue({ type: "connected" });
    vi.stubGlobal("RTCPeerConnection", FakePc);
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

  it("releases the mic and closes the PCs even when voice_leave cannot be sent", async () => {
    const { stream, track } = fakeStream();
    vi.stubGlobal("navigator", { mediaDevices: { getUserMedia: vi.fn().mockResolvedValue(stream) } });
    const adapter = new BrowserVoiceAdapter();
    expect((await adapter.join("ch")).ok).toBe(true);

    wsSend.mockRejectedValue(new Error("WebSocket not connected"));
    const result = await adapter.leave();

    expect(result.ok).toBe(true);
    expect(track.stop).toHaveBeenCalled();
    expect(pcs.every((pc) => pc.closed)).toBe(true);
    expect(adapter.getChannelId()).toBeNull();
    expect(adapter.getState()).toBe("disconnected");
  });
});
