/**
 * One owner for the voice session (desktop path).
 *
 * The voice store runs against the REAL TauriVoiceAdapter; the native side
 * is a small model of the Rust commands (`begin_voice_session`,
 * `join_voice`, `leave_voice`) with one native voice state, the epoch
 * floor and the state events. Every command can be parked, so user actions
 * land while a native call is still on its way.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const realImmediate = setImmediate;

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T) => void;
}
function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

// --- model of the native voice commands ------------------------------------

interface NativeModel {
  /** Last session epoch handed out. */
  last: number;
  /** Joins below this epoch are cancelled. */
  floor: number;
  /** The one native voice session, if any. */
  held: { channelId: string; epoch: number | null } | null;
  /** What went to the server: "join:<id>" / "leave:<id>". */
  wire: string[];
  /** Channels whose native session was torn down, in order. */
  tornDown: string[];
  /** What the native peer connection object says when asked. */
  publisherAlive: boolean;
  /** Whether the server answers a ping on the native socket. */
  socketAnswers: boolean;
  listeners: Map<string, Array<(event: { payload: unknown }) => void>>;
  parked: Map<string, Deferred<void>[]>;
}

const native: NativeModel = {
  last: 0,
  floor: 0,
  held: null,
  wire: [],
  tornDown: [],
  publisherAlive: true,
  socketAnswers: true,
  listeners: new Map(),
  parked: new Map(),
};

function emit(event: string, payload: unknown): void {
  for (const listener of native.listeners.get(event) ?? []) listener({ payload });
}

/** Native reports a publisher state for the session it holds. */
function nativeState(state: string, epoch = native.held?.epoch ?? null): void {
  emit("voice:state_change", state);
  emit("voice:session_state", { epoch, state });
}

/** The next call of `command` is processed only when released. */
function park(command: string): Deferred<void> {
  const gate = deferred<void>();
  native.parked.set(command, [...(native.parked.get(command) ?? []), gate]);
  return gate;
}

async function fakeInvoke(command: string, args: Record<string, unknown> = {}): Promise<unknown> {
  const gate = native.parked.get(command)?.shift();
  if (gate) await gate.promise;
  switch (command) {
    case "begin_voice_session":
      native.last += 1;
      return native.last;
    case "join_voice": {
      const channelId = args.channelId as string;
      const epoch = (args.epoch as number | undefined) ?? null;
      if (epoch !== null && epoch < native.floor) throw "VOICE_JOIN_CANCELLED";
      if (native.held) throw "Already in a voice channel. Leave first.";
      native.held = { channelId, epoch };
      native.wire.push(`join:${channelId}`);
      return undefined;
    }
    case "leave_voice": {
      const epoch = (args.epoch as number | undefined) ?? null;
      // Without an epoch: everything allocated so far ends (older callers).
      native.floor = Math.max(native.floor, (epoch ?? native.last) + 1);
      const held = native.held;
      if (!held) return undefined;
      // A leave for an older session never ends a newer one.
      if (epoch !== null && held.epoch !== null && held.epoch > epoch) return undefined;
      native.held = null;
      native.tornDown.push(held.channelId);
      if (!args.silent) native.wire.push(`leave:${held.channelId}`);
      nativeState("Disconnected", held.epoch);
      return undefined;
    }
    case "voice_probe":
      return native.held !== null && native.held.epoch === args.epoch && native.publisherAlive;
    default:
      return undefined;
  }
}

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: Record<string, unknown>) => fakeInvoke(command, args),
  isTauri: () => true,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: async (event: string, handler: (event: { payload: unknown }) => void) => {
    native.listeners.set(event, [...(native.listeners.get(event) ?? []), handler]);
    return () => {
      native.listeners.set(
        event,
        (native.listeners.get(event) ?? []).filter((h) => h !== handler),
      );
    };
  },
}));

vi.mock("@/lib/tauri/common", () => ({
  isTauri: () => true,
}));

vi.mock("@/lib/tauri", () => ({
  wsPing: async () => {
    if (native.socketAnswers) (await import("../voiceOwner")).voiceOwner.noteInbound();
  },
  wsSend: vi.fn().mockResolvedValue(undefined),
  wsScreenShareStart: vi.fn(),
  wsScreenShareStop: vi.fn(),
  wsWebcamStart: vi.fn(),
  wsWebcamStop: vi.fn(),
}));

vi.mock("@/stores/channels", () => ({
  channelsState: { channels: [] },
}));

