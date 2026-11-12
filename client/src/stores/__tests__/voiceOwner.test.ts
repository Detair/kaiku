/**
 * The voice owner on its own, with a recording adapter: order of the
 * adapter calls for sequences of intent changes.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { VoiceAdapter } from "@/lib/webrtc/types";
import { HOOK_LIMIT_MS, TEARDOWN_LIMIT_MS, VoiceOwner } from "../voiceOwner";

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

/** Time passes normally: timers run, both clocks move. */
async function pass(ms: number): Promise<void> {
  mono += ms;
  await vi.advanceTimersByTimeAsync(ms);
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

  describe("a healthy session and the clock", () => {
    /** Join A and have its publisher connected: socket and media carry it. */
    async function healthy(hooks: { gaveUp?: () => void } = {}) {
      const a = owner.wish("channel", "A", hooks);
      await owner.request(a);
      mediaEvents.onStateChange!("connected", { token: a.token, channelId: "A" });
      await pass(20_000);
      return a;
    }
    const session = (a: { token: number }) => ({ token: a.token, channelId: "A" });

    it("the wall clock jumps forward 10 minutes: the call stays up", async () => {
      const gaveUp = vi.fn();
      const a = await healthy({ gaveUp });
      vi.setSystemTime(Date.now() + 10 * 60_000); // e.g. a time sync; the device is awake
      await pass(5 * 60_000);
      expect(gaveUp).not.toHaveBeenCalled();
      expect(log).toEqual(["join:A"]);
      expect(owner.wants(a)).toBe(true);
      expect(owner.isRecovering()).toBe(false);
    });

    it("timers held back for 5 minutes, then running again: the call stays up", async () => {
      const gaveUp = vi.fn();
      const a = await healthy({ gaveUp });
      // Real time passes on both clocks, but no timer runs (a throttled tab).
      mono += 5 * 60_000;
      vi.setSystemTime(Date.now() + 5 * 60_000);
      owner.wake();
      await pass(5 * 60_000);
      expect(gaveUp).not.toHaveBeenCalled();
      expect(log).toEqual(["join:A"]);
      expect(owner.wants(a)).toBe(true);
    });

    it("heartbeats only once a minute for a long time: still up, and a short outage is still recovered", async () => {
      const gaveUp = vi.fn();
      await healthy({ gaveUp });
      for (let minute = 0; minute < 10; minute++) {
        mono += 60_000;
        vi.setSystemTime(Date.now() + 60_000);
        owner.wake(); // the only thing that runs in that minute
      }
      expect(gaveUp).not.toHaveBeenCalled();
      // A real, short outage now is recovered as usual.
      await pass(10_000);
      owner.socketLost();
      owner.socketReady();
      await pass(1000);
      expect(log).toEqual(["join:A", "teardown:A", "join:A"]);
      expect(gaveUp).not.toHaveBeenCalled();
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

    it("after a sleep the stale state looks fine; the loss reported later ends it without a rejoin", async () => {
      const gaveUp = vi.fn();
      await healthy({ gaveUp });
      vi.setSystemTime(Date.now() + 5 * 60_000); // lid closed; nothing reported yet
      owner.wake();
      expect(gaveUp).not.toHaveBeenCalled(); // nothing proves it dead yet
      await pass(20_000); // the dead socket is noticed a little later
      owner.socketLost();
      expect(gaveUp).toHaveBeenCalledTimes(1);
      owner.socketReady();
      await pass(30_000);
      expect(log).toEqual(["join:A", "teardown:A"]);
    });

    it("after a sleep the media failing first ends it without a rejoin", async () => {
      const gaveUp = vi.fn();
      const a = await healthy({ gaveUp });
      vi.setSystemTime(Date.now() + 5 * 60_000);
      await pass(10_000); // heartbeats run again, flags still look fine
      expect(gaveUp).not.toHaveBeenCalled();
      mediaEvents.onStateChange!("reconnecting", session(a)); // ICE notices
      expect(gaveUp).toHaveBeenCalledTimes(1);
      await pass(30_000);
      expect(log).toEqual(["join:A", "teardown:A"]);
    });

    it("a clean minute awake after a long gap proves the session; a later loss is recovered", async () => {
      const gaveUp = vi.fn();
      await healthy({ gaveUp });
      vi.setSystemTime(Date.now() + 10 * 60_000); // the clock was corrected
      await pass(65_000); // more than a minute awake, everything fine
      owner.socketLost();
      expect(gaveUp).not.toHaveBeenCalled();
      owner.socketReady();
      await pass(1000);
      expect(log).toEqual(["join:A", "teardown:A", "join:A"]);
    });

    it("suspended 20 s: the 20 s count, give-up comes 60 s after the last proof", async () => {
      const gaveUp = vi.fn();
      await healthy({ gaveUp });
      vi.setSystemTime(Date.now() + 20_000);
      owner.socketLost(); // noticed on resume; the socket does not come back
      await pass(39_000);
      expect(gaveUp).not.toHaveBeenCalled();
      await pass(2_000); // 61 s after the last heartbeat before the sleep
      expect(gaveUp).toHaveBeenCalledTimes(1);
    });

    it("suspended 20 s and the socket returns: rejoined", async () => {
      const gaveUp = vi.fn();
      await healthy({ gaveUp });
      vi.setSystemTime(Date.now() + 20_000);
      owner.socketLost();
      owner.socketReady();
      await pass(1000);
      expect(log).toEqual(["join:A", "teardown:A", "join:A"]);
      expect(gaveUp).not.toHaveBeenCalled();
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
