/**
 * Shared Types for VoiceChat Client
 *
 * These types mirror the Rust types in shared/vc-common
 */

// User Types

export type UserStatus = "online" | "away" | "busy" | "offline";

export interface UserProfile {
  id: string;
  username: string;
  display_name: string;
  avatar_url: string | null;
  status: UserStatus;
}

export interface User extends UserProfile {
  email: string | null;
  mfa_enabled: boolean;
  created_at: string;
}

// Guild Types

export interface Guild {
  id: string;
  name: string;
  owner_id: string;
  icon_url: string | null;
  description: string | null;
  created_at: string;
}

export interface GuildMember {
  user_id: string;
  username: string;
  display_name: string;
  avatar_url: string | null;
  nickname: string | null;
  joined_at: string;
}

// Channel Types

export type ChannelType = "text" | "voice" | "dm";

export interface Channel {
  id: string;
  name: string;
  channel_type: ChannelType;
  category_id: string | null;
  guild_id: string | null;
  topic: string | null;
  user_limit: number | null;
  position: number;
  created_at: string;
}

export interface ChannelCategory {
  id: string;
  name: string;
  position: number;
  channels: Channel[];
}

// Message Types

export interface Attachment {
  id: string;
  filename: string;
  mime_type: string;
  size: number;
  url: string;
}

export interface Message {
  id: string;
  channel_id: string;
  author: UserProfile;
  content: string;
  encrypted: boolean;
  attachments: Attachment[];
  reply_to: string | null;
  edited_at: string | null;
  created_at: string;
}

// Voice Types

export interface VoiceParticipant {
  user_id: string;
  username?: string;
  display_name?: string;
  muted: boolean;
  speaking: boolean;
}

// Auth Types

export interface LoginRequest {
  server_url: string;
  username: string;
  password: string;
}

export interface RegisterRequest {
  server_url: string;
  username: string;
  email?: string;
  password: string;
  display_name?: string;
}

export interface TokenResponse {
  access_token: string;
  refresh_token: string;
  expires_in: number;
  token_type: string;
}

// WebSocket Events

export type ClientEvent =
  | { type: "ping" }
  | { type: "subscribe"; channel_id: string }
  | { type: "unsubscribe"; channel_id: string }
  | { type: "typing"; channel_id: string }
  | { type: "stop_typing"; channel_id: string }
  | { type: "voice_join"; channel_id: string }
  | { type: "voice_leave"; channel_id: string }
  | { type: "voice_answer"; channel_id: string; sdp: string }
  | { type: "voice_ice_candidate"; channel_id: string; candidate: string }
  | { type: "voice_mute"; channel_id: string }
  | { type: "voice_unmute"; channel_id: string };

export type ServerEvent =
  | { type: "ready"; user_id: string }
  | { type: "pong" }
  | { type: "subscribed"; channel_id: string }
  | { type: "unsubscribed"; channel_id: string }
  | { type: "message_new"; channel_id: string; message: Message }
  | {
      type: "message_edit";
      channel_id: string;
      message_id: string;
      content: string;
      edited_at: string;
    }
  | { type: "message_delete"; channel_id: string; message_id: string }
  | { type: "typing_start"; channel_id: string; user_id: string }
  | { type: "typing_stop"; channel_id: string; user_id: string }
  | { type: "presence_update"; user_id: string; status: UserStatus }
  | { type: "voice_offer"; channel_id: string; sdp: string }
  | { type: "voice_ice_candidate"; channel_id: string; candidate: string }
  | { type: "voice_user_joined"; channel_id: string; user_id: string; username: string; display_name: string }
  | { type: "voice_user_left"; channel_id: string; user_id: string }
  | { type: "voice_user_muted"; channel_id: string; user_id: string }
  | { type: "voice_user_unmuted"; channel_id: string; user_id: string }
  | {
      type: "voice_room_state";
      channel_id: string;
      participants: VoiceParticipant[];
    }
  | { type: "voice_error"; code: string; message: string }
  | { type: "error"; code: string; message: string };

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
  voice_activity_detection: boolean;
  vad_threshold: number;
}

export interface AppSettings {
  audio: AudioSettings;
  voice: VoiceSettings;
  theme: "dark" | "light";
  notifications_enabled: boolean;
}
