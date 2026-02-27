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
} from "./types";
import * as Sentry from "@sentry/browser";

export class TauriVoiceAdapter implements VoiceAdapter {
  private state: VoiceConnectionState = "disconnected";
  private channelId: string | null = null;
  private muted = false;
  private deafened = false;
  private noiseSuppression = false;
  private screenSharing = false;
  private webcamActive = false;

  // Screen share state (native Rust capture)
  private screenShareSourceName: string | null = null;
  private screenShareWithAudio = false;

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

    // Clean up screen share if active (native capture is stopped via Rust command)
    if (this.screenSharing) {
      try {
        await invoke("stop_screen_share");
      } catch {
        // Best effort — leave_voice will also clean up
      }
      this.screenSharing = false;
      this.screenShareSourceName = null;
      this.screenShareWithAudio = false;
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
      // Noise suppression might not be implemented in Tauri backend yet
      // Fall back to just storing the state locally
      console.warn(
        "[TauriVoiceAdapter] Noise suppression not implemented in backend",
      );
      this.noiseSuppression = enabled;
      return { ok: true, value: undefined };
    }
  }

  // Signaling

  async handleOffer(
    channelId: string,
    sdp: string,
  ): Promise<VoiceResult<string>> {
    console.log(`[TauriVoiceAdapter] Handling offer for channel: ${channelId}`);

    try {
      await invoke("handle_voice_offer", { channelId, sdp });
      // Note: The answer is sent automatically by the Tauri backend
      return { ok: true, value: "" };
    } catch (err) {
      return { ok: false, error: this.mapTauriError(err) };
    }
  }

  async handleIceCandidate(
    channelId: string,
    candidate: string,
  ): Promise<VoiceResult<void>> {
    const startTime = performance.now();

    try {
      await invoke("handle_voice_ice_candidate", { channelId, candidate });

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
    // TODO: Add Tauri command to fetch native WebRTC connection stats
    // Blocked on connectivity monitoring feature
    return null;
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
    return this.screenSharing;
  }

  /**
   * Get information about the current screen share.
   * Returns null if not sharing.
   */
  getScreenShareInfo(): { hasAudio: boolean; sourceLabel: string } | null {
    if (!this.screenSharing) {
      return null;
    }

    return {
      hasAudio: this.screenShareWithAudio,
      sourceLabel: this.screenShareSourceName || "Screen",
    };
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

    if (this.screenSharing) {
      return {
        ok: false,
        error: { type: "unknown", message: "Already sharing screen" },
      };
    }

    if (!options?.sourceId) {
      return {
        ok: false,
        error: { type: "unknown", message: "No source selected" },
      };
    }

    try {
      await invoke("start_screen_share", {
        sourceId: options.sourceId,
        quality: options.quality ?? "medium",
        withAudio: options.withAudio ?? false,
      });

      this.screenSharing = true;
      this.screenShareWithAudio = options.withAudio ?? false;
      // Fetch source name from status
      try {
        const status = await invoke<{ source_name: string } | null>(
          "get_screen_share_status",
        );
        this.screenShareSourceName = status?.source_name ?? "Screen";
      } catch {
        this.screenShareSourceName = "Screen";
      }

      console.log("[TauriVoiceAdapter] Native screen share started");
      return { ok: true, value: undefined };
    } catch (err) {
      console.error("[TauriVoiceAdapter] Failed to start screen share:", err);
      return { ok: false, error: this.mapTauriError(err) };
    }
  }

  async stopScreenShare(): Promise<VoiceResult<void>> {
    console.log("[TauriVoiceAdapter] Stopping native screen share");

    if (!this.screenSharing) {
      return {
        ok: false,
        error: { type: "unknown", message: "Not sharing screen" },
      };
    }

    try {
      await invoke("stop_screen_share");

      this.screenSharing = false;
      this.screenShareSourceName = null;
      this.screenShareWithAudio = false;

      console.log("[TauriVoiceAdapter] Screen share stopped");
      return { ok: true, value: undefined };
    } catch (err) {
      console.error("[TauriVoiceAdapter] Failed to stop screen share:", err);
      return { ok: false, error: this.mapTauriError(err) };
    }
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

    // Clean up screen share if active (fire-and-forget)
    if (this.screenSharing) {
      invoke("stop_screen_share").catch(() => {});
      this.screenSharing = false;
      this.screenShareSourceName = null;
      this.screenShareWithAudio = false;
    }

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

    // Listen for remote tracks
    this.unlisteners.push(
      await listen<string>("voice:remote_track", (event) => {
        console.log(`[TauriVoiceAdapter] Remote track: ${event.payload}`);
        // Remote track handling is done in Rust, just log here
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
