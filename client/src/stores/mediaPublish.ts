/**
 * Screen share and webcam start/stop (contract C30).
 *
 * A start is a request with an answer: the capture is opened first, the
 * start is announced to the server, and the track is added only after the
 * server accepted it (`screen_share_started` with our `stream_id`,
 * `webcam_started` with our user id). A refusal (`voice_error`) or no
 * answer within 5 s stops the capture and gives a text for it. One start is
 * in flight at a time; the server answers starts in order, so a refusal
 * (which carries no stream id) belongs to the start in flight.
 *
 * Every way a share ends — the button, the tile, the browser's own "Stop
 * sharing" bar, a closed window, a revoked permission, a dropped video —
 * goes through the same stop: senders removed, capture stopped, the server
 * told, the local state reset.
 *
 * Loaded on the first share or camera action (not in the startup payload).
 */

import type { MessageKey } from "@/i18n";
import type { VoiceAdapter, VoiceError, ScreenShareQuality } from "@/lib/webrtc/types";
import { isSameSession, type VoiceSessionHandle } from "@/lib/webrtc/voiceSession";
import { setMediaAnswerListener, type MediaAnswer } from "./mediaAckHook";

/** How long a start waits for the server's answer. */
export const MEDIA_START_TIMEOUT_MS = 5000;

/** For 1.0 a person shares at most one screen at a time. */
export const MAX_OWN_SCREEN_SHARES = 1;

export type MediaKind = "screen" | "webcam";

/**
 * Result of a start. `error` is a ready text; a start the user cancelled
 * has neither (`note` is a calm hint, e.g. no screen chosen in the portal).
 */
export type MediaStartResult =
  | { ok: true }
  | { ok: false; error?: string; note?: string };

/** What the publish flow needs from the voice session (a seam for tests). */
export interface MediaContext {
  adapter: () => Promise<VoiceAdapter>;
  /** The held channel session (null: none). */
  session: () => VoiceSessionHandle | null;
  connected: () => boolean;
  channelId: () => string | null;
  myUserId: () => string | null;
  myName: () => string;
  /** The WebSocket is connected (a stop can be sent). */
  socketUp: () => boolean;
  sendScreenStart: (
    channelId: string,
    streamId: string,
    quality: ScreenShareQuality,
    hasAudio: boolean,
    sourceLabel: string,
  ) => Promise<void>;
  sendScreenStop: (channelId: string, streamId: string) => Promise<void>;
  sendWebcamStart: (channelId: string, quality: ScreenShareQuality) => Promise<void>;
  sendWebcamStop: (channelId: string) => Promise<void>;
  setFlags: (flags: { screenSharing?: boolean; webcamActive?: boolean }) => void;
  /** Own screen preview (browser: the SFU never sends our track back). */
  addPreview: (streamId: string, track: MediaStreamTrack, sourceLabel: string) => void;
  removePreview: (streamId: string) => void;
  t: (key: MessageKey, params?: Record<string, string | number>) => string;
  /** A share ended by something else than a click (e.g. a dropped video). */
  notify: (text: string) => void;
  /** Not reachable on the screen: a stop could not be sent. */
  warnStopFailed: () => void;
  newStreamId: () => string;
}

type Answer =
  | { kind: "accepted" }
  | { kind: "refused"; code: string; message: string }
  | { kind: "timeout" }
  | { kind: "aborted" };

interface InFlight {
  kind: MediaKind;
  streamId: string | null;
  channelId: string | null;
  /** The session the start belongs to. */
  session: VoiceSessionHandle | null;
  /** Sent: from now on (and only now) an ack or refusal can answer it. */
  armed: boolean;
  settle: (answer: Answer) => void;
}

interface Live {
  session: VoiceSessionHandle;
  publishedAt: number;
  /** Announced to the server (its stop must be sent). */
  announced: boolean;
}

/**
 * Codes that refuse a start (contract C30), plus what the server's start
 * handlers return before their own checks (`ROOM_NOT_FOUND`, `UNAUTHORIZED`)
 * and rc.5's single `VOICE_ERROR`. Everything else is not an answer to a
 * start (a start without an answer times out).
 */
const START_REFUSAL_CODES = new Set([
  "SCREEN_SHARE_FORBIDDEN",
  "SCREEN_SHARE_LIMIT",
  "RATE_LIMITED",
  "PARTICIPANT_NOT_FOUND",
  "DUPLICATE_STREAM_ID",
  "SIGNALING_ERROR",
  "ROOM_NOT_FOUND",
  "UNAUTHORIZED",
  "VOICE_ERROR",
]);

