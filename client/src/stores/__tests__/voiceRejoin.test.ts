/**
 * C32: voice rejoin after a lost connection; C31: never after voice_removed.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type Session = { token: number; channelId: string };
type Handlers = {
  onStateChange: (s: string, session: Session | null) => void;
  onConnectionLost?: (session: Session | null) => void;
};

/** Every handler installed on the adapter (the owner's and the store's). */
let handlers: Handlers | null = null;

const mockAdapter = {
  join: vi.fn(),
  leave: vi.fn().mockResolvedValue({ ok: true }),
  teardown: vi.fn(),
  setMute: vi.fn().mockResolvedValue({ ok: true }),
  setDeafen: vi.fn().mockResolvedValue({ ok: true }),
  setEventHandlers: vi.fn((h: Partial<Handlers>) => {
    handlers = { ...handlers, ...h } as Handlers;
  }),
  getConnectionMetrics: vi.fn().mockResolvedValue(null),
  isScreenSharing: vi.fn().mockReturnValue(false),
  setVadConfig: vi.fn(),
  setInputDevice: vi.fn().mockResolvedValue({ ok: true }),
  setOutputDevice: vi.fn().mockResolvedValue({ ok: true }),
};

vi.mock("@/lib/webrtc", () => ({
  createVoiceAdapter: vi.fn(() => mockAdapter),
  getVoiceAdapter: vi.fn(() => mockAdapter),
}));

vi.mock("@/lib/tauri", () => ({
  wsSend: vi.fn(),
  wsScreenShareStart: vi.fn(),
  wsScreenShareStop: vi.fn(),
  wsWebcamStart: vi.fn(),
  wsWebcamStop: vi.fn(),
}));

vi.mock("@/stores/channels", () => ({
  channelsState: { channels: [{ id: "ch-1", name: "voice-1" }] },
}));

vi.mock("@/stores/settings", () => ({
  updateAudioSetting: vi.fn().mockResolvedValue(undefined),
  appSettings: vi.fn(() => ({
    voice: {
      push_to_talk: false,
      push_to_talk_key: null,
      push_to_talk_release_delay: 200,
      push_to_mute: false,
      push_to_mute_key: null,
      push_to_mute_release_delay: 200,
      voice_activity_detection: false,
      vad_threshold: 0.5,
    },
  })),
}));

vi.mock("@/components/ui/Toast", () => ({
  showToast: vi.fn(),
  dismissToast: vi.fn(),
}));

vi.mock("@/stores/webcamViewer", () => ({
  clearAll: vi.fn(),
  addAvailableTrack: vi.fn(),
  removeAvailableTrack: vi.fn(),
}));

vi.mock("@/lib/pttManager", () => ({
  PttController: vi.fn().mockImplementation(() => ({
    activate: vi.fn(),
    deactivate: vi.fn(),
    isPttOrPtmEnabled: vi.fn().mockReturnValue(false),
  })),
  createTauriPttListeners: vi.fn().mockResolvedValue(vi.fn()),
}));

import {
  voiceState,
  setVoiceState,
  joinVoice,
  leaveVoice,
  handleVoiceRemoved,
  rejoinVoiceAfterReconnect,
  noteSignallingLost,
  isVoiceRejoinRunning,
  dismissVoiceRejoin,
  toggleMute,
  noteSignallingUp,
  voiceRejoinAttemptsUsed,
  REJOIN_ATTEMPT_TIMEOUT_MS,
} from "../voice";
import { RECOVERY_OFFLINE_CAP_MS } from "../voiceOwner";

/** Make the next adapter.join hang until released; returns its session and release. */
function joinHangs(): { session: () => Session | undefined; release: (ok?: boolean) => void } {
  let session: Session | undefined;
  let release!: (ok?: boolean) => void;
  mockAdapter.join.mockImplementationOnce(
    (joined: Session) =>
      new Promise((resolve) => {
        session = joined;
        release = (ok = true) =>
          resolve(
            ok
              ? { ok: true, value: joined }
              : { ok: false, error: { type: "unknown", message: "x" } },
          );
      }),
  );
  return { session: () => session, release: (ok) => release(ok) };
}

/** The session the adapter was asked to join last. */
function lastSession(): Session {
  const calls = mockAdapter.join.mock.calls;
  return calls[calls.length - 1]![0] as Session;
}

