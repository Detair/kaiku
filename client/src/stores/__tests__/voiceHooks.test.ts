/**
 * The voice store's set-up, clean-up and recovery steps belong to one
 * session each: a step that finishes late (after the owner stopped waiting
 * for it) only releases what it created itself and never touches the
 * session the user is in by then. Also: the end of the signed-in session
 * and `voice_removed` for a session taken over by another device.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type Session = { token: number; channelId: string };
type Handlers = {
  onStateChange: (s: string, session: Session | null) => void;
  onConnectionLost?: (session: Session | null) => void;
};

interface Deferred {
  promise: Promise<void>;
  resolve: () => void;
}
function deferred(): Deferred {
  let resolve!: () => void;
  const promise = new Promise<void>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

let handlers: Handlers | null = null;

const mockAdapter = {
  join: vi.fn(),
  leave: vi.fn(),
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
}));

vi.mock("@/lib/tauri/common", () => ({
  isTauri: () => false,
}));

vi.mock("@/stores/channels", () => ({
  channelsState: { channels: [] },
}));

const account = vi.hoisted(() => ({ id: "u1" as string | null }));
vi.mock("@/stores/auth", () => ({
  currentUser: () => (account.id ? { id: account.id } : null),
}));

const settings = vi.hoisted(() => ({ pushToTalk: true }));
vi.mock("@/stores/settings", () => ({
  updateAudioSetting: vi.fn().mockResolvedValue(undefined),
  appSettings: vi.fn(() => ({
    voice: {
      push_to_talk: settings.pushToTalk,
      push_to_talk_key: "KeyV",
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

/** Push-to-talk as the store sees it: controllers and shortcut listeners. */
const ptt = vi.hoisted(() => ({
  controllers: [] as Array<{ active: boolean }>,
  /** Unregister functions handed out, with the controller they belong to. */
  listeners: [] as Array<{ controller: unknown; unregister: ReturnType<typeof vi.fn> }>,
  /** The next registration resolves only when this does. */
  slowRegistration: null as Promise<void> | null,
  /** The next unregistration resolves only when this does. */
  slowUnregistration: null as Promise<void> | null,
}));

vi.mock("@/lib/pttManager", () => ({
  PttController: class {
    active = false;
    constructor(readonly setMute: (muted: boolean) => unknown) {
      ptt.controllers.push(this);
    }
    activate() {
      this.active = true;
    }
    deactivate() {
      this.active = false;
    }
    isPttOrPtmEnabled() {
      return this.active;
    }
  },
  createTauriPttListeners: async (controller: unknown) => {
    const registering = ptt.slowRegistration;
    ptt.slowRegistration = null;
    const unregister = vi.fn(() => {
      const slow = ptt.slowUnregistration;
      ptt.slowUnregistration = null;
      return slow ?? undefined;
    });
    if (registering) await registering;
    ptt.listeners.push({ controller, unregister });
    return unregister;
  },
}));

import {
  voiceState,
  setVoiceState,
  joinVoice,
  leaveVoice,
  handleVoiceRemoved,
  isPttActive,
  updatePttFromSettings,
  noteSignallingLost,
  noteSignallingUp,
  rejoinVoiceAfterReconnect,
} from "../voice";
import { HOOK_LIMIT_MS, voiceOwner } from "../voiceOwner";

async function flush(): Promise<void> {
  for (let i = 0; i < 20; i++) await Promise.resolve();
}

function lastSession(): Session {
  const calls = mockAdapter.join.mock.calls;
  return calls[calls.length - 1]![0] as Session;
}

function connected(): void {
  handlers!.onStateChange("connected", lastSession());
}

function joinConnects(): void {
  mockAdapter.join.mockImplementationOnce(async (session: Session) => {
    queueMicrotask(() => handlers!.onStateChange("connected", session));
    return { ok: true, value: session };
  });
}

