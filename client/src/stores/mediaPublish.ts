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
  settle: (answer: Answer) => void;
}

interface Live {
  session: VoiceSessionHandle;
  publishedAt: number;
  /** Announced to the server (its stop must be sent). */
  announced: boolean;
}

/** Codes that are never the answer to a start. */
const NOT_A_START_ANSWER = new Set([
  "VIDEO_SOURCE_UNANNOUNCED",
  "CHANNEL_FULL",
  "JOIN_REFUSED",
  "ALREADY_JOINED",
  "NOT_IN_CHANNEL",
  "ICE_FAILED",
]);

/**
 * Whether a `voice_error` answers the start in flight. Unknown codes count
 * as a refusal (C30: handled as a generic refusal); a rate limit of another
 * request (`voice_stats`, `voice_join`) does not.
 */
export function isStartRefusal(code: string, message: string): boolean {
  if (NOT_A_START_ANSWER.has(code)) return false;
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

  const held = (session: VoiceSessionHandle | null): boolean =>
    session !== null && isSameSession(session, ctx.session()) && ctx.connected();

  /** Drop what belongs to an ended session (its adapter already released it). */
  function prune(): void {
    const current = ctx.session();
    for (const [id, live] of screens) {
      if (!isSameSession(live.session, current)) screens.delete(id);
    }
    if (webcam && !isSameSession(webcam.session, current)) webcam = null;
  }

  function syncFlags(): void {
    ctx.setFlags({ screenSharing: screens.size > 0, webcamActive: webcam !== null });
  }

  function waitForAnswer(kind: MediaKind, streamId: string | null): {
    answer: Promise<Answer>;
    abort: () => void;
  } {
    let settle!: (a: Answer) => void;
    const answer = new Promise<Answer>((resolve) => {
      let done = false;
      const timer = setTimeout(() => settle({ kind: "timeout" }), MEDIA_START_TIMEOUT_MS);
      settle = (a) => {
        if (done) return;
        done = true;
        clearTimeout(timer);
        if (inFlight?.settle === settle) inFlight = null;
        resolve(a);
      };
    });
    inFlight = { kind, streamId, channelId: ctx.channelId(), settle };
    return { answer, abort: () => settle({ kind: "aborted" }) };
  }

  function handleAnswer(answer: MediaAnswer): void {
    if (answer.type === "voice_error") {
      if (answer.code === "VIDEO_SOURCE_UNANNOUNCED") {
        void handleUnannounced();
        return;
      }
      if (inFlight && isStartRefusal(answer.code, answer.message)) {
        inFlight.settle({ kind: "refused", code: answer.code, message: answer.message });
      }
      return;
    }
    if (!inFlight || answer.channel_id !== inFlight.channelId) return;
    if (
      answer.type === "screen_share_started" &&
      inFlight.kind === "screen" &&
      answer.stream_id === inFlight.streamId
    ) {
      inFlight.settle({ kind: "accepted" });
    } else if (
      answer.type === "webcam_started" &&
      inFlight.kind === "webcam" &&
      answer.user_id === ctx.myUserId()
    ) {
      inFlight.settle({ kind: "accepted" });
    }
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
      if ((answer.kind === "timeout" || answer.kind === "aborted") && held(session)) {
        void ctx.sendScreenStop(channelId, streamId).catch(() => {});
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
      await adapter.stopScreenShare(streamId);
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
      if (live?.announced && channelId && held(live.session)) {
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
      if ((answer.kind === "timeout" || answer.kind === "aborted") && held(session)) {
        void ctx.sendWebcamStop(channelId).catch(() => {});
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
    if (channelId && held(live.session)) {
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
  async function handleUnannounced(): Promise<void> {
    prune();
    let newest: { kind: MediaKind; id: string | null; at: number } | null = null;
    for (const [id, live] of screens) {
      if (!newest || live.publishedAt >= newest.at) newest = { kind: "screen", id, at: live.publishedAt };
    }
    if (webcam && (!newest || webcam.publishedAt >= newest.at)) {
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
