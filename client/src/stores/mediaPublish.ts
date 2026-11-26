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
 * A start that does not end in a live share always ends with a stop to the
 * server (a `voice_error` names no stream, so it may belong to something
 * else, and the server may still accept the start). A publish that
 * completes after its capture ended, or after the held session changed or
 * started to reconnect, is taken down, not shown. A publish that throws
 * counts as a failed one, and the start's reservation is always released.
 *
 * Webcam answers carry no id. One camera attempt at a time: after a
 * camera start was given up, the next one waits until that attempt's late
 * answer came or a grace period passed, so an answer is never taken for
 * a newer attempt.
 *
 * Every way a share ends — the button, the tile, the browser's own "Stop
 * sharing" bar, a closed window, a revoked permission, a dropped video —
 * goes through the same stop: senders removed, capture stopped, the server
 * told, the local state reset. A stop always ends by syncing the
 * sharing/camera flags, so a stale "on" is cleared by pressing Stop.
 *
 * Loaded on the first share or camera action (not in the startup payload).
 */

import type { MessageKey } from "@/i18n";
import type { VoiceAdapter, VoiceError, VoiceResult, ScreenShareQuality } from "@/lib/webrtc/types";
import { isSameSession, type VoiceSessionHandle } from "@/lib/webrtc/voiceSession";
import { setMediaAnswerListener, type MediaAnswer } from "./mediaAckHook";

/** How long a start waits for the server's answer. */
export const MEDIA_START_TIMEOUT_MS = 5000;

/**
 * After a camera start was given up, how long its late answer may still
 * come before the next camera start is allowed.
 */
export const WEBCAM_LATE_ANSWER_GRACE_MS = 5000;

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
  /** `otherError`: an unlisted `voice_error` came while the start waited. */
  | { kind: "timeout"; otherError: boolean }
  /** `reason`: the capture ended by itself (see `endedReasonKey`). */
  | { kind: "aborted"; reason?: string };

interface InFlight {
  kind: MediaKind;
  streamId: string | null;
  channelId: string | null;
  /** The session the start belongs to. */
  session: VoiceSessionHandle | null;
  /** Sent: from now on (and only now) an ack or refusal can answer it. */
  armed: boolean;
  /** A `voice_error` outside the refusal list arrived while armed. */
  otherErrorSeen: boolean;
  settle: (answer: Answer) => void;
}

/** The publish step of an accepted start (after the answer, before "live"). */
interface Publishing {
  cancelled: boolean;
  /** The server ended the share itself (nothing is sent back). */
  serverEnded?: boolean;
  /** The capture ended by itself during the step (its reason). */
  captureEnded?: boolean;
  endedReason?: string;
  /** The server dropped the new track (no announcement for it). */
  unannounced?: boolean;
}

interface Live {
  session: VoiceSessionHandle;
  publishedAt: number;
  /** Announced to the server (its stop must be sent). */
  announced: boolean;
}

/**
 * Codes that refuse a start (contract C30), plus what the server's start
 * handlers return themselves (`ROOM_NOT_FOUND`, `UNAUTHORIZED`,
 * `INTERNAL_ERROR`, `DATABASE_ERROR`) and rc.5's single `VOICE_ERROR`. Everything else is not an answer to a
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
  "INTERNAL_ERROR",
  "DATABASE_ERROR",
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
  if (effective === "DUPLICATE_STREAM_ID") return "voice.screenShare.error.tryAgain";
  return "voice.screenShare.error.refused";
}

/** Marks the webcam's publish step (it has no stream id). */
const WEBCAM_ID = "\u0000webcam";

/** Text when the server ended an own share or webcam. */
export function serverStoppedKey(kind: MediaKind, reason: string): MessageKey {
  if (reason === "no_media") {
    return kind === "screen" ? "voice.screenShare.error.noMedia" : "voice.webcam.error.noMedia";
  }
  return kind === "screen"
    ? "voice.screenShare.error.endedByServer"
    : "voice.webcam.error.endedByServer";
}

/** Text for a share that ended by itself; null when the source just ended. */
export function endedReasonKey(reason?: string): MessageKey | null {
  switch (reason) {
    case "no_frames":
      return "voice.screenShare.error.noFrames";
    case "encoder_failed":
      return "voice.media.error.encoderFailed";
    case "not_sent":
      return "voice.media.error.notSent";
    default:
      return null;
  }
}

