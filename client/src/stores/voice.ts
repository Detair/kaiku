/**
 * Voice Store
 *
 * Manages voice channel state including connection, participants, and audio settings.
 */

import { createEffect, onCleanup } from "solid-js";
import { createStore } from "solid-js/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  createVoiceAdapter,
  getVoiceAdapter,
  type VoiceError,
  type ConnectionMetrics,
  type ParticipantMetrics,
  type QualityLevel,
} from "@/lib/webrtc";
import type {
  ScreenShareInfo,
  ScreenShareQuality,
  VoiceAdapter,
  VoiceConnectionState,
} from "@/lib/webrtc/types";
import type { VoiceParticipant, WebcamServerInfo } from "@/lib/types";
import { channelsState } from "@/stores/channels";
import { appSettings, updateAudioSetting } from "@/stores/settings";
import { currentUser } from "@/stores/auth";
import * as tauri from "@/lib/tauri";
import { showToast, dismissToast } from "@/components/ui/Toast";
import { t } from "@/i18n";
import { voiceErrorText } from "@/lib/webrtc/voiceErrorText";
import { PttController, createTauriPttListeners, type PttFullConfig } from "@/lib/pttManager";
import { isTauri } from "@/lib/tauri/common";
import { isSameSession, type VoiceSessionHandle } from "@/lib/webrtc/voiceSession";
import {
  voiceOwner,
  RECOVERY_ATTEMPT_TIMEOUT_MS,
  RECOVERY_DELAYS_MS,
  type VoiceHooks,
  type VoiceWish,
} from "./voiceOwner";

const getErrorMessage = voiceErrorText;

// Voice connection state. "reconnecting": the media connection had a short
// outage or the rejoin flow (C32) is running; the call is still on screen.
type VoiceState = "disconnected" | "connecting" | "connected" | "reconnecting";

interface VoiceStoreState {
  // Current state
  state: VoiceState;
  // Connected channel ID
  channelId: string | null;
  // Local user state
  muted: boolean;
  deafened: boolean;
  speaking: boolean;
  // Participants in the channel
  participants: Record<string, VoiceParticipant>;
  // Error
  error: VoiceError | null;
  /**
   * The rejoin flow (C32) gave up for this channel: the voice UI offers a
   * "Rejoin" action. Cleared by any own join or leave.
   */
  lostChannelId: string | null;

  // Screen sharing
  screenSharing: boolean;
  screenShares: ScreenShareInfo[]; // All active screen shares in channel

  // Webcam
  webcamActive: boolean;
  webcams: WebcamServerInfo[]; // All active webcams in channel

  // Session tracking for metrics
  sessionId: string | null;
  connectedAt: number | null;
  // Local connection metrics
  localMetrics: ConnectionMetrics | "unknown" | null;
  // Per-participant metrics from server
  participantMetrics: Map<string, ParticipantMetrics>;
}

// Create the store
const [voiceState, setVoiceState] = createStore<VoiceStoreState>({
  state: "disconnected",
  channelId: null,
  muted: false,
  deafened: false,
  speaking: false,
  participants: {},
  error: null,
  lostChannelId: null,
  screenSharing: false,
  screenShares: [],
  webcamActive: false,
  webcams: [],
  sessionId: null,
  connectedAt: null,
  localMetrics: null,
  participantMetrics: new Map(),
});

// PTT/PTM controller instance
let pttController: PttController | null = null;
let pttCleanup: (() => void) | null = null;

// Metrics collection interval
let metricsInterval: number | null = null;

/**
 * The channel voice session this store last asked the voice owner for. It
 * may have been replaced since (a DM call, give-up); `voiceOwner.wants`
 * says whether it is still the intent. The store never touches the
 * adapter's session itself: joining, leaving and recovery are the owner's.
 */
let channelWish: VoiceWish | null = null;

// The owner uses the same adapter instance as this store (already loaded
// here, so the owner does not wait for a module to load before a join).
voiceOwner.useAdapter(createVoiceAdapter);

/** The adapter session of the channel voice the user is in, if any. */
function heldSession(): VoiceSessionHandle | null {
  return voiceOwner.sessionOf(channelWish);
}

/** Remote `<audio>` elements per user (browser mode), for cleanup. */
const remoteAudioElements = new Map<string, HTMLAudioElement>();

function removeRemoteAudioElements(): void {
  for (const [, el] of remoteAudioElements) {
    el.srcObject = null;
    el.remove();
  }
  remoteAudioElements.clear();
}

/** Number of remote audio elements currently playing (exported for tests). */
export function remoteAudioElementCount(): number {
  return remoteAudioElements.size;
}

/** Store state for an adapter state. */
function storeStateFor(state: VoiceConnectionState): VoiceState {
  switch (state) {
    case "connected":
      return "connected";
    case "reconnecting":
      return "reconnecting";
    case "disconnected":
      return "disconnected";
    default:
      return "connecting";
  }
}

// Notification state for packet loss incidents
let currentIncidentStart: number | null = null;
let goodQualityStartTime: number | null = null;
const INCIDENT_RECOVERY_THRESHOLD = 10_000; // 10s of good quality to clear incident

/** Toast ids for native audio stream failures (one each, replaced on repeat). */
const AUDIO_CAPTURE_TOAST_ID = "voice-audio-capture-failed";
const AUDIO_PLAYBACK_TOAST_ID = "voice-audio-playback-failed";

/**
 * Convert QualityLevel to numeric value for server transmission.
 * Uses semantic quality names: good=3, warning=2, poor=1, unknown=0
 */
