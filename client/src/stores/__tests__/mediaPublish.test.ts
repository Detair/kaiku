/**
 * Screen share / webcam start as a request with an answer (C30) and one
 * stop for every way a share ends, sent whenever the server may hold it.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createMediaPublisher,
  isStartRefusal,
  refusalKey,
  MEDIA_START_TIMEOUT_MS,
  type MediaContext,
} from "../mediaPublish";
import type { VoiceAdapter } from "@/lib/webrtc/types";

const ME = "me";
const CH = "ch-1";

function setup(overrides: Partial<MediaContext> = {}) {
  const log: string[] = [];
  const session = { current: { token: 1, channelId: CH } as { token: number; channelId: string } | null };
  /** Voice state of the held session and the socket. */
  const voice = { state: "connected" as "connected" | "reconnecting", socket: true };
  const flags: { screenSharing?: boolean; webcamActive?: boolean } = {};
  const adapter = {
    captureScreen: vi.fn(async ({ streamId }: { streamId: string }) => {
      log.push(`capture:${streamId}`);
      return { ok: true, value: { hasAudio: false, sourceLabel: "Window" } };
    }),
    publishScreenShare: vi.fn(async (id: string) => {
      log.push(`publish:${id}`);
      return { ok: true, value: undefined };
    }),
    stopScreenShare: vi.fn(async (id: string) => {
      log.push(`adapterStop:${id}`);
      return { ok: true, value: undefined };
    }),
    getScreenShareTrack: vi.fn(() => null),
    captureWebcam: vi.fn(async () => {
      log.push("captureWebcam");
      return { ok: true, value: undefined };
    }),
    publishWebcam: vi.fn(async () => {
      log.push("publishWebcam");
      return { ok: true, value: undefined };
    }),
    stopWebcam: vi.fn(async () => {
      log.push("adapterStopWebcam");
      return { ok: true, value: undefined };
    }),
  };
  let n = 0;
  const notify = vi.fn();
  const ctx: MediaContext = {
    adapter: async () => adapter as unknown as VoiceAdapter,
    session: () => session.current,
    connected: () => session.current !== null && voice.state === "connected",
    socketUp: () => voice.socket,
    channelId: () => (session.current ? CH : null),
    myUserId: () => ME,
    myName: () => "Me",
    sendScreenStart: vi.fn(async (_c, id) => {
      log.push(`sendStart:${id}`);
    }),
    sendScreenStop: vi.fn(async (_c, id) => {
      log.push(`sendStop:${id}`);
    }),
    sendWebcamStart: vi.fn(async () => {
      log.push("sendWebcamStart");
    }),
    sendWebcamStop: vi.fn(async () => {
      log.push("sendWebcamStop");
    }),
    setFlags: (f) => Object.assign(flags, f),
    addPreview: vi.fn(),
    removePreview: vi.fn((id) => log.push(`preview-:${id}`)),
    t: (key) => key,
    notify,
    warnStopFailed: vi.fn(),
    newStreamId: () => `s${++n}`,
    ...overrides,
  };
  const pub = createMediaPublisher(ctx);
  return { pub, ctx, adapter, log, flags, session, notify, voice };
}

async function flush() {
  for (let i = 0; i < 20; i++) await Promise.resolve();
}

const started = (stream_id: string) =>
  ({ type: "screen_share_started", channel_id: CH, user_id: ME, stream_id }) as const;

