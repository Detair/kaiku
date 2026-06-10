/**
 * Tauri Voice Adapter
 *
 * Wrapper that delegates to Tauri commands (native Rust implementation)
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  VoiceAdapter,
  VoiceConnectionState,
  VoiceError,
  VoiceResult,
  VoiceAdapterEvents,
  AudioDeviceList,
  ScreenShareOptions,
  WebcamOptions,
  ConnectionMetrics,
  CaptureSource,
  RawConnectionStats,
} from "./types";
import { rawStatsToMetrics } from "./types";
import * as Sentry from "@sentry/browser";

export class TauriVoiceAdapter implements VoiceAdapter {
  private state: VoiceConnectionState = "disconnected";
  private channelId: string | null = null;
  private muted = false;
  private deafened = false;
  private noiseSuppression = false;
  private webcamActive = false;

  // Screen share state (native Rust capture) — multi-stream
  private activeScreenShares: Map<
    string,
    { sourceName: string; withAudio: boolean }
  > = new Map();

  // Event handlers
  private eventHandlers: Partial<VoiceAdapterEvents> = {};
  private unlisteners: UnlistenFn[] = [];

  constructor() {
    console.log("[TauriVoiceAdapter] Initialized");
    this.setupEventListeners();
  }

  // Lifecycle methods

  async join(channelId: string): Promise<VoiceResult<void>> {
    console.log(`[TauriVoiceAdapter] Joining channel: ${channelId}`);

    try {
      await invoke("join_voice", { channelId });
      this.channelId = channelId;
      this.setState("connecting");
      return { ok: true, value: undefined };
    } catch (err) {
      return { ok: false, error: this.mapTauriError(err) };
    }
  }

  async leave(): Promise<VoiceResult<void>> {
    console.log("[TauriVoiceAdapter] Leaving voice");

    // Clean up all screen shares (native capture is stopped via Rust command)
    if (this.activeScreenShares.size > 0) {
      for (const streamId of [...this.activeScreenShares.keys()]) {
        try {
          await invoke("stop_screen_share", { streamId });
        } catch {
          // Best effort — leave_voice will also clean up
        }
      }
      this.activeScreenShares.clear();
    }

    try {
      await invoke("leave_voice");
      this.channelId = null;
      this.setState("disconnected");
      return { ok: true, value: undefined };
    } catch (err) {
      return { ok: false, error: this.mapTauriError(err) };
    }
  }

  // Audio control

  async setMute(muted: boolean): Promise<VoiceResult<void>> {
    console.log(`[TauriVoiceAdapter] Set mute: ${muted}`);

    try {
      await invoke("set_mute", { muted });
      this.muted = muted;
      this.eventHandlers.onLocalMuteChange?.(muted);
      return { ok: true, value: undefined };
    } catch (err) {
      return { ok: false, error: this.mapTauriError(err) };
    }
  }

  async setDeafen(deafened: boolean): Promise<VoiceResult<void>> {
    console.log(`[TauriVoiceAdapter] Set deafen: ${deafened}`);

    try {
      await invoke("set_deafen", { deafened });
      this.deafened = deafened;

      // Also mute when deafened
      if (deafened && !this.muted) {
        await this.setMute(true);
      }

      return { ok: true, value: undefined };
    } catch (err) {
      return { ok: false, error: this.mapTauriError(err) };
    }
  }

  async setNoiseSuppression(enabled: boolean): Promise<VoiceResult<void>> {
    console.log(`[TauriVoiceAdapter] Set noise suppression: ${enabled}`);

    try {
      await invoke("set_noise_suppression", { enabled });
      this.noiseSuppression = enabled;
      return { ok: true, value: undefined };
    } catch (err) {
      console.error("[TauriVoiceAdapter] Failed to set noise suppression:", err);
      return { ok: false, error: { type: "unknown", message: String(err) } };
    }
  }

  setVadConfig(enabled: boolean, threshold: number): void {
    invoke("set_vad_config", { enabled, threshold }).catch((err) =>
      console.error("[TauriVoiceAdapter] Failed to set VAD config:", err),
    );
  }

  // Signaling — dual PeerConnection model
  // Note: Tauri backend handles WebRTC internally; these are thin wrappers

  async handlePublisherAnswer(
    channelId: string,
    sdp: string,
  ): Promise<VoiceResult<void>> {
    console.log(`[TauriVoiceAdapter] Handling publisher answer for channel: ${channelId}`);

    try {
      await invoke("handle_voice_publisher_answer", { channelId, sdp });
      return { ok: true, value: undefined };
    } catch (err) {
      return { ok: false, error: this.mapTauriError(err) };
    }
  }

  async handleSubscriberOffer(
    channelId: string,
    sdp: string,
  ): Promise<VoiceResult<string>> {
    console.log(`[TauriVoiceAdapter] Handling subscriber offer for channel: ${channelId}`);

    try {
      const answer = await invoke<string>("handle_voice_subscriber_offer", { channelId, sdp });
      return { ok: true, value: answer };
    } catch (err) {
      return { ok: false, error: this.mapTauriError(err) };
    }
  }

  async handleIceCandidate(
    channelId: string,
    candidate: string,
    pcType: string = "publisher",
  ): Promise<VoiceResult<void>> {
    const startTime = performance.now();

    try {
      await invoke("handle_voice_ice_candidate", { channelId, candidate, pcType });

      const elapsed = performance.now() - startTime;
      console.log(
        `[TauriVoiceAdapter] ICE candidate processed (${elapsed.toFixed(2)}ms)`,
      );

      return { ok: true, value: undefined };
    } catch (err) {
      const elapsed = performance.now() - startTime;
      console.error(
        `[TauriVoiceAdapter] ICE candidate failed after ${elapsed.toFixed(2)}ms:`,
        err,
      );

      return { ok: false, error: this.mapTauriError(err) };
    }
  }

  // State methods

  getState(): VoiceConnectionState {
    return this.state;
  }

  getChannelId(): string | null {
    return this.channelId;
  }

  isMuted(): boolean {
    return this.muted;
  }

  isDeafened(): boolean {
    return this.deafened;
  }

  isNoiseSuppressionEnabled(): boolean {
    return this.noiseSuppression;
  }

  async getConnectionMetrics(): Promise<ConnectionMetrics | null> {
    try {
      const raw = await invoke<RawConnectionStats | null>(
        "voice_connection_stats",
      );
      if (!raw) return null;
      return rawStatsToMetrics(raw, Date.now());
    } catch (err) {
      console.warn("[TauriVoiceAdapter] Failed to fetch native stats:", err);
      return null;
    }
  }

  setEventHandlers(handlers: Partial<VoiceAdapterEvents>): void {
    this.eventHandlers = { ...this.eventHandlers, ...handlers };
  }

  // Microphone test

  async startMicTest(deviceId?: string): Promise<VoiceResult<void>> {
    console.log("[TauriVoiceAdapter] Starting mic test");

    try {
      await invoke("start_mic_test", { deviceId: deviceId || null });
      return { ok: true, value: undefined };
    } catch (err) {
      return { ok: false, error: this.mapTauriError(err) };
    }
  }

  async stopMicTest(): Promise<VoiceResult<void>> {
    console.log("[TauriVoiceAdapter] Stopping mic test");

    try {
      await invoke("stop_mic_test");
      return { ok: true, value: undefined };
    } catch (err) {
      return { ok: false, error: this.mapTauriError(err) };
    }
  }

  getMicTestLevel(): number {
    // Synchronous call not supported in Tauri, need to poll
    // This should be called periodically from the UI
    return 0; // Will be updated by event
  }

  // Device enumeration

  async getAudioDevices(): Promise<VoiceResult<AudioDeviceList>> {
    try {
      const devices = await invoke<AudioDeviceList>("get_audio_devices");
      return { ok: true, value: devices };
    } catch (err) {
      return { ok: false, error: this.mapTauriError(err) };
    }
  }

  async setInputDevice(deviceId: string): Promise<VoiceResult<void>> {
    console.log(`[TauriVoiceAdapter] Setting input device: ${deviceId}`);

    try {
      await invoke("set_input_device", { deviceId });
      return { ok: true, value: undefined };
    } catch (err) {
      return { ok: false, error: this.mapTauriError(err) };
    }
  }

  async setOutputDevice(deviceId: string): Promise<VoiceResult<void>> {
    console.log(`[TauriVoiceAdapter] Setting output device: ${deviceId}`);

    try {
      await invoke("set_output_device", { deviceId });
      return { ok: true, value: undefined };
    } catch (err) {
      return { ok: false, error: this.mapTauriError(err) };
    }
  }

  // Screen sharing (native Rust capture via Tauri commands)

  isScreenSharing(): boolean {
    return this.activeScreenShares.size > 0;
  }

  /**
   * Get information about a screen share by stream ID.
   * Returns null if not sharing or stream not found.
   */
  getScreenShareInfo(streamId?: string): { streamId: string; hasAudio: boolean; sourceLabel: string } | null {
    if (this.activeScreenShares.size === 0) {
      return null;
    }

    if (streamId) {
      const info = this.activeScreenShares.get(streamId);
      if (!info) return null;
      return {
        streamId,
        hasAudio: info.withAudio,
        sourceLabel: info.sourceName,
      };
    }

    // Return first share if no streamId specified
    const [firstId, firstInfo] = [...this.activeScreenShares.entries()][0];
    return {
      streamId: firstId,
      hasAudio: firstInfo.withAudio,
      sourceLabel: firstInfo.sourceName,
    };
  }

  /**
   * Get info for all active screen shares.
   */
  getAllScreenShareInfo(): { streamId: string; hasAudio: boolean; sourceLabel: string }[] {
    return [...this.activeScreenShares.entries()].map(([id, info]) => ({
      streamId: id,
      hasAudio: info.withAudio,
      sourceLabel: info.sourceName,
    }));
  }

  /**
   * Get the video track for a local screen share.
   * Tauri uses native capture — no MediaStreamTrack available.
   */
  getScreenShareTrack(_streamId: string): MediaStreamTrack | null {
    return null;
  }

  /**
   * Enumerate native capture sources (monitors and windows).
   */
  async enumerateCaptureSources(): Promise<CaptureSource[] | null> {
    try {
      return await invoke<CaptureSource[]>("enumerate_capture_sources");
    } catch (err) {
      console.error(
        "[TauriVoiceAdapter] Failed to enumerate capture sources:",
        err,
      );
      return null;
    }
  }

  async startScreenShare(
    options?: ScreenShareOptions,
  ): Promise<VoiceResult<void>> {
    console.log("[TauriVoiceAdapter] Starting native screen share", options);

    if (this.activeScreenShares.size >= 3) {
      return {
        ok: false,
        error: { type: "unknown", message: "Maximum of 3 concurrent screen shares reached" },
      };
    }

    if (!options?.sourceId) {
      return {
        ok: false,
        error: { type: "unknown", message: "No source selected" },
      };
    }

    const streamId = options.streamId ?? crypto.randomUUID();

    try {
      await invoke("start_screen_share", {
        sourceId: options.sourceId,
        quality: options.quality ?? "medium",
        withAudio: options.withAudio ?? false,
        streamId,
      });

      // Fetch source name from status
      let sourceName = "Screen";
      try {
        const statuses = await invoke<{ stream_id: string; source_name: string }[]>(
          "get_screen_share_status",
        );
        const thisStatus = statuses.find((s) => s.stream_id === streamId);
        sourceName = thisStatus?.source_name ?? "Screen";
      } catch {
        // Fall through with default
      }

      this.activeScreenShares.set(streamId, {
        sourceName,
        withAudio: options.withAudio ?? false,
      });

      console.log(`[TauriVoiceAdapter] Native screen share started: ${streamId}`);
      return { ok: true, value: undefined };
    } catch (err) {
      console.error("[TauriVoiceAdapter] Failed to start screen share:", err);
      return { ok: false, error: this.mapTauriError(err) };
    }
  }

  async stopScreenShare(streamId?: string): Promise<VoiceResult<void>> {
    if (this.activeScreenShares.size === 0) {
      return {
        ok: false,
        error: { type: "unknown", message: "Not sharing screen" },
      };
    }

    // If no streamId given, stop the first one
    const targetId = streamId ?? [...this.activeScreenShares.keys()][0];
    console.log(`[TauriVoiceAdapter] Stopping native screen share: ${targetId}`);

    try {
      await invoke("stop_screen_share", { streamId: targetId });

      this.activeScreenShares.delete(targetId);

      console.log(`[TauriVoiceAdapter] Screen share stopped: ${targetId}`);
      return { ok: true, value: undefined };
    } catch (err) {
      console.error("[TauriVoiceAdapter] Failed to stop screen share:", err);
      return { ok: false, error: this.mapTauriError(err) };
    }
  }

  async stopAllScreenShares(): Promise<VoiceResult<void>> {
    const streamIds = [...this.activeScreenShares.keys()];
    for (const streamId of streamIds) {
      try {
        await invoke("stop_screen_share", { streamId });
      } catch (err) {
        console.error(`[TauriVoiceAdapter] Failed to stop screen share ${streamId}:`, err);
      }
    }
    this.activeScreenShares.clear();
    return { ok: true, value: undefined };
  }

  // Webcam (delegates to Tauri Rust backend)

  isWebcamActive(): boolean {
    return this.webcamActive;
  }

  async startWebcam(options?: WebcamOptions): Promise<VoiceResult<void>> {
    console.log("[TauriVoiceAdapter] Starting webcam", options);

    if (this.webcamActive) {
      return {
        ok: false,
        error: { type: "unknown", message: "Webcam already active" },
      };
    }

    try {
      await invoke("start_webcam", {
        quality: options?.quality ?? "medium",
        deviceId: options?.deviceId ?? null,
      });
      this.webcamActive = true;
      console.log("[TauriVoiceAdapter] Webcam started");
      return { ok: true, value: undefined };
    } catch (err) {
      console.error("[TauriVoiceAdapter] Failed to start webcam:", err);
      return { ok: false, error: this.mapTauriError(err) };
    }
  }

  async stopWebcam(): Promise<VoiceResult<void>> {
    console.log("[TauriVoiceAdapter] Stopping webcam");

    if (!this.webcamActive) {
      return {
        ok: false,
        error: { type: "unknown", message: "Webcam not active" },
      };
    }

    try {
      await invoke("stop_webcam");
      this.webcamActive = false;
      console.log("[TauriVoiceAdapter] Webcam stopped");
      return { ok: true, value: undefined };
    } catch (err) {
      console.error("[TauriVoiceAdapter] Failed to stop webcam:", err);
      return { ok: false, error: this.mapTauriError(err) };
    }
  }

  // Cleanup

  dispose(): void {
    console.log("[TauriVoiceAdapter] Disposing");

    // Clean up webcam if active (fire-and-forget)
    if (this.webcamActive) {
      invoke("stop_webcam").catch(() => {});
      this.webcamActive = false;
    }

    // Clean up all screen shares (fire-and-forget)
    for (const streamId of this.activeScreenShares.keys()) {
      invoke("stop_screen_share", { streamId }).catch(() => {});
    }
    this.activeScreenShares.clear();

    this.unlisteners.forEach((unlisten) => unlisten());
    this.unlisteners = [];
  }

  // Private helper methods

  private async setupEventListeners() {
    // Listen for voice state changes
    this.unlisteners.push(
      await listen<string>("voice:state_change", (event) => {
        console.log(`[TauriVoiceAdapter] State change: ${event.payload}`);
        // Map Rust ConnectionState to VoiceConnectionState
        const stateMap: Record<string, VoiceConnectionState> = {
          Disconnected: "disconnected",
          Connecting: "connecting",
          Connected: "connected",
          Failed: "disconnected",
        };
        const newState = stateMap[event.payload] || "disconnected";
        if (event.payload === "Connected") {
          Sentry.addBreadcrumb({ category: "voice", message: "voice_connected_native", data: { channel_id: this.channelId ?? "" }, level: "info" });
        }
        this.setState(newState);
      }),
    );

    // Remote audio track events
    this.unlisteners.push(
      await listen<{ user_id: string; source_type: string }>("voice:remote_track", (event) => {
        console.log(`[TauriVoiceAdapter] Remote track: ${event.payload.user_id}:${event.payload.source_type}`);
        // The existing onRemoteTrack callback expects a RemoteTrack object.
        // In Tauri mode, audio plays natively via CPAL — the frontend only needs
        // the presence info so the UI shows the participant.
        this.eventHandlers.onRemoteTrack?.({
          trackId: `${event.payload.user_id}:${event.payload.source_type}`,
          userId: event.payload.user_id,
          stream: null, // No MediaStream in Tauri — audio plays natively via CPAL
          muted: false,
        });
      }),
    );

    this.unlisteners.push(
      await listen<{ user_id: string }>("voice:remote_track_removed", (event) => {
        console.log(`[TauriVoiceAdapter] Remote track removed: ${event.payload.user_id}`);
        this.eventHandlers.onRemoteTrackRemoved?.(event.payload.user_id);
      }),
    );

    // Screen share track events
    this.unlisteners.push(
      await listen<{ user_id: string; stream_id: string; source_type: string }>("voice:screen_share_track", (event) => {
        console.log(`[TauriVoiceAdapter] Screen share track: ${event.payload.user_id}:${event.payload.stream_id}`);
        this.eventHandlers.onScreenShareTrack?.(
          event.payload.user_id,
          event.payload.stream_id,
          null,
        );
      }),
    );

    this.unlisteners.push(
      await listen<{ user_id: string; stream_id: string; source_type: string }>("voice:screen_share_track_removed", (event) => {
        console.log(`[TauriVoiceAdapter] Screen share removed: ${event.payload.user_id}:${event.payload.stream_id}`);
        this.eventHandlers.onScreenShareTrackRemoved?.(event.payload.user_id, event.payload.stream_id);
      }),
    );

    // Webcam track events
    this.unlisteners.push(
      await listen<{ user_id: string; stream_id: string; source_type: string }>("voice:webcam_track", (event) => {
        console.log(`[TauriVoiceAdapter] Webcam track: ${event.payload.user_id}`);
        this.eventHandlers.onWebcamTrack?.(event.payload.user_id, null);
      }),
    );

    this.unlisteners.push(
      await listen<{ user_id: string; stream_id: string; source_type: string }>("voice:webcam_track_removed", (event) => {
        console.log(`[TauriVoiceAdapter] Webcam removed: ${event.payload.user_id}`);
        this.eventHandlers.onWebcamTrackRemoved?.(event.payload.user_id);
      }),
    );

    // VAD speaking indicator
    this.unlisteners.push(
      await listen<boolean>("voice:speaking", (event) => {
        this.eventHandlers.onSpeakingChange?.(event.payload);
      }),
    );
  }

  private setState(state: VoiceConnectionState) {
    this.state = state;
    this.eventHandlers.onStateChange?.(state);
  }

  private mapTauriError(err: unknown): VoiceError {
    const message = typeof err === "string" ? err : String(err);

    // Try to parse common error patterns
    if (message.includes("permission") || message.includes("denied")) {
      return {
        type: "permission_denied",
        message: "Microphone access denied",
      };
    }

    if (message.includes("not found") || message.includes("device")) {
      return {
        type: "device_not_found",
        message: "Audio device not found",
      };
    }

    if (message.includes("in use")) {
      return {
        type: "device_in_use",
        message: "Audio device is in use",
      };
    }

    if (message.includes("Already in a voice channel")) {
      return {
        type: "already_connected",
        channelId: this.channelId || "unknown",
      };
    }

    if (
      message.includes("Not connected") ||
      message.includes("not initialized")
    ) {
      return {
        type: "not_connected",
      };
    }

    return {
      type: "unknown",
      message,
    };
  }
}
