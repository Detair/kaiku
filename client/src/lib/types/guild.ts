/**
 * Guild, guild settings, members, invites, roles, emojis, and discovery types.
 */

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
  discovery_prompt_dismissed: boolean;
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

export interface GuildEmoji {
  id: string;
  name: string;
  guild_id: string;
  image_url: string;
  animated: boolean;
  uploaded_by: string;
  created_at: string;
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

// Member with roles (extended GuildMember)

export interface GuildMemberWithRoles extends GuildMember {
  role_ids: string[];
}