describe("screen share start waits for the server", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("sends the start, adds NO track until screen_share_started for its own stream", async () => {
    const { pub, log, flags } = setup();
    const start = pub.startScreenShare("medium");
    await flush();
    expect(log).toEqual(["capture:s1", "sendStart:s1"]);

    // Someone else's share and another stream id are not our answer.
    pub.handleAnswer({ ...started("other"), user_id: "u2" });
    await flush();
    expect(log).not.toContain("publish:s1");

    pub.handleAnswer(started("s1"));
    expect(await start).toEqual({ ok: true });
    expect(log).toEqual(["capture:s1", "sendStart:s1", "publish:s1"]);
    expect(flags.screenSharing).toBe(true);
  });

  it.each([
    ["SCREEN_SHARE_FORBIDDEN", "voice.screenShare.error.noPermission"],
    ["SCREEN_SHARE_LIMIT", "voice.screenShare.error.limitReached"],
    ["RATE_LIMITED", "voice.media.error.rateLimited"],
    ["DUPLICATE_STREAM_ID", "voice.screenShare.error.refused"],
    ["UNAUTHORIZED", "voice.screenShare.error.refused"],
    ["VOICE_ERROR", "voice.screenShare.error.refused"],
  ])("refusal %s stops the capture and gives %s", async (code, key) => {
    const { pub, log, flags } = setup();
    const start = pub.startScreenShare();
    await flush();
    pub.handleAnswer({ type: "voice_error", code, message: "x" });
    expect(await start).toEqual({ ok: false, error: key });
    expect(log).toEqual(["capture:s1", "sendStart:s1", "adapterStop:s1"]);
    expect(log).not.toContain("publish:s1");
    expect(flags.screenSharing).toBeFalsy();
  });

  it("no answer within 5 s: capture stopped, stop sent (frees a late slot), timeout text", async () => {
    const { pub, log } = setup();
    const start = pub.startScreenShare();
    await flush();
    vi.advanceTimersByTime(MEDIA_START_TIMEOUT_MS - 1);
    await flush();
    expect(log).not.toContain("adapterStop:s1");
    vi.advanceTimersByTime(1);
    expect(await start).toEqual({ ok: false, error: "voice.screenShare.error.noAnswer" });
    expect(log).toEqual(["capture:s1", "sendStart:s1", "adapterStop:s1", "sendStop:s1"]);
  });

  it("a picker open longer than 5 s still starts: the wait begins when the start is sent", async () => {
    const { pub, adapter, log } = setup();
    let pick!: () => void;
    adapter.captureScreen.mockImplementationOnce(
      ({ streamId }: { streamId: string }) =>
        new Promise((resolve) => {
          pick = () => {
            log.push(`capture:${streamId}`);
            resolve({ ok: true, value: { hasAudio: false, sourceLabel: "W" } });
          };
        }) as never,
    );
    const start = pub.startScreenShare();
    await flush();
    vi.advanceTimersByTime(20_000); // the user looks for the right window
    pick();
    await flush();
    expect(log).toEqual(["capture:s1", "sendStart:s1"]);
    pub.handleAnswer(started("s1"));
    expect(await start).toEqual({ ok: true });
  });

  it("a late acceptance after the timeout publishes nothing", async () => {
    const { pub, log } = setup();
    const start = pub.startScreenShare();
    await flush();
    vi.advanceTimersByTime(MEDIA_START_TIMEOUT_MS);
    await start;
    pub.handleAnswer(started("s1"));
    await flush();
    expect(log).not.toContain("publish:s1");
  });

  it("one start in flight: a second start is refused while the first waits", async () => {
    const { pub, adapter } = setup();
    const first = pub.startScreenShare();
    await flush();
    expect(await pub.startWebcam()).toEqual({ ok: false, error: "voice.media.error.busy" });
    expect(await pub.startScreenShare()).toEqual({ ok: false, error: "voice.media.error.busy" });
    expect(adapter.captureScreen).toHaveBeenCalledTimes(1);
    pub.handleAnswer(started("s1"));
    await first;
  });

  it("at most one own share: a second start while sharing is refused", async () => {
    const { pub } = setup();
    const first = pub.startScreenShare();
    await flush();
    pub.handleAnswer(started("s1"));
    await first;
    expect(await pub.startScreenShare()).toEqual({
      ok: false,
      error: "voice.screenShare.error.alreadySharing",
    });
  });

  it("a cancelled picker gives no text and sends nothing", async () => {
    const { pub, adapter, ctx } = setup();
    adapter.captureScreen.mockResolvedValueOnce({
      ok: false,
      error: { type: "cancelled", message: "Screen share cancelled" },
    } as never);
    expect(await pub.startScreenShare()).toEqual({ ok: false });
    expect(ctx.sendScreenStart).not.toHaveBeenCalled();
    expect(pub.isStarting()).toBe(false);
  });

  it("a cancelled Linux portal gives a calm note, a missing portal an error", async () => {
    const { pub, adapter } = setup();
    adapter.captureScreen.mockResolvedValueOnce({
      ok: false,
      error: { type: "cancelled", message: "portal" },
    } as never);
    expect(await pub.startScreenShare()).toEqual({
      ok: false,
      note: "voice.screenShare.portalCancelled",
    });
    adapter.captureScreen.mockResolvedValueOnce({
      ok: false,
      error: { type: "not_found", message: "portal_unavailable" },
    } as never);
    expect(await pub.startScreenShare()).toEqual({
      ok: false,
      error: "voice.screenShare.error.portalMissing",
    });
  });

  it("permission blocked shows the screen text, not the microphone text", async () => {
    const { pub, adapter } = setup();
    adapter.captureScreen.mockResolvedValueOnce({
      ok: false,
      error: { type: "permission_denied", message: "x" },
    } as never);
    expect(await pub.startScreenShare()).toEqual({
      ok: false,
      error: "voice.screenShare.error.blocked",
    });
  });

  it("capture ended while waiting: capture released, stop sent, later acceptance ignored", async () => {
    const { pub, log } = setup();
    const start = pub.startScreenShare();
    await flush();
    await pub.captureEnded("screen", "s1");
    expect(await start).toEqual({ ok: false });
    expect(log).toEqual(["capture:s1", "sendStart:s1", "adapterStop:s1", "sendStop:s1"]);
    pub.handleAnswer(started("s1"));
    await flush();
    expect(log).not.toContain("publish:s1");
  });

  it("call ended while waiting: nothing is published", async () => {
    const { pub, log, session } = setup();
    const start = pub.startScreenShare();
    await flush();
    session.current = null;
    pub.handleAnswer(started("s1"));
    expect(await start).toEqual({ ok: false, error: "voice.error.notConnected" });
    expect(log).not.toContain("publish:s1");
    expect(log).toContain("adapterStop:s1");
  });
});

