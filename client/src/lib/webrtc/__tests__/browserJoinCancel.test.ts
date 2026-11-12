/**
 * A browser voice session is ended by its own handle (C32 amendment "leave
 * and voice_removed win"; W1-C1, Codex 2, W1d): ending it releases exactly
 * what its join acquired, a join step that resumes afterwards releases what
 * it just got and never sends `voice_join`, and no other session is
 * touched. Each adapter step is suspended on a deferred so the end lands
 * exactly inside it.
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

const A = { token: 1, channelId: "ch-a" };
const B = { token: 2, channelId: "ch-b" };

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

describe("BrowserVoiceAdapter sessions (one handle per join)", () => {
  let gum: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    pcs.length = 0;
    wsSend.mockReset().mockResolvedValue(undefined);
    wsStatus.mockReset().mockResolvedValue({ type: "connected" });
    gum = vi.fn();
    vi.stubGlobal("RTCPeerConnection", FakePc);
    vi.stubGlobal("navigator", { mediaDevices: { getUserMedia: gum } });
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

  it("leave during getUserMedia: the late grant is stopped, no PC, nothing sent", async () => {
    const mic = deferred<MediaStream>();
    gum.mockReturnValue(mic.promise);
    const adapter = new BrowserVoiceAdapter();
    const join = adapter.join(A);
    await flush();

    await adapter.leave(A);
    const { stream, track } = fakeStream();
    mic.resolve(stream);
    const result = await join;

    expect(result).toEqual({ ok: false, error: expect.objectContaining({ type: "cancelled" }) });
    expect(track.stop).toHaveBeenCalled();
    expect(pcs).toHaveLength(0);
    // Nothing reached the server for this session, so nothing is taken back.
    expect(wsSend).not.toHaveBeenCalled();
  });

  it("teardown during the ICE config fetch: PCs never created, mic stopped", async () => {
    const { stream, track } = fakeStream();
    gum.mockResolvedValue(stream);
    const ice = deferred<unknown>();
    vi.stubGlobal("fetch", vi.fn().mockReturnValue(ice.promise));
    const adapter = new BrowserVoiceAdapter();
    const join = adapter.join(A);
    await flush();

    await adapter.teardown(A);
    expect(track.stop).toHaveBeenCalledTimes(1); // released at once, not on resume
    ice.resolve({ ok: true, json: async () => ({ ice_servers: [] }) });
    const result = await join;

    expect(result.ok).toBe(false);
    expect(track.stop).toHaveBeenCalledTimes(1);
    expect(pcs).toHaveLength(0);
    expect(voiceJoins()).toHaveLength(0);
  });

  it("ended during the WebSocket status check: PCs closed, no voice_join", async () => {
    const { stream, track } = fakeStream();
    gum.mockResolvedValue(stream);
    const status = deferred<{ type: string }>();
    wsStatus.mockReturnValueOnce(status.promise);
    const adapter = new BrowserVoiceAdapter();
    const join = adapter.join(A);
    await vi.waitFor(() => expect(wsStatus).toHaveBeenCalled());
    expect(pcs).toHaveLength(2);

    await adapter.teardown(A);
    status.resolve({ type: "connected" });
    const result = await join;

    expect(result.ok).toBe(false);
    expect(pcs.every((pc) => pc.closed)).toBe(true);
    expect(track.stop).toHaveBeenCalled();
    expect(voiceJoins()).toHaveLength(0);
  });

  it("leave while voice_join is being sent: no tracks published, PCs closed, leave follows", async () => {
    const { stream, track } = fakeStream();
    gum.mockResolvedValue(stream);
    const sent = deferred<void>();
    wsSend.mockReturnValueOnce(sent.promise);
    const adapter = new BrowserVoiceAdapter();
    const join = adapter.join(A);
    await vi.waitFor(() => expect(voiceJoins()).toHaveLength(1));

    await adapter.leave(A);
    sent.resolve();
    const result = await join;

    expect(result.ok).toBe(false);
    expect(pcs.every((pc) => pc.closed && pc.tracks.length === 0)).toBe(true);
    expect(track.stop).toHaveBeenCalled();
    expect(voiceJoins()).toHaveLength(1);
    // The leave went out after the join on the same socket, for A.
    expect(wsSend.mock.calls[wsSend.mock.calls.length - 1]?.[0]).toEqual({
      type: "voice_leave",
      channel_id: "ch-a",
    });
  });

  it("a newer session does not lose its mic to an older one that resumes late", async () => {
    const oldMic = deferred<MediaStream>();
    const older = fakeStream();
    const newer = fakeStream();
    gum.mockReturnValueOnce(oldMic.promise).mockResolvedValueOnce(newer.stream);
    const adapter = new BrowserVoiceAdapter();
    const first = adapter.join(A);
    await flush();

    await adapter.teardown(A);
    const second = adapter.join(B);
    await flush();
    oldMic.resolve(older.stream);
    expect((await first).ok).toBe(false);
    expect(await second).toEqual({ ok: true, value: B });

    expect(older.track.stop).toHaveBeenCalled();
    expect(newer.track.stop).not.toHaveBeenCalled();
    expect(voiceJoins()).toEqual([[{ type: "voice_join", channel_id: "ch-b" }]]);
    expect(pcs.filter((pc) => !pc.closed)).toHaveLength(2);
  });

  it("a slow leave of A never touches B that was joined meanwhile (W1d)", async () => {
    const micA = fakeStream();
    const micB = fakeStream();
    gum.mockResolvedValueOnce(micA.stream).mockResolvedValueOnce(micB.stream);
    const adapter = new BrowserVoiceAdapter();
    const states = vi.fn();
    adapter.setEventHandlers({ onStateChange: states });
    expect((await adapter.join(A)).ok).toBe(true);

    // A's leave hangs while it tells the server.
    const leaveSent = deferred<void>();
    wsSend.mockReturnValueOnce(leaveSent.promise);
    const leaving = adapter.leave(A);
    await vi.waitFor(() =>
      expect(wsSend).toHaveBeenCalledWith({ type: "voice_leave", channel_id: "ch-a" }),
    );
    // Everything of A is already released, before the send has finished.
    expect(micA.track.stop).toHaveBeenCalledTimes(1);
    expect(pcs.slice(0, 2).every((pc) => pc.closed)).toBe(true);

    expect((await adapter.join(B)).ok).toBe(true);
    leaveSent.resolve();
    await leaving;
    await adapter.leave(A); // and a repeated leave of A is nothing at all
    await adapter.teardown(A);

    expect(micB.track.stop).not.toHaveBeenCalled();
    expect(pcs.slice(2).every((pc) => !pc.closed)).toBe(true);
    expect(adapter.getChannelId()).toBe("ch-b");
    expect(wsSend.mock.calls.filter(([m]) => (m as { type: string }).type === "voice_leave")).toEqual([
      [{ type: "voice_leave", channel_id: "ch-a" }],
    ]);
    // A's end was reported for A, never for B.
    expect(states.mock.calls.filter(([state]) => state === "disconnected")).toEqual([
      ["disconnected", A],
    ]);
  });

  it("a screen picked after the call ended is released, and never reaches the next call", async () => {
    const micA = fakeStream();
    const micB = fakeStream();
    gum.mockResolvedValueOnce(micA.stream).mockResolvedValueOnce(micB.stream);
    const picker = deferred<MediaStream>();
    vi.stubGlobal("navigator", {
      mediaDevices: { getUserMedia: gum, getDisplayMedia: () => picker.promise },
    });
    const adapter = new BrowserVoiceAdapter();
    await adapter.join(A);
    const sharing = adapter.startScreenShare({ streamId: "s1" });
    await flush();

    await adapter.leave(A);
    await adapter.join(B);
    const screenStop = vi.fn();
    const screenTrack = { stop: screenStop, kind: "video" };
    picker.resolve({
      getTracks: () => [screenTrack],
      getVideoTracks: () => [screenTrack],
      getAudioTracks: () => [],
    } as unknown as MediaStream);
    const result = await sharing;

    expect(result).toEqual({ ok: false, error: { type: "not_connected" } });
    expect(screenStop).toHaveBeenCalled();
    expect(adapter.isScreenSharing()).toBe(false);
    // B's connections carry only B's microphone.
    expect(pcs.slice(2).every((pc) => pc.tracks.every((t) => t !== screenTrack))).toBe(true);
  });

  it("joining while a session is still open ends that session first, by its handle", async () => {
    const micA = fakeStream();
    const micB = fakeStream();
    gum.mockResolvedValueOnce(micA.stream).mockResolvedValueOnce(micB.stream);
    const adapter = new BrowserVoiceAdapter();
    const states = vi.fn();
    adapter.setEventHandlers({ onStateChange: states });
    await adapter.join(A);
    expect((await adapter.join(B)).ok).toBe(true);

    expect(micA.track.stop).toHaveBeenCalledTimes(1);
    expect(micB.track.stop).not.toHaveBeenCalled();
    expect(states).toHaveBeenCalledWith("disconnected", A);
    expect(pcs.filter((pc) => !pc.closed)).toHaveLength(2);
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
    expect((await adapter.join(A)).ok).toBe(true);

    wsSend.mockRejectedValue(new Error("WebSocket not connected"));
    const result = await adapter.leave(A);

    expect(result.ok).toBe(true);
    expect(track.stop).toHaveBeenCalled();
    expect(pcs.every((pc) => pc.closed)).toBe(true);
    expect(adapter.getChannelId()).toBeNull();
    expect(adapter.getState()).toBe("disconnected");
  });
});
