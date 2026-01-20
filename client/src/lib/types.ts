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
  status: "online" | "idle" | "offline";
  last_seen_at: string | null;
}

export interface GuildInvite {
  id: string;
  guild_id: string;
  code: string;
  created_by: string;
  expires_at: string | null;
  use_count: number;
  created_at: string;
}

export interface InviteResponse {
  id: string;
  code: string;
  guild_id: string;
  guild_name: string;
  expires_at: string | null;
  use_count: number;
  created_at: string;
}

export type InviteExpiry = "30m" | "1h" | "1d" | "7d" | "never";

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
  screen_sharing: boolean;
}

export interface ScreenShareServerInfo {
  user_id: string;
  username: string;
  source_label: string;
  has_audio: boolean;
  quality: "low" | "medium" | "high" | "premium";
  started_at: string;
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
      screen_shares?: ScreenShareServerInfo[];
    }
  | { type: "voice_error"; code: string; message: string }
  // Screen share events
  | {
      type: "screen_share_started";
      channel_id: string;
      user_id: string;
      username: string;
      source_label: string;
      has_audio: boolean;
      quality: "low" | "medium" | "high" | "premium";
      started_at?: string;
    }
  | {
      type: "screen_share_stopped";
      channel_id: string;
      user_id: string;
      reason: string;
    }
  | {
      type: "screen_share_quality_changed";
      channel_id: string;
      user_id: string;
      new_quality: "low" | "medium" | "high" | "premium";
    }
  | { type: "error"; code: string; message: string }
  // Call events
  | { type: "incoming_call"; channel_id: string; initiator: string; initiator_name: string }
  | { type: "call_started"; channel_id: string }
  | { type: "call_ended"; channel_id: string; reason: string; duration_secs: number | null }
  | { type: "call_participant_joined"; channel_id: string; user_id: string; username: string }
  | { type: "call_participant_left"; channel_id: string; user_id: string }
  | { type: "call_declined"; channel_id: string; user_id: string }
  // Voice metrics events
  | { type: "voice_user_stats"; channel_id: string; user_id: string; latency: number; packet_loss: number; jitter: number; quality: number };

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

// Friends Types

export type FriendshipStatus = "pending" | "accepted" | "blocked";

export interface Friendship {
  id: string;
  requester_id: string;
  addressee_id: string;
  status: FriendshipStatus;
  created_at: string;
  updated_at: string;
}

export interface Friend {
  user_id: string;
  username: string;
  display_name: string;
  avatar_url: string | null;
  status_message: string | null;
  is_online: boolean;
  friendship_id: string;
  friendship_status: FriendshipStatus;
  created_at: string;
}

// DM Types

export interface DMParticipant {
  user_id: string;
  username: string;
  display_name: string;
  avatar_url: string | null;
  joined_at: string;
}

export interface DMChannel {
  channel: Channel;
  participants: DMParticipant[];
}

// Enhanced DM types for Home view

export interface LastMessagePreview {
  id: string;
  content: string;
  user_id: string;
  username: string;
  created_at: string;
}

export interface DMListItem {
  id: string;
  name: string;
  channel_type: ChannelType;
  category_id: string | null;
  guild_id: string | null;
  topic: string | null;
  user_limit: number | null;
  position: number;
  created_at: string;
  participants: DMParticipant[];
  last_message: LastMessagePreview | null;
  unread_count: number;
}

// Pages Types

export interface Page {
  id: string;
  guild_id: string | null;
  title: string;
  slug: string;
  content: string;
  content_hash: string;
  position: number;
  requires_acceptance: boolean;
  created_by: string;
  updated_by: string;
  created_at: string;
  updated_at: string;
  deleted_at: string | null;
}

export interface PageListItem {
  id: string;
  guild_id: string | null;
  title: string;
  slug: string;
  position: number;
  requires_acceptance: boolean;
  updated_at: string;
}

export interface CreatePageRequest {
  title: string;
  slug?: string;
  content: string;
  requires_acceptance?: boolean;
}

export interface UpdatePageRequest {
  title?: string;
  slug?: string;
  content?: string;
  requires_acceptance?: boolean;
}

// Role Types

export interface GuildRole {
  id: string;
  guild_id: string;
  name: string;
  color: string | null;
  permissions: number;
  position: number;
  is_default: boolean;
  created_at: string;
}

export interface CreateRoleRequest {
  name: string;
  color?: string;
  permissions?: number;
}

export interface UpdateRoleRequest {
  name?: string;
  color?: string;
  permissions?: number;
  position?: number;
}

export interface AssignRoleResponse {
  assigned: boolean;
  user_id: string;
  role_id: string;
}

export interface RemoveRoleResponse {
  removed: boolean;
  user_id: string;
  role_id: string;
}

export interface DeleteRoleResponse {
  deleted: boolean;
  role_id: string;
}

// Channel Override Types

export interface ChannelOverride {
  id: string;
  channel_id: string;
  role_id: string;
  allow_permissions: number;
  deny_permissions: number;
}

export interface SetChannelOverrideRequest {
  allow?: number;
  deny?: number;
}

// Member with roles (extended GuildMember)

export interface GuildMemberWithRoles extends GuildMember {
  role_ids: string[];
}

// Admin Types

export interface AdminStats {
  user_count: number;
  guild_count: number;
  banned_count: number;
}

export interface AdminStatus {
  is_admin: boolean;
  is_elevated: boolean;
  elevation_expires_at: string | null;
}

export interface UserSummary {
  id: string;
  username: string;
  display_name: string;
  email: string | null;
  created_at: string;
  is_banned: boolean;
}

export interface GuildSummary {
  id: string;
  name: string;
  owner_id: string;
  member_count: number;
  created_at: string;
  suspended_at: string | null;
}

export interface AuditLogEntry {
  id: string;
  actor_id: string;
  actor_username: string | null;
  action: string;
  target_type: string | null;
  target_id: string | null;
  details: Record<string, unknown> | null;
  ip_address: string | null;
  created_at: string;
}

export interface PaginatedResponse<T> {
  items: T[];
  total: number;
  limit: number;
  offset: number;
}

export interface ElevateResponse {
  elevated: boolean;
  expires_at: string;
  session_id: string;
}

// Call State Types

export type CallEndReason = "cancelled" | "all_declined" | "no_answer" | "last_left";

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
