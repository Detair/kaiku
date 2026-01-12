/**
 * VoiceAdapter Types and Interfaces
 *
 * Defines the interface that both browser and Tauri voice implementations must follow.
 */

/**
 * Voice connection states
 */
export type VoiceConnectionState =
  | "disconnected"
  | "requesting_media"    // Getting microphone access
  | "connecting"          // WebRTC handshake in progress
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
  trackId: string;     // Unique track identifier
  userId: string;      // Who this track belongs to
  stream: MediaStream; // Audio stream for playback
  muted: boolean;      // Remote user's mute state
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
  inputs: AudioDevice[];   // Microphones
  outputs: AudioDevice[];  // Speakers/Headphones
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
  handleIceCandidate(channelId: string, candidate: string): Promise<VoiceResult<void>>;

  // State
  getState(): VoiceConnectionState;
  getChannelId(): string | null;
  isMuted(): boolean;
  isDeafened(): boolean;
  isNoiseSuppressionEnabled(): boolean;

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

  // Cleanup
  dispose(): void;
}