function qualityToNumber(quality: QualityLevel): number {
  switch (quality) {
    case "good":
      return 3;
    case "warning":
      return 2;
    case "poor":
      return 1;
    case "unknown":
      return 0;
  }
}

/**
 * Convert numeric quality value to QualityLevel.
 * Uses semantic quality names: 3=good, 2=warning, 1=poor, 0=unknown
 */
function numberToQuality(n: number): QualityLevel {
  switch (n) {
    case 3:
      return "good";
    case 2:
      return "warning";
    case 1:
      return "poor";
    default:
      return "unknown";
  }
}

/**
 * Check packet loss thresholds and show/dismiss notifications.
 * - >= 3%: Warning notification
 * - >= 7%: Critical (error) notification
 * - < 3% for 10s: Clear incident
 */
function checkPacketLossThresholds(metrics: ConnectionMetrics): void {
  const now = Date.now();
  const isBadQuality = metrics.packetLoss >= 3;

  if (isBadQuality) {
    // Reset recovery tracking when quality is bad
    goodQualityStartTime = null;

    if (!currentIncidentStart) {
      // New incident started
      currentIncidentStart = now;

      if (metrics.packetLoss >= 7) {
        // Critical packet loss - persistent error toast
        showToast({
          type: "error",
          title: t("voice.toast.degraded"),
          message: t("voice.toast.packetLoss", {
            percent: metrics.packetLoss.toFixed(1),
          }),
          duration: 0,
          id: "connection-critical",
        });
      } else {
        // Warning level packet loss
        showToast({
          type: "warning",
          title: t("voice.toast.unstable"),
          message: t("voice.toast.packetLoss", {
            percent: metrics.packetLoss.toFixed(1),
          }),
          duration: 5000,
          id: "connection-warning",
        });
      }
    } else if (metrics.packetLoss >= 7) {
      // Escalate from warning to critical
      dismissToast("connection-warning");
      showToast({
        type: "error",
        title: t("voice.toast.degraded"),
        message: t("voice.toast.packetLoss", {
            percent: metrics.packetLoss.toFixed(1),
          }),
        duration: 0,
        id: "connection-critical",
      });
    }
  } else {
    // Quality is good - track recovery
    if (!goodQualityStartTime) {
      goodQualityStartTime = now; // Start tracking recovery
    }

    // Check if we should clear the incident (quality good for 10s)
    if (
      currentIncidentStart &&
      now - goodQualityStartTime > INCIDENT_RECOVERY_THRESHOLD
    ) {
      currentIncidentStart = null;
      goodQualityStartTime = null;
      dismissToast("connection-critical");
      dismissToast("connection-warning");
    }
  }
}

/**
 * Start the metrics collection loop.
 * Collects local WebRTC stats every 3 seconds and sends to server.
 * Safe to call multiple times - always clears existing interval first to prevent orphans.
 */
function startMetricsLoop(): void {
  // Always stop first to prevent orphaned intervals from rapid join/leave
  stopMetricsLoop();

  metricsInterval = window.setInterval(async () => {
    const adapter = getVoiceAdapter();
    if (!adapter) return;

    try {
      const metrics = await adapter.getConnectionMetrics();
      if (metrics) {
        setVoiceState("localMetrics", metrics);

        // Check packet loss thresholds and show notifications
        checkPacketLossThresholds(metrics);

        // Send to server
        const sessionId = voiceState.sessionId;
        if (sessionId && voiceState.channelId) {
          tauri.wsSend({
            type: "voice_stats",
            channel_id: voiceState.channelId,
            session_id: sessionId,
            latency: metrics.latency,
            packet_loss: metrics.packetLoss,
            jitter: metrics.jitter,
            quality: qualityToNumber(metrics.quality),
            timestamp: metrics.timestamp,
          });
        }
      } else {
        setVoiceState("localMetrics", "unknown");
      }
    } catch (err) {
      console.warn("Failed to collect metrics:", err);
    }
  }, 3000);
}

/**
 * Stop the metrics collection loop.
 */
function stopMetricsLoop(): void {
  if (metricsInterval) {
    clearInterval(metricsInterval);
    metricsInterval = null;
  }
}

// Tab visibility handling - pause metrics when tab is hidden to save resources
if (typeof document !== "undefined") {
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) {
      stopMetricsLoop();
    } else if (voiceState.state === "connected" && !metricsInterval) {
      startMetricsLoop();
    }
  });
}

/**
 * Stop voice background work (metrics collection).
 */
export async function cleanupVoice(): Promise<void> {
  // Stop metrics collection to prevent orphaned intervals
  stopMetricsLoop();
}

/**
 * Browser mode: a stored device that no longer exists (unplugged, or the
 * browser rotated its id) is cleared so the next join starts clean
 * on the system default. Transient failures (mic busy, permission denied)
 * keep the saved choice. Native keeps its own device bookkeeping.
 */
async function forgetStoredDevice(
  key: "input_device" | "output_device",
): Promise<void> {
  if (isTauri()) return;
  try {
    await updateAudioSetting(key, null);
  } catch (err) {
    console.warn("[Voice] Failed to clear stale device setting:", err);
  }
}

/**
 * Browser mode keeps the noise-suppression preference only in app settings
 * (the native backend owns its own copy). Applied BEFORE joining so the
 * microphone is opened with the stored value after a page reload.
 */
export async function applyStoredNoiseSuppression(
  adapter: VoiceAdapter,
): Promise<void> {
  const stored = appSettings()?.audio?.noise_suppression;
  if (isTauri() || typeof stored !== "boolean") return;
  if (
    typeof adapter.isNoiseSuppressionEnabled === "function" &&
    adapter.isNoiseSuppressionEnabled() === stored
  ) {
    return;
  }
  await adapter.setNoiseSuppression(stored);
}