/** A dropped track (C30): never the refusal of a start. */
const TRACK_DROPPED_CODES = new Set(["VIDEO_SOURCE_UNANNOUNCED", "AUDIO_SOURCE_UNANNOUNCED"]);

/**
 * Whether a `voice_error` answers the (sent) start in flight. A rate limit
 * of another request (`voice_stats`, `voice_join`) does not.
 */
export function isStartRefusal(code: string, message: string): boolean {
  if (!START_REFUSAL_CODES.has(code)) return false;
  if (/voice_stats|voice_join/.test(message)) return false;
  return true;
}

/** Text key for a refused start. rc.5 servers send `VOICE_ERROR` with a message only. */
export function refusalKey(kind: MediaKind, code: string, message: string): MessageKey {
  let effective = code;
  if (code === "VOICE_ERROR") {
    if (/rate limit/i.test(message)) effective = "RATE_LIMITED";
    else if (/limit/i.test(message)) effective = "SCREEN_SHARE_LIMIT";
    else if (/authori|permission/i.test(message)) effective = "SCREEN_SHARE_FORBIDDEN";
  }
  if (effective === "RATE_LIMITED") return "voice.media.error.rateLimited";
  if (kind === "webcam") return "voice.webcam.error.refused";
  if (effective === "SCREEN_SHARE_FORBIDDEN") return "voice.screenShare.error.noPermission";
  if (effective === "SCREEN_SHARE_LIMIT") return "voice.screenShare.error.limitReached";
  return "voice.screenShare.error.refused";
}

/** Text for a capture that could not be opened; null when the user cancelled. */
export function captureErrorKey(kind: MediaKind, error: VoiceError): MessageKey | null {
  switch (error.type) {
    case "cancelled":
      return null;
    case "permission_denied":
      return kind === "screen" ? "voice.screenShare.error.blocked" : "voice.webcam.error.blocked";
    case "device_not_found":
      return kind === "screen" ? "voice.screenShare.error.noSource" : "voice.webcam.error.notFound";
    case "not_found":
      if (error.message === "portal_unavailable") return "voice.screenShare.error.portalMissing";
      return kind === "screen" ? "voice.screenShare.error.noSource" : "voice.webcam.error.notFound";
    case "device_in_use":
      return kind === "screen" ? "voice.screenShare.error.failed" : "voice.webcam.error.inUse";
    case "not_connected":
      return "voice.error.notConnected";
    default:
      return kind === "screen" ? "voice.screenShare.error.failed" : "voice.webcam.error.failed";
  }
}

