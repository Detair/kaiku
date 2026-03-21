/**
 * Voice Store
 *
 * Manages voice channel state including connection, participants, and audio settings.
 */

import { createStore, produce } from "solid-js/store";
import {
  createVoiceAdapter,
  getVoiceAdapter,
  type VoiceError,
  type ConnectionMetrics,
  type ParticipantMetrics,
  type QualityLevel,
} from "@/lib/webrtc";
import type { ScreenShareInfo, ScreenShareQuality } from "@/lib/webrtc/types";
import type { VoiceParticipant, WebcamServerInfo } from "@/lib/types";
import { channelsState } from "@/stores/channels";
import { appSettings } from "@/stores/settings";
import * as tauri from "@/lib/tauri";
import { showToast, dismissToast } from "@/components/ui/Toast";
import { PttController, createTauriPttListeners, type PttFullConfig } from "@/lib/pttManager";

// Detect if running in Tauri
const isTauri = typeof window !== "undefined" && "__TAURI__" in window;

// Type for unlisten function
type UnlistenFn = () => void;

// Helper to get error message from VoiceError
function getErrorMessage(error: VoiceError): string {
  switch (error.type) {
    case "permission_denied":
    case "device_not_found":
    case "device_in_use":
    case "ice_failed":
    case "cancelled":
    case "not_found":
    case "hardware_error":
    case "constraint_error":
    case "unknown":
      return error.message;
    case "server_rejected":
      return `Server rejected: ${error.message} (${error.code})`;
    case "connection_failed":
      return `Connection failed: ${error.reason}`;
    case "timeout":
      return `Timeout during ${error.operation}`;
    case "already_connected":
      return `Already connected to channel ${error.channelId}`;
    case "not_connected":
      return "Not connected to voice channel";
  }
}

// Voice connection state
type VoiceState = "disconnected" | "connecting" | "connected";

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

// Event listeners
let unlisteners: UnlistenFn[] = [];

// Metrics collection interval
let metricsInterval: number | null = null;

// Guard against concurrent join calls from rapid clicks
let isJoining = false;

