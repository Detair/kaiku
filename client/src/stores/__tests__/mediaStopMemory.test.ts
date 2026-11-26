/**
 * The memory of a camera or share that stopped before its first track
 * belongs to one voice session: switching to another channel forgets it,
 * so a camera that already runs in the new channel is shown.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type Session = { token: number; channelId: string };
type Handlers = { onStateChange: (s: string, session: Session | null) => void };

let handlers: Handlers | null = null;

const mockAdapter = {
  join: vi.fn(async (session: Session) => ({ ok: true, value: session })),
  leave: vi.fn(async (session: Session) => {
    handlers?.onStateChange("disconnected", session);
    return { ok: true };
  }),
  teardown: vi.fn(async (session: Session) => {
    handlers?.onStateChange("disconnected", session);
    return { ok: true };
  }),
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
vi.mock("@/lib/tauri", () => ({ wsSend: vi.fn() }));
vi.mock("@/lib/tauri/common", () => ({ isTauri: () => false }));
vi.mock("@/stores/channels", () => ({ channelsState: { channels: [] } }));
vi.mock("@/stores/auth", () => ({ currentUser: () => ({ id: "u1" }) }));
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
vi.mock("@/components/ui/Toast", () => ({ showToast: vi.fn(), dismissToast: vi.fn() }));

import { joinVoice, leaveVoice, rejoinVoiceAfterReconnect, setVoiceState } from "../voice";
import { HOOK_LIMIT_MS } from "../voiceOwner";
import { handleScreenShareStopped, handleWebcamStopped } from "../websocket/mediaEvents";
import * as webcams from "../webcamViewer";
import * as shares from "../screenShareViewer";

const track = (id: string) =>
  ({ id, kind: "video", readyState: "live" }) as unknown as MediaStreamTrack;

function lastSession(): Session {
  const calls = mockAdapter.join.mock.calls;
  return calls[calls.length - 1]![0] as Session;
}

async function joinAndConnect(channelId: string): Promise<void> {
  const joining = joinVoice(channelId);
  await vi.advanceTimersByTimeAsync(0);
  handlers!.onStateChange("connected", lastSession());
  await joining;
  await vi.advanceTimersByTimeAsync(0);
}

describe("the stop memory ends with the voice session", () => {
  beforeEach(async () => {
    vi.useFakeTimers();
    for (const level of ["log", "info", "warn"] as const) {
      vi.spyOn(console, level).mockImplementation(() => {});
    }
    await rejoinVoiceAfterReconnect(); // socket known up
    setVoiceState({ state: "disconnected", channelId: null, lostChannelId: null });
  });

  afterEach(async () => {
    const leaving = leaveVoice();
    await vi.advanceTimersByTimeAsync(HOOK_LIMIT_MS);
    await leaving;
    webcams.clearAll();
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  it("a camera stop in the old channel does not hide a camera already running in the new one", async () => {
    await joinAndConnect("A");
    // Bea's camera stops in A before its picture reached us.
    await handleWebcamStopped({ channel_id: "A", user_id: "u2", reason: "user" } as never);
    // Within the memory time we switch to B, where Bea's camera already runs
    // (no webcam_started reaches us for it).
    vi.advanceTimersByTime(2000);
    await joinAndConnect("B");
    const running = track("cam-in-b");
    webcams.addAvailableTrack("u2", running);
    expect(webcams.getTrack("u2")).toBe(running);
  });

  it("a late camera stop of the channel just left does not hide a camera in the new one", async () => {
    await joinAndConnect("A");
    await joinAndConnect("B");
    await handleWebcamStopped({ channel_id: "A", user_id: "u2", reason: "user" } as never);
    const running = track("cam-in-b");
    webcams.addAvailableTrack("u2", running);
    expect(webcams.getTrack("u2")).toBe(running);
  });

  it("a share stop in the old channel is forgotten in the new one", async () => {
    await joinAndConnect("A");
    await handleScreenShareStopped({ channel_id: "A", user_id: "u2", stream_id: "s-x", reason: "user" } as never);
    await joinAndConnect("B");
    const v = track("v-x");
    shares.addAvailableTrack("s-x", v, "u2", "bea", "Screen");
    expect(shares.viewerState.availableTracks.get("s-x")?.track).toBe(v);
    shares.removeAvailableTrack("s-x");
  });

  it("in the same channel a late first track after its stop stays out", async () => {
    await joinAndConnect("A");
    await handleWebcamStopped({ channel_id: "A", user_id: "u2", reason: "user" } as never);
    webcams.addAvailableTrack("u2", track("cam-late"));
    expect(webcams.getTrack("u2")).toBeUndefined();
  });
});