describe("voice store steps are bound to their session", () => {
  beforeEach(async () => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    account.id = "u1";
    settings.pushToTalk = true;
    ptt.controllers.length = 0;
    ptt.listeners.length = 0;
    ptt.slowRegistration = null;
    ptt.slowUnregistration = null;
    const ends = async (session: Session) => {
      handlers?.onStateChange("disconnected", session);
      return { ok: true };
    };
    mockAdapter.join.mockReset().mockImplementation(async (session: Session) => ({
      ok: true,
      value: session,
    }));
    mockAdapter.leave.mockReset().mockImplementation(ends);
    mockAdapter.teardown.mockReset().mockImplementation(ends);
    for (const level of ["log", "info", "warn"] as const) {
      vi.spyOn(console, level).mockImplementation(() => {});
    }
    await rejoinVoiceAfterReconnect(); // socket known up
    setVoiceState({ state: "disconnected", channelId: null, muted: false, deafened: false, lostChannelId: null });
  });

  afterEach(async () => {
    const leaving = leaveVoice();
    await vi.advanceTimersByTimeAsync(HOOK_LIMIT_MS);
    await leaving;
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  it("a clean-up that finishes late leaves the next channel's push-to-talk alone", async () => {
    await joinVoice("A");
    connected();
    expect(isPttActive()).toBe(true);
    const controllerOfA = ptt.controllers[0]!;

    // Leaving A: taking the shortcut away is slow.
    const unregistering = deferred();
    ptt.slowUnregistration = unregistering.promise;
    const leaving = leaveVoice();
    await vi.advanceTimersByTimeAsync(HOOK_LIMIT_MS); // the owner goes on without it
    await leaving;

    await joinVoice("B");
    connected();
    const controllerOfB = ptt.controllers[ptt.controllers.length - 1]!;
    expect(controllerOfB).not.toBe(controllerOfA);
    expect(controllerOfB.active).toBe(true);
    expect(isPttActive()).toBe(true);
    mockAdapter.setMute.mockClear();
    mockAdapter.setVadConfig.mockClear();

    // A's clean-up resumes while the user is in B.
    unregistering.resolve();
    await flush();

    expect(controllerOfB.active).toBe(true); // B still talks only while the key is held
    expect(isPttActive()).toBe(true);
    expect(mockAdapter.setMute).not.toHaveBeenCalled(); // B's microphone was not opened
    expect(mockAdapter.setVadConfig).not.toHaveBeenCalled();
    expect(controllerOfA.active).toBe(false);
  });

  it("a set-up that finishes late releases what it registered and leaves the next channel alone", async () => {
    // Joining A: registering the shortcut is slow.
    const registering = deferred();
    ptt.slowRegistration = registering.promise;
    const joining = joinVoice("A");
    await vi.advanceTimersByTimeAsync(HOOK_LIMIT_MS);
    await joining;
    connected();
    await leaveVoice();

    await joinVoice("B");
    connected();
    // B has its own push-to-talk, whatever A's registration is doing.
    const controllerOfB = ptt.controllers[ptt.controllers.length - 1]!;
    expect(controllerOfB).not.toBe(ptt.controllers[0]);
    expect(controllerOfB.active).toBe(true);
    expect(isPttActive()).toBe(true);
    mockAdapter.setVadConfig.mockClear();
    mockAdapter.setMute.mockClear();

    // A's registration completes while the user is in B.
    registering.resolve();
    await flush();

    const listenerOfA = ptt.listeners.find((l) => l.controller === ptt.controllers[0]);
    expect(listenerOfA?.unregister).toHaveBeenCalledTimes(1); // released again
    expect(controllerOfB.active).toBe(true);
    expect(isPttActive()).toBe(true);
    expect(mockAdapter.setVadConfig).not.toHaveBeenCalled();
    expect(mockAdapter.setMute).not.toHaveBeenCalled();
  });

  it("a recovery step that finishes late does not change mute in the next channel", async () => {
    settings.pushToTalk = false;
    await joinVoice("A");
    connected();
    setVoiceState({ muted: true, deafened: true });

    // The rejoin connects; restoring deafen on the new connection hangs.
    let finishRestore!: () => void;
    mockAdapter.setDeafen
      .mockResolvedValueOnce({ ok: true })
      .mockImplementationOnce(() => new Promise((r) => (finishRestore = () => r({ ok: true }))));
    joinConnects();
    void rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(1000);
    await flush();
    expect(finishRestore).toBeTypeOf("function");

    // The user moves on to B and talks there.
    await joinVoice("B");
    connected();
    setVoiceState({ muted: false, deafened: false });
    mockAdapter.setMute.mockClear();
    mockAdapter.setDeafen.mockClear();
    mockAdapter.setInputDevice.mockClear();

    finishRestore();
    await flush();

    expect(mockAdapter.setMute).not.toHaveBeenCalled();
    expect(mockAdapter.setDeafen).not.toHaveBeenCalled();
    expect(voiceState.muted).toBe(false);
    expect(voiceState.channelId).toBe("B");
  });

  it("two overlapping push-to-talk settings changes and a leave: no shortcut stays registered", async () => {
    await joinVoice("A");
    connected();
    expect(ptt.listeners).toHaveLength(1);

    // The user changes the key twice in a row; the first registration is slow.
    const firstRegistration = deferred();
    ptt.slowRegistration = firstRegistration.promise;
    const first = updatePttFromSettings();
    await flush();
    const second = updatePttFromSettings();
    await flush();
    firstRegistration.resolve();
    await first;
    await second;

    // Exactly one shortcut is registered now: the last one.
    const registered = () => ptt.listeners.filter((l) => l.unregister.mock.calls.length === 0);
    expect(registered()).toHaveLength(1);
    expect(isPttActive()).toBe(true);

    await leaveVoice();
    expect(registered()).toHaveLength(0);
    expect(ptt.controllers.every((c) => !c.active)).toBe(true);
    expect(isPttActive()).toBe(false);
  });

  it("signing out in a voice channel: left at once, and the next account gets nothing of it", async () => {
    await joinVoice("A");
    connected();
    const session = lastSession();

    voiceOwner.shutdown(); // what every end of the signed-in session calls first
    // Before anything is awaited: adapter told, screen cleared.
    expect(mockAdapter.leave).toHaveBeenCalledWith(session);
    expect(voiceState.state).toBe("disconnected");
    expect(voiceState.channelId).toBeNull();

    // The next account signs in; its socket connects within the minute.
    account.id = "u2";
    noteSignallingLost();
    void rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(90_000);
    expect(mockAdapter.join).toHaveBeenCalledTimes(1);
    expect(voiceState.lostChannelId).toBeNull();
    expect(voiceState.state).toBe("disconnected");
  });

  it("signing out while the Rejoin offer is shown clears the offer", async () => {
    await joinVoice("A");
    connected();
    mockAdapter.join.mockResolvedValue({ ok: false, error: { type: "unknown", message: "x" } });
    void rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(13_000);
    expect(voiceState.lostChannelId).toBe("A");

    voiceOwner.shutdown();
    expect(voiceState.lostChannelId).toBeNull();
  });

  it("a Rejoin offer is never shown to another account, even if nobody ended the session", async () => {
    await joinVoice("A");
    connected();
    mockAdapter.join.mockResolvedValue({ ok: false, error: { type: "unknown", message: "x" } });
    void rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(13_000);
    expect(voiceState.lostChannelId).toBe("A");

    account.id = "u2"; // another account, and no sign-out was reported
    noteSignallingUp();
    expect(voiceState.lostChannelId).toBeNull();
  });

  it("taken over by another device (session_replaced): given up, never rejoined", async () => {
    settings.pushToTalk = false;
    await joinVoice("A");
    connected();
    const session = lastSession();
    expect(await handleVoiceRemoved("A", "session_replaced")).toBe(true);
    expect(mockAdapter.teardown).toHaveBeenCalledWith(session);
    expect(mockAdapter.leave).not.toHaveBeenCalled(); // a leave would hit the other device
    expect(voiceState.state).toBe("disconnected");

    // Neither an ICE failure reported late nor a new socket brings it back.
    handlers!.onConnectionLost!(session);
    noteSignallingLost();
    void rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(90_000);
    expect(mockAdapter.join).toHaveBeenCalledTimes(1);
    expect(voiceState.lostChannelId).toBeNull();
  });
});