// Notification state for packet loss incidents
let currentIncidentStart: number | null = null;
let goodQualityStartTime: number | null = null;
const INCIDENT_RECOVERY_THRESHOLD = 10_000; // 10s of good quality to clear incident

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
          title: "Connection severely degraded",
          message: `${metrics.packetLoss.toFixed(1)}% packet loss`,
          duration: 0,
          id: "connection-critical",
        });
      } else {
        // Warning level packet loss
        showToast({
          type: "warning",
          title: "Your connection is unstable",
          message: `${metrics.packetLoss.toFixed(1)}% packet loss`,
          duration: 5000,
          id: "connection-warning",
        });
      }
    } else if (metrics.packetLoss >= 7) {
      // Escalate from warning to critical
      dismissToast("connection-warning");
      showToast({
        type: "error",
        title: "Connection severely degraded",
        message: `${metrics.packetLoss.toFixed(1)}% packet loss`,
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
 * Initialize voice event listeners.
 */
export async function initVoice(): Promise<void> {
  // Clean up existing listeners
  await cleanupVoice();

  // Tauri-specific event listeners
  if (!isTauri) {
    // Browser mode - WebSocket events are handled in websocket store
    return;
  }

  const { listen } = await import("@tauri-apps/api/event");

  // Voice user events
  unlisteners.push(
    await listen<{ channel_id: string; user_id: string }>(
      "ws:voice_user_joined",
      (event) => {
        const { channel_id, user_id } = event.payload;
        if (channel_id === voiceState.channelId) {
          addParticipant(user_id);
        }
      },
    ),
  );

  unlisteners.push(
    await listen<{ channel_id: string; user_id: string }>(
      "ws:voice_user_left",
      (event) => {
        const { channel_id, user_id } = event.payload;
        if (channel_id === voiceState.channelId) {
          removeParticipant(user_id);
        }
      },
    ),
  );

  unlisteners.push(
    await listen<{ channel_id: string; user_id: string }>(
      "ws:voice_user_muted",
      (event) => {
        const { channel_id, user_id } = event.payload;
        if (channel_id === voiceState.channelId) {
          updateParticipant(user_id, { muted: true });
        }
      },
    ),
  );

  unlisteners.push(
    await listen<{ channel_id: string; user_id: string }>(
      "ws:voice_user_unmuted",
      (event) => {
        const { channel_id, user_id } = event.payload;
        if (channel_id === voiceState.channelId) {
          updateParticipant(user_id, { muted: false });
        }
      },
    ),
  );

  unlisteners.push(
    await listen<{ channel_id: string; participants: VoiceParticipant[] }>(
      "ws:voice_room_state",
      (event) => {
        const { channel_id, participants } = event.payload;
        if (channel_id === voiceState.channelId) {
          setParticipants(participants);
        }
      },
    ),
  );

  unlisteners.push(
    await listen<{ code: string; message: string }>(
      "ws:voice_error",
      (event) => {
        console.error("Voice error:", event.payload);
        const error: VoiceError = {
          type: "server_rejected",
          code: event.payload.code,
          message: event.payload.message,
        };
        setVoiceState({ error });
      },
    ),
  );
}

/**
 * Cleanup voice listeners.
 */
export async function cleanupVoice(): Promise<void> {
  // Stop metrics collection to prevent orphaned intervals
  stopMetricsLoop();

  for (const unlisten of unlisteners) {
    unlisten();
  }
  unlisteners = [];
}

/**
 * Join a voice channel.
 */
export async function joinVoice(channelId: string): Promise<void> {
  if (isJoining) {
    console.warn("[Voice] Join already in progress, ignoring duplicate call");
    return;
  }
  isJoining = true;

  try {
    // Leave current channel if connected (client-side)
    if (voiceState.state !== "disconnected") {
      await leaveVoice();
    }

    // Always send voice_leave first to clean up any stale server session
    // (e.g., from a previous browser tab that didn't disconnect properly)
    try {
      await tauri.wsSend({ type: "voice_leave", channel_id: channelId });
    } catch {
      // Ignore — the user might not have been in a channel
    }

    setVoiceState({ state: "connecting", channelId, error: null });

    const adapter = await createVoiceAdapter();

    // Set up adapter event handlers
    adapter.setEventHandlers({
      onStateChange: (state) => {
        const stateMap = {
          disconnected: "disconnected" as const,
          requesting_media: "connecting" as const,
          connecting: "connecting" as const,
          connected: "connected" as const,
          reconnecting: "connecting" as const,
        };
        setVoiceState({ state: stateMap[state] });
      },
      onError: (error) => {
        setVoiceState({ error });
      },
      onLocalMuteChange: (muted) => {
        setVoiceState({ muted });
      },
      onSpeakingChange: (speaking) => {
        setVoiceState({ speaking });
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
      onIceCandidate: (candidate) => {
        import("@/lib/tauri").then(({ wsSend }) => {
          wsSend({
            type: "voice_ice_candidate",
            channel_id: channelId,
            candidate,
          });
        });
      },
    });

    const result = await adapter.join(channelId);
    if (!result.ok) {
      setVoiceState({
        state: "disconnected",
        channelId: null,
        error: result.error,
      });
      throw new Error(getErrorMessage(result.error));
    }

    // Start session tracking and metrics collection
    setVoiceState("sessionId", crypto.randomUUID());
    setVoiceState("connectedAt", Date.now());
    startMetricsLoop();

    // Activate PTT/PTM if configured
    await activatePtt();
  } finally {
    isJoining = false;
  }
}

/**
 * Leave the current voice channel.
 */
export async function leaveVoice(): Promise<void> {
  if (voiceState.state === "disconnected") return;

  // Stop metrics collection first
  stopMetricsLoop();

  // Deactivate PTT/PTM
  await deactivatePtt();

  // Reset notification state and dismiss any active connection toasts
  currentIncidentStart = null;
  goodQualityStartTime = null;
  dismissToast("connection-critical");
  dismissToast("connection-warning");

  const adapter = await createVoiceAdapter();
  const result = await adapter.leave();

  if (!result.ok) {
    console.error("Failed to leave voice:", result.error);
  }

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
}

/** Re-sync PTT state when settings change mid-call. */
export async function updatePttFromSettings(): Promise<void> {
  if (voiceState.state !== "connected") return;
  await activatePtt();
}

/** Whether PTT or PTM is currently controlling the mute state. */
export function isPttActive(): boolean {
  return pttController !== null && pttController.isPttOrPtmEnabled();
}

/**
 * Toggle mute state.
 */
export async function toggleMute(): Promise<void> {
  const newMuted = !voiceState.muted;
  const adapter = await createVoiceAdapter();
  const result = await adapter.setMute(newMuted);

  if (!result.ok) {
    console.error("Failed to toggle mute:", result.error);
    setVoiceState({ error: result.error });
  }
}

/**
 * Toggle deafen state.
 */
export async function toggleDeafen(): Promise<void> {
  const newDeafened = !voiceState.deafened;
  const adapter = await createVoiceAdapter();
  const result = await adapter.setDeafen(newDeafened);

  if (!result.ok) {
    console.error("Failed to toggle deafen:", result.error);
    setVoiceState({ error: result.error });
  } else {
    setVoiceState({
      deafened: newDeafened,
      // Deafening also mutes
      muted: newDeafened ? true : voiceState.muted,
    });
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
 * Set speaking state (temporary for testing until VAD is implemented).
 * @phase1 - Backend needs to implement Voice Activity Detection (VAD)
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
    return { ok: false, error: "Not connected to voice channel" };
  }

  // Generate a stream ID for this screen share
  const streamId = crypto.randomUUID();

  const adapter = await createVoiceAdapter();

  // Start the capture (native source for Tauri, browser getDisplayMedia for web)
  const result = await adapter.startScreenShare({ quality, sourceId, streamId });

  if (!result.ok) {
    console.error("Failed to start screen share:", result.error);
    return { ok: false, error: getErrorMessage(result.error) };
  }

  // Get actual screen share info from the adapter
  const shareInfo = adapter.getScreenShareInfo(streamId);
  const hasAudio = shareInfo?.hasAudio ?? false;
  const sourceLabel = shareInfo?.sourceLabel ?? "Screen";

  // Notify server about screen share start
  try {
    await tauri.wsScreenShareStart(
      voiceState.channelId,
      streamId,
      quality || "medium",
      hasAudio,
      sourceLabel,
    );
  } catch (err) {
    console.error("Failed to notify server of screen share start:", err);
    // Stop capture since server notification failed
    await adapter.stopScreenShare(streamId);
    return { ok: false, error: "Failed to notify server" };
  }

  setVoiceState({ screenSharing: true });

  // Register local screen share track so the user can view their own share
  const videoTrack = adapter.getScreenShareTrack(streamId);
  if (videoTrack) {
    const { currentUser } = await import("@/stores/auth");
    const user = currentUser();
    const { addAvailableTrack } = await import("@/stores/screenShareViewer");
    addAvailableTrack(streamId, videoTrack, user?.id || "", user?.username || "You", sourceLabel);
  }

  return { ok: true };
}

/**
 * Stop a specific screen share stream, or all streams if no streamId given.
 */
export async function stopScreenShare(streamId?: string): Promise<void> {
  if (!voiceState.screenSharing) return;

  const channelId = voiceState.channelId;
  const adapter = await createVoiceAdapter();

  if (streamId) {
    // Stop a specific stream
    const result = await adapter.stopScreenShare(streamId);
    if (!result.ok) {
      console.error("Failed to stop screen share:", result.error);
    }

    // Notify server about this specific stream stop
    if (channelId) {
      try {
        await tauri.wsScreenShareStop(channelId, streamId);
      } catch (err) {
        console.error("Failed to notify server of screen share stop:", err);
        showToast({
          type: "warning",
          title: "Screen share stopped locally",
          message: "Server may still show you as sharing. Reconnect if needed.",
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
    return { ok: false, error: "Not connected to voice channel" };
  }

  if (voiceState.webcamActive) {
    return { ok: false, error: "Webcam already active" };
  }

  const adapter = await createVoiceAdapter();

  const result = await adapter.startWebcam({ quality, deviceId });

  if (!result.ok) {
    console.error("Failed to start webcam:", result.error);
    return { ok: false, error: getErrorMessage(result.error) };
  }

  // Notify server about webcam start
  try {
    await tauri.wsWebcamStart(voiceState.channelId, quality || "medium");
  } catch (err) {
    console.error("Failed to notify server of webcam start:", err);
    await adapter.stopWebcam();
    return { ok: false, error: "Failed to notify server" };
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

// Participant management

function addParticipant(userId: string): void {
  setVoiceState(
    produce((state) => {
      state.participants[userId] = {
        user_id: userId,
        muted: false,
        speaking: false,
        screen_sharing: false,
      };
    }),
  );
}

function removeParticipant(userId: string): void {
  setVoiceState(
    produce((state) => {
      delete state.participants[userId];
    }),
  );
}

function updateParticipant(
  userId: string,
  update: Partial<VoiceParticipant>,
): void {
  setVoiceState(
    produce((state) => {
      if (state.participants[userId]) {
        Object.assign(state.participants[userId], update);
      }
    }),
  );
}

function setParticipants(participants: VoiceParticipant[]): void {
  setVoiceState(
    produce((state) => {
      state.participants = {};
      for (const p of participants) {
        state.participants[p.user_id] = p;
      }
    }),
  );
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
 * Check if connected to a specific channel.
 */
export function isInChannel(channelId: string): boolean {
  return voiceState.state === "connected" && voiceState.channelId === channelId;
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
    return { id: voiceState.channelId, name: "Unknown Channel" };
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
export { voiceState, setVoiceState };
