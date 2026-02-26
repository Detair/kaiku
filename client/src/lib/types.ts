/**
 * Shared Types for VoiceChat Client
 *
 * These types mirror the Rust types in shared/vc-common
 */

// Theme Types (canonical source of truth for theme names)

/** All available theme identifiers. Add new themes here. */
export const THEME_NAMES = [
  "focused-hybrid",
  "solarized-dark",
  "solarized-light",
  "pixel-cozy",
] as const;

/** Valid theme name identifier. Derived from THEME_NAMES array. */
export type ThemeName = (typeof THEME_NAMES)[number];

// User Types

export type UserStatus = "online" | "idle" | "dnd" | "invisible" | "offline";

// Quality and Status Indicator Types (for accessibility shapes)

export type QualityLevel = "good" | "warning" | "poor" | "unknown";

export type StatusShape = "circle" | "triangle" | "hexagon" | "empty-circle";

export const STATUS_SHAPES: Record<QualityLevel, StatusShape> = {
  good: "circle",
  warning: "triangle",
  poor: "hexagon",
  unknown: "empty-circle",
};

export const STATUS_COLORS = {
  good: "#23a55a",
  warning: "#f0b232",
  poor: "#f23f43",
  unknown: "#80848e",
  streaming: "#593695",
} as const;

/** Type of activity the user is engaged in. */
export type ActivityType = "game" | "listening" | "watching" | "coding" | "custom";

/** Rich presence activity data. */
export interface Activity {
  /** Type of activity. */
  type: ActivityType;
  /** Display name (e.g., "Minecraft", "VS Code"). */
  name: string;
  /** ISO timestamp when the activity started. */
  started_at: string;
  /** Optional details (e.g., "Creative Mode"). */
  details?: string;
}

/** Custom status set by the user. */
export interface CustomStatus {
  /** Display text for the custom status. */
  text: string;
  /** Optional emoji to show with the status. */
  emoji?: string;
  /** ISO timestamp when the custom status expires. */
  expiresAt?: string;
}

/** Extended presence data with activity. */
export interface UserPresence {
  /** Current user status. */
  status: UserStatus;
  /** Custom status set by the user, if any. */
  customStatus?: CustomStatus | null;
  /** Current activity, if any. */
  activity?: Activity | null;
  /** ISO timestamp of when the user was last seen (for offline users). */
  lastSeen?: string;
}

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
  threads_enabled: boolean;
  discoverable: boolean;
  tags: string[];
  banner_url: string | null;
  plan: string;
  created_at: string;
}

export interface UsageStat {
  current: number;
  limit: number;
}

export interface GuildUsageStats {
  guild_id: string;
  plan: string;
  members: UsageStat;
  channels: UsageStat;
  roles: UsageStat;
  emojis: UsageStat;
  bots: UsageStat;
  pages: UsageStat;
}

export interface GuildSettings {
  threads_enabled: boolean;
  discoverable: boolean;
  tags: string[];
  banner_url: string | null;
}

export interface DiscoverableGuild {
  id: string;
  name: string;
  icon_url: string | null;
  banner_url: string | null;
  description: string | null;
  tags: string[];
  member_count: number;
  created_at: string;
}

export interface DiscoverResponse {
  guilds: DiscoverableGuild[];
  total: number;
  limit: number;
  offset: number;
}

export interface JoinDiscoverableResponse {
  guild_id: string;
  guild_name: string;
  already_member: boolean;
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
  icon_url: string | null;
  user_limit: number | null;
  position: number;
  created_at: string;
}

/** Channel with unread message count (returned from guild channel list). */
export interface ChannelWithUnread extends Channel {
  /** Number of unread messages (only for text channels). */
  unread_count: number;
}

export interface ChannelCategory {
  id: string;
  guild_id: string;
  name: string;
  position: number;
  parent_id: string | null;
  collapsed: boolean;
  created_at: string;
}

/** ChannelCategory with nested channels for UI rendering */
export interface ChannelCategoryWithChannels extends ChannelCategory {
  channels: ChannelWithUnread[];
}

// Message Types

export interface Attachment {
  id: string;
  filename: string;
  mime_type: string;
  size: number;
  url: string;
  width?: number;
  height?: number;
  blurhash?: string;
  thumbnail_url?: string;
  medium_url?: string;
}

export interface Reaction {
  emoji: string;
  count: number;
  users?: string[];  // User IDs (for tooltip, optional)
  me: boolean;      // Did current user react
}

export interface GuildEmoji {
  id: string;
  name: string;
  guild_id: string;
  image_url: string;
  animated: boolean;
  uploaded_by: string;
  created_at: string;
}