describe("one stop for every way a share ends", () => {
  async function sharing() {
    const s = setup();
    const start = s.pub.startScreenShare();
    await flush();
    s.pub.handleAnswer(started("s1"));
    await start;
    s.log.length = 0;
    return s;
  }

  it("the browser's Stop sharing bar (capture ended) runs the button's stop", async () => {
    const viaBar = await sharing();
    await viaBar.pub.captureEnded("screen", "s1");
    const viaButton = await sharing();
    await viaButton.pub.stopScreenShare("s1");

    expect(viaBar.log).toEqual(["adapterStop:s1", "preview-:s1", "sendStop:s1"]);
    expect(viaButton.log).toEqual(viaBar.log);
    expect(viaBar.flags.screenSharing).toBe(false);
  });

  it("share, stop from the bar, share again — six times", async () => {
    const { pub, log, flags } = setup();
    for (let i = 1; i <= 6; i++) {
      const start = pub.startScreenShare();
      await flush();
      pub.handleAnswer(started(`s${i}`));
      expect(await start).toEqual({ ok: true });
      await pub.captureEnded("screen", `s${i}`);
      expect(flags.screenSharing).toBe(false);
    }
    expect(log.filter((l) => l.startsWith("sendStop:"))).toHaveLength(6);
    expect(log.filter((l) => l.startsWith("publish:"))).toHaveLength(6);
  });

  it("VIDEO_SOURCE_UNANNOUNCED stops the newest own video and says so", async () => {
    const { pub, log, notify } = await sharing();
    pub.handleAnswer({ type: "voice_error", code: "VIDEO_SOURCE_UNANNOUNCED", message: "x" });
    await flush();
    expect(log).toEqual(["adapterStop:s1", "preview-:s1", "sendStop:s1"]);
    expect(notify).toHaveBeenCalledWith("voice.media.error.unannounced");
  });

  it("VIDEO_SOURCE_UNANNOUNCED is never taken as the refusal of a start", async () => {
    vi.useFakeTimers();
    const { pub, log } = setup();
    const start = pub.startScreenShare();
    await flush();
    pub.handleAnswer({ type: "voice_error", code: "VIDEO_SOURCE_UNANNOUNCED", message: "x" });
    await flush();
    pub.handleAnswer(started("s1"));
    expect(await start).toEqual({ ok: true });
    expect(log).toContain("publish:s1");
    vi.useRealTimers();
  });
});