vi.mock("@/stores/auth", () => ({
  currentUser: () => null,
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
  noteSignallingLost,
  rejoinVoiceAfterReconnect,
} from "../voice";
import { resetVoiceAdapter } from "@/lib/webrtc";
import { voiceOwner } from "../voiceOwner";

/** Let everything that can run, run (microtasks, due timers, module loads). */
async function settle(rounds = 6): Promise<void> {
  for (let i = 0; i < rounds; i++) {
    await vi.advanceTimersByTimeAsync(0);
    await new Promise<void>((r) => realImmediate(r));
  }
}

/** Join `channelId` and reach "connected". */
async function inChannel(channelId: string): Promise<void> {
  await joinVoice(channelId);
  await settle();
  expect(native.held?.channelId).toBe(channelId);
  nativeState("Connected");
  expect(voiceState.state).toBe("connected");
  expect(voiceState.channelId).toBe(channelId);
}

const joinsOnWire = (from = 0) => native.wire.slice(from).filter((m) => m.startsWith("join:"));

describe("one owner for the voice session (desktop adapter)", () => {
  beforeEach(async () => {
    vi.useFakeTimers();
    native.last = 0;
    native.floor = 0;
    native.held = null;
    native.wire = [];
    native.tornDown = [];
    native.publisherAlive = true;
    native.socketAnswers = true;
    native.parked.clear();
    for (const level of ["log", "info", "warn", "error"] as const) {
      vi.spyOn(console, level).mockImplementation(() => {});
    }
    await rejoinVoiceAfterReconnect(); // socket known up
    setVoiceState({ state: "disconnected", channelId: null, lostChannelId: null });
  });

  afterEach(async () => {
    for (const gates of native.parked.values()) gates.forEach((g) => g.resolve());
    await leaveVoice();
    await settle();
    resetVoiceAdapter();
    native.listeners.clear();
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  it("A → B → C while A's native leave is still running: only C is joined", async () => {
    await inChannel("A");

    const leaveOfA = park("leave_voice"); // A's native leave is slow
    const toB = joinVoice("B").catch(() => undefined);
    await settle();
    const toC = joinVoice("C").catch(() => undefined);
    await settle();
    const wireAtLastClick = native.wire.length;
    expect(voiceState.channelId).toBe("C"); // the UI follows the click at once

    leaveOfA.resolve();
    await toB;
    await toC;
    await settle();
    await vi.advanceTimersByTimeAsync(30_000);
    await settle();

    expect(voiceState.channelId).toBe("C");
    expect(native.held?.channelId).toBe("C");
    expect(joinsOnWire(wireAtLastClick)).toEqual(["join:C"]);
    expect(native.wire).not.toContain("join:B");
    // A was torn down once, and nothing else was.
    expect(native.tornDown).toEqual(["A"]);
    expect(native.wire.filter((m) => m === "leave:A")).toHaveLength(1);

    nativeState("Connected");
    expect(voiceState.state).toBe("connected");
    expect(voiceState.channelId).toBe("C");
  });

  it("slept 8 hours, native connections dead, nothing reported yet: given up, no rejoin", async () => {
    await inChannel("A");
    await vi.advanceTimersByTimeAsync(20_000);
    vi.setSystemTime(Date.now() + 8 * 3_600_000);
    native.publisherAlive = false; // what the native peer connection says when asked
    native.socketAnswers = false;

    voiceOwner.wake();
    expect(voiceState.state).toBe("reconnecting");
    await settle();
    expect(voiceState.state).toBe("disconnected");
    expect(voiceState.lostChannelId).toBe("A");
    expect(native.held).toBeNull();
    expect(native.wire).not.toContain("leave:A"); // given up: no voice_leave

    noteSignallingLost();
    void rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(60_000);
    await settle();
    expect(joinsOnWire()).toEqual(["join:A"]);
  });

  it("slept 8 hours and the native connection survived: the call goes on", async () => {
    await inChannel("A");
    await vi.advanceTimersByTimeAsync(20_000);
    vi.setSystemTime(Date.now() + 8 * 3_600_000);

    voiceOwner.wake();
    await settle();
    expect(voiceState.state).toBe("connected");
    expect(native.held?.channelId).toBe("A");
    expect(joinsOnWire()).toEqual(["join:A"]);
    expect(voiceState.lostChannelId).toBeNull();
  });

  it("voice_removed for the channel being left does not touch the one being joined", async () => {
    await inChannel("A");
    const joinOfB = park("join_voice"); // B's native join is slow
    const toB = joinVoice("B").catch(() => undefined);
    await settle();
    expect(native.held).toBeNull(); // A is gone, B not there yet

    await handleVoiceRemoved("A", "kicked");
    await settle();
    expect(voiceState.channelId).toBe("B");
    expect(voiceState.state).toBe("connecting");

    joinOfB.resolve();
    await toB;
    await settle();
    expect(native.held?.channelId).toBe("B");
    expect(joinsOnWire().filter((m) => m === "join:B")).toHaveLength(1);
    nativeState("Connected");
    expect(voiceState.state).toBe("connected");
    expect(voiceState.channelId).toBe("B");
  });

  it("voice_removed for the held session tears it down natively and it is never rejoined", async () => {
    await inChannel("A");
    expect(await handleVoiceRemoved("A", "banned")).toBe(true);
    await settle();

    expect(voiceState.state).toBe("disconnected");
    expect(voiceState.channelId).toBeNull();
    expect(native.held).toBeNull();
    expect(native.tornDown).toEqual(["A"]);
    expect(native.wire).not.toContain("leave:A"); // the server already removed the peer

    noteSignallingLost();
    void rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(30_000);
    await settle();
    expect(joinsOnWire()).toEqual(["join:A"]);
    expect(native.held).toBeNull();
    expect(voiceState.lostChannelId).toBeNull();
  });
});