/** The start/stop flow over a voice session. */
export function createMediaPublisher(ctx: MediaContext) {
  let inFlight: InFlight | null = null;
  const screens = new Map<string, Live>();
  let webcam: Live | null = null;

  /** Whether `session` is the held one and still connected (for starting). */
  const held = (session: VoiceSessionHandle | null): boolean =>
    session !== null && isSameSession(session, ctx.session()) && ctx.connected();

  /**
   * Whether a stop for something of `session` must go out now: the same
   * session, in any voice state ("reconnecting" too), with the socket up.
   * Without a socket nothing is owed: the server removes the peer and its
   * shares and webcam when the socket closes, and the client rejoins with
   * a new session.
   */
  const mayStop = (session: VoiceSessionHandle | null): boolean =>
    session !== null && isSameSession(session, ctx.session()) && ctx.socketUp();

  /**
   * Starts that ended on our side after they were sent (timeout, capture
   * ended, cancelled): a late acceptance is answered with a stop.
   */
  const abandonedScreens = new Set<string>();
  let abandonedWebcam = false;

  /** Drop what belongs to an ended session (its adapter already released it). */
  function prune(): void {
    const current = ctx.session();
    for (const [id, live] of screens) {
      if (!isSameSession(live.session, current)) screens.delete(id);
    }
    if (webcam && !isSameSession(webcam.session, current)) webcam = null;
    // A start of an ended session is over: it must not block the next one.
    if (inFlight && !isSameSession(inFlight.session, current)) {
      inFlight.settle({ kind: "aborted" });
    }
  }

  function syncFlags(): void {
    ctx.setFlags({ screenSharing: screens.size > 0, webcamActive: webcam !== null });
  }

  /**
   * Reserve the one start in flight. The 5 s for the server's answer start
   * with `arm()`, right after the start was sent (a browser picker may stay
   * open much longer than that).
   */
  function waitForAnswer(kind: MediaKind, streamId: string | null): {
    answer: Promise<Answer>;
    abort: () => void;
    arm: () => void;
  } {
    let settle!: (a: Answer) => void;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const answer = new Promise<Answer>((resolve) => {
      let done = false;
      settle = (a) => {
        if (done) return;
        done = true;
        if (timer) clearTimeout(timer);
        if (inFlight?.settle === settle) inFlight = null;
        resolve(a);
      };
    });
    const entry: InFlight = {
      kind,
      streamId,
      channelId: ctx.channelId(),
      session: ctx.session(),
      armed: false,
      settle,
    };
    inFlight = entry;
    return {
      answer,
      abort: () => settle({ kind: "aborted" }),
      arm: () => {
        entry.armed = true;
        timer ??= setTimeout(() => settle({ kind: "timeout" }), MEDIA_START_TIMEOUT_MS);
      },
    };
  }

  function handleAnswer(answer: MediaAnswer): void {
    if (answer.type === "voice_error") {
      if (TRACK_DROPPED_CODES.has(answer.code)) {
        void handleUnannounced(answer.code === "AUDIO_SOURCE_UNANNOUNCED" ? "screen" : null);
        return;
      }
      // Only a start that was sent can be refused (R-2).
      if (inFlight?.armed && isStartRefusal(answer.code, answer.message)) {
        inFlight.settle({ kind: "refused", code: answer.code, message: answer.message });
      }
      return;
    }
    if (answer.channel_id !== ctx.channelId() && answer.channel_id !== inFlight?.channelId) return;
    const mine =
      answer.type === "screen_share_started" ||
      (answer.type === "webcam_started" && answer.user_id === ctx.myUserId());
    if (!mine) return;
    if (inFlight?.armed && answer.channel_id === inFlight.channelId) {
      if (answer.type === "screen_share_started" && inFlight.kind === "screen" && answer.stream_id === inFlight.streamId) {
        inFlight.settle({ kind: "accepted" });
        return;
      }
      if (answer.type === "webcam_started" && inFlight.kind === "webcam") {
        inFlight.settle({ kind: "accepted" });
        return;
      }
    }
    // An acceptance of a start we no longer want: take it back.
    if (answer.type === "screen_share_started" && abandonedScreens.delete(answer.stream_id)) {
      if (mayStop(ctx.session())) void ctx.sendScreenStop(answer.channel_id, answer.stream_id).catch(() => {});
    } else if (answer.type === "webcam_started" && abandonedWebcam && !webcam) {
      abandonedWebcam = false;
      if (mayStop(ctx.session())) void ctx.sendWebcamStop(answer.channel_id).catch(() => {});
    }
  }

  /** The user closed the dialog while the start was running: abort it. */
  function cancelStart(): void {
    inFlight?.settle({ kind: "aborted" });
  }

  /** Common checks before a start. */
  function refuseStart(): MediaStartResult | null {
    if (!ctx.connected() || !ctx.channelId() || !ctx.session()) {
      return { ok: false, error: ctx.t("voice.error.notConnected") };
    }
    if (inFlight) return { ok: false, error: ctx.t("voice.media.error.busy") };
    return null;
  }

  async function startScreenShare(
    quality: ScreenShareQuality = "medium",
    sourceId?: string,
  ): Promise<MediaStartResult> {
    prune();
    const refused = refuseStart();
    if (refused) return refused;
    if (screens.size >= MAX_OWN_SCREEN_SHARES) {
      return { ok: false, error: ctx.t("voice.screenShare.error.alreadySharing") };
    }
    const session = ctx.session();
    const channelId = ctx.channelId() as string;
    const streamId = ctx.newStreamId();
    // Reserve the one start before the first await.
    const wait = waitForAnswer("screen", streamId);
    const adapter = await ctx.adapter();

    const captured = await adapter.captureScreen({ quality, sourceId, streamId });
    if (!captured.ok) {
      wait.abort();
      if (captured.error.type === "cancelled" && captured.error.message === "portal") {
        return { ok: false, note: ctx.t("voice.screenShare.portalCancelled") };
      }
      const key = captureErrorKey("screen", captured.error);
      return key ? { ok: false, error: ctx.t(key) } : { ok: false };
    }
    if (!held(session) || !inFlightIs(streamId)) {
      wait.abort();
      await adapter.stopScreenShare(streamId);
      return { ok: false, error: ctx.t("voice.error.notConnected") };
    }

    // Armed as the message goes out: the answer may arrive before the send
    // call returns.
    wait.arm();
    try {
      await ctx.sendScreenStart(
        channelId,
        streamId,
        quality,
        captured.value.hasAudio,
        captured.value.sourceLabel,
      );
    } catch (err) {
      console.error("[MediaPublish] Screen share start could not be sent:", err);
      wait.abort();
      await adapter.stopScreenShare(streamId);
      return { ok: false, error: ctx.t("voice.error.notifyFailed") };
    }

    const answer = await wait.answer;
    if (answer.kind !== "accepted") {
      await adapter.stopScreenShare(streamId);
      // No answer, or the capture ended while waiting: the server may still
      // accept the start; a stop right after it frees that slot.
      if (answer.kind === "timeout" || answer.kind === "aborted") {
        abandonedScreens.add(streamId);
        if (mayStop(session)) void ctx.sendScreenStop(channelId, streamId).catch(() => {});
      }
      if (answer.kind === "refused") {
        return { ok: false, error: ctx.t(refusalKey("screen", answer.code, answer.message)) };
      }
      if (answer.kind === "timeout") {
        return { ok: false, error: ctx.t("voice.screenShare.error.noAnswer") };
      }
      return { ok: false };
    }

    if (!held(session)) {
      // Accepted, but the call ended or is reconnecting: the server holds
      // the announcement, so it is taken back (R-1).
      await adapter.stopScreenShare(streamId);
      if (mayStop(session)) void ctx.sendScreenStop(channelId, streamId).catch(() => {});
      return { ok: false, error: ctx.t("voice.error.notConnected") };
    }
    const published = await adapter.publishScreenShare(streamId);
    if (!published.ok) {
      await adapter.stopScreenShare(streamId);
      void ctx.sendScreenStop(channelId, streamId).catch(() => {});
      const key = captureErrorKey("screen", published.error);
      return key ? { ok: false, error: ctx.t(key) } : { ok: false };
    }
    screens.set(streamId, { session: session!, publishedAt: Date.now(), announced: true });
    const track = adapter.getScreenShareTrack(streamId);
    if (track) ctx.addPreview(streamId, track, captured.value.sourceLabel);
    syncFlags();
    return { ok: true };
  }

  function inFlightIs(streamId: string): boolean {
    return inFlight?.streamId === streamId;
  }

  /** Stop one share (or all own shares): the one path for every way a share ends. */
  async function stopScreenShare(streamId?: string): Promise<void> {
    prune();
    const ids = streamId ? [streamId] : [...screens.keys()];
    if (ids.length === 0) return;
    const channelId = ctx.channelId();
    const adapter = await ctx.adapter();
    for (const id of ids) {
      const live = screens.get(id);
      screens.delete(id);
      const result = await adapter.stopScreenShare(id);
      if (!result.ok) console.warn("[MediaPublish] Adapter stop:", result.error);
      ctx.removePreview(id);
      if (live?.announced && channelId && mayStop(live.session)) {
        try {
          await ctx.sendScreenStop(channelId, id);
        } catch (err) {
          console.error("[MediaPublish] Screen share stop could not be sent:", err);
          ctx.warnStopFailed();
        }
      }
    }
    syncFlags();
  }

  async function startWebcam(
    quality: ScreenShareQuality = "medium",
    deviceId?: string,
  ): Promise<MediaStartResult> {
    prune();
    const refused = refuseStart();
    if (refused) return refused;
    if (webcam) return { ok: false, error: ctx.t("voice.error.webcamActive") };
    const session = ctx.session();
    const channelId = ctx.channelId() as string;
    const wait = waitForAnswer("webcam", null);
    const adapter = await ctx.adapter();

    const captured = await adapter.captureWebcam({ quality, deviceId });
    if (!captured.ok) {
      wait.abort();
      const key = captureErrorKey("webcam", captured.error);
      return key ? { ok: false, error: ctx.t(key) } : { ok: false };
    }
    if (!held(session) || inFlight?.kind !== "webcam") {
      wait.abort();
      await adapter.stopWebcam();
      return { ok: false, error: ctx.t("voice.error.notConnected") };
    }
    wait.arm();
    try {
      await ctx.sendWebcamStart(channelId, quality);
    } catch (err) {
      console.error("[MediaPublish] Webcam start could not be sent:", err);
      wait.abort();
      await adapter.stopWebcam();
      return { ok: false, error: ctx.t("voice.error.notifyFailed") };
    }
    const answer = await wait.answer;
    if (answer.kind !== "accepted") {
      await adapter.stopWebcam();
      if (answer.kind === "timeout" || answer.kind === "aborted") {
        abandonedWebcam = true;
        if (mayStop(session)) void ctx.sendWebcamStop(channelId).catch(() => {});
      }
      if (answer.kind === "refused") {
        return { ok: false, error: ctx.t(refusalKey("webcam", answer.code, answer.message)) };
      }
      if (answer.kind === "timeout") {
        return { ok: false, error: ctx.t("voice.webcam.error.noAnswer") };
      }
      return { ok: false };
    }
    if (!held(session)) {
      await adapter.stopWebcam();
      if (mayStop(session)) void ctx.sendWebcamStop(channelId).catch(() => {});
      return { ok: false, error: ctx.t("voice.error.notConnected") };
    }
    const published = await adapter.publishWebcam();
    if (!published.ok) {
      await adapter.stopWebcam();
      void ctx.sendWebcamStop(channelId).catch(() => {});
      const key = captureErrorKey("webcam", published.error);
      return key ? { ok: false, error: ctx.t(key) } : { ok: false };
    }
    webcam = { session: session!, publishedAt: Date.now(), announced: true };
    abandonedWebcam = false;
    syncFlags();
    return { ok: true };
  }

  async function stopWebcam(): Promise<void> {
    prune();
    const live = webcam;
    if (!live) return;
    webcam = null;
    const channelId = ctx.channelId();
    const adapter = await ctx.adapter();
    const result = await adapter.stopWebcam();
    if (!result.ok) console.warn("[MediaPublish] Adapter webcam stop:", result.error);
    if (channelId && mayStop(live.session)) {
      try {
        await ctx.sendWebcamStop(channelId);
      } catch (err) {
        console.error("[MediaPublish] Webcam stop could not be sent:", err);
      }
    }
    syncFlags();
  }

  /**
   * A capture ended by itself. During its start the start is aborted (the
   * start's own path releases it); once live the normal stop runs.
   */
  async function captureEnded(kind: MediaKind, streamId: string | null): Promise<void> {
    if (inFlight && inFlight.kind === kind && (kind === "webcam" || inFlight.streamId === streamId)) {
      inFlight.settle({ kind: "aborted" });
      return;
    }
    if (kind === "screen" && streamId) await stopScreenShare(streamId);
    else if (kind === "webcam") await stopWebcam();
  }

  /**
   * The server dropped a video track it had no announcement for. The error
   * names no stream: the most recently published own video is the one.
   */
  async function handleUnannounced(only: MediaKind | null): Promise<void> {
    prune();
    let newest: { kind: MediaKind; id: string | null; at: number } | null = null;
    for (const [id, live] of screens) {
      if (!newest || live.publishedAt >= newest.at) newest = { kind: "screen", id, at: live.publishedAt };
    }
    if (only !== "screen" && webcam && (!newest || webcam.publishedAt >= newest.at)) {
      newest = { kind: "webcam", id: null, at: webcam.publishedAt };
    }
    if (!newest) return;
    if (newest.kind === "screen" && newest.id) await stopScreenShare(newest.id);
    else await stopWebcam();
    ctx.notify(ctx.t("voice.media.error.unannounced"));
  }

  return {
    startScreenShare,
    stopScreenShare,
    startWebcam,
    stopWebcam,
    captureEnded,
    handleAnswer,
    cancelStart,
    /** Whether a start is waiting (tests, UI). */
    isStarting: () => inFlight !== null,
  };
}

export type MediaPublisher = ReturnType<typeof createMediaPublisher>;

let instance: MediaPublisher | null = null;

/** The app's publisher over the channel voice session. */
export async function mediaPublisher(): Promise<MediaPublisher> {
  if (instance) return instance;
  const { voiceMediaContext } = await import("./voiceMediaContext");
  instance ??= createMediaPublisher(await voiceMediaContext());
  setMediaAnswerListener(instance.handleAnswer);
  return instance;
}