/**
 * Apply the input/output devices stored in settings to the voice adapter.
 * Called on every voice join (browser: before joining, so the adapter only
 * records them; native: after joining). Failures are non-fatal (the system default
 * device keeps working) but are logged for diagnosis. Exported for tests.
 */
export async function applyStoredAudioDevices(
  adapter: VoiceAdapter,
): Promise<void> {
  const audio = appSettings()?.audio;
  if (!audio) return;

  if (audio.input_device) {
    const result = await adapter.setInputDevice(audio.input_device);
    if (!result.ok) {
      console.warn(
        "[Voice] Failed to apply stored input device:",
        result.error,
      );
      if (result.error.type === "device_not_found") {
        await forgetStoredDevice("input_device");
      }
    }
  }
  if (audio.output_device) {
    const result = await adapter.setOutputDevice(audio.output_device);
    if (!result.ok) {
      console.warn(
        "[Voice] Failed to apply stored output device:",
        result.error,
      );
      if (result.error.type === "device_not_found") {
        await forgetStoredDevice("output_device");
      }
    }
  }
}

/**
 * Route the adapter's events into this store (every join). The connection
 * state and its loss go to the voice owner, which reports them per session
 * (`channelHooks`); events that name a session count only for the one held.
 */
function installAdapterHandlers(adapter: VoiceAdapter): void {
  adapter.setEventHandlers({
    onError: (error, session) => {
      if (session && !isSameSession(session, heldSession())) return;
      setVoiceState({ error });
    },
    onRemoteTrack: (remoteTrack, session) => {
      // Tauri mode — audio plays natively via CPAL, no browser Audio needed
      if (!remoteTrack.stream) return;
      // Audio of a session this store is not in is not played.
      if (session && !isSameSession(session, heldSession())) return;

      console.log("[Voice] Remote audio track received:", remoteTrack.userId);
      // Create an audio element to play the remote user's audio.
      // It must be in the DOM with data-stream-id so the browser
      // adapter's setOutputDevice() can find it for setSinkId.
      const audio = new Audio();
      audio.srcObject = remoteTrack.stream;
      audio.autoplay = true;
      audio.dataset.streamId = remoteTrack.stream.id;
      audio.style.display = "none";
      document.body.appendChild(audio);
      // Route to the speaker the adapter remembers (default otherwise);
      // a speaker that no longer exists is forgotten in settings too.
      void adapter.applyOutputDeviceTo?.(audio).then(async (result) => {
        if (!result.ok && result.error.type === "device_not_found") {
          await forgetStoredDevice("output_device");
        }
      });
      // Remove old element for this user if exists
      const old = remoteAudioElements.get(remoteTrack.userId);
      if (old) {
        old.srcObject = null;
        old.remove();
      }
      remoteAudioElements.set(remoteTrack.userId, audio);
      audio.play().catch((err) => {
        console.warn("[Voice] Audio autoplay blocked:", err);
      });
    },
    onRemoteTrackRemoved: (userId) => {
      console.log("[Voice] Remote audio track removed:", userId);
      const el = remoteAudioElements.get(userId);
      if (el) {
        el.srcObject = null;
        el.remove();
        remoteAudioElements.delete(userId);
      }
    },
    onLocalMuteChange: (muted) => {
      setVoiceState({ muted });
    },
    onSpeakingChange: (speaking) => {
      setVoiceState({ speaking });
    },
    onAudioError: (audioError) => {
      // A stream stopping while leaving or while the recovery drops the
      // old connection is not news.
      if (voiceState.state === "disconnected" || voiceOwner.isRecovering(channelWish)) return;
      // Native stream died (unplugged, stalled, could not open): never let
      // the call look healthy with a dead mic. Stays until audio works
      // again (onAudioRecovered), it is dismissed, or the call ends.
      const capture = audioError.kind === "capture_failed";
      showToast({
        type: "error",
        title: t(capture ? "voice.audio.captureFailed" : "voice.audio.playbackFailed"),
        message: audioError.device,
        duration: 0,
        id: capture ? AUDIO_CAPTURE_TOAST_ID : AUDIO_PLAYBACK_TOAST_ID,
      });
    },
    onAudioRecovered: (recovered) => {
      dismissToast(
        recovered.kind === "capture_failed" ? AUDIO_CAPTURE_TOAST_ID : AUDIO_PLAYBACK_TOAST_ID,
      );
    },
    onScreenShareTrack: (userId, streamId, track) => {
      console.log("[Voice] Screen share track received:", userId, streamId);
      // Import and call viewer store — register track (auto-views if nothing active)
      import("@/stores/screenShareViewer").then(
        ({ addAvailableTrack }) => {
          const shareInfo = voiceState.screenShares.find(
            (s) => s.stream_id === streamId,
          );
          const username = shareInfo?.username || userId.slice(0, 8);
          const sourceLabel = shareInfo?.source_label || "Screen";
          addAvailableTrack(streamId, track, userId, username, sourceLabel);
        },
      );
    },
    onScreenShareTrackRemoved: (userId, streamId) => {
      console.log("[Voice] Screen share track removed:", userId, streamId);
      import("@/stores/screenShareViewer").then(
        ({ removeAvailableTrack }) => {
          removeAvailableTrack(streamId);
        },
      );
    },
    onScreenShareStopped: (_userId, _streamId, reason) => {
      console.log("[Voice] Screen share stopped:", reason);
      // Sync local state only when no more streams remain
      if (!adapter.isScreenSharing()) {
        setVoiceState({ screenSharing: false });
      }
    },
    onWebcamTrack: (userId, track) => {
      console.log("[Voice] Webcam track received:", userId);
      import("@/stores/webcamViewer").then(({ addAvailableTrack }) => {
        addAvailableTrack(userId, track);
      });
    },
    onWebcamTrackRemoved: (userId) => {
      console.log("[Voice] Webcam track removed:", userId);
      import("@/stores/webcamViewer").then(({ removeAvailableTrack }) => {
        removeAvailableTrack(userId);
      });
    },
  });
}

