/**
 * Admin API types: stats, user/guild summaries, audit log, bulk actions.
 */

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
  /** C28: the "Kaiku" system account (servers hide it from lists anyway). */
  is_system?: boolean;
  /** Not in C27's list; when present, admins are left out of recovery-contact choices. */
  is_admin?: boolean;
}

/** Request body for `POST /api/admin/users` (wire format). */
export interface AdminCreateUserRequest {
  username: string;
  display_name: string | null;
  /** C27: a passwordless account; its phone is set up from the admin's phone. */
  recovery_contact_id?: string | null;
  /** Servers the new account joins right away (contract C23, max 10). */
  guild_ids?: string[];
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
  /** C27 admin user view (absent on older servers). */
  is_admin?: boolean;
  may_approve?: boolean;
  key_devices?: { id: string; label: string; state: string; ready_at?: string | null }[];
  passkeys?: { id: string; label: string; state: string; ready_at?: string | null }[];
  password_state?: "active" | "throttled" | "compromised" | null;
  totp_state?: "active" | "throttled" | "compromised" | null;
  recovery_codes_left?: number;
  bootstrap_open?: boolean;
  recovery_contact?: { user_id: string; username: string; display_name: string; confirmed_at: string | null } | null;
  open_request?: import("./login").AccountRequestSummary | null;
  open_sessions?: { full: number; probation: number };
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
