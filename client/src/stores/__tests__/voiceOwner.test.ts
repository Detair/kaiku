/**
 * The voice owner on its own, with a recording adapter: order of the
 * adapter calls for sequences of intent changes.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { VoiceAdapter } from "@/lib/webrtc/types";
import { HOOK_LIMIT_MS, PROBE_LIMIT_MS, TEARDOWN_LIMIT_MS, VoiceOwner } from "../voiceOwner";

type Session = { token: number; channelId: string };

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

/** What the adapter was asked to do, in order: "join:A", "leave:A", ... */
let log: string[];
/** Gates by call ("join:A"): that call returns only when released. */
let gates: Map<string, Deferred>;
let owner: VoiceOwner;

function park(call: string): Deferred {
  const gate = deferred();
  gates.set(call, gate);
  return gate;
}

async function adapterCall(name: string, session: Session): Promise<void> {
  const call = `${name}:${session.channelId}`;
  log.push(call);
  await gates.get(call)?.promise;
}

const adapter = {
  join: async (session: Session) => {
    await adapterCall("join", session);
    return { ok: true, value: session };
  },
  leave: async (session: Session) => {
    await adapterCall("leave", session);
    return { ok: true, value: undefined };
  },
  teardown: async (session: Session) => {
    await adapterCall("teardown", session);
    return { ok: true, value: undefined };
  },
  probePublisher: async () => publisherAlive,
  setEventHandlers: (handlers: typeof mediaEvents) => {
    mediaEvents = { ...mediaEvents, ...handlers };
  },
} as unknown as VoiceAdapter;

/** The handlers the owner installed on the adapter. */
let mediaEvents: {
  onStateChange?: (state: string, session: Session | null) => void;
  onConnectionLost?: (session: Session | null) => void;
} = {};

/** A monotonic clock the tests move by hand (it does not follow the wall clock). */
let mono = 0;
/** Whether the server answers a ping with a pong (the socket is in fact alive). */
let socketAnswers = true;
/** What the publisher connection object itself says. */
let publisherAlive = true;

/** Time passes normally: timers run, both clocks move. */
async function pass(ms: number): Promise<void> {
  // In steps, so the monotonic clock never runs far ahead of the timers.
  for (let left = ms; left > 0; left -= 1000) {
    const step = Math.min(1000, left);
    mono += step;
    await vi.advanceTimersByTimeAsync(step);
  }
}

async function settle(): Promise<void> {
  await vi.advanceTimersByTimeAsync(0);
}