/**
 * What the user sees when the channel session is over (left, replaced,
 * removed, given up). Synchronous: the screen follows the click at once;
 * the voice owner ends the adapter session in the background.
 */
function showVoiceLeft(): void {
  stopMetricsLoop();
  // Reset notification state and dismiss any active connection toasts
  currentIncidentStart = null;
  goodQualityStartTime = null;
  dismissToast("connection-critical");
  dismissToast("connection-warning");
  removeRemoteAudioElements();
  resetVoiceSession();
}

/** Channel voice as the voice owner runs it (see `VoiceHooks`). */
function channelHooks(channelId: string): VoiceHooks {
  /** Browser: the microphone stored in settings when the join began. */
  let storedInput: string | null = null;
  return {
    prepare: async (adapter) => {
      installAdapterHandlers(adapter);
      await applyStoredNoiseSuppression(adapter);
      // Browser mode: hand the stored devices over BEFORE joining. Outside a
      // call the adapter only records them, so join opens the stored mic
      // directly (falling back to the default mic if it is gone) and remote
      // audio arriving later uses the stored speaker.
      storedInput = isTauri() ? null : (appSettings()?.audio?.input_device ?? null);
      if (!isTauri()) await applyStoredAudioDevices(adapter);
    },
    established: async (adapter) => {
      if (isTauri()) {
        // Native: apply the audio devices configured in settings — without
        // this the stored selection never reaches the adapter and voice
        // always uses the system default devices.
        await applyStoredAudioDevices(adapter);
      } else if (
        storedInput &&
        typeof adapter.getInputDevice === "function" &&
        adapter.getInputDevice() === null
      ) {
        // Join found the stored mic gone and used the default instead.
        await forgetStoredDevice("input_device");
      }
      // Activate PTT/PTM if configured (also handles VAD disable when PTT active)
      await activatePtt();
    },
    live: (adapter) => {
      // Start session tracking and metrics collection
      setVoiceState("sessionId", crypto.randomUUID());
      setVoiceState("connectedAt", Date.now());
      startMetricsLoop();
      // Apply VAD config only if PTT didn't take over
      if (!isPttActive()) syncVadConfig(adapter);
    },
    media: (state) => {
      // A short outage ("reconnecting") keeps every audio element (A8):
      // when ICE recovers, the same streams play on. Only an ended
      // connection removes them.
      if (state === "disconnected") removeRemoteAudioElements();
      setVoiceState({ state: storeStateFor(state) });
    },
    recovering: () => {
      setVoiceState({ state: "reconnecting" });
      stopMetricsLoop();
    },
    attemptStarts: () => {
      // The old connection is gone: its audio, shares and webcam with it
      // (screen share and webcam are not restarted, C32).
      removeRemoteAudioElements();
      import("@/stores/webcamViewer").then(({ clearAll }) => clearAll());
      setVoiceState({ screenSharing: false, webcamActive: false });
    },
    rejoined: async (adapter) => {
      // Deafened stays deafened from the first remote track on (native
      // plays tracks that arrive before "connected", W1-M10).
      if (voiceState.deafened) await adapter.setDeafen(true);
    },
    restore: async (adapter) => {
      // The latest choice wins: mute/deafen may have changed while
      // reconnecting (R2).
      const deafened = voiceState.deafened;
      await adapter.setDeafen(deafened);
      await adapter.setMute(voiceState.muted || deafened);
      if (isTauri()) {
        // Native keeps no device choice across the silent leave.
        await applyStoredAudioDevices(adapter);
      }
    },
    recovered: (adapter) => {
      setVoiceState({
        state: "connected",
        muted: voiceState.muted || voiceState.deafened,
        error: null,
        sessionId: crypto.randomUUID(),
        connectedAt: Date.now(),
      });
      startMetricsLoop();
      if (!isPttActive()) syncVadConfig(adapter);
      console.info("[Voice] Rejoined voice channel:", channelId);
    },
    failed: (error) => {
      setVoiceState({ state: "disconnected", channelId: null, error });
    },
    gaveUp: () => {
      showVoiceLeft();
      // A failed attempt's error is not news once the call is over (W1-M2).
      setVoiceState({ lostChannelId: channelId, error: null });
    },
    // Something else took the voice session (a DM call): the channel is left.
    superseded: showVoiceLeft,
    released: async () => {
      // After the adapter session ended: with PTT active this restores open
      // mic locally only (the adapter has no channel any more).
      await deactivatePtt();
      // After the streams stopped, so an error raised while leaving is cleared too.
      dismissToast(AUDIO_CAPTURE_TOAST_ID);
      dismissToast(AUDIO_PLAYBACK_TOAST_ID);
    },
  };
}

/**
 * Join a voice channel: the channel becomes the voice session the user
 * wants, replacing whatever was wanted before. The screen switches at once;
 * the voice owner ends the old session completely and then joins this one,
 * unless the user has changed their mind again by then. Resolves when the
 * join is through or was replaced; throws when it failed.
 */
