/**
 * Voice participant, webcam, screen share, and call state types.
 */

export interface VoiceParticipant {
  user_id: string;
  username?: string;
  display_name?: string;
  muted: boolean;
  speaking: boolean;
  screen_sharing: boolean;
  webcam_active?: boolean;
}

export interface WebcamServerInfo {
  user_id: string;
  username: string;
  quality: "low" | "medium" | "high" | "premium";
}

export interface ScreenShareServerInfo {
  stream_id: string;
  user_id: string;
  username: string;
  source_label: string;
  has_audio: boolean;
  quality: "low" | "medium" | "high" | "premium";
  started_at: string;
}

// Call State Types

export type CallEndReason =
  | "cancelled"
  | "all_declined"
  | "no_answer"
  | "last_left";

export interface CallStateResponse {
  channel_id: string;
  status: "ringing" | "active" | "ended";
  started_by?: string;
  started_at?: string;
  declined_by?: string[];
  target_users?: string[];
  participants?: string[];
  reason?: CallEndReason;
  duration_secs?: number;
  ended_at?: string;
  capabilities?: string[];
}
