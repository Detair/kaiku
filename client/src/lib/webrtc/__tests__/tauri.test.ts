/**
 * TauriVoiceAdapter owns the one set of native `voice:*` listeners.
 *
 * Registration is async (`listen()` resolves later), so an adapter disposed
 * before its registrations finished used to leave them behind; the next
 * adapter then added a second set and every native voice event ran twice.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

type Handler = (event: { payload: unknown }) => void;

/** Live listeners per event name, as the Tauri event bus would hold them. */
const registry = new Map<string, Set<Handler>>();
/** Pending `listen()` resolutions, released by `flushListen()`. */
let pending: Array<() => void> = [];
/** Event names whose registration fails. */
const failing = new Set<string>();

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((name: string, handler: Handler) => {
    return new Promise<() => void>((resolve, reject) => {
      pending.push(() => {
        if (failing.has(name)) {
          reject(new Error(`listen ${name} failed`));
          return;
        }
        let set = registry.get(name);
        if (!set) {
          set = new Set();
          registry.set(name, set);
        }
        set.add(handler);
        resolve(() => set.delete(handler));
      });
    });
  }),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@sentry/browser", () => ({ addBreadcrumb: vi.fn() }));

import { TauriVoiceAdapter } from "../tauri";

/** Let every queued `listen()` resolve (repeatedly, for sequential awaits). */
async function flushListen(): Promise<void> {
  for (let i = 0; i < 50; i++) {
    const batch = pending;
    pending = [];
    batch.forEach((release) => release());
    await Promise.resolve();
    await Promise.resolve();
  }
}

function emit(name: string, payload: unknown): void {
  for (const handler of registry.get(name) ?? []) handler({ payload });
}

function listenerCount(name: string): number {
  return registry.get(name)?.size ?? 0;
}