/** Text for a capture that could not be opened; null when the user cancelled. */
export function captureErrorKey(kind: MediaKind, error: VoiceError): MessageKey | null {
  switch (error.type) {
    case "cancelled":
      return null;
    case "permission_denied":
      if (error.message === "portal_denied") return "voice.screenShare.error.portalDenied";
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
   * A start whose publish step is running: closing the dialog now still
   * stops it (`cancelStart`).
   */
  let publishing: Publishing | null = null;
  /** Which start the publish step belongs to (stream id, or the webcam). */
  let inFlightIdPublishing: string | null = null;

  /**
   * A camera start was given up: its answer may still come. Until it does
   * (or the grace period ends) no new camera start is made, so that answer
   * cannot be taken for the new one.
   */
  let webcamAnswerOwed: ReturnType<typeof setTimeout> | null = null;

  function owePastWebcamAnswer(): void {
    if (webcamAnswerOwed) clearTimeout(webcamAnswerOwed);
    webcamAnswerOwed = setTimeout(() => {
      webcamAnswerOwed = null;
    }, WEBCAM_LATE_ANSWER_GRACE_MS);
  }

  function pastWebcamAnswerCame(): void {
    if (webcamAnswerOwed) clearTimeout(webcamAnswerOwed);
    webcamAnswerOwed = null;
  }

  /**
   * Tell the server a share is over. Never throws: a failure is logged (and
   * for a screen share the user is warned, like the normal stop does).
   */
  function sendStop(kind: MediaKind, channelId: string, streamId: string | null): void {
    const sent =
      kind === "screen" && streamId
        ? ctx.sendScreenStop(channelId, streamId)
        : ctx.sendWebcamStop(channelId);
    void sent.catch((err) => {
      console.error(`[MediaPublish] ${kind} stop could not be sent:`, err);
      if (kind === "screen") ctx.warnStopFailed();
    });
  }

  /** Text for a start whose capture ended by itself; none when it just ended. */
  function endedResult(reason?: string): MediaStartResult {
    const key = endedReasonKey(reason);
    return key ? { ok: false, error: ctx.t(key) } : { ok: false };
  }

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
      otherErrorSeen: false,
      settle,
    };
    inFlight = entry;
    return {
      answer,
      abort: () => settle({ kind: "aborted" }),
      arm: () => {
        entry.armed = true;
        timer ??= setTimeout(
          () => settle({ kind: "timeout", otherError: entry.otherErrorSeen }),
          MEDIA_START_TIMEOUT_MS,
        );
      },
    };
  }

  function handleAnswer(answer: MediaAnswer): void {
    if (answer.type === "screen_share_stopped" || answer.type === "webcam_stopped") {
      void serverStopped(answer);
      return;
    }
    if (answer.type === "voice_error") {
      // The session of a waiting start ended (taken over on another device,
      // removed): a late error of the old seat is not shown, the start just ends.
      if (inFlight && !isSameSession(inFlight.session, ctx.session())) {
        inFlight.settle({ kind: "aborted" });
        return;
      }
      if (TRACK_DROPPED_CODES.has(answer.code)) {
        void handleUnannounced(answer.code === "AUDIO_SOURCE_UNANNOUNCED" ? "screen" : null);
        return;
      }
      // Only a start that was sent can be refused (R-2); the list decides.
      if (inFlight?.armed && isStartRefusal(answer.code, answer.message)) {
        inFlight.settle({ kind: "refused", code: answer.code, message: answer.message });
      } else if (inFlight?.armed) {
        // Not settled by it, but remembered: if the start then times out,
        // the text says the server did not accept it.
        inFlight.otherErrorSeen = true;
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
    // An acceptance of a start given up on our side changes nothing: the
    // stop sent when it was given up follows the start on the same socket,
    // so the server ends it right after (exactly one stop per start). For
    // the camera it was the answer still owed: the next start may go.
    if (answer.type === "webcam_started") pastWebcamAnswerCame();
  }

  /**
   * The server ended an own share or webcam (any reason, e.g. `no_media`
   * when no picture arrived): the share is over here too. Nothing is sent
   * back (the server already ended it).
   */
  async function serverStopped(
    event: Extract<MediaAnswer, { type: "screen_share_stopped" | "webcam_stopped" }>,
  ): Promise<void> {
    if (event.user_id !== ctx.myUserId() || event.channel_id !== ctx.channelId()) return;
    if (event.type === "screen_share_stopped") {
      if (publishing && inFlightIdPublishing === event.stream_id) {
        publishing.cancelled = true;
        publishing.serverEnded = true;
        return;
      }
      if (!screens.has(event.stream_id)) return; // our own stop's echo
      screens.delete(event.stream_id);
      const adapter = await ctx.adapter();
      await adapter.stopScreenShare(event.stream_id);
      ctx.removePreview(event.stream_id);
      syncFlags();
      ctx.notify(ctx.t(serverStoppedKey("screen", event.reason)));
    } else {
      if (publishing && inFlightIdPublishing === WEBCAM_ID) {
        publishing.cancelled = true;
        publishing.serverEnded = true;
        return;
      }
      if (!webcam) return;
      webcam = null;
      const adapter = await ctx.adapter();
      await adapter.stopWebcam();
      syncFlags();
      ctx.notify(ctx.t(serverStoppedKey("webcam", event.reason)));
    }
  }

  /** The user closed the dialog while the start was running: abort it. */
  function cancelStart(): void {
    if (inFlight) inFlight.settle({ kind: "aborted" });
    else if (publishing) publishing.cancelled = true;
  }

  /** Common checks before a start. */
  function refuseStart(): MediaStartResult | null {
    if (!ctx.connected() || !ctx.channelId() || !ctx.session()) {
      return { ok: false, error: ctx.t("voice.error.notConnected") };
    }
    // One start at a time, its publish step included.
    if (inFlight || publishing) return { ok: false, error: ctx.t("voice.media.error.busy") };
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
      // Whatever ended the start (no answer, the dialog, the capture, a
      // refusal that names no stream and may belong to something else):
      // the server may still accept it, so a stop follows the start.
      if (mayStop(session)) sendStop("screen", channelId, streamId);
      if (answer.kind === "refused") {
        return { ok: false, error: ctx.t(refusalKey("screen", answer.code, answer.message)) };
      }
      if (answer.kind === "timeout") {
        return {
          ok: false,
          error: ctx.t(
            answer.otherError ? "voice.screenShare.error.refused" : "voice.screenShare.error.noAnswer",
          ),
        };
      }
      return endedResult(answer.reason);
    }

    if (!held(session)) {
      // Accepted, but the call ended or is reconnecting: the server holds
      // the announcement, so it is taken back (R-1).
      await adapter.stopScreenShare(streamId);
      if (mayStop(session)) sendStop("screen", channelId, streamId);
      return { ok: false, error: ctx.t("voice.error.notConnected") };
    }
    const run: Publishing = { cancelled: false };
    publishing = run;
    inFlightIdPublishing = streamId;
    const published = await publishStep(run, "screen", () => adapter.publishScreenShare(streamId));
    if (published.ok && run.cancelled) {
      // The dialog was closed, the capture ended, the server dropped the
      // track or ended the share during the publish step: stop it again.
      await adapter.stopScreenShare(streamId);
      if (!run.serverEnded && mayStop(session)) sendStop("screen", channelId, streamId);
      return cancelledResult("screen", run);
    }
    if (!published.ok) {
      await adapter.stopScreenShare(streamId);
      if (mayStop(session)) sendStop("screen", channelId, streamId);
      if (run.cancelled) return cancelledResult("screen", run);
      const key = captureErrorKey("screen", published.error);
      return key ? { ok: false, error: ctx.t(key) } : { ok: false };
    }
    if (!held(session)) {
      // Published for a session that is no longer the held one (or is
      // reconnecting): a stale success is taken back, never shown.
      await adapter.stopScreenShare(streamId);
      if (mayStop(session)) sendStop("screen", channelId, streamId);
      return { ok: false, error: ctx.t("voice.error.notConnected") };
    }
    screens.set(streamId, { session: session!, publishedAt: Date.now(), announced: true });
    const track = adapter.getScreenShareTrack(streamId);
    if (track) ctx.addPreview(streamId, track, captured.value.sourceLabel);
    syncFlags();
    return { ok: true };
  }

  /**
   * Run the publish step of `run`. A throw (the browser refusing to add the
   * track) counts as a failed publish, and the start's reservation is
   * released either way.
   */
  async function publishStep(
    run: Publishing,
    kind: MediaKind,
    publish: () => Promise<VoiceResult<void>>,
  ): Promise<VoiceResult<void>> {
    try {
      return await publish();
    } catch (err) {
      console.error(`[MediaPublish] ${kind} could not be published:`, err);
      return { ok: false, error: { type: "unknown", message: String(err) } };
    } finally {
      if (publishing === run) {
        publishing = null;
        inFlightIdPublishing = null;
      }
    }
  }

  /** Result of a start whose publish step was cancelled (see `Publishing`). */
  function cancelledResult(kind: MediaKind, run: Publishing): MediaStartResult {
    if (run.serverEnded) {
      return {
        ok: false,
        error: ctx.t(
          kind === "screen" ? "voice.screenShare.error.endedByServer" : "voice.webcam.error.endedByServer",
        ),
      };
    }
    if (run.unannounced) return { ok: false, error: ctx.t("voice.media.error.unannounced") };
    if (run.captureEnded) return endedResult(run.endedReason);
    return { ok: false };
  }

  function inFlightIs(streamId: string): boolean {
    return inFlight?.streamId === streamId;
  }

  /** Stop one share (or all own shares): the one path for every way a share ends. */
  async function stopScreenShare(streamId?: string): Promise<void> {
    prune();
    const ids = streamId ? [streamId] : [...screens.keys()];
    if (ids.length === 0) {
      // Nothing of the held session is left: a stale "sharing" flag ends here.
      syncFlags();
      return;
    }
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
    // The answer of a camera start given up just before may still come.
    if (webcamAnswerOwed) return { ok: false, error: ctx.t("voice.media.error.busy") };
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
      // As for a share: a stop follows the start. The camera's answer names
      // no attempt, so the next start waits for this one's late answer.
      if (mayStop(session)) {
        sendStop("webcam", channelId, null);
        owePastWebcamAnswer();
      }
      if (answer.kind === "refused") {
        return { ok: false, error: ctx.t(refusalKey("webcam", answer.code, answer.message)) };
      }
      if (answer.kind === "timeout") {
        return {
          ok: false,
          error: ctx.t(answer.otherError ? "voice.webcam.error.refused" : "voice.webcam.error.noAnswer"),
        };
      }
      return endedResult(answer.reason);
    }
    if (!held(session)) {
      await adapter.stopWebcam();
      if (mayStop(session)) sendStop("webcam", channelId, null);
      return { ok: false, error: ctx.t("voice.error.notConnected") };
    }
    const run: Publishing = { cancelled: false };
    publishing = run;
    inFlightIdPublishing = WEBCAM_ID;
    const published = await publishStep(run, "webcam", () => adapter.publishWebcam());
    if (published.ok && run.cancelled) {
      await adapter.stopWebcam();
      if (!run.serverEnded && mayStop(session)) sendStop("webcam", channelId, null);
      return cancelledResult("webcam", run);
    }
    if (!published.ok) {
      await adapter.stopWebcam();
      if (mayStop(session)) sendStop("webcam", channelId, null);
      if (run.cancelled) return cancelledResult("webcam", run);
      const key = captureErrorKey("webcam", published.error);
      return key ? { ok: false, error: ctx.t(key) } : { ok: false };
    }
    if (!held(session)) {
      await adapter.stopWebcam();
      if (mayStop(session)) sendStop("webcam", channelId, null);
      return { ok: false, error: ctx.t("voice.error.notConnected") };
    }
    webcam = { session: session!, publishedAt: Date.now(), announced: true };
    syncFlags();
    return { ok: true };
  }

  async function stopWebcam(): Promise<void> {
    prune();
    const live = webcam;
    if (!live) {
      syncFlags(); // a stale "camera on" flag ends here
      return;
    }
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
   * A capture ended by itself. During its start (the wait or the publish
   * step) the start ends, says why and sends its stop; once live the
   * normal stop runs.
   */
  async function captureEnded(
    kind: MediaKind,
    streamId: string | null,
    reason?: string,
  ): Promise<void> {
    if (inFlight && inFlight.kind === kind && (kind === "webcam" || inFlight.streamId === streamId)) {
      inFlight.settle({ kind: "aborted", reason });
      return;
    }
    const startId = kind === "webcam" ? WEBCAM_ID : streamId;
    if (publishing && startId !== null && inFlightIdPublishing === startId) {
      publishing.cancelled = true;
      publishing.captureEnded = true;
      publishing.endedReason = reason;
      return;
    }
    const wasLive = kind === "screen" ? !!streamId && screens.has(streamId) : webcam !== null;
    if (kind === "screen" && streamId) await stopScreenShare(streamId);
    else if (kind === "webcam") await stopWebcam();
    const key = endedReasonKey(reason);
    if (wasLive && key) ctx.notify(ctx.t(key));
  }

  /**
   * The server dropped a video track it had no announcement for. The error
   * names no stream: the most recently published own video is the one.
   */
  async function handleUnannounced(only: MediaKind | null): Promise<void> {
    prune();
    // A track just being added is the newest: its publish step ends.
    if (publishing && (only === null || inFlightIdPublishing !== WEBCAM_ID)) {
      publishing.cancelled = true;
      publishing.unannounced = true;
      return;
    }
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

/** One publisher per loader, created on the first call (a seam for tests). */
export function publisherLoader(
  loadContext: () => Promise<MediaContext>,
): () => Promise<MediaPublisher> {
  let instance: Promise<MediaPublisher> | null = null;
  return () => {
    // Cached before the first await: concurrent first callers share it.
    instance ??= (async () => {
      const publisher = createMediaPublisher(await loadContext());
      setMediaAnswerListener(publisher.handleAnswer);
      return publisher;
    })().catch((err: unknown) => {
      instance = null; // a failed load is tried again by the next caller
      throw err;
    });
    return instance;
  };
}

/** The app's publisher over the channel voice session. */
export const mediaPublisher = publisherLoader(async () => {
  const { voiceMediaContext } = await import("./voiceMediaContext");
  return voiceMediaContext();
});