describe("webcam start waits for webcam_started", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("adds the camera only after webcam_started for the own user", async () => {
    const { pub, log, flags } = setup();
    const start = pub.startWebcam("medium");
    await flush();
    expect(log).toEqual(["captureWebcam", "sendWebcamStart"]);
    pub.handleAnswer({ type: "webcam_started", channel_id: CH, user_id: "u2" });
    await flush();
    expect(log).not.toContain("publishWebcam");
    pub.handleAnswer({ type: "webcam_started", channel_id: CH, user_id: ME });
    expect(await start).toEqual({ ok: true });
    expect(log).toEqual(["captureWebcam", "sendWebcamStart", "publishWebcam"]);
    expect(flags.webcamActive).toBe(true);
  });

  it("refusal and timeout stop the camera", async () => {
    const { pub, log } = setup();
    const refused = pub.startWebcam();
    await flush();
    pub.handleAnswer({ type: "voice_error", code: "SIGNALING_ERROR", message: "x" });
    expect(await refused).toEqual({ ok: false, error: "voice.webcam.error.refused" });
    expect(log).toContain("adapterStopWebcam");

    const silent = pub.startWebcam();
    await flush();
    vi.advanceTimersByTime(MEDIA_START_TIMEOUT_MS);
    expect(await silent).toEqual({ ok: false, error: "voice.webcam.error.noAnswer" });
    expect(log.filter((l) => l === "sendWebcamStop")).toHaveLength(1);
  });

  it("a camera that ends by itself runs the normal stop", async () => {
    const { pub, log } = setup();
    const start = pub.startWebcam();
    await flush();
    pub.handleAnswer({ type: "webcam_started", channel_id: CH, user_id: ME });
    await start;
    await pub.captureEnded("webcam", null);
    expect(log.slice(-2)).toEqual(["adapterStopWebcam", "sendWebcamStop"]);
  });
});

describe("old servers (rc.5) and correlation", () => {
  it("rc.5 echoes screen_share_started right away: the start works unchanged", async () => {
    const { pub, ctx, log } = setup();
    (ctx.sendScreenStart as ReturnType<typeof vi.fn>).mockImplementation(async (_c, id) => {
      log.push(`sendStart:${id}`);
      // rc.5 broadcasts synchronously in its start handler.
      queueMicrotask(() => pub.handleAnswer(started(id)));
    });
    expect(await pub.startScreenShare()).toEqual({ ok: true });
  });

  it("rc.5 refusal is VOICE_ERROR with a message: mapped where recognisable", () => {
    expect(refusalKey("screen", "VOICE_ERROR", "Signaling error: Screen share limit reached")).toBe(
      "voice.screenShare.error.limitReached",
    );
    expect(refusalKey("screen", "VOICE_ERROR", "Not authorized to join this voice channel")).toBe(
      "voice.screenShare.error.noPermission",
    );
    expect(refusalKey("screen", "VOICE_ERROR", "Room not found")).toBe(
      "voice.screenShare.error.refused",
    );
  });

  it("an unknown code is not the answer to a start (it times out instead)", async () => {
    vi.useFakeTimers();
    const { pub } = setup();
    const start = pub.startScreenShare();
    await flush();
    pub.handleAnswer({ type: "voice_error", code: "SOMETHING_NEW", message: "x" });
    vi.advanceTimersByTime(MEDIA_START_TIMEOUT_MS);
    expect(await start).toEqual({ ok: false, error: "voice.screenShare.error.noAnswer" });
    vi.useRealTimers();
  });

  it("errors of other requests are not the answer to a start", () => {
    expect(isStartRefusal("CHANNEL_FULL", "")).toBe(false);
    expect(isStartRefusal("RATE_LIMITED", "Rate limited: voice_stats")).toBe(false);
    expect(isStartRefusal("VOICE_ERROR", "Rate limited: voice_stats")).toBe(false);
    expect(isStartRefusal("RATE_LIMITED", "Rate limited: media_start")).toBe(true);
    expect(isStartRefusal("VOICE_ERROR", "Screen share limit reached")).toBe(true);
  });
});

