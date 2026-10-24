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

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((name: string, handler: Handler) => {
    return new Promise<() => void>((resolve) => {
      pending.push(() => {
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
    adapter.setEventHandlers({ onSpeakingChange, onAudioError, onStateChange });
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

    expect(onSpeakingChange).toHaveBeenCalledTimes(1);
    expect(onSpeakingChange).toHaveBeenCalledWith(true);
    expect(onStateChange).toHaveBeenCalledTimes(1);
    expect(onAudioError).toHaveBeenCalledTimes(1);
    expect(onAudioError).toHaveBeenCalledWith({ kind: "capture_failed", device: "USB Mic" });
    expect(staleSpeaking).not.toHaveBeenCalled();

    adapter.dispose();
    expect(listenerCount("voice:speaking")).toBe(0);
    expect(listenerCount("voice:audio_error")).toBe(0);
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
});
