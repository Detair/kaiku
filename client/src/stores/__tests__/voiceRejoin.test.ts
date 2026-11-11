/**
 * C32: voice rejoin after a lost connection; C31: never after voice_removed.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type Handlers = {
  onStateChange: (s: string) => void;
  onConnectionLost?: () => void;
};

let handlers: Handlers | null = null;

const mockAdapter = {
  join: vi.fn(),
  leave: vi.fn().mockResolvedValue({ ok: true }),
  teardown: vi.fn(),
  setMute: vi.fn().mockResolvedValue({ ok: true }),
  setDeafen: vi.fn().mockResolvedValue({ ok: true }),
  setEventHandlers: vi.fn((h: Handlers) => {
    handlers = h;
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
  noteSignallingUp,
  isVoiceRejoinRunning,
  dismissVoiceRejoin,
  REJOIN_ATTEMPT_TIMEOUT_MS,
} from "../voice";

/** Let pending promise callbacks run without advancing time. */
async function flush(): Promise<void> {
  for (let i = 0; i < 10; i++) await Promise.resolve();
}

/** Join ch-1 and reach "connected". */
async function inCall(): Promise<void> {
  mockAdapter.join.mockResolvedValue({ ok: true });
  await joinVoice("ch-1");
  handlers!.onStateChange("connected");
  expect(voiceState.state).toBe("connected");
}

/** Make the next adapter.join connect right away. */
function joinConnects(): void {
  mockAdapter.join.mockImplementationOnce(async () => {
    queueMicrotask(() => handlers!.onStateChange("connected"));
    return { ok: true };
  });
}

describe("voice rejoin (C32)", () => {
  beforeEach(async () => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    mockAdapter.teardown.mockImplementation(async () => {
      handlers?.onStateChange("disconnected");
      return { ok: true };
    });
    noteSignallingUp();
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
    expect(mockAdapter.join).toHaveBeenLastCalledWith("ch-1");
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
    handlers!.onConnectionLost!();
    expect(voiceState.state).toBe("reconnecting");
    await vi.advanceTimersByTimeAsync(1000);
    await flush();
    expect(mockAdapter.join).toHaveBeenCalledTimes(2);
    expect(voiceState.state).toBe("connected");
  });

  it("an ICE failure while the socket is down waits for the socket", async () => {
    await inCall();
    noteSignallingLost();
    handlers!.onConnectionLost!();
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
});
