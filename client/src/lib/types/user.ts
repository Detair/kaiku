/**
 * User, presence, activity, custom status, and session types.
 */

import type { UserStatus } from "./common";

/** Type of activity the user is engaged in. */
export type ActivityType =
  | "game"
  | "listening"
  | "watching"
  | "coding"
  | "custom";

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
  expires_at?: string;
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
  status_message?: string | null;
}

// Session Management Types

export interface SessionInfo {
  id: string;
  device: string;
  ip_address: string | null;
  city: string | null;
  country: string | null;
  created_at: string;
  expires_at: string;
  is_current: boolean;
}

export interface SessionListResponse {
  sessions: SessionInfo[];
}

// Linked external (OIDC) identities

export interface IdentityInfo {
  id: string;
  provider_slug: string;
  provider_name: string;
  email: string | null;
  created_at: string;
  last_used_at: string | null;
}

export interface IdentityListResponse {
  identities: IdentityInfo[];
}

export interface RevokeAllResponse {
  revoked_count: number;
}