export interface Message {
  id: string;
  channel_id: string;
  author: UserProfile;
  content: string;
  encrypted: boolean;
  attachments: Attachment[];
  reply_to: string | null;
  parent_id: string | null;
  thread_reply_count: number;
  thread_last_reply_at: string | null;
  edited_at: string | null;
  created_at: string;
  mention_type: "direct" | "everyone" | "here" | null;
  reactions?: Reaction[];
  thread_info?: ThreadInfo;
}

export interface ThreadInfo {
  reply_count: number;
  last_reply_at: string | null;
  participant_ids: string[];
  participant_avatars: Array<string | null>;
  has_unread?: boolean;
}

// Paginated Response Types

export interface PaginatedMessages {
  items: Message[];
  has_more: boolean;
  next_cursor: string | null;
}

// Search Types

export interface SearchAuthor {
  id: string;
  username: string;
  display_name: string;
  avatar_url: string | null;
}

export interface SearchResult {
  id: string;
  channel_id: string;
  channel_name: string;
  author: SearchAuthor;
  content: string;
  created_at: string;
  headline: string;
  rank: number;
}

export interface SearchResponse {
  results: SearchResult[];
  total: number;
  limit: number;
  offset: number;
}

export interface SearchFilters {
  date_from?: string;
  date_to?: string;
  channel_id?: string;
  author_id?: string;
  has?: "link" | "file";
  sort?: "relevance" | "date";
}

// Global Search Types

export interface GlobalSearchSource {
  type: "guild" | "dm";
  guild_id: string | null;
  guild_name: string | null;
}

export interface GlobalSearchResult extends SearchResult {
  source: GlobalSearchSource;
}

export interface GlobalSearchResponse {
  results: GlobalSearchResult[];
  total: number;
  limit: number;
  offset: number;
}

