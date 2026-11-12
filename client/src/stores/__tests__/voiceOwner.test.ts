/**
 * The voice owner on its own, with a recording adapter: order of the
 * adapter calls for sequences of intent changes.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { VoiceAdapter } from "@/lib/webrtc/types";
import { HOOK_LIMIT_MS, VoiceOwner } from "../voiceOwner";

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
  setEventHandlers: () => {},
} as unknown as VoiceAdapter;

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
});