describe("voice owner", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    log = [];
    gates = new Map();
    owner = new VoiceOwner();
    owner.useAdapter(async () => adapter);
    mono = 0;
    owner.useClock(() => ({ wall: Date.now(), mono }));
    socketAnswers = true;
    publisherAlive = true;
    owner.usePing(async () => {
      if (socketAnswers) owner.noteInbound();
    });
    vi.spyOn(console, "warn").mockImplementation(() => {});
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  it("ends the held session completely before it joins, and joins only the latest wish", async () => {
    const a = owner.wish("channel", "A", {});
    expect(await owner.request(a)).toEqual({ type: "established" });

    const leaveOfA = park("leave:A");
    const b = owner.wish("channel", "B", {});
    const c = owner.wish("channel", "C", {});
    const toB = owner.request(b);
    await settle();
    const toC = owner.request(c);
    await settle();
    expect(log).toEqual(["join:A", "leave:A"]); // nothing is joined while A is being left

    leaveOfA.resolve();
    expect(await toB).toEqual({ type: "superseded" });
    expect(await toC).toEqual({ type: "established" });
    expect(log).toEqual(["join:A", "leave:A", "join:C"]);
    expect(owner.sessionOf(c)).toEqual({ token: c.token, channelId: "C" });
  });

  it("a join that never returns is ended by its handle and does not hold up the next wish", async () => {
    park("join:A"); // never released: a permission prompt nobody answers
    const a = owner.wish("channel", "A", {});
    const toA = owner.request(a);
    await settle();

    const b = owner.wish("channel", "B", {});
    expect(await owner.request(b)).toEqual({ type: "established" });
    expect(await toA).toEqual({ type: "superseded" });
    expect(log).toEqual(["join:A", "leave:A", "join:B"]);
  });

  it("dropping names a wish: an older wish's drop never ends a newer session", async () => {
    const a = owner.wish("channel", "A", {});
    await owner.request(a);
    const b = owner.wish("dm-call", "B", {});
    await owner.request(b);
    await owner.drop(a); // e.g. a late leave of the channel
    await settle();
    expect(owner.wants(b)).toBe(true);
    expect(log).toEqual(["join:A", "leave:A", "join:B"]);
    await owner.drop(b, { silent: true });
    expect(log).toEqual(["join:A", "leave:A", "join:B", "teardown:B"]);
  });

  it("a session the server removed is ended without voice_leave, even while being replaced", async () => {
    const a = owner.wish("channel", "A", {});
    await owner.request(a);
    const b = owner.wish("channel", "B", {});
    const toB = owner.request(b); // A is about to be left
    owner.serverRemoved("A"); // ...and the server says it already removed us
    expect(await toB).toEqual({ type: "established" });
    expect(log).toEqual(["join:A", "teardown:A", "join:B"]);
  });

  it("a set-up step that hangs delays a leave by the hook limit at most", async () => {
    const hooks = { established: () => new Promise<void>(() => {}) };
    const a = owner.wish("channel", "A", hooks);
    void owner.request(a);
    await settle();
    expect(log).toEqual(["join:A"]);

    let left = false;
    void owner.drop(a).then(() => {
      left = true;
    });
    await vi.advanceTimersByTimeAsync(HOOK_LIMIT_MS - 1);
    expect(left).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    await settle();
    expect(log).toEqual(["join:A", "leave:A"]);
    expect(left).toBe(true);
  });

  it("the server's refusal of a call (ready rejects) fails the wish without a join", async () => {
    const failed = vi.fn();
    const call = owner.wish("dm-call", "D", { ready: Promise.reject(new Error("refused")), failed });
    const outcome = await owner.request(call);
    expect(outcome.type).toBe("failed");
    expect(failed).toHaveBeenCalled();
    expect(log).toEqual([]);
    expect(owner.current).toBeNull();
  });

  describe("the signed-in session ends", () => {
    it("ends the held session at once, by its handle, and nothing comes back", async () => {
      const gaveUp = vi.fn();
      const a = owner.wish("channel", "A", { gaveUp });
      await owner.request(a);

      owner.shutdown();
      // Synchronously: the adapter was told before anything was awaited.
      expect(log).toEqual(["join:A", "leave:A"]);
      expect(owner.current).toBeNull();

      // The next account's socket comes up within the minute.
      owner.socketLost();
      owner.socketReady();
      await vi.advanceTimersByTimeAsync(90_000);
      expect(log).toEqual(["join:A", "leave:A"]);
      expect(gaveUp).not.toHaveBeenCalled(); // no "Rejoin" offer for the next account
    });

    it("during a recovery attempt: the attempt's session is ended at once", async () => {
      const a = owner.wish("channel", "A", {});
      await owner.request(a);
      owner.socketReady(); // re-established socket: recovery
      park("join:A"); // the attempt's join hangs
      await vi.advanceTimersByTimeAsync(1000);
      expect(log).toEqual(["join:A", "teardown:A", "join:A"]);

      owner.shutdown();
      expect(log).toEqual(["join:A", "teardown:A", "join:A", "leave:A"]);
      await vi.advanceTimersByTimeAsync(90_000);
      expect(log).toHaveLength(4);
      expect(owner.isRecovering()).toBe(false);
    });

    it("during a join that waits for the microphone: ended at once, the late grant joins nothing", async () => {
      const prompt = park("join:A");
      const a = owner.wish("channel", "A", {});
      const joining = owner.request(a);
      await settle();
      owner.shutdown();
      expect(log).toEqual(["join:A", "leave:A"]);
      prompt.resolve();
      expect(await joining).toEqual({ type: "superseded" });
      await vi.advanceTimersByTimeAsync(90_000);
      expect(log).toEqual(["join:A", "leave:A"]);
    });

    it("a session of another user is never recovered or joined (second line of defence)", async () => {
      let user: string | null = "u1";
      owner.useIdentity(() => user);
      const gaveUp = vi.fn();
      const a = owner.wish("channel", "A", { gaveUp });
      await owner.request(a);
      user = "u2"; // the account changed and nobody told the owner
      owner.socketLost();
      owner.socketReady();
      await vi.advanceTimersByTimeAsync(90_000);
      expect(log.filter((call) => call === "join:A")).toHaveLength(1);
      expect(log[log.length - 1]).toMatch(/^(leave|teardown):A$/);
      expect(owner.current).toBeNull();
      expect(gaveUp).not.toHaveBeenCalled();
    });
  });

  describe("real elapsed time (the device slept)", () => {
    /** Time passes without any timer running, as while suspended. */
    function sleep(ms: number): void {
      vi.setSystemTime(Date.now() + ms);
    }

    it("resume after 5 minutes: gives up instead of rejoining, media released at once", async () => {
      const gaveUp = vi.fn();
      const a = owner.wish("channel", "A", { gaveUp });
      await owner.request(a);
      await vi.advanceTimersByTimeAsync(20_000); // in the call for a while

      sleep(5 * 60_000);
      // On resume the socket turns out to be gone and comes back.
      owner.socketLost();
      expect(gaveUp).toHaveBeenCalledTimes(1);
      expect(log).toEqual(["join:A", "teardown:A"]); // at once, without voice_leave
      owner.socketReady();
      await vi.advanceTimersByTimeAsync(30_000);
      expect(log).toEqual(["join:A", "teardown:A"]);
      expect(owner.current).toBeNull();
    });

    it("resume after 5 minutes when only the socket's return is reported", async () => {
      const gaveUp = vi.fn();
      const a = owner.wish("channel", "A", { gaveUp });
      await owner.request(a);
      await vi.advanceTimersByTimeAsync(20_000);
      sleep(5 * 60_000);
      owner.socketReady();
      await vi.advanceTimersByTimeAsync(30_000);
      expect(gaveUp).toHaveBeenCalledTimes(1);
      expect(log).toEqual(["join:A", "teardown:A"]);
    });

    it("resume after 5 minutes, publisher not connected, no socket event: the wake-up check gives up", async () => {
      const gaveUp = vi.fn();
      const a = owner.wish("channel", "A", { gaveUp });
      await owner.request(a);
      await vi.advanceTimersByTimeAsync(20_000);
      sleep(5 * 60_000);
      owner.wake(); // visibilitychange / online / the next heartbeat
      await settle();
      expect(gaveUp).toHaveBeenCalledTimes(1);
      expect(log).toEqual(["join:A", "teardown:A"]);
    });

    it("publisher not connected: the next heartbeat after a long gap gives up by itself", async () => {
      const gaveUp = vi.fn();
      const a = owner.wish("channel", "A", { gaveUp });
      await owner.request(a);
      await vi.advanceTimersByTimeAsync(20_000);
      sleep(5 * 60_000);
      await vi.advanceTimersByTimeAsync(10_000); // timers run again after resume
      expect(gaveUp).toHaveBeenCalledTimes(1);
      expect(log).toEqual(["join:A", "teardown:A"]);
    });

    it("resume after 5 minutes when the first thing noticed is the media failing", async () => {
      const gaveUp = vi.fn();
      const a = owner.wish("channel", "A", { gaveUp });
      await owner.request(a);
      await vi.advanceTimersByTimeAsync(20_000);
      sleep(5 * 60_000);
      mediaEvents.onConnectionLost!({ token: a.token, channelId: "A" });
      expect(gaveUp).toHaveBeenCalledTimes(1);
      await vi.advanceTimersByTimeAsync(30_000);
      expect(log).toEqual(["join:A", "teardown:A"]);
    });

    it("resume after 20 seconds: rejoins", async () => {
      const gaveUp = vi.fn();
      const a = owner.wish("channel", "A", { gaveUp });
      await owner.request(a);
      await vi.advanceTimersByTimeAsync(20_000);
      sleep(20_000);
      owner.socketLost();
      owner.socketReady();
      await vi.advanceTimersByTimeAsync(1000);
      expect(log).toEqual(["join:A", "teardown:A", "join:A"]);
      expect(gaveUp).not.toHaveBeenCalled();
    });

    it("slept through most of the minute while reconnecting: the deadline counts the sleep", async () => {
      const gaveUp = vi.fn();
      const a = owner.wish("channel", "A", { gaveUp });
      await owner.request(a);
      owner.socketLost(); // t = 0, awake
      await vi.advanceTimersByTimeAsync(5_000);
      sleep(50_000); // t = 55 s, no timer ran
      expect(gaveUp).not.toHaveBeenCalled();
      await vi.advanceTimersByTimeAsync(5_000); // t = 60 s of real time
      expect(gaveUp).toHaveBeenCalledTimes(1);
    });
  });

  describe("after a gap the session is verified, not assumed", () => {
    /** Join A and have its publisher connected: socket and media carry it. */
    async function healthy(hooks: Record<string, () => void> = {}) {
      const a = owner.wish("channel", "A", hooks);
      await owner.request(a);
      mediaEvents.onStateChange!("connected", { token: a.token, channelId: "A" });
      await pass(20_000);
      return a;
    }
    /** The device is suspended: the wall clock moves on, nothing runs. */
    const suspend = (ms: number) => vi.setSystemTime(Date.now() + ms);
    const HOURS = 3_600_000;

    it("suspended 8 h, connections dead, the failure is reported 3 s after wake-up: give-up, no rejoin", async () => {
      const gaveUp = vi.fn();
      const verifying = vi.fn();
      await healthy({ gaveUp, verifying });
      suspend(8 * HOURS);
      socketAnswers = false;
      publisherAlive = false;
      owner.wake();
      expect(verifying).toHaveBeenCalledTimes(1); // shown as reconnecting, not as fine
      await pass(3_000);
      owner.socketLost();
      expect(gaveUp).toHaveBeenCalledTimes(1);
      owner.socketReady();
      await pass(60_000);
      expect(log).toEqual(["join:A", "teardown:A"]);
    });

    it("suspended 8 h, connections dead, no failure is ever reported: give-up within the probe window", async () => {
      const gaveUp = vi.fn();
      await healthy({ gaveUp });
      suspend(8 * HOURS);
      socketAnswers = false; // the remembered state still says "fine"
      owner.wake();
      await pass(PROBE_LIMIT_MS - 1);
      expect(gaveUp).not.toHaveBeenCalled();
      await pass(1);
      expect(gaveUp).toHaveBeenCalledTimes(1);
      await pass(120_000);
      expect(log).toEqual(["join:A", "teardown:A"]);
    });

    it("suspended 8 h, the socket answers but the publisher connection is dead: give-up", async () => {
      const gaveUp = vi.fn();
      await healthy({ gaveUp });
      suspend(8 * HOURS);
      publisherAlive = false; // read from the connection itself
      owner.wake();
      await settle();
      expect(gaveUp).toHaveBeenCalledTimes(1);
      expect(log).toEqual(["join:A", "teardown:A"]);
    });

    it("suspended 8 h but the connection survived (fresh pong, live publisher): the call goes on", async () => {
      const gaveUp = vi.fn();
      const verifying = vi.fn();
      const verified = vi.fn();
      const a = await healthy({ gaveUp, verifying, verified });
      suspend(8 * HOURS);
      owner.wake();
      await settle();
      expect(verifying).toHaveBeenCalledTimes(1);
      expect(verified).toHaveBeenCalledTimes(1);
      expect(gaveUp).not.toHaveBeenCalled();
      expect(owner.wants(a)).toBe(true);
      // The proof is new: a loss now is an ordinary outage with a full minute.
      await pass(10_000);
      owner.socketLost();
      await pass(30_000);
      expect(gaveUp).not.toHaveBeenCalled();
      owner.socketReady();
      await pass(1000);
      expect(log).toEqual(["join:A", "teardown:A", "join:A"]);
    });

    it("suspended 20 s, connections dead: recovered with the time that is left", async () => {
      const gaveUp = vi.fn();
      await healthy({ gaveUp });
      suspend(20_000);
      socketAnswers = false;
      owner.wake();
      await pass(PROBE_LIMIT_MS); // not verified: not carried since before the sleep
      expect(owner.isRecovering()).toBe(true);
      expect(gaveUp).not.toHaveBeenCalled();
      owner.socketLost();
      // 60 s after the last proof, i.e. 35 s from here, it would be over.
      await pass(34_000);
      expect(gaveUp).not.toHaveBeenCalled();
      owner.socketReady();
      await pass(500);
      expect(gaveUp).not.toHaveBeenCalled();
      await pass(500);
      expect(gaveUp).toHaveBeenCalledTimes(1); // the minute ran out during the pause
    });

    it("suspended 20 s, dead, the socket returns at once: rejoined", async () => {
      const gaveUp = vi.fn();
      await healthy({ gaveUp });
      suspend(20_000);
      owner.socketLost();
      owner.socketReady();
      await pass(1000);
      expect(log).toEqual(["join:A", "teardown:A", "join:A"]);
      expect(gaveUp).not.toHaveBeenCalled();
    });

    it("timers held back to one per minute, connection alive: goes on, never gives up", async () => {
      const gaveUp = vi.fn();
      const verified = vi.fn();
      const a = await healthy({ gaveUp, verified });
      for (let minute = 0; minute < 10; minute++) {
        mono += 60_000;
        vi.setSystemTime(Date.now() + 60_000);
        owner.wake(); // the only thing that runs in that minute
        await settle();
      }
      expect(verified).toHaveBeenCalledTimes(10);
      expect(gaveUp).not.toHaveBeenCalled();
      expect(owner.wants(a)).toBe(true);
      expect(log).toEqual(["join:A"]);
    });

    it("the wall clock jumps forward 10 minutes during a healthy call: verified, stays up", async () => {
      const gaveUp = vi.fn();
      const a = await healthy({ gaveUp });
      vi.setSystemTime(Date.now() + 10 * 60_000);
      await pass(5 * 60_000);
      expect(gaveUp).not.toHaveBeenCalled();
      expect(log).toEqual(["join:A"]);
      expect(owner.wants(a)).toBe(true);
      expect(owner.isRecovering()).toBe(false);
    });

    it("not carried and the wall clock is set back an hour: still gives up after 60 s", async () => {
      const gaveUp = vi.fn();
      await healthy({ gaveUp });
      owner.socketLost();
      vi.setSystemTime(Date.now() - 3_600_000);
      await pass(59_999);
      expect(gaveUp).not.toHaveBeenCalled();
      await pass(1);
      expect(gaveUp).toHaveBeenCalledTimes(1);
    });

    it("a pong that arrived before the probe does not count; only a fresh frame does", async () => {
      const gaveUp = vi.fn();
      await healthy({ gaveUp });
      owner.noteInbound(); // old traffic, before the sleep
      suspend(8 * HOURS);
      socketAnswers = false;
      owner.wake();
      await pass(PROBE_LIMIT_MS);
      expect(gaveUp).toHaveBeenCalledTimes(1);
    });
  });

  describe("a teardown that does not return", () => {
    it("the owner goes on after 5 s and joins the next wish", async () => {
      park("leave:A"); // native leave hangs for good
      const a = owner.wish("channel", "A", {});
      await owner.request(a);
      const b = owner.wish("channel", "B", {});
      const toB = owner.request(b);
      await vi.advanceTimersByTimeAsync(TEARDOWN_LIMIT_MS - 1);
      expect(log).toEqual(["join:A", "leave:A"]);
      await vi.advanceTimersByTimeAsync(1);
      expect(await toB).toEqual({ type: "established" });
      expect(log).toEqual(["join:A", "leave:A", "join:B"]);
    });

    it("inside a recovery: the attempt goes on after 5 s", async () => {
      const a = owner.wish("channel", "A", {});
      await owner.request(a);
      park("teardown:A"); // dropping the old connection hangs
      owner.socketReady();
      await vi.advanceTimersByTimeAsync(1000 + TEARDOWN_LIMIT_MS);
      expect(log).toEqual(["join:A", "teardown:A", "join:A"]);
    });

    it("a leave resolves for its caller although the teardown never returned", async () => {
      park("leave:A");
      const a = owner.wish("channel", "A", {});
      await owner.request(a);
      let left = false;
      void owner.drop(a).then(() => {
        left = true;
      });
      await vi.advanceTimersByTimeAsync(TEARDOWN_LIMIT_MS);
      await settle();
      expect(left).toBe(true);
    });
  });
});
