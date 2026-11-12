/**
 * One owner for the voice session (browser path).
 *
 * The voice store runs here against the REAL BrowserVoiceAdapter; only the
 * platform is faked (microphone, peer connections, ICE fetch, the socket).
 * Every platform step can be parked or given a delay, so user actions land
 * inside a join or a teardown exactly as they can in the app.
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

// --- the socket -------------------------------------------------------------

type Sent = { type: string; channel_id?: string };
const sent: Sent[] = [];
/** Parked sends by message type: the send resolves when the test says so. */
const parkedSends = new Map<string, Deferred<void>[]>();
/** Delay of a send by message type, in (virtual) ms. */
const sendDelay = new Map<string, number>();

function parkSend(type: string): Deferred<void> {
  const gate = deferred<void>();
  parkedSends.set(type, [...(parkedSends.get(type) ?? []), gate]);
  return gate;
}

async function fakeWsSend(message: Sent): Promise<void> {
  sent.push(message);
  const gate = parkedSends.get(message.type)?.shift();
  if (gate) await gate.promise;
  const delay = sendDelay.get(message.type);
  if (delay) await new Promise((r) => setTimeout(r, delay));
}

vi.mock("@/lib/tauri", () => ({
  wsSend: (m: Sent) => fakeWsSend(m),
  wsStatus: async () => ({ type: "connected" }),
  wsConnect: async () => undefined,
  getServerUrl: () => "http://test",
  getAccessToken: () => "tok",
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

vi.mock("@/lib/tauri/common", () => ({
  isTauri: () => false,
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

// --- microphone and peer connections ---------------------------------------

interface FakeMic {
  stream: MediaStream;
  stop: ReturnType<typeof vi.fn>;
}
const mics: FakeMic[] = [];
const parkedMics: Deferred<void>[] = [];
let gum: ReturnType<typeof vi.fn>;

function newMic(): FakeMic {
  const stop = vi.fn();
  const track = {
    stop,
    enabled: true,
    kind: "audio",
    clone: () => ({ stop: vi.fn(), enabled: true, kind: "audio" }),
  };
  const mic: FakeMic = {
    stop,
    stream: {
      id: `mic-${mics.length}`,
      getTracks: () => [track],
      getAudioTracks: () => [track],
    } as unknown as MediaStream,
  };
  mics.push(mic);
  return mic;
}

/** The next getUserMedia call hangs (permission prompt) until released. */
function parkMic(): Deferred<void> {
  const gate = deferred<void>();
  parkedMics.push(gate);
  return gate;
}

const pcs: FakePc[] = [];
class FakePc {
  connectionState = "new";
  closed = false;
  tracks: unknown[] = [];
  onnegotiationneeded: (() => void) | null = null;
  onicecandidate: (() => void) | null = null;
  onconnectionstatechange: (() => void) | null = null;
  ontrack: (() => void) | null = null;
  constructor() {
    pcs.push(this);
  }
  addTrack(track: unknown) {
    this.tracks.push(track);
    return {};
  }
  getSenders() {
    return [];
  }
  close() {
    this.closed = true;
    this.connectionState = "closed";
  }
  /** The browser reports a new connection state. */
  report(state: string) {
    this.connectionState = state;
    this.onconnectionstatechange?.();
  }
}

const openMics = () => mics.filter((m) => m.stop.mock.calls.length === 0);
const openPcs = () => pcs.filter((pc) => !pc.closed);
const sentOf = (type: string, channel?: string) =>
  sent.filter((m) => m.type === type && (!channel || m.channel_id === channel));

/** Let everything that can run, run (microtasks, due timers, module loads). */
async function settle(rounds = 6): Promise<void> {
  for (let i = 0; i < rounds; i++) {
    await vi.advanceTimersByTimeAsync(0);
    await new Promise<void>((r) => realImmediate(r));
  }
}

/** The open publisher connection reports "connected". */
function publisherConnects(): void {
  const open = openPcs();
  expect(open).toHaveLength(2);
  open[0]!.report("connected");
}

/** Join `channelId` and reach "connected". */
async function inChannel(channelId: string): Promise<void> {
  await joinVoice(channelId);
  await settle();
  publisherConnects();
  expect(voiceState.state).toBe("connected");
  expect(voiceState.channelId).toBe(channelId);
}

describe("one owner for the voice session (browser adapter)", () => {
  beforeEach(async () => {
    vi.useFakeTimers();
    sent.length = 0;
    mics.length = 0;
    pcs.length = 0;
    parkedSends.clear();
    parkedMics.length = 0;
    sendDelay.clear();
    gum = vi.fn(async () => {
      const gate = parkedMics.shift();
      if (gate) await gate.promise;
      return newMic().stream;
    });
    vi.stubGlobal("RTCPeerConnection", FakePc);
    vi.stubGlobal("navigator", { mediaDevices: { getUserMedia: gum } });
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({ ok: true, json: async () => ({ ice_servers: [] }) })),
    );
    for (const level of ["log", "info", "warn", "error"] as const) {
      vi.spyOn(console, level).mockImplementation(() => {});
    }
    await rejoinVoiceAfterReconnect(); // socket known up
    setVoiceState({ state: "disconnected", channelId: null, lostChannelId: null });
  });

  afterEach(async () => {
    for (const gates of parkedSends.values()) gates.forEach((g) => g.resolve());
    parkedMics.forEach((g) => g.resolve());
    await leaveVoice();
    await settle();
    resetVoiceAdapter();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  it("A → B → C while A's leave is still running: only C is joined (W1d)", async () => {
    await inChannel("A");
    const micA = mics[0]!;

    // A's teardown hangs in its last step (telling the server).
    const leaveOfA = parkSend("voice_leave");
    const toB = joinVoice("B").catch(() => undefined);
    await settle();
    // The user changes their mind before that leave has finished.
    const toC = joinVoice("C").catch(() => undefined);
    await settle();
    const sentAtLastClick = sent.length;
    expect(voiceState.channelId).toBe("C"); // the UI follows the click at once

    leaveOfA.resolve();
    await toB;
    await toC;
    await settle();
    await vi.advanceTimersByTimeAsync(30_000);
    await settle();

    expect(voiceState.channelId).toBe("C");
    expect(voiceState.state).toBe("connecting");
    // Exactly one voice_join after the last click, and it is for C.
    expect(sent.slice(sentAtLastClick).filter((m) => m.type === "voice_join")).toEqual([
      { type: "voice_join", channel_id: "C" },
    ]);
    // Nothing of B was ever acquired or announced.
    expect(sentOf("voice_join", "B")).toHaveLength(0);
    expect(gum).toHaveBeenCalledTimes(2); // A and C
    // A was torn down once; C's microphone and connections are the only ones.
    expect(sentOf("voice_leave", "A")).toHaveLength(1);
    expect(micA.stop).toHaveBeenCalledTimes(1);
    expect(openMics()).toHaveLength(1);
    expect(openPcs()).toHaveLength(2);

    publisherConnects();
    expect(voiceState.state).toBe("connected");
    expect(voiceState.channelId).toBe("C");
  });

  it("join A, leave, join A, join B, leave within 50 ms: left, nothing kept, no join after the leave", async () => {
    // A leave takes longer than the pause between the clicks.
    sendDelay.set("voice_leave", 25);

    const actions: Promise<unknown>[] = [];
    actions.push(joinVoice("A").catch(() => undefined));
    await vi.advanceTimersByTimeAsync(10);
    actions.push(leaveVoice());
    await vi.advanceTimersByTimeAsync(10);
    actions.push(joinVoice("A").catch(() => undefined));
    await vi.advanceTimersByTimeAsync(10);
    actions.push(joinVoice("B").catch(() => undefined));
    await vi.advanceTimersByTimeAsync(10);
    actions.push(leaveVoice()); // t = 40 ms
    const sentAtFinalLeave = sent.length;
    expect(voiceState.state).toBe("disconnected"); // left at once on screen
    expect(voiceState.channelId).toBeNull();

    await vi.advanceTimersByTimeAsync(5_000);
    await settle();
    await Promise.all(actions);
    await vi.advanceTimersByTimeAsync(30_000);
    await settle();

    expect(voiceState.state).toBe("disconnected");
    expect(voiceState.channelId).toBeNull();
    expect(openMics()).toHaveLength(0);
    expect(openPcs()).toHaveLength(0);
    expect(sent.slice(sentAtFinalLeave).filter((m) => m.type === "voice_join")).toHaveLength(0);
    // Every room that was joined on the server was also left there.
    for (const join of sentOf("voice_join")) {
      expect(sentOf("voice_leave", join.channel_id).length).toBeGreaterThan(0);
    }
  });

  it("a join waiting for microphone permission is abandoned: the late grant is stopped, the next channel is not delayed", async () => {
    const prompt = parkMic();
    const toA = joinVoice("A").catch(() => undefined);
    await settle();
    expect(gum).toHaveBeenCalledTimes(1);
    expect(voiceState.state).toBe("connecting");

    const left = leaveVoice();
    const toB = joinVoice("B").catch(() => undefined);
    await left;
    await toB; // resolves although A's prompt is still open
    await settle();
    expect(voiceState.channelId).toBe("B");
    expect(sentOf("voice_join")).toEqual([{ type: "voice_join", channel_id: "B" }]);
    publisherConnects();
    expect(voiceState.state).toBe("connected");
    const micB = mics[0]!;
    const pcsOfB = openPcs();

    // The user answers A's prompt much later.
    prompt.resolve();
    await toA;
    await settle();

    const lateMic = mics[1]!;
    expect(lateMic.stop).toHaveBeenCalledTimes(1);
    expect(micB.stop).not.toHaveBeenCalled();
    expect(openPcs()).toEqual(pcsOfB);
    expect(sentOf("voice_join", "A")).toHaveLength(0);
    expect(voiceState.state).toBe("connected");
    expect(voiceState.channelId).toBe("B");
  });

  it("a recovery attempt in flight is abandoned when the user switches channel", async () => {
    await inChannel("A");
    noteSignallingLost();
    expect(voiceState.state).toBe("reconnecting");
    const flow = rejoinVoiceAfterReconnect();
    const prompt = parkMic(); // the attempt's own join hangs on the microphone
    await vi.advanceTimersByTimeAsync(1000);
    await settle();
    expect(gum).toHaveBeenCalledTimes(2);

    const sentAtSwitch = sent.length;
    await joinVoice("B");
    await settle();
    expect(voiceState.channelId).toBe("B");
    publisherConnects();

    prompt.resolve();
    await settle();
    await vi.advanceTimersByTimeAsync(60_000);
    await settle();
    await flow;

    expect(sent.slice(sentAtSwitch).filter((m) => m.type === "voice_join")).toEqual([
      { type: "voice_join", channel_id: "B" },
    ]);
    expect(voiceState.channelId).toBe("B");
    expect(voiceState.state).toBe("connected");
    expect(voiceState.lostChannelId).toBeNull();
    expect(openMics()).toHaveLength(1);
    expect(openPcs()).toHaveLength(2);
  });

  it("voice_removed for the channel being left does not touch the one being joined", async () => {
    await inChannel("A");
    const prompt = parkMic(); // B's join waits for the microphone
    const toB = joinVoice("B").catch(() => undefined);
    await settle();

    await handleVoiceRemoved("A", "kicked");
    await settle();
    expect(voiceState.channelId).toBe("B");
    expect(voiceState.state).toBe("connecting");

    prompt.resolve();
    await toB;
    await settle();
    publisherConnects();
    expect(voiceState.state).toBe("connected");
    expect(voiceState.channelId).toBe("B");
    expect(sentOf("voice_join", "B")).toHaveLength(1);
    expect(openMics()).toHaveLength(1);
  });

  it("voice_removed for the held session tears it down and it is never rejoined", async () => {
    await inChannel("A");
    expect(await handleVoiceRemoved("A", "kicked")).toBe(true);
    await settle();

    expect(voiceState.state).toBe("disconnected");
    expect(voiceState.channelId).toBeNull();
    expect(openMics()).toHaveLength(0);
    expect(openPcs()).toHaveLength(0);
    expect(sentOf("voice_leave")).toHaveLength(0); // the server already removed the peer

    noteSignallingLost();
    void rejoinVoiceAfterReconnect();
    await vi.advanceTimersByTimeAsync(30_000);
    await settle();
    expect(sentOf("voice_join")).toHaveLength(1);
    expect(voiceState.lostChannelId).toBeNull();
  });

  it("a flapping socket that never carries the rejoin gives up 60 s after the first loss", async () => {
    await inChannel("A");
    noteSignallingLost(); // t = 0: the socket that carried the join is gone
    // The socket comes back and drops again before any attempt can start:
    // back at 10 s, 30 s and 50 s, gone again half a second later each time.
    await vi.advanceTimersByTimeAsync(10_000);
    for (let round = 0; round < 3; round++) {
      void rejoinVoiceAfterReconnect();
      await vi.advanceTimersByTimeAsync(500);
      noteSignallingLost();
      if (round < 2) await vi.advanceTimersByTimeAsync(19_500);
    }
    await vi.advanceTimersByTimeAsync(9_499); // t = 59.999 s
    expect(voiceState.state).toBe("reconnecting");
    expect(voiceState.channelId).toBe("A");

    await vi.advanceTimersByTimeAsync(1);
    await settle();
    expect(voiceState.state).toBe("disconnected");
    expect(voiceState.channelId).toBeNull();
    expect(voiceState.lostChannelId).toBe("A");
    expect(openMics()).toHaveLength(0);
    expect(sentOf("voice_join")).toHaveLength(1); // only the first join
  });

  it("a rejoin carried by the new socket at 59 s clears the 60 s deadline", async () => {
    await inChannel("A");
    noteSignallingLost(); // t = 0
    await vi.advanceTimersByTimeAsync(58_000);
    void rejoinVoiceAfterReconnect(); // t = 58 s, first attempt at 59 s
    await vi.advanceTimersByTimeAsync(1000);
    await settle();
    expect(sentOf("voice_join")).toHaveLength(2);

    await vi.advanceTimersByTimeAsync(5_000); // t = 64 s: past the deadline
    await settle();
    expect(voiceState.state).toBe("reconnecting");
    expect(voiceState.channelId).toBe("A");
    expect(voiceState.lostChannelId).toBeNull();

    publisherConnects();
    await settle();
    expect(voiceState.state).toBe("connected");
  });
});