// Voice Types

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
  | { type: "voice_unmute"; channel_id: string }
  // Webcam events
  | { type: "voice_webcam_start"; channel_id: string; quality: string }
  | { type: "voice_webcam_stop"; channel_id: string }
  // Admin events
  | { type: "admin_subscribe" }
  | { type: "admin_unsubscribe" };

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
  | { type: "rich_presence_update"; user_id: string; activity: Activity | null }
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
    webcams?: WebcamServerInfo[];
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
  // Webcam events
  | {
    type: "webcam_started";
    channel_id: string;
    user_id: string;
    username: string;
    quality: "low" | "medium" | "high" | "premium";
  }
  | {
    type: "webcam_stopped";
    channel_id: string;
    user_id: string;
    reason: string;
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
  | { type: "voice_user_stats"; channel_id: string; user_id: string; latency: number; packet_loss: number; jitter: number; quality: number }
  // Admin events
  | { type: "admin_user_banned"; user_id: string; username: string }
  | { type: "admin_user_unbanned"; user_id: string; username: string }
  | { type: "admin_guild_suspended"; guild_id: string; guild_name: string }
  | { type: "admin_guild_unsuspended"; guild_id: string; guild_name: string }
  | { type: "admin_user_deleted"; user_id: string; username: string }
  | { type: "admin_guild_deleted"; guild_id: string; guild_name: string }
  // DM read sync event
  | { type: "dm_read"; channel_id: string }
  // Guild channel read sync event
  | { type: "channel_read"; channel_id: string; last_read_message_id?: string }
  // Preferences events
  | { type: "preferences_updated"; preferences: Partial<UserPreferences>; updated_at: string }
  // Reaction events
  | { type: "reaction_add"; channel_id: string; message_id: string; user_id: string; emoji: string }
  | { type: "reaction_remove"; channel_id: string; message_id: string; user_id: string; emoji: string }
  // Guild emoji events
  | { type: "guild_emoji_updated"; guild_id: string; emojis: GuildEmoji[] }
  // Friend events
  | { type: "friend_request_received"; friendship_id: string; from_user_id: string; from_username: string; from_display_name: string; from_avatar_url: string | null }
  | { type: "friend_request_accepted"; friendship_id: string; user_id: string; username: string; display_name: string; avatar_url: string | null }
  // DM metadata events
  | { type: "dm_name_updated"; channel_id: string; name: string; updated_by: string }
  // Block events
  | { type: "user_blocked"; user_id: string }
  | { type: "user_unblocked"; user_id: string }
  // Admin report events
  | { type: "admin_report_created"; report_id: string; category: string; target_type: string }
  | { type: "admin_report_resolved"; report_id: string }
  // Thread events
  | { type: "thread_reply_new"; channel_id: string; parent_id: string; message: Message; thread_info: ThreadInfo }
  | { type: "thread_reply_delete"; channel_id: string; parent_id: string; message_id: string; thread_info: ThreadInfo }
  | { type: "thread_read"; thread_parent_id: string; last_read_message_id: string | null }
  // State sync events
  | { type: "patch"; entity_type: string; entity_id: string; diff: Record<string, unknown> }
  // Bot command response events
  | { type: "command_response"; interaction_id: string; content: string; command_name: string; bot_name: string; channel_id: string; ephemeral: boolean }
  | { type: "command_response_timeout"; interaction_id: string; command_name: string; channel_id: string };

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

export interface UiState {
  category_collapse: Record<string, boolean>;
}

// Display Preferences Types
export type DisplayMode = "dense" | "minimal" | "discord";
export type ReactionStyle = "bar" | "compact";

export interface DisplayPreferences {
  /** How status indicators are displayed (dense=full info, minimal=compact, discord=Discord-style) */
  indicatorMode: DisplayMode;
  /** Whether to show latency numbers on voice indicators */
  showLatencyNumbers: boolean;
  /** How reactions are displayed on messages */
  reactionStyle: ReactionStyle;
  /** Minutes of inactivity before user is marked as idle */
  idleTimeoutMinutes: number;
}

export const DEFAULT_DISPLAY_PREFERENCES: DisplayPreferences = {
  indicatorMode: "dense",
  showLatencyNumbers: true,
  reactionStyle: "bar",
  idleTimeoutMinutes: 5,
};

// Focus Mode Types

export type FocusTriggerCategory = "game" | "coding" | "listening" | "watching";

export type FocusSuppressionLevel = "all" | "except_mentions" | "except_dms";

export interface FocusMode {
  id: string;
  name: string;
  icon: string;
  builtin: boolean;
  triggerCategories: FocusTriggerCategory[] | null;
  autoActivateEnabled: boolean;
  suppressionLevel: FocusSuppressionLevel;
  vipUserIds: string[];
  vipChannelIds: string[];
  emergencyKeywords: string[];
}

export interface FocusPreferences {
  modes: FocusMode[];
  autoActivateGlobal: boolean;
}

export interface FocusState {
  activeModeId: string | null;
  autoActivated: boolean;
  activatedAt: string | null;
  triggeringCategory: FocusTriggerCategory | null;
}

// User Preferences (synced across devices)
export interface UserPreferences {
  // Theme
  theme: ThemeName;

  // Sound settings
  sound: {
    enabled: boolean;
    volume: number; // 0-100
    soundType: "default" | "subtle" | "ping" | "chime" | "bell";
    quietHours: {
      enabled: boolean;
      startTime: string; // "HH:MM" format
      endTime: string;
    };
  };

  // Connection display
  connection: {
    displayMode: "circle" | "number";
    showNotifications: boolean;
  };

  // Per-channel notification levels
  channelNotifications: Record<string, "all" | "mentions" | "muted">;

  // Home sidebar section collapse states
  homeSidebar: {
    collapsed: {
      unread: boolean;
      activeNow: boolean;
      pending: boolean;
      pins: boolean;
    };
  };

  // Display preferences for UI customization
  display: DisplayPreferences;

  // Focus mode preferences
  focus: FocusPreferences;

  // Onboarding completion flag
  onboarding_completed: boolean;
}

export interface PreferencesResponse {
  preferences: Partial<UserPreferences>;
  updated_at: string; // ISO timestamp
}

export interface StoredPreferences {
  data: UserPreferences;
  updated_at: string;
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
  icon_url: string | null;
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
  category_id: string | null;
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
  category_id: string | null;
  updated_at: string;
}

export interface CreatePageRequest {
  title: string;
  slug?: string;
  content: string;
  requires_acceptance?: boolean;
  category_id?: string;
}

export interface UpdatePageRequest {
  title?: string;
  slug?: string;
  content?: string;
  requires_acceptance?: boolean;
  category_id?: string | null;
}

export interface PageRevision {
  id: string;
  page_id: string;
  revision_number: number;
  content: string | null;
  content_hash: string | null;
  title: string | null;
  created_by: string | null;
  created_at: string;
}

export interface RevisionListItem {
  id: string;
  page_id: string;
  revision_number: number;
  content_hash: string | null;
  title: string | null;
  created_by: string | null;
  created_at: string;
}

export interface PageCategory {
  id: string;
  guild_id: string;
  name: string;
  position: number;
  created_at: string;
}

// ============================================================================
// Pins Types
// ============================================================================

export type PinType = "note" | "link" | "message";

export interface Pin {
  id: string;
  pin_type: PinType;
  content: string;
  title?: string;
  metadata: Record<string, unknown>;
  created_at: string;
  position: number;
}

export interface CreatePinRequest {
  pin_type: PinType;
  content: string;
  title?: string;
  metadata?: Record<string, unknown>;
}

export interface UpdatePinRequest {
  content?: string;
  title?: string;
  metadata?: Record<string, unknown>;
}

// ============================================================================
// Favorites Types
// ============================================================================

export interface FavoriteChannel {
  channel_id: string;
  channel_name: string;
  channel_type: "text" | "voice";
  guild_id: string;
  guild_name: string;
  guild_icon: string | null;
  guild_position: number;
  channel_position: number;
}

export interface FavoritesResponse {
  favorites: FavoriteChannel[];
}

export interface Favorite {
  channel_id: string;
  guild_id: string;
  guild_position: number;
  channel_position: number;
  created_at: string;
}

export interface ReorderChannelsRequest {
  guild_id: string;
  channel_ids: string[];
}

export interface ReorderGuildsRequest {
  guild_ids: string[];
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
  avatar_url: string | null;
  created_at: string;
  is_banned: boolean;
}

export interface GuildSummary {
  id: string;
  name: string;
  owner_id: string;
  icon_url: string | null;
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

// User Detail Types

export interface UserGuildMembership {
  guild_id: string;
  guild_name: string;
  guild_icon_url: string | null;
  joined_at: string;
  is_owner: boolean;
}

export interface UserDetailsResponse {
  id: string;
  username: string;
  display_name: string;
  email: string | null;
  avatar_url: string | null;
  created_at: string;
  is_banned: boolean;
  last_login: string | null;
  guild_count: number;
  guilds: UserGuildMembership[];
}

// Guild Detail Types

export interface GuildMemberInfo {
  user_id: string;
  username: string;
  display_name: string;
  avatar_url: string | null;
  joined_at: string;
}

export interface GuildOwnerInfo {
  user_id: string;
  username: string;
  display_name: string;
  avatar_url: string | null;
}

export interface GuildDetailsResponse {
  id: string;
  name: string;
  icon_url: string | null;
  member_count: number;
  created_at: string;
  suspended_at: string | null;
  owner: GuildOwnerInfo;
  top_members: GuildMemberInfo[];
}

// Bulk Action Types

export interface BulkActionFailure {
  id: string;
  reason: string;
}

export interface BulkBanResponse {
  banned_count: number;
  already_banned: number;
  failed: BulkActionFailure[];
}

export interface BulkSuspendResponse {
  suspended_count: number;
  already_suspended: number;
  failed: BulkActionFailure[];
}

// OIDC / SSO Types

/** Public OIDC provider info returned from server settings. */
export interface OidcProvider {
  slug: string;
  display_name: string;
  icon_hint: string | null;
}

/** Result from Tauri OIDC login flow (tokens returned directly). */
export interface OidcLoginResult {
  access_token: string;
  refresh_token: string;
  expires_in: number;
  setup_required: boolean;
}

/** Which auth methods are enabled on the server. */
export interface AuthMethodsConfig {
  local: boolean;
  oidc: boolean;
}

/** Server settings response (public, unauthenticated). */
export interface ServerSettings {
  require_e2ee_setup: boolean;
  oidc_enabled: boolean;
  oidc_providers: OidcProvider[];
  auth_methods: AuthMethodsConfig;
  registration_policy: string;
}

/** Admin auth settings response. */
export interface AuthSettingsResponse {
  auth_methods: AuthMethodsConfig;
  registration_policy: string;
}

/** Admin OIDC provider (full detail, secrets masked). */
export interface AdminOidcProvider {
  id: string;
  slug: string;
  display_name: string;
  icon_hint: string | null;
  provider_type: string;
  issuer_url: string | null;
  authorization_url: string | null;
  token_url: string | null;
  userinfo_url: string | null;
  client_id: string;
  scopes: string;
  enabled: boolean;
  position: number;
  created_at: string;
}

// E2EE Types

export interface E2EEStatus {
  initialized: boolean;
  device_id: string | null;
  has_identity_keys: boolean;
}

export interface InitE2EEResponse {
  device_id: string;
  identity_key_ed25519: string;
  identity_key_curve25519: string;
  prekeys: PrekeyData[];
}

export interface PrekeyData {
  key_id: string;
  public_key: string;
}

export interface DeviceKeys {
  device_id: string;
  device_name: string | null;
  identity_key_ed25519: string;
  identity_key_curve25519: string;
}

export interface UserKeysResponse {
  devices: DeviceKeys[];
}

export interface ClaimedPrekeyResponse {
  device_id: string;
  identity_key_ed25519: string;
  identity_key_curve25519: string;
  one_time_prekey: {
    key_id: string;
    public_key: string;
  } | null;
}

export interface E2EEContent {
  sender_key: string;
  recipients: Record<string, Record<string, EncryptedMessage>>;
}

export interface EncryptedMessage {
  message_type: number;
  ciphertext: string;
}

export interface ClaimedPrekeyInput {
  user_id: string;
  device_id: string;
  identity_key_ed25519: string;
  identity_key_curve25519: string;
  one_time_prekey: {
    key_id: string;
    public_key: string;
  } | null;
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