/** The adapter reports the publisher of the last joined session connected. */
function connected(): void {
  handlers!.onStateChange("connected", lastSession());
}

/** The adapter reports an ICE failure of the last joined session. */
function connectionLost(): void {
  handlers!.onConnectionLost!(lastSession());
}

/** Whether the adapter was told to end exactly this session. */
function wasEnded(session: Session | undefined): boolean {
  const ended = [...mockAdapter.leave.mock.calls, ...mockAdapter.teardown.mock.calls].map(
    ([s]) => (s as Session).token,
  );
  return !!session && ended.includes(session.token);
}

/** Let pending promise callbacks run without advancing time. */
async function flush(): Promise<void> {
  for (let i = 0; i < 10; i++) await Promise.resolve();
}

/** Join ch-1 and reach "connected". */
async function inCall(): Promise<void> {
  mockAdapter.join.mockResolvedValue({ ok: true });
  await joinVoice("ch-1");
  connected();
  expect(voiceState.state).toBe("connected");
}

/** Make the next adapter.join connect right away. */
function joinConnects(): void {
  mockAdapter.join.mockImplementationOnce(async (session: Session) => {
    queueMicrotask(() => handlers!.onStateChange("connected", session));
    return { ok: true, value: session };
  });
}

describe("voice rejoin (C32)", () => {
  beforeEach(async () => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    // Like the real adapters: ending a session reports it as disconnected.
    const ends = async (session: Session) => {
      handlers?.onStateChange("disconnected", session);
      return { ok: true };
    };
    mockAdapter.teardown.mockImplementation(ends);
    mockAdapter.leave.mockImplementation(ends);
    await rejoinVoiceAfterReconnect(); // socket known up
    await leaveVoice();
    setVoiceState({
      state: "disconnected",
      channelId: null,
      muted: false,
      deafened: false,
      lostChannelId: null,
      participants: {},
    });
  });

  afterEach(async () => {
    await leaveVoice();
    vi.useRealTimers();
  });

  it("rejoins 1 s after the socket is back and restores mute", async () => {
    await inCall();
    setVoiceState({ muted: true });

    noteSignallingLost();
    expect(voiceState.state).toBe("reconnecting");

    joinConnects();
    const flow = rejoinVoiceAfterReconnect();
    await flush();
    expect(isVoiceRejoinRunning()).toBe(true);
    expect(mockAdapter.join).toHaveBeenCalledTimes(1); // only the first join

    await vi.advanceTimersByTimeAsync(1000);
    await flow;

    expect(mockAdapter.teardown).toHaveBeenCalledTimes(1);
    expect(mockAdapter.leave).not.toHaveBeenCalled();
    expect(mockAdapter.join).toHaveBeenCalledTimes(2);
    expect(mockAdapter.join).toHaveBeenLastCalledWith({ token: expect.any(Number), channelId: "ch-1" });
    expect(mockAdapter.setMute).toHaveBeenCalledWith(true);
    expect(voiceState.state).toBe("connected");
    expect(voiceState.channelId).toBe("ch-1");
    expect(voiceState.muted).toBe(true);
    expect(isVoiceRejoinRunning()).toBe(false);
  });

  it("restores deafen on the new connection", async () => {
    await inCall();
    setVoiceState({ muted: true, deafened: true });
    joinConnects();
    const flow = rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(1000);
    await flow;
    expect(mockAdapter.setDeafen).toHaveBeenCalledWith(true);
    expect(voiceState.deafened).toBe(true);
  });

  it("gives up after three failed attempts (1 s, 3 s, 9 s) and offers Rejoin", async () => {
    await inCall();
    mockAdapter.join.mockResolvedValue({ ok: false, error: { type: "unknown", message: "x" } });

    const flow = rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(999);
    expect(mockAdapter.join).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(mockAdapter.join).toHaveBeenCalledTimes(2);
    expect(voiceState.state).toBe("reconnecting");
    await vi.advanceTimersByTimeAsync(3000);
    expect(mockAdapter.join).toHaveBeenCalledTimes(3);
    await vi.advanceTimersByTimeAsync(9000);
    await flow;

    expect(mockAdapter.join).toHaveBeenCalledTimes(4);
    expect(voiceState.state).toBe("disconnected");
    expect(voiceState.channelId).toBeNull();
    expect(voiceState.lostChannelId).toBe("ch-1");

    dismissVoiceRejoin();
    expect(voiceState.lostChannelId).toBeNull();
  });

  it("counts an attempt that never connects as failed", async () => {
    await inCall();
    mockAdapter.join.mockResolvedValue({ ok: true }); // never reaches connected
    const flow = rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(1000 + REJOIN_ATTEMPT_TIMEOUT_MS);
    expect(mockAdapter.join).toHaveBeenCalledTimes(2);
    expect(voiceState.state).toBe("reconnecting");
    joinConnects();
    await vi.advanceTimersByTimeAsync(3000);
    await flow;
    expect(voiceState.state).toBe("connected");
    expect(mockAdapter.join).toHaveBeenCalledTimes(3);
  });

  it("an own leave stops the flow", async () => {
    await inCall();
    const flow = rejoinVoiceAfterReconnect();
    await flush();
    await leaveVoice();
    await vi.advanceTimersByTimeAsync(20_000);
    await flow;
    expect(mockAdapter.join).toHaveBeenCalledTimes(1);
    expect(voiceState.state).toBe("disconnected");
    expect(voiceState.lostChannelId).toBeNull();
  });

  it("does nothing when not in voice", async () => {
    await rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(20_000);
    expect(mockAdapter.join).not.toHaveBeenCalled();
  });

  it("an ICE failure rejoins without a socket drop", async () => {
    await inCall();
    joinConnects();
    connectionLost();
    expect(voiceState.state).toBe("reconnecting");
    await vi.advanceTimersByTimeAsync(1000);
    await flush();
    expect(mockAdapter.join).toHaveBeenCalledTimes(2);
    expect(voiceState.state).toBe("connected");
  });

  it("an ICE failure while the socket is down waits for the socket", async () => {
    await inCall();
    noteSignallingLost();
    connectionLost();
    await vi.advanceTimersByTimeAsync(20_000);
    expect(mockAdapter.join).toHaveBeenCalledTimes(1);
    expect(voiceState.state).toBe("reconnecting");

    joinConnects();
    const flow = rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(1000);
    await flow;
    expect(voiceState.state).toBe("connected");
  });

  it("never rejoins after voice_removed (C31)", async () => {
    await inCall();
    await handleVoiceRemoved("ch-1", "kicked");
    expect(voiceState.state).toBe("disconnected");

    await rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(20_000);
    expect(mockAdapter.join).toHaveBeenCalledTimes(1);
    expect(voiceState.lostChannelId).toBeNull();
  });

  it("voice_removed during the flow stops it", async () => {
    await inCall();
    const flow = rejoinVoiceAfterReconnect();
    await flush();
    await handleVoiceRemoved("ch-1", "banned");
    await vi.advanceTimersByTimeAsync(20_000);
    await flow;
    expect(mockAdapter.join).toHaveBeenCalledTimes(1);
    expect(voiceState.channelId).toBeNull();
    expect(voiceState.lostChannelId).toBeNull();
  });

  it("attempt timeout is 15 s (amendment)", () => {
    expect(REJOIN_ATTEMPT_TIMEOUT_MS).toBe(15_000);
  });

  it("leave while an attempt's join is pending: it stays left, nothing comes back", async () => {
    await inCall();
    const hang = joinHangs();
    const flow = rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(1000);
    expect(mockAdapter.join).toHaveBeenCalledTimes(2);

    await leaveVoice();
    expect(wasEnded(hang.session())).toBe(true);
    // The adapter's join resumes afterwards and even reports connected.
    hang.release();
    connected();
    await vi.advanceTimersByTimeAsync(30_000);
    await flow;

    expect(mockAdapter.join).toHaveBeenCalledTimes(2);
    expect(voiceState.state).toBe("disconnected");
    expect(voiceState.channelId).toBeNull();
    expect(voiceState.lostChannelId).toBeNull();
    expect(isVoiceRejoinRunning()).toBe(false);
  });

  it("voice_removed while an attempt's join is pending wins", async () => {
    await inCall();
    const hang = joinHangs();
    const flow = rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(1000);

    await handleVoiceRemoved("ch-1", "kicked");
    expect(wasEnded(hang.session())).toBe(true);
    hang.release();
    connected();
    await vi.advanceTimersByTimeAsync(30_000);
    await flow;

    expect(mockAdapter.join).toHaveBeenCalledTimes(2);
    expect(voiceState.channelId).toBeNull();
    expect(voiceState.state).toBe("disconnected");
    expect(mockAdapter.teardown).toHaveBeenCalled();
  });

  it("leave while a manual join is pending: the late join changes nothing", async () => {
    const hang = joinHangs();
    const joining = joinVoice("ch-1");
    await flush();
    expect(voiceState.state).toBe("connecting");

    await leaveVoice();
    expect(wasEnded(hang.session())).toBe(true);
    hang.release();
    await joining;

    expect(voiceState.state).toBe("disconnected");
    expect(voiceState.channelId).toBeNull();
    expect(voiceState.sessionId).toBeNull();
  });

  it("no Join for the same channel while reconnecting (R2)", async () => {
    await inCall();
    noteSignallingLost();
    await joinVoice("ch-1");
    expect(mockAdapter.join).toHaveBeenCalledTimes(1);
    expect(mockAdapter.leave).not.toHaveBeenCalled();
    expect(voiceState.state).toBe("reconnecting");
  });

  it("Join for another channel while an attempt is pending: leave, then join there", async () => {
    await inCall();
    const hang = joinHangs();
    void rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(1000);

    mockAdapter.join.mockResolvedValueOnce({ ok: true });
    await joinVoice("ch-2");
    expect(wasEnded(hang.session())).toBe(true);
    expect(mockAdapter.leave).toHaveBeenCalledTimes(1);
    expect(mockAdapter.join).toHaveBeenLastCalledWith({ token: expect.any(Number), channelId: "ch-2" });
    hang.release();
    await vi.advanceTimersByTimeAsync(30_000);

    expect(voiceState.channelId).toBe("ch-2");
    expect(isVoiceRejoinRunning()).toBe(false);
    expect(mockAdapter.join).toHaveBeenCalledTimes(3);
  });

  it("a join that never resolves is timed out at 15 s and its session cancelled", async () => {
    await inCall();
    const hang = joinHangs();
    const flow = rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(1000 + REJOIN_ATTEMPT_TIMEOUT_MS - 1);
    expect(wasEnded(hang.session())).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    expect(wasEnded(hang.session())).toBe(true);
    expect(voiceRejoinAttemptsUsed()).toBe(1);

    joinConnects();
    await vi.advanceTimersByTimeAsync(3000);
    await flow;
    expect(voiceState.state).toBe("connected");
  });

  it("socket loss during attempt 3 keeps the budget and resumes on the next socket", async () => {
    await inCall();
    mockAdapter.join
      .mockResolvedValueOnce({ ok: false, error: { type: "unknown", message: "x" } })
      .mockResolvedValueOnce({ ok: false, error: { type: "unknown", message: "x" } });
    const flow = rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(1000 + 3000);
    expect(voiceRejoinAttemptsUsed()).toBe(2);

    const hang = joinHangs();
    await vi.advanceTimersByTimeAsync(9000);
    expect(mockAdapter.join).toHaveBeenCalledTimes(4);

    noteSignallingLost();
    await flush();
    expect(wasEnded(hang.session())).toBe(true);
    expect(voiceRejoinAttemptsUsed()).toBe(2);
    expect(voiceState.state).toBe("reconnecting");
    await vi.advanceTimersByTimeAsync(30_000);
    expect(mockAdapter.join).toHaveBeenCalledTimes(4); // waits for the socket

    joinConnects();
    void rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(9000);
    await flow;
    expect(mockAdapter.join).toHaveBeenCalledTimes(5);
    expect(voiceState.state).toBe("connected");
    expect(voiceState.lostChannelId).toBeNull();
  });

  it("gives up after 60 s without a socket and releases the microphone", async () => {
    await inCall();
    noteSignallingLost();
    await vi.advanceTimersByTimeAsync(RECOVERY_OFFLINE_CAP_MS - 1);
    expect(voiceState.state).toBe("reconnecting");
    mockAdapter.teardown.mockClear();

    await vi.advanceTimersByTimeAsync(1);
    await flush();
    expect(mockAdapter.teardown).toHaveBeenCalled();
    expect(voiceState.state).toBe("disconnected");
    expect(voiceState.channelId).toBeNull();
    expect(voiceState.lostChannelId).toBe("ch-1");
    expect(isVoiceRejoinRunning()).toBe(false);

    // A socket that comes back later does not rejoin.
    await rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(20_000);
    expect(mockAdapter.join).toHaveBeenCalledTimes(1);
  });

  it("the 60 s deadline also ends an attempt whose join has not gone out yet", async () => {
    await inCall();
    noteSignallingLost(); // t = 0
    await vi.advanceTimersByTimeAsync(58_500);
    const hang = joinHangs(); // e.g. waiting for the microphone
    void rejoinVoiceAfterReconnect(); // socket back at 58.5 s, attempt at 59.5 s
    await vi.advanceTimersByTimeAsync(1000);
    expect(mockAdapter.join).toHaveBeenCalledTimes(2);
    expect(voiceState.state).toBe("reconnecting");

    await vi.advanceTimersByTimeAsync(500); // t = 60 s, no voice_join was carried
    await flush();
    expect(voiceState.state).toBe("disconnected");
    expect(voiceState.lostChannelId).toBe("ch-1");
    expect(wasEnded(hang.session())).toBe(true);
    expect(mockAdapter.leave).not.toHaveBeenCalled(); // given up: no voice_leave

    // Whatever the abandoned join does later changes nothing.
    hang.release();
    await vi.advanceTimersByTimeAsync(30_000);
    expect(voiceState.state).toBe("disconnected");
    expect(mockAdapter.join).toHaveBeenCalledTimes(2);
  });

  it("mute stays usable while reconnecting and the latest choice is applied", async () => {
    await inCall();
    noteSignallingLost();
    mockAdapter.setMute.mockRejectedValueOnce(new Error("socket down"));
    await toggleMute();
    expect(voiceState.muted).toBe(true);

    joinConnects();
    const flow = rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(1000);
    await flow;
    expect(mockAdapter.setMute).toHaveBeenLastCalledWith(true);
    expect(voiceState.muted).toBe(true);
    expect(voiceState.state).toBe("connected");
  });

  it("a socket that opened without voice counts as up for a later ICE failure", async () => {
    noteSignallingLost();
    noteSignallingUp();
    await inCall();
    joinConnects();
    connectionLost();
    await vi.advanceTimersByTimeAsync(1000);
    await flush();
    expect(mockAdapter.join).toHaveBeenCalledTimes(2);
    expect(voiceState.state).toBe("connected");
  });

  it("a deafened rejoin is deafened before it reports connected (W1-M10)", async () => {
    await inCall();
    setVoiceState({ muted: true, deafened: true });
    const hang = joinHangs();
    void rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(1000);
    mockAdapter.setDeafen.mockClear();
    hang.release();
    await flush();
    expect(mockAdapter.setDeafen).toHaveBeenCalledWith(true);
    expect(voiceState.state).toBe("reconnecting");
    await leaveVoice();
  });

  it("Leave while the socket is down always ends the session; the next socket does not rejoin (W1-N1)", async () => {
    await inCall();
    noteSignallingLost();
    mockAdapter.leave.mockRejectedValueOnce(new Error("WebSocket not connected"));
    await leaveVoice();
    expect(voiceState.state).toBe("disconnected");
    expect(voiceState.channelId).toBeNull();
    expect(isVoiceRejoinRunning()).toBe(false);

    await rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(30_000);
    expect(mockAdapter.join).toHaveBeenCalledTimes(1);
  });

  it("a manual Join that fails during a socket loss leaves no hidden recovery (W1-N2)", async () => {
    const hang = joinHangs();
    const joining = joinVoice("ch-1").catch(() => undefined);
    await flush();
    noteSignallingLost();
    expect(isVoiceRejoinRunning()).toBe(false); // a pending join has no session to recover
    hang.release(false); // the join fails (WS timeout)
    await joining;
    expect(voiceState.state).toBe("disconnected");
    expect(isVoiceRejoinRunning()).toBe(false);

    await rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(60_000);
    expect(mockAdapter.join).toHaveBeenCalledTimes(1);
  });

  it("a socket drop while the rejoin restores mute keeps recovery and its 60 s deadline (Codex W1b-3)", async () => {
    await inCall();
    setVoiceState({ deafened: true, muted: true });
    let finishRestore!: () => void;
    mockAdapter.setDeafen
      .mockResolvedValueOnce({ ok: true }) // right after join (W1-M10)
      .mockImplementationOnce(
        () => new Promise((r) => (finishRestore = () => r({ ok: true }))),
      );
    joinConnects();
    void rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(1000);
    await flush();
    expect(finishRestore).toBeTypeOf("function");

    // The socket drops while mute/deafen are being restored, then the
    // restore completes anyway: that must not declare the call connected.
    noteSignallingLost();
    finishRestore();
    await flush();
    expect(voiceState.state).toBe("reconnecting");
    expect(isVoiceRejoinRunning()).toBe(true);
    expect(voiceRejoinAttemptsUsed()).toBe(0);

    // Without a socket the deadline still runs out.
    await vi.advanceTimersByTimeAsync(RECOVERY_OFFLINE_CAP_MS);
    expect(voiceState.state).toBe("disconnected");
    expect(voiceState.lostChannelId).toBe("ch-1");
  });

  it("after a drop during finalisation the next socket resumes and connects", async () => {
    await inCall();
    mockAdapter.setDeafen.mockImplementationOnce(() => new Promise(() => {}));
    joinConnects();
    const flow = rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(1000);
    await flush();
    noteSignallingLost();
    await flush();
    expect(voiceState.state).toBe("reconnecting");

    joinConnects();
    void rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(1000);
    await flow;
    expect(voiceState.state).toBe("connected");
    expect(mockAdapter.join).toHaveBeenCalledTimes(3);
  });

  it("an attempt the adapter reports as cancelled is ended and the flow goes on (W1-N5)", async () => {
    await inCall();
    mockAdapter.join.mockImplementationOnce(async () => ({
      ok: false,
      error: { type: "cancelled", message: "x" },
    }));
    void rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(1000);
    await flush();
    expect(isVoiceRejoinRunning()).toBe(true);
    expect(voiceState.state).toBe("reconnecting");
    joinConnects();
    await vi.advanceTimersByTimeAsync(3000);
    await flush();
    expect(voiceState.state).toBe("connected");
  });

  it("(b) manual join pending through a socket loss, then fails: no recovery, no voice_join", async () => {
    const hang = joinHangs();
    const joining = joinVoice("ch-1").catch(() => undefined);
    await flush();
    noteSignallingLost();
    expect(isVoiceRejoinRunning()).toBe(false); // nothing established yet
    void rejoinVoiceAfterReconnect(); // socket back
    await vi.advanceTimersByTimeAsync(1000); // where an attempt would bump the epoch
    hang.release(false);
    await joining;
    await vi.advanceTimersByTimeAsync(60_000);

    expect(mockAdapter.join).toHaveBeenCalledTimes(1);
    expect(isVoiceRejoinRunning()).toBe(false);
    expect(voiceState.state).toBe("disconnected");
    expect(voiceState.channelId).toBeNull();
  });

  it("(c) manual join succeeded, then a socket loss: recovery runs (control)", async () => {
    await inCall();
    noteSignallingLost();
    expect(isVoiceRejoinRunning()).toBe(true);
    joinConnects();
    const flow = rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(1000);
    await flow;
    expect(mockAdapter.join).toHaveBeenCalledTimes(2);
    expect(voiceState.state).toBe("connected");
  });

  it("(d) an older join failing late leaves a newer session and its recovery alone", async () => {
    const older = joinHangs();
    const olderJoin = joinVoice("ch-1").catch(() => undefined);
    await flush();
    // The user leaves and joins another channel while the old one hangs.
    await leaveVoice();
    mockAdapter.join.mockResolvedValueOnce({ ok: true });
    await joinVoice("ch-2");
    connected();
    expect(voiceState.channelId).toBe("ch-2");

    noteSignallingLost();
    expect(isVoiceRejoinRunning()).toBe(true);
    older.release(false);
    await olderJoin;
    expect(isVoiceRejoinRunning()).toBe(true);
    expect(voiceState.channelId).toBe("ch-2");

    joinConnects();
    const flow = rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(1000);
    await flow;
    expect(mockAdapter.join).toHaveBeenLastCalledWith({ token: expect.any(Number), channelId: "ch-2" });
    expect(voiceState.state).toBe("connected");
  });

  it("a slow leave does not reset a session started after it (W1c audit)", async () => {
    await inCall();
    let leaveDone!: () => void;
    mockAdapter.leave.mockImplementationOnce(
      () => new Promise((r) => (leaveDone = () => r({ ok: true }))),
    );
    const leaving = leaveVoice();
    await flush();
    mockAdapter.join.mockResolvedValueOnce({ ok: true });
    const joining = joinVoice("ch-2");
    await flush();
    leaveDone();
    await leaving;
    await joining;
    expect(voiceState.channelId).toBe("ch-2");
    expect(voiceState.state).not.toBe("disconnected");
  });
});