export async function joinVoice(channelId: string): Promise<void> {
  // Joining, connected or reconnecting in this very channel (R2): one
  // session is enough.
  if (voiceOwner.wants(channelWish) && channelWish?.id === channelId) {
    console.info("[Voice] Already in this channel, join ignored");
    return;
  }
  removedChannels.delete(channelId);
  const wish = voiceOwner.wish("channel", channelId, channelHooks(channelId));
  channelWish = wish;
  const outcome = voiceOwner.request(wish);
  setVoiceState({ state: "connecting", channelId, error: null, lostChannelId: null });

  const result = await outcome;
  if (result.type === "failed") throw new Error(getErrorMessage(result.error));
}

/**
 * Leave the current voice channel. The screen shows "left" at once; the
 * promise resolves when the voice owner has ended the session.
 */
export async function leaveVoice(): Promise<void> {
  setVoiceState({ lostChannelId: null });
  const wish = channelWish;
  if (voiceOwner.wants(wish) || voiceState.state !== "disconnected") showVoiceLeft();
  // Only this store's own wish is dropped; a DM call that took over since
  // is not this leave's to end.
  await voiceOwner.drop(wish);
}

/** Clear everything about the current call (state stays `disconnected`). */
function resetVoiceSession(): void {
  // Clear webcam viewer tracks
  import("@/stores/webcamViewer").then(({ clearAll }) => clearAll());

  setVoiceState({
    state: "disconnected",
    channelId: null,
    participants: {},
    speaking: false,
    screenSharing: false,
    screenShares: [],
    webcamActive: false,
    webcams: [],
    sessionId: null,
    connectedAt: null,
    localMetrics: null,
    participantMetrics: new Map(),
  });
}

/** One line of text per `voice_removed` reason (C31); unknown = kicked. */
const VOICE_REMOVED_TEXT = {
  kicked: "voice.removed.kicked",
  banned: "voice.removed.banned",
  permission_lost: "voice.removed.permissionLost",
  channel_deleted: "voice.removed.channelDeleted",
  session_replaced: "voice.removed.sessionReplaced",
} as const;

export function voiceRemovedText(reason: string): string {
  const key =
    VOICE_REMOVED_TEXT[reason as keyof typeof VOICE_REMOVED_TEXT] ??
    VOICE_REMOVED_TEXT.kicked;
  return t(key);
}

/**
 * Channels the server removed us from (C31) since the last own join there.
 */
const removedChannels = new Set<string>();

/** Whether `voice_removed` arrived for this channel since the last own join. */
export function wasRemovedFrom(channelId: string): boolean {
  return removedChannels.has(channelId);
}

/**
 * C31 `voice_removed`: the server dropped this socket's voice peer. The
 * channel session is over: the voice owner ends it WITHOUT `voice_leave`
 * and never rejoins it; the store says why in one line. A removal for a
 * channel the user is no longer in changes nothing they see. Returns
 * whether the removal was for the channel voice the user is in.
 */
export async function handleVoiceRemoved(
  channelId: string,
  reason: string,
): Promise<boolean> {
  // Whatever is still held for that channel goes without `voice_leave`:
  // the server already removed the peer.
  voiceOwner.serverRemoved(channelId);
  const wish = channelWish;
  if (!wish || wish.id !== channelId || !voiceOwner.wants(wish)) return false;
  removedChannels.add(channelId);
  showVoiceLeft();
  const ended = voiceOwner.drop(wish, { silent: true });

  showToast({
    type: "warning",
    title: voiceRemovedText(reason),
    duration: 8000,
    id: "voice-removed",
  });
  await ended;
  return true;
}

// ============================================================================
// Rejoin after a lost connection (C32) — run by the voice owner
// ============================================================================

/** Wait before each rejoin attempt (C32: at most 3, spaced 1 s, 3 s, 9 s). */
export const REJOIN_DELAYS_MS = RECOVERY_DELAYS_MS;
/** An attempt fails when the publisher is not connected after this long. */
export const REJOIN_ATTEMPT_TIMEOUT_MS = RECOVERY_ATTEMPT_TIMEOUT_MS;

/** Whether the channel session is being recovered (exported for tests). */
export function isVoiceRejoinRunning(): boolean {
  return voiceOwner.isRecovering(channelWish);
}

/** Attempts the running recovery has used up (exported for tests). */
export function voiceRejoinAttemptsUsed(): number {
  return voiceOwner.isRecovering(channelWish) ? voiceOwner.attemptsUsed : 0;
}

/**
 * The signalling socket dropped. The server removes this user's voice peer
 * when the socket closes, so an established session shows "reconnecting"
 * and waits for the next authenticated socket (at most 60 s, see
 * `voiceOwner.ts`). Covers channel voice and DM-call voice alike.
 */
export function noteSignallingLost(): void {
  voiceOwner.socketLost();
}

/** The socket is open again (before re-subscription). */
export function noteSignallingUp(): void {
  voiceOwner.socketOpen();
}

/**
 * C32 trigger: the socket is back and the text re-subscription finished.
 * The voice owner recovers the session the user is still in (not left, not
 * removed). Resolves when no recovery is running any more.
 */
export function rejoinVoiceAfterReconnect(): Promise<void> {
  voiceOwner.socketReady();
  return voiceOwner.whenCalm();
}

/** The user dismissed the "Rejoin" offer. */
export function dismissVoiceRejoin(): void {
  setVoiceState({ lostChannelId: null });
}