describe("TauriVoiceAdapter native events", () => {
  beforeEach(() => {
    registry.clear();
    pending = [];
    failing.clear();
  });

  it("voice events are handled once per event", async () => {
    // An adapter disposed while its registrations are still in flight…
    const stale = new TauriVoiceAdapter();
    const staleSpeaking = vi.fn();
    stale.setEventHandlers({ onSpeakingChange: staleSpeaking });
    stale.dispose();

    // …and the adapter that replaces it.
    const adapter = new TauriVoiceAdapter();
    const onSpeakingChange = vi.fn();
    const onAudioError = vi.fn();
    const onStateChange = vi.fn();
    const onAudioRecovered = vi.fn();
    adapter.setEventHandlers({ onSpeakingChange, onAudioError, onStateChange, onAudioRecovered });
    await flushListen();

    for (const name of [
      "voice:speaking",
      "voice:state_change",
      "voice:audio_error",
      "voice:remote_track",
    ]) {
      expect(listenerCount(name), name).toBe(1);
    }

    emit("voice:speaking", true);
    emit("voice:state_change", "Connected");
    emit("voice:audio_error", { kind: "capture_failed", device: "USB Mic" });
    emit("voice:audio_recovered", { kind: "capture_failed", device: "USB Mic" });

    expect(onSpeakingChange).toHaveBeenCalledTimes(1);
    expect(onSpeakingChange).toHaveBeenCalledWith(true);
    expect(onStateChange).toHaveBeenCalledTimes(1);
    expect(onAudioError).toHaveBeenCalledTimes(1);
    expect(onAudioError).toHaveBeenCalledWith({ kind: "capture_failed", device: "USB Mic" });
    expect(onAudioRecovered).toHaveBeenCalledTimes(1);
    expect(staleSpeaking).not.toHaveBeenCalled();

    adapter.dispose();
    expect(listenerCount("voice:speaking")).toBe(0);
    expect(listenerCount("voice:audio_error")).toBe(0);
  });

  it("releases the registered listeners when one registration fails", async () => {
    failing.add("voice:speaking");
    const errors = vi.spyOn(console, "error").mockImplementation(() => {});
    const adapter = new TauriVoiceAdapter();
    await flushListen();

    for (const name of ["voice:state_change", "voice:audio_error", "voice:remote_track"]) {
      expect(listenerCount(name), name).toBe(0);
    }
    expect(errors).toHaveBeenCalled();
    errors.mockRestore();
    adapter.dispose();
  });

  it("forwards per-person volume to the native mixer", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    const adapter = new TauriVoiceAdapter();
    await flushListen();

    const result = await adapter.setUserVolume("u-1", 0.5);

    expect(result.ok).toBe(true);
    expect(invoke).toHaveBeenCalledWith("set_user_volume", { userId: "u-1", volume: 0.5 });
    adapter.dispose();
  });

  it("reports a busy device as in use and a default fallback with its message", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    const adapter = new TauriVoiceAdapter();
    await flushListen();

    vi.mocked(invoke).mockRejectedValueOnce(
      "Audio device is busy: another program or the sound server is using it. Choose the system default instead.",
    );
    const busy = await adapter.setInputDevice("alsa:sysdefault:CARD=USB");
    expect(busy).toMatchObject({ ok: false, error: { type: "device_in_use" } });

    const fallback =
      "Could not switch the microphone (x). The previous microphone could not be reopened either, so the system default is used now.";
    vi.mocked(invoke).mockRejectedValueOnce(fallback);
    const fellBack = await adapter.setInputDevice("alsa:sysdefault:CARD=USB");
    expect(fellBack).toEqual({ ok: false, error: { type: "unknown", message: fallback } });
    adapter.dispose();
  });

  it("maps the native device list to device ids, not names", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockResolvedValueOnce({
      inputs: [
        { device_id: "alsa:default", label: "System default (PipeWire)", is_default: true, curated: true },
        {
          device_id: "alsa:eq",
          label: "Equalizer [eq]",
          is_default: false,
          curated: false,
          legacy_name: "Equalizer",
        },
      ],
      outputs: [],
    });
    const adapter = new TauriVoiceAdapter();
    await flushListen();

    const result = await adapter.getAudioDevices();

    expect(result).toEqual({
      ok: true,
      value: {
        inputs: [
          { deviceId: "alsa:default", label: "System default (PipeWire)", isDefault: true, curated: true },
          {
            deviceId: "alsa:eq",
            label: "Equalizer [eq]",
            isDefault: false,
            curated: false,
            legacyName: "Equalizer",
          },
        ],
        outputs: [],
      },
    });
    adapter.dispose();
  });
});

describe("TauriVoiceAdapter join epoch (C32 cancellation)", () => {
  it("passes the epoch to join_voice and to leave/teardown", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    const { currentVoiceEpoch } = await import("../voiceSession");
    const adapter = new TauriVoiceAdapter();
    const epoch = currentVoiceEpoch();
    await adapter.join("ch", { epoch });
    expect(invoke).toHaveBeenCalledWith("join_voice", { channelId: "ch", epoch });
    await adapter.teardown();
    expect(invoke).toHaveBeenCalledWith("leave_voice", { silent: true, epoch });
  });

  it("a native VOICE_JOIN_CANCELLED is a cancelled result, not an error", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    const { currentVoiceEpoch } = await import("../voiceSession");
    vi.mocked(invoke).mockRejectedValueOnce("VOICE_JOIN_CANCELLED");
    const adapter = new TauriVoiceAdapter();
    const result = await adapter.join("ch", { epoch: currentVoiceEpoch() });
    expect(result).toEqual({ ok: false, error: expect.objectContaining({ type: "cancelled" }) });
    expect(adapter.getState()).toBe("disconnected");
  });

  it("a join that returns after its session ended does not claim the channel", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    const { currentVoiceEpoch, bumpVoiceEpoch } = await import("../voiceSession");
    let release!: () => void;
    vi.mocked(invoke).mockImplementationOnce(
      () => new Promise<undefined>((r) => (release = () => r(undefined))),
    );
    const adapter = new TauriVoiceAdapter();
    const join = adapter.join("ch", { epoch: currentVoiceEpoch() });
    bumpVoiceEpoch();
    release();
    expect((await join).ok).toBe(false);
    expect(adapter.getChannelId()).toBeNull();
  });
});
