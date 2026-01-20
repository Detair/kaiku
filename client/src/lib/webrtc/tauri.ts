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
  ConnectionMetrics,
} from "./types";

export class TauriVoiceAdapter implements VoiceAdapter {
  private state: VoiceConnectionState = "disconnected";
  private channelId: string | null = null;
  private muted = false;
  private deafened = false;
  private noiseSuppression = false;
  private screenSharing = false;

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
      console.warn("[TauriVoiceAdapter] Noise suppression not implemented in backend");
      this.noiseSuppression = enabled;
      return { ok: true, value: undefined };
    }
  }

  // Signaling

  async handleOffer(
    channelId: string,
    sdp: string
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
    candidate: string
  ): Promise<VoiceResult<void>> {
    const startTime = performance.now();

    try {
      await invoke("handle_voice_ice_candidate", { channelId, candidate });

      const elapsed = performance.now() - startTime;
      console.log(`[TauriVoiceAdapter] ICE candidate processed (${elapsed.toFixed(2)}ms)`);

      return { ok: true, value: undefined };
    } catch (err) {
      const elapsed = performance.now() - startTime;
      console.error(`[TauriVoiceAdapter] ICE candidate failed after ${elapsed.toFixed(2)}ms:`, err);

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
    // TODO: Implement via Tauri command to get native WebRTC stats
    // This will be implemented when the connectivity monitoring feature is complete
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

  // Screen sharing

  isScreenSharing(): boolean {
    return this.screenSharing;
  }

  async startScreenShare(options?: ScreenShareOptions): Promise<VoiceResult<void>> {
    console.log("[TauriVoiceAdapter] Starting screen share", options);

    try {
      await invoke("start_screen_share", {
        quality: options?.quality ?? "medium",
        withAudio: options?.withAudio ?? false,
      });
      this.screenSharing = true;
      return { ok: true, value: undefined };
    } catch (err) {
      console.error("[TauriVoiceAdapter] Failed to start screen share:", err);
      return { ok: false, error: this.mapTauriError(err) };
    }
  }

  async stopScreenShare(): Promise<VoiceResult<void>> {
    console.log("[TauriVoiceAdapter] Stopping screen share");

    try {
      await invoke("stop_screen_share");
      this.screenSharing = false;
      return { ok: true, value: undefined };
    } catch (err) {
      console.error("[TauriVoiceAdapter] Failed to stop screen share:", err);
      return { ok: false, error: this.mapTauriError(err) };
    }
  }

  // Cleanup

  dispose(): void {
    console.log("[TauriVoiceAdapter] Disposing");
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
        this.setState(newState);
      })
    );

    // Listen for remote tracks
    this.unlisteners.push(
      await listen<string>("voice:remote_track", (event) => {
        console.log(`[TauriVoiceAdapter] Remote track: ${event.payload}`);
        // Remote track handling is done in Rust, just log here
      })
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

    if (message.includes("Not connected") || message.includes("not initialized")) {
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