describe("a stop is never withheld (same session, any voice state)", () => {
  async function sharing() {
    const s = setup();
    const start = s.pub.startScreenShare();
    await flush();
    s.pub.handleAnswer(started("s1"));
    await start;
    s.log.length = 0;
    return s;
  }

  it("stop while reconnecting (socket up) still tells the server", async () => {
    const s = await sharing();
    s.voice.state = "reconnecting";
    await s.pub.stopScreenShare("s1");
    expect(s.log).toContain("sendStop:s1");
  });

  it("the browser bar ending the capture while reconnecting tells the server", async () => {
    const s = await sharing();
    s.voice.state = "reconnecting";
    await s.pub.captureEnded("screen", "s1");
    expect(s.log).toContain("sendStop:s1");
  });

  it("an acceptance that arrives while reconnecting is taken back", async () => {
    const s = setup();
    const start = s.pub.startScreenShare();
    await flush();
    s.voice.state = "reconnecting";
    s.pub.handleAnswer(started("s1"));
    await start;
    expect(s.log).toContain("sendStop:s1");
    expect(s.log).not.toContain("publish:s1");
  });

  it("without a socket nothing is sent (the server drops the peer's shares on socket close)", async () => {
    const s = await sharing();
    s.voice.socket = false;
    await s.pub.stopScreenShare("s1");
    expect(s.log).not.toContain("sendStop:s1");
    expect(s.ctx.warnStopFailed).not.toHaveBeenCalled();
  });

  it("webcam stop while reconnecting tells the server", async () => {
    const s = setup();
    const start = s.pub.startWebcam();
    await flush();
    s.pub.handleAnswer({ type: "webcam_started", channel_id: CH, user_id: ME });
    await start;
    s.voice.state = "reconnecting";
    await s.pub.stopWebcam();
    expect(s.log).toContain("sendWebcamStop");
  });

  it("a late acceptance after a timeout is answered with a stop", async () => {
    vi.useFakeTimers();
    const s = setup();
    const start = s.pub.startScreenShare();
    await flush();
    vi.advanceTimersByTime(MEDIA_START_TIMEOUT_MS);
    await start;
    s.log.length = 0;
    s.pub.handleAnswer(started("s1"));
    await flush();
    expect(s.log).toEqual(["sendStop:s1"]);
    vi.useRealTimers();
  });
});

describe("only a sent start can be answered", () => {
  function pickerOpen(s: ReturnType<typeof setup>) {
    let pick!: () => void;
    s.adapter.captureScreen.mockImplementationOnce(
      ({ streamId }: { streamId: string }) =>
        new Promise((resolve) => {
          pick = () => {
            s.log.push(`capture:${streamId}`);
            resolve({ ok: true, value: { hasAudio: false, sourceLabel: "W" } });
          };
        }) as never,
    );
    return () => pick();
  }

  it("a voice_error while the picker is open does not settle the start", async () => {
    const s = setup();
    const pick = pickerOpen(s);
    const start = s.pub.startScreenShare();
    await flush();
    s.pub.handleAnswer({ type: "voice_error", code: "SIGNALING_ERROR", message: "offer" });
    s.pub.handleAnswer({ type: "voice_error", code: "VOICE_ERROR", message: "Room not found" });
    pick();
    await flush();
    expect(s.log).toEqual(["capture:s1", "sendStart:s1"]);
    s.pub.handleAnswer(started("s1"));
    expect(await start).toEqual({ ok: true });
  });

  it("codes the contract does not name for a start are not a refusal", async () => {
    vi.useFakeTimers();
    const s = setup();
    const start = s.pub.startScreenShare();
    await flush();
    for (const code of ["WEBRTC_ERROR", "INTERNAL_ERROR", "AUDIO_SOURCE_UNANNOUNCED", "CHANNEL_FULL"]) {
      s.pub.handleAnswer({ type: "voice_error", code, message: "x" });
    }
    await flush();
    s.pub.handleAnswer(started("s1"));
    expect(await start).toEqual({ ok: true });
    vi.useRealTimers();
  });

  it("AUDIO_SOURCE_UNANNOUNCED stops the share (a dropped track), not the camera", async () => {
    const s = setup();
    const cam = s.pub.startWebcam();
    await flush();
    s.pub.handleAnswer({ type: "webcam_started", channel_id: CH, user_id: ME });
    await cam;
    const share = s.pub.startScreenShare();
    await flush();
    s.pub.handleAnswer(started("s1"));
    await share;
    s.log.length = 0;
    s.pub.handleAnswer({ type: "voice_error", code: "AUDIO_SOURCE_UNANNOUNCED", message: "x" });
    await flush();
    expect(s.log).toContain("sendStop:s1");
    expect(s.log).not.toContain("sendWebcamStop");
  });
});