function getPttConfig(): PttFullConfig | null {
  const settings = appSettings();
  if (!settings) return null;

  const { voice } = settings;
  if (!voice.push_to_talk && !voice.push_to_mute) return null;

  return {
    pttEnabled: voice.push_to_talk,
    pttKey: voice.push_to_talk_key,
    pttReleaseDelay: voice.push_to_talk_release_delay,
    ptmEnabled: voice.push_to_mute,
    ptmKey: voice.push_to_mute_key,
    ptmReleaseDelay: voice.push_to_mute_release_delay,
  };
}

let pttActivating = false;

async function activatePtt(): Promise<void> {
  // Guard against concurrent activation from rapid setting changes
  if (pttActivating) return;
  pttActivating = true;
  try {
    await deactivatePtt();

    const config = getPttConfig();
    if (!config) return;

    pttController = new PttController(setMute);
    pttController.activate(config);
    pttCleanup = await createTauriPttListeners(pttController, config);

    // PTT takes priority over VAD — disable gating while PTT controls mute
    const adapter = getVoiceAdapter();
    if (adapter) {
      const settings = appSettings();
      adapter.setVadConfig(false, settings?.voice.vad_threshold ?? 0.5);
    }
  } finally {
    pttActivating = false;
  }
}

async function deactivatePtt(): Promise<void> {
  const wasActive = pttController !== null && pttController.isPttOrPtmEnabled();
  if (pttCleanup) {
    const fn = pttCleanup;
    pttCleanup = null;
    await Promise.resolve(fn());
  }
  if (pttController) {
    pttController.deactivate();
    pttController = null;
  }
  // Restore open-mic when PTT/PTM is deactivated so the user isn't left
  // silently muted (PTT rest state is muted).
  if (wasActive) {
    await setMute(false);
  }

  // Restore VAD config from settings
  const adapter = getVoiceAdapter();
  if (adapter) syncVadConfig(adapter);
}

/**
 * Push current VAD settings to the adapter.
 * Skipped when PTT/PTM is controlling the mute state.
 */
function syncVadConfig(adapter: import("@/lib/webrtc/types").VoiceAdapter): void {
  const settings = appSettings();
  if (!settings) return;

  const isPtt = pttController !== null && pttController.isPttOrPtmEnabled();
  const vadEnabled = !isPtt && settings.voice.voice_activity_detection;
  adapter.setVadConfig(vadEnabled, settings.voice.vad_threshold);
}

/** Re-sync PTT state when settings change mid-call. */
export async function updatePttFromSettings(): Promise<void> {
  if (voiceState.state !== "connected") return;
  await activatePtt();
}

/** Re-sync VAD config when settings change mid-call. */
export function updateVadFromSettings(): void {
  if (voiceState.state !== "connected") return;
  const adapter = getVoiceAdapter();
  if (adapter) syncVadConfig(adapter);
}

/** Whether PTT or PTM is currently controlling the mute state. */
export function isPttActive(): boolean {
  return pttController !== null && pttController.isPttOrPtmEnabled();
}

/**
 * While reconnecting (R2) mute/deafen stay usable: the store keeps the
 * user's latest choice, the adapter applies it locally where it can, and
 * the next successful attempt applies it to the new connection.
 */
function reconnecting(): boolean {
  return voiceState.state === "reconnecting";
}

/** Run an adapter call; while reconnecting a failure only gets logged. */
async function applyToAdapter(
  what: string,
  call: (adapter: VoiceAdapter) => Promise<{ ok: boolean; error?: VoiceError }>,
): Promise<boolean> {
  try {
    const adapter = await createVoiceAdapter();
    const result = await call(adapter);
    if (result.ok) return true;
    if (reconnecting()) {
      console.warn(`[Voice] ${what} kept for the reconnect:`, result.error);
      return true;
    }
    console.error(`Failed to ${what}:`, result.error);
    setVoiceState({ error: result.error ?? null });
    return false;
  } catch (err) {
    if (reconnecting()) {
      console.warn(`[Voice] ${what} kept for the reconnect:`, err);
      return true;
    }
    throw err;
  }
}

/**
 * Toggle mute state.
 */
export async function toggleMute(): Promise<void> {
  const newMuted = !voiceState.muted;
  if (reconnecting()) setVoiceState({ muted: newMuted });
  await applyToAdapter("toggle mute", (adapter) => adapter.setMute(newMuted));
}

/**
 * Toggle deafen state.
 */
export async function toggleDeafen(): Promise<void> {
  const newDeafened = !voiceState.deafened;
  const next = {
    deafened: newDeafened,
    // Deafening also mutes
    muted: newDeafened ? true : voiceState.muted,
  };
  if (reconnecting()) setVoiceState(next);
  if (await applyToAdapter("toggle deafen", (adapter) => adapter.setDeafen(newDeafened))) {
    setVoiceState(next);
  }
}

/**
 * Set mute state directly.
 */
export async function setMute(muted: boolean): Promise<void> {
  const adapter = await createVoiceAdapter();
  const result = await adapter.setMute(muted);

  if (!result.ok) {
    console.error("Failed to set mute:", result.error);
    setVoiceState({ error: result.error });
  }
}

/**
 * Set deafen state directly.
 */
export async function setDeafen(deafened: boolean): Promise<void> {
  const adapter = await createVoiceAdapter();
  const result = await adapter.setDeafen(deafened);

  if (!result.ok) {
    console.error("Failed to set deafen:", result.error);
    setVoiceState({ error: result.error });
  } else {
    setVoiceState({ deafened, muted: deafened ? true : voiceState.muted });
  }
}

/**
 * Manually set speaking state.
 * Used by mic test UI (AudioDeviceSettings) to drive the speaking indicator.
 * During live calls, speaking state is driven by the adapter's onSpeakingChange event.
 */
