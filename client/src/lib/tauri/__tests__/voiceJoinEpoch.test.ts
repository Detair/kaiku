/**
 * Codex W1b-2: a DM-call join runs under the session epoch captured when
 * the call was started or accepted (synchronously), not one read after the
 * HTTP signalling; a leave in between means native join is never invoked.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke, isTauri: () => true }));
vi.mock("../common", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../common")>()),
  isTauri: () => true,
}));

import { joinVoice, leaveVoice } from "../voice";
import { bumpVoiceEpoch, currentVoiceEpoch } from "@/lib/webrtc/voiceSession";

beforeEach(() => {
  let last = 0;
  invoke.mockReset().mockImplementation(async (cmd: string) =>
    cmd === "begin_voice_session" ? ++last : undefined,
  );
});

const joins = () => invoke.mock.calls.filter(([cmd]) => cmd === "join_voice");

describe("DM-call join epoch", () => {
  it("joins under a native epoch when nothing happened since the call started", async () => {
    const token = currentVoiceEpoch(); // returned by startCall / joinCall
    await joinVoice("dm-1", token);
    expect(joins()).toEqual([["join_voice", { channelId: "dm-1", epoch: 1 }]]);
  });

  it("a hang-up between start and the native join: join_voice is never invoked", async () => {
    const token = currentVoiceEpoch();
    // ... HTTP start/accept is in flight; the user hangs up meanwhile.
    await leaveVoice();
    await joinVoice("dm-1", token);
    expect(joins()).toEqual([]);
  });

  it("a leave while the wrapper is loading the native API is honoured", async () => {
    const join = joinVoice("dm-1", currentVoiceEpoch());
    bumpVoiceEpoch(); // what leaveVoice / voice_removed do first, synchronously
    await join;
    expect(joins()).toEqual([]);
  });

  it("a leave while native allocates the epoch: no join_voice", async () => {
    let release!: (n: number) => void;
    invoke.mockImplementation((cmd: string) =>
      cmd === "begin_voice_session"
        ? new Promise<number>((r) => (release = r))
        : Promise.resolve(undefined),
    );
    const join = joinVoice("dm-1", currentVoiceEpoch());
    await vi.waitFor(() => expect(release).toBeTypeOf("function"));
    await leaveVoice();
    release(5);
    await join;
    expect(joins()).toEqual([]);
  });

  it("a join without its session token is a programming error", async () => {
    await expect(joinVoice("dm-1", undefined as unknown as number)).rejects.toThrow(/token/);
    expect(joins()).toEqual([]);
  });
});