describe("starts end with their session and with the dialog", () => {
  it("leave during the wait: the next session can start at once (no 'busy')", async () => {
    const s = setup();
    const first = s.pub.startScreenShare();
    await flush();
    s.session.current = { token: 2, channelId: CH }; // rejoin / new call
    const second = s.pub.startScreenShare();
    await flush();
    expect(await first).toEqual({ ok: false });
    s.pub.handleAnswer(started("s2"));
    expect(await second).toEqual({ ok: true });
  });

  it("closing the dialog during the wait aborts the start; a later acceptance is taken back", async () => {
    const s = setup();
    const start = s.pub.startScreenShare();
    await flush();
    s.pub.cancelStart();
    expect(await start).toEqual({ ok: false });
    expect(s.log).toContain("adapterStop:s1");
    s.log.length = 0;
    s.pub.handleAnswer(started("s1"));
    await flush();
    expect(s.log).not.toContain("publish:s1");
  });

  it("closing the dialog while the picker is open releases the capture when it arrives", async () => {
    const s = setup();
    let pick!: () => void;
    s.adapter.captureScreen.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          pick = () => resolve({ ok: true, value: { hasAudio: false, sourceLabel: "W" } });
        }) as never,
    );
    const start = s.pub.startScreenShare();
    await flush();
    s.pub.cancelStart();
    pick();
    await start;
    expect(s.ctx.sendScreenStart).not.toHaveBeenCalled();
    expect(s.log).toContain("adapterStop:s1");
  });

  it("a publish that fails after the acceptance stops the capture and tells the server", async () => {
    const s = setup();
    s.adapter.publishScreenShare.mockResolvedValueOnce({
      ok: false,
      error: { type: "cancelled", message: "Capture ended" },
    } as never);
    const start = s.pub.startScreenShare();
    await flush();
    s.pub.handleAnswer(started("s1"));
    expect(await start).toEqual({ ok: false });
    expect(s.log).toEqual(expect.arrayContaining(["adapterStop:s1", "sendStop:s1"]));
  });

  it("a start that cannot be sent releases the capture", async () => {
    const s = setup();
    (s.ctx.sendScreenStart as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error("socket"));
    expect(await s.pub.startScreenShare()).toEqual({ ok: false, error: "voice.error.notifyFailed" });
    expect(s.log).toContain("adapterStop:s1");
    expect(s.pub.isStarting()).toBe(false);
  });

  it("a stop that fails while the socket is up warns", async () => {
    const s = setup();
    const start = s.pub.startScreenShare();
    await flush();
    s.pub.handleAnswer(started("s1"));
    await start;
    (s.ctx.sendScreenStop as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error("send"));
    await s.pub.stopScreenShare("s1");
    expect(s.ctx.warnStopFailed).toHaveBeenCalled();
  });

  it("the camera ending while its start waits aborts it", async () => {
    const s = setup();
    const start = s.pub.startWebcam();
    await flush();
    await s.pub.captureEnded("webcam", null);
    expect(await start).toEqual({ ok: false });
    expect(s.log).toContain("adapterStopWebcam");
  });
});