export function setSpeaking(speaking: boolean): void {
  setVoiceState({ speaking });
}

/**
 * Start screen sharing.
 *
 * @param quality - Quality tier (low/medium/high/premium)
 * @param sourceId - Native capture source ID (Tauri only, from ScreenShareSourcePicker)
 */
export async function startScreenShare(
  quality?: ScreenShareQuality,
  sourceId?: string,
): Promise<{ ok: boolean; error?: string }> {
  if (voiceState.state !== "connected" || !voiceState.channelId) {
    return { ok: false, error: t("voice.error.notConnected") };
  }

  // Generate a stream ID for this screen share
  const streamId = crypto.randomUUID();
  // The share belongs to this session; the picker can take long.
  const session = heldSession();
  const channelId = voiceState.channelId;

  const adapter = await createVoiceAdapter();

  // Start the capture (native source for Tauri, browser getDisplayMedia for web)
  const result = await adapter.startScreenShare({ quality, sourceId, streamId });

  if (!result.ok) {
    console.error("Failed to start screen share:", result.error);
    return { ok: false, error: getErrorMessage(result.error) };
  }
  if (!isSameSession(session, heldSession()) || voiceState.state !== "connected") {
    await adapter.stopScreenShare(streamId);
    return { ok: false, error: t("voice.error.notConnected") };
  }

  // Get actual screen share info from the adapter
  const shareInfo = adapter.getScreenShareInfo(streamId);
  const hasAudio = shareInfo?.hasAudio ?? false;
  const sourceLabel = shareInfo?.sourceLabel ?? "Screen";

  // Notify server about screen share start
  try {
    await tauri.wsScreenShareStart(
      channelId,
      streamId,
      quality || "medium",
      hasAudio,
      sourceLabel,
    );
  } catch (err) {
    console.error("Failed to notify server of screen share start:", err);
    // Stop capture since server notification failed
    await adapter.stopScreenShare(streamId);
    return { ok: false, error: t("voice.error.notifyFailed") };
  }

  // Register our own capture track for self-preview. The SFU forwards our track
  // to OTHER subscribers, never back to us, so our own tile would otherwise have
  // no track and render blank. The browser adapter exposes the local track; the
  // native adapter returns null (it renders its own preview), so this is a no-op
  // there.
  const localTrack = adapter.getScreenShareTrack(streamId);
  if (localTrack) {
    const me = currentUser();
    const { addAvailableTrack } = await import("@/stores/screenShareViewer");
    addAvailableTrack(
      streamId,
      localTrack,
      me?.id ?? "self",
      me?.display_name || me?.username || "You",
      sourceLabel,
    );
  }

  setVoiceState({ screenSharing: true });
  return { ok: true };
}

/**
 * Stop a specific screen share stream, or all streams if no streamId given.
 */
export async function stopScreenShare(streamId?: string): Promise<void> {
  if (!voiceState.screenSharing) return;

  const channelId = voiceState.channelId;
  const adapter = await createVoiceAdapter();

  // Remove our own self-preview track(s) from the viewer. Stopping a track
  // programmatically doesn't reliably fire `onended`, so clear it explicitly.
  const { removeAvailableTrack } = await import("@/stores/screenShareViewer");

  if (streamId) {
    // Stop a specific stream
    const result = await adapter.stopScreenShare(streamId);
    if (!result.ok) {
      console.error("Failed to stop screen share:", result.error);
    }
    removeAvailableTrack(streamId);

    // Notify server about this specific stream stop
    if (channelId) {
      try {
        await tauri.wsScreenShareStop(channelId, streamId);
      } catch (err) {
        console.error("Failed to notify server of screen share stop:", err);
        showToast({
          type: "warning",
          title: t("voice.toast.shareStopped.title"),
          message: t("voice.toast.shareStopped.message"),
          duration: 5000,
          id: "screen-share-stop-warning",
        });
      }
    }

    // If no more streams are active, clear sharing state
    if (!adapter.isScreenSharing()) {
      setVoiceState({ screenSharing: false });
    }
  } else {
    // Stop all streams
    const allInfo = adapter.getAllScreenShareInfo();
    const result = await adapter.stopAllScreenShares();
    if (!result.ok) {
      console.error("Failed to stop all screen shares:", result.error);
    }
    for (const info of allInfo) removeAvailableTrack(info.streamId);

    // Notify server about each stream stop
    if (channelId) {
      for (const info of allInfo) {
        try {
          await tauri.wsScreenShareStop(channelId, info.streamId);
        } catch (err) {
          console.error("Failed to notify server of screen share stop:", err);
        }
      }
    }

    setVoiceState({ screenSharing: false });
  }
}

/**
 * Start webcam.
 *
 * @param quality - Quality tier (low/medium/high/premium)
 * @param deviceId - Camera device ID
 */
export async function startWebcam(
  quality?: ScreenShareQuality,
  deviceId?: string,
): Promise<{ ok: boolean; error?: string }> {
  if (voiceState.state !== "connected" || !voiceState.channelId) {
    return { ok: false, error: t("voice.error.notConnected") };
  }

  if (voiceState.webcamActive) {
    return { ok: false, error: t("voice.error.webcamActive") };
  }

  const session = heldSession();
  const channelId = voiceState.channelId;
  const adapter = await createVoiceAdapter();

  const result = await adapter.startWebcam({ quality, deviceId });

  if (!result.ok) {
    console.error("Failed to start webcam:", result.error);
    return { ok: false, error: getErrorMessage(result.error) };
  }
  if (!isSameSession(session, heldSession()) || voiceState.state !== "connected") {
    await adapter.stopWebcam();
    return { ok: false, error: t("voice.error.notConnected") };
  }

  // Notify server about webcam start
  try {
    await tauri.wsWebcamStart(channelId, quality || "medium");
  } catch (err) {
    console.error("Failed to notify server of webcam start:", err);
    await adapter.stopWebcam();
    return { ok: false, error: t("voice.error.notifyFailed") };
  }

  setVoiceState({ webcamActive: true });
  return { ok: true };
}

