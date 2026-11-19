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
    // A session, so there is one whose state can be reported.
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockImplementation((async (cmd: string) =>
      cmd === "begin_voice_session" ? 5 : undefined) as typeof invoke);
    const session = { token: 1, channelId: "ch" };
    await adapter.join(session);
    onStateChange.mockClear();

    for (const name of [
      "voice:speaking",
      "voice:session_state",
      "voice:audio_error",
      "voice:remote_track",
    ]) {
      expect(listenerCount(name), name).toBe(1);
    }

    emit("voice:speaking", true);
    emit("voice:session_state", { epoch: 5, state: "Connected" });
    emit("voice:audio_error", { kind: "capture_failed", device: "USB Mic" });
    emit("voice:audio_recovered", { kind: "capture_failed", device: "USB Mic" });

    expect(onSpeakingChange).toHaveBeenCalledTimes(1);
    expect(onSpeakingChange).toHaveBeenCalledWith(true);
    expect(onStateChange).toHaveBeenCalledTimes(1);
    expect(onStateChange).toHaveBeenCalledWith("connected", session);
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

    for (const name of ["voice:session_state", "voice:audio_error", "voice:remote_track"]) {
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

describe("TauriVoiceAdapter sessions (one handle per join)", () => {
  beforeEach(async () => {
    registry.clear();
    pending = [];
    failing.clear();
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockReset();
  });

  /** Native epoch allocator as the Rust process would run it. */
  async function nativeCounter(start = 100): Promise<() => number> {
    const { invoke } = await import("@tauri-apps/api/core");
    let last = start;
    vi.mocked(invoke).mockImplementation((async (cmd: string) => {
      if (cmd === "begin_voice_session") return ++last;
      return undefined;
    }) as typeof invoke);
    return () => last;
  }

  const A = { token: 1, channelId: "ch-a" };
  const B = { token: 2, channelId: "ch-b" };

  it("joins under a native epoch and ends the session by that epoch", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    const last = await nativeCounter();
    const adapter = new TauriVoiceAdapter();
    const joined = await adapter.join(A);
    expect(joined).toEqual({ ok: true, value: A });
    expect(invoke).toHaveBeenCalledWith("begin_voice_session");
    expect(invoke).toHaveBeenCalledWith("join_voice", { channelId: "ch-a", epoch: last() });
    await adapter.teardown(A);
    expect(invoke).toHaveBeenCalledWith("leave_voice", { silent: true, epoch: last() });
    expect(adapter.getChannelId()).toBeNull();
  });

  it("leave tells the server; teardown does not", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    const last = await nativeCounter();
    const adapter = new TauriVoiceAdapter();
    await adapter.join(A);
    await adapter.leave(A);
    expect(invoke).toHaveBeenLastCalledWith("leave_voice", { silent: false, epoch: last() });
  });

  it("ending a session the adapter does not have does nothing (never another one's)", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    await nativeCounter();
    const adapter = new TauriVoiceAdapter();
    await adapter.join(B);
    vi.mocked(invoke).mockClear();
    await adapter.leave(A); // A was never (or is no longer) this adapter's
    await adapter.teardown(A);
    expect(invoke).not.toHaveBeenCalled();
    expect(adapter.getChannelId()).toBe("ch-b");
  });

  it("ended while native allocates the epoch: join_voice and leave_voice are never invoked", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    let release!: (v: number) => void;
    vi.mocked(invoke).mockImplementation(((cmd: string) =>
      cmd === "begin_voice_session"
        ? new Promise<number>((r) => (release = r))
        : Promise.resolve(undefined)) as typeof invoke);
    const adapter = new TauriVoiceAdapter();
    const join = adapter.join(A);
    await Promise.resolve();
    await adapter.teardown(A);
    release(7);
    expect(await join).toEqual({ ok: false, error: expect.objectContaining({ type: "cancelled" }) });
    expect(invoke).not.toHaveBeenCalledWith("join_voice", expect.anything());
    expect(invoke).not.toHaveBeenCalledWith("leave_voice", expect.anything());
  });

  it("ended while join_voice runs: native is told the epoch, the join is a silent cancel", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    let fail!: (reason: string) => void;
    vi.mocked(invoke).mockImplementation(((cmd: string) => {
      if (cmd === "begin_voice_session") return Promise.resolve(9);
      if (cmd === "join_voice") return new Promise((_, reject) => (fail = reject));
      return Promise.resolve(undefined);
    }) as typeof invoke);
    const adapter = new TauriVoiceAdapter();
    const join = adapter.join(A);
    await vi.waitFor(() => expect(fail).toBeTypeOf("function"));
    await adapter.leave(A);
    expect(invoke).toHaveBeenCalledWith("leave_voice", { silent: false, epoch: 9 });
    fail("VOICE_JOIN_CANCELLED"); // what native answers to the cancelled join
    expect(await join).toEqual({ ok: false, error: expect.objectContaining({ type: "cancelled" }) });
    expect(adapter.getChannelId()).toBeNull();
  });

  it("native VOICE_JOIN_CANCELLED while this window still wants the join is a failure", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockImplementation((async (cmd: string) => {
      if (cmd === "begin_voice_session") return 1;
      if (cmd === "join_voice") throw "VOICE_JOIN_CANCELLED";
      return undefined;
    }) as typeof invoke);
    const adapter = new TauriVoiceAdapter();
    const result = await adapter.join(A);
    expect(result).toEqual({ ok: false, error: expect.objectContaining({ type: "connection_failed" }) });
  });

  it("a failed join leaves nothing behind natively", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockImplementation((async (cmd: string) => {
      if (cmd === "begin_voice_session") return 4;
      if (cmd === "join_voice") throw "create offer failed";
      return undefined;
    }) as typeof invoke);
    const adapter = new TauriVoiceAdapter();
    expect((await adapter.join(A)).ok).toBe(false);
    expect(invoke).toHaveBeenCalledWith("leave_voice", { silent: false, epoch: 4 });
    expect(adapter.getChannelId()).toBeNull();
  });

  it("a join that returns after its session ended does not claim the channel", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    let release!: () => void;
    vi.mocked(invoke).mockImplementation(((cmd: string) =>
      cmd === "join_voice"
        ? new Promise<undefined>((r) => (release = () => r(undefined)))
        : Promise.resolve(1)) as typeof invoke);
    const adapter = new TauriVoiceAdapter();
    const join = adapter.join(A);
    await vi.waitFor(() => expect(release).toBeTypeOf("function"));
    await adapter.teardown(A);
    release();
    expect((await join).ok).toBe(false);
    expect(adapter.getChannelId()).toBeNull();
  });

  it("leave ends the local session even when native leave fails", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockImplementation((async (cmd: string) => {
      if (cmd === "begin_voice_session") return 1;
      if (cmd === "leave_voice") throw "boom";
      return undefined;
    }) as typeof invoke);
    const adapter = new TauriVoiceAdapter();
    await adapter.join(A);
    expect(adapter.getChannelId()).toBe("ch-a");
    await adapter.leave(A);
    expect(adapter.getChannelId()).toBeNull();
    expect(adapter.getState()).toBe("disconnected");
  });

  it("the publisher probe asks native about this session's epoch, never from remembered state", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    let alive = true;
    vi.mocked(invoke).mockImplementation((async (cmd: string) => {
      if (cmd === "begin_voice_session") return 7;
      if (cmd === "voice_probe") return alive;
      return undefined;
    }) as typeof invoke);
    const adapter = new TauriVoiceAdapter();
    await adapter.join(A);
    expect(await adapter.probePublisher(A)).toBe(true);
    expect(invoke).toHaveBeenLastCalledWith("voice_probe", { epoch: 7 });
    alive = false; // no state event arrived, the adapter's own state is unchanged
    expect(await adapter.probePublisher(A)).toBe(false);
    // A session the adapter does not hold is never reported alive.
    vi.mocked(invoke).mockClear();
    expect(await adapter.probePublisher(B)).toBe(false);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("ending a session reports disconnected for that session only", async () => {
    await nativeCounter();
    const adapter = new TauriVoiceAdapter();
    const onStateChange = vi.fn();
    adapter.setEventHandlers({ onStateChange });
    await adapter.join(A);
    await adapter.leave(A);
    await adapter.join(B);
    expect(onStateChange.mock.calls).toEqual([
      ["connecting", A],
      ["disconnected", A],
      ["connecting", B],
    ]);
  });

  it("a late native state of an ended session is not the next session's", async () => {
    const last = await nativeCounter(40);
    const adapter = new TauriVoiceAdapter();
    const onStateChange = vi.fn();
    const onConnectionLost = vi.fn();
    adapter.setEventHandlers({ onStateChange, onConnectionLost });
    await flushListen();
    await adapter.join(A);
    const epochOfA = last();
    await adapter.leave(A);
    await adapter.join(B);
    onStateChange.mockClear();

    // A's connection closing (and failing) is reported after B joined.
    emit("voice:session_state", { epoch: epochOfA, state: "Disconnected" });
    emit("voice:session_state", { epoch: epochOfA, state: "Failed" });
    expect(onStateChange).not.toHaveBeenCalled();
    expect(onConnectionLost).not.toHaveBeenCalled();

    emit("voice:session_state", { epoch: last(), state: "Connected" });
    expect(onStateChange).toHaveBeenCalledWith("connected", B);
    emit("voice:session_state", { epoch: last(), state: "Failed" });
    expect(onConnectionLost).toHaveBeenCalledWith(B);
    adapter.dispose();
  });

  it("a reloaded window (new JS context) joins under the native order", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    const last = await nativeCounter();
    // Old window: a session, left again.
    const before = new TauriVoiceAdapter();
    await before.join(A);
    await before.leave(A);
    // Reload: fresh JS modules (tokens start over), same native process.
    vi.resetModules();
    const fresh = await import("../tauri");
    const session = await import("../voiceSession");
    const after = new fresh.TauriVoiceAdapter();
    const handle = { token: session.nextVoiceToken(), channelId: "ch-a" };
    expect(handle.token).toBe(1);
    const result = await after.join(handle);
    expect(result.ok).toBe(true);
    expect(invoke).toHaveBeenLastCalledWith("join_voice", { channelId: "ch-a", epoch: last() });
  });
});
