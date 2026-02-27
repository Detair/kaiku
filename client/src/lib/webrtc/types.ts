/**
 * VoiceAdapter Types and Interfaces
 *
 * Defines the interface that both browser and Tauri voice implementations must follow.
 */

import type { QualityLevel } from "../types";

/**
 * Voice connection states
 */
export type VoiceConnectionState =
  | "disconnected"
  | "requesting_media" // Getting microphone access
  | "connecting" // WebRTC handshake in progress
  | "connected"
  | "reconnecting";

/**
 * Explicit error types for voice operations
 */
export type VoiceError =
  | { type: "permission_denied"; message: string }
  | { type: "device_not_found"; message: string }
  | { type: "device_in_use"; message: string }
  | { type: "connection_failed"; reason: string; retriable: boolean }
  | { type: "server_rejected"; code: string; message: string }
  | { type: "ice_failed"; message: string }
  | { type: "timeout"; operation: string }
  | { type: "already_connected"; channelId: string }
  | { type: "not_connected" }
  | { type: "cancelled"; message: string }
  | { type: "not_found"; message: string }
  | { type: "hardware_error"; message: string }
  | { type: "constraint_error"; message: string }
  | { type: "unknown"; message: string };

/**
 * Result type for voice operations
 */
export type VoiceResult<T> =
  | { ok: true; value: T }
  | { ok: false; error: VoiceError };

/**
 * Remote audio track info
 */
export interface RemoteTrack {
  trackId: string; // Unique track identifier
  userId: string; // Who this track belongs to
  stream: MediaStream; // Audio stream for playback
  muted: boolean; // Remote user's mute state
}

/**
 * Voice adapter events
 */
export interface VoiceAdapterEvents {
  onStateChange: (state: VoiceConnectionState) => void;
  onError: (error: VoiceError) => void;
  onRemoteTrack: (track: RemoteTrack) => void;
  onRemoteTrackRemoved: (userId: string) => void;
  onLocalMuteChange: (muted: boolean) => void;
  onSpeakingChange: (speaking: boolean) => void;
  onIceCandidate: (candidate: string) => void;

  // Screen share events
  onScreenShareStarted?: (info: ScreenShareInfo) => void;
  onScreenShareStopped?: (userId: string, reason: string) => void;
  onScreenShareTrack?: (userId: string, track: MediaStreamTrack) => void;
  onScreenShareTrackRemoved?: (userId: string) => void;

  // Webcam events
  onWebcamTrack?: (userId: string, track: MediaStreamTrack) => void;
  onWebcamTrackRemoved?: (userId: string) => void;
}

/**
 * Audio device information
 */
export interface AudioDevice {
  deviceId: string;
  label: string;
  isDefault: boolean;
}

export interface AudioDeviceList {
  inputs: AudioDevice[]; // Microphones
  outputs: AudioDevice[]; // Speakers/Headphones
}

/**
 * Screen share quality tier
 */
export type ScreenShareQuality = "low" | "medium" | "high" | "premium";

/**
 * Options for starting a screen share
 */
export interface ScreenShareOptions {
  /**
   * Source ID from `enumerateCaptureSources`.
   * Required for the Tauri native adapter. Ignored by the browser adapter (uses `getDisplayMedia`).
   */
  sourceId?: string;
  quality?: ScreenShareQuality;
  withAudio?: boolean;
}

/**
 * A native capture source (monitor or window).
 * Returned by the Tauri `enumerate_capture_sources` command.
 */
export interface CaptureSource {
  id: string;
  name: string;
  source_type: "monitor" | "window";
  thumbnail: string | null;
  is_primary: boolean;
}

/**
 * Result of a screen share attempt
 */
export type ScreenShareResult =
  | { approved: true; stream: MediaStream }
  | {
      approved: false;
      reason: "user_cancelled" | "permission_denied" | "no_source";
    };

/**
 * Information about an active screen share
 */
export interface ScreenShareInfo {
  user_id: string;
  username: string;
  source_label: string;
  has_audio: boolean;
  quality: ScreenShareQuality;
  started_at: string;
}

/**
 * Pre-capture permission check result
 */
export interface ScreenShareCheckResult {
  allowed: boolean;
  granted_quality: ScreenShareQuality;
  error?: "no_permission" | "limit_reached" | "not_in_channel";
}

/**
 * Options for starting a webcam
 */
export interface WebcamOptions {
  quality?: ScreenShareQuality;
  deviceId?: string;
}

// Re-export QualityLevel from shared types for convenience
export type { QualityLevel } from "../types";

/**
 * Connection metrics from WebRTC stats
 */
export interface ConnectionMetrics {
  latency: number; // RTT in ms
  packetLoss: number; // 0-100 percentage
  jitter: number; // ms
  quality: QualityLevel;
  timestamp: number;
}

/**
 * Per-participant connection metrics
 */
export interface ParticipantMetrics {
  userId: string;
  latency: number;
  packetLoss: number;
  jitter: number;
  quality: QualityLevel;
}

/**
 * VoiceAdapter interface - implemented by both browser and Tauri
 *
 * Ensures feature parity between implementations and allows
 * future MLS E2EE integration as a drop-in replacement.
 */
export interface VoiceAdapter {
  // Lifecycle
  join(channelId: string): Promise<VoiceResult<void>>;
  leave(): Promise<VoiceResult<void>>;

  // Audio control
  setMute(muted: boolean): Promise<VoiceResult<void>>;
  setDeafen(deafened: boolean): Promise<VoiceResult<void>>;
  setNoiseSuppression(enabled: boolean): Promise<VoiceResult<void>>;

  // Signaling (called by WebSocket store)
  handleOffer(channelId: string, sdp: string): Promise<VoiceResult<string>>; // Returns answer SDP
  handleIceCandidate(
    channelId: string,
    candidate: string,
  ): Promise<VoiceResult<void>>;

  // State
  getState(): VoiceConnectionState;
  getChannelId(): string | null;
  isMuted(): boolean;
  isDeafened(): boolean;
  isNoiseSuppressionEnabled(): boolean;

  /** Get current connection metrics from WebRTC stats */
  getConnectionMetrics(): Promise<ConnectionMetrics | null>;

  // Event registration
  setEventHandlers(handlers: Partial<VoiceAdapterEvents>): void;

  // Microphone Test (local only, no server connection)
  startMicTest(deviceId?: string): Promise<VoiceResult<void>>;
  stopMicTest(): Promise<VoiceResult<void>>;
  getMicTestLevel(): number; // Returns 0-100 volume level

  // Device enumeration
  getAudioDevices(): Promise<VoiceResult<AudioDeviceList>>;
  setInputDevice(deviceId: string): Promise<VoiceResult<void>>;
  setOutputDevice(deviceId: string): Promise<VoiceResult<void>>;

  // Screen sharing
  startScreenShare(options?: ScreenShareOptions): Promise<VoiceResult<void>>;
  stopScreenShare(): Promise<VoiceResult<void>>;
  isScreenSharing(): boolean;
  /** Get info about current screen share (hasAudio, sourceLabel). Returns null if not sharing. */
  getScreenShareInfo(): { hasAudio: boolean; sourceLabel: string } | null;
  /** Enumerate native capture sources (Tauri only). Returns null if not supported. */
  enumerateCaptureSources?(): Promise<CaptureSource[] | null>;

  // Webcam
  startWebcam(options?: WebcamOptions): Promise<VoiceResult<void>>;
  stopWebcam(): Promise<VoiceResult<void>>;
  isWebcamActive(): boolean;

  // Cleanup
  dispose(): void;
}