/**
 * Stop webcam.
 */
export async function stopWebcam(): Promise<void> {
  if (!voiceState.webcamActive) return;

  const channelId = voiceState.channelId;

  const adapter = await createVoiceAdapter();
  const result = await adapter.stopWebcam();

  if (!result.ok) {
    console.error("Failed to stop webcam:", result.error);
  }

  // Notify server about webcam stop
  if (channelId) {
    try {
      await tauri.wsWebcamStop(channelId);
    } catch (err) {
      console.error("Failed to notify server of webcam stop:", err);
    }
  }

  setVoiceState({ webcamActive: false });
}

/**
 * Get list of participants.
 */
export function getParticipants(): VoiceParticipant[] {
  return Object.values(voiceState.participants);
}

/**
 * Check if connected to voice.
 */
export function isInVoice(): boolean {
  return voiceState.state === "connected";
}

/**
 * Check if in a specific channel (connected or reconnecting).
 */
export function isInChannel(channelId: string): boolean {
  // Reconnecting is still the same voice session (R2).
  return (
    (voiceState.state === "connected" || voiceState.state === "reconnecting") &&
    voiceState.channelId === channelId
  );
}

/**
 * Get current voice channel information
 * Returns null if not connected
 *
 * @note This is a derived helper to decouple VoiceIsland from channelsState.
 * When Phase 3 arrives and channels become guild-scoped, only this function
 * needs updating instead of every component that displays channel info.
 */
export function getVoiceChannelInfo(): { id: string; name: string } | null {
  if (!voiceState.channelId) return null;

  const channel = channelsState.channels.find(
    (c: { id: string }) => c.id === voiceState.channelId,
  );

  if (!channel) {
    return { id: voiceState.channelId, name: t("voice.channel.unknown") };
  }

  return { id: channel.id, name: channel.name };
}

/**
 * Get local connection metrics.
 * Returns null if not connected, 'unknown' if metrics unavailable.
 */
export function getLocalMetrics(): ConnectionMetrics | "unknown" | null {
  return voiceState.localMetrics;
}

/**
 * Get metrics for a specific participant.
 */
export function getParticipantMetrics(
  userId: string,
): ParticipantMetrics | undefined {
  return voiceState.participantMetrics.get(userId);
}

/**
 * Handle incoming voice_user_stats event from server.
 * Updates participant metrics in the store.
 */
export function handleVoiceUserStats(data: {
  channel_id: string;
  user_id: string;
  latency: number;
  packet_loss: number;
  jitter: number;
  quality: number;
}): void {
  const { channel_id, user_id, latency, packet_loss, jitter, quality } = data;

  // Only update if we're in the same channel
  if (channel_id !== voiceState.channelId) return;

  const newMetrics = new Map(voiceState.participantMetrics);
  newMetrics.set(user_id, {
    userId: user_id,
    latency,
    packetLoss: packet_loss,
    jitter,
    quality: numberToQuality(quality),
  });
  setVoiceState("participantMetrics", newMetrics);
}

// Export the store for reading and writing
// ============================================================================
// System tray (desktop)
// ============================================================================

/** Event the native tray emits for its "Leave voice" / "Mute" items (`tray.rs`). */
const TRAY_VOICE_ACTION_EVENT = "tray-voice-action";

/**
 * Voice state as the tray sees it. Connecting counts as in voice: the mic may
 * already be open, so a close during the join must still warn.
 */
export function trayVoiceReport(
  state: VoiceState,
  muted: boolean,
): { inVoice: boolean; muted: boolean } {
  return { inVoice: state !== "disconnected", muted };
}

/** Run a tray menu voice action (payload of `tray-voice-action`). */
export async function handleTrayVoiceAction(action: string): Promise<void> {
  if (action === "leave") {
    await leaveVoice();
  } else if (action === "toggle_mute" && voiceState.state !== "disconnected") {
    await toggleMute();
  }
}

/** Tray event handler: an event callback, not a reactive scope. */
function onTrayVoiceAction(event: { payload: string }): void {
  void handleTrayVoiceAction(event.payload);
}

/**
 * Keep the native tray in sync with the call: reports in-voice/muted via
 * `set_tray_voice_state` and runs the tray's voice menu items. Must be called
 * inside a component context (createEffect/onCleanup); a no-op in the browser.
 */
export function initTrayVoiceSync(): void {
  if (!isTauri()) return;

  createEffect(() => {
    const report = trayVoiceReport(voiceState.state, voiceState.muted);
    invoke("set_tray_voice_state", report).catch((err) => {
      console.warn("[Tray] Failed to report voice state:", err);
    });
  });

  let disposed = false;
  let unlisten: (() => void) | null = null;
  listen<string>(TRAY_VOICE_ACTION_EVENT, onTrayVoiceAction)
    .then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    })
    .catch((err) => {
      console.warn("[Tray] Failed to listen for tray voice actions:", err);
    });

  onCleanup(() => {
    disposed = true;
    unlisten?.();
  });
}

export { voiceState, setVoiceState };
