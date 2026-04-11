/**
 * Client app settings, UI state, pins, and favorites types.
 */

// Settings Types

export interface AudioSettings {
  input_device: string | null;
  output_device: string | null;
  input_volume: number;
  output_volume: number;
  noise_suppression: boolean;
  echo_cancellation: boolean;
}

export interface VoiceSettings {
  push_to_talk: boolean;
  push_to_talk_key: string | null;
  push_to_talk_release_delay: number;
  push_to_mute: boolean;
  push_to_mute_key: string | null;
  push_to_mute_release_delay: number;
  voice_activity_detection: boolean;
  vad_threshold: number;
}

export interface AppSettings {
  audio: AudioSettings;
  voice: VoiceSettings;
  theme: "dark" | "light";
  notifications_enabled: boolean;
}

export interface UiState {
  category_collapse: Record<string, boolean>;
}
