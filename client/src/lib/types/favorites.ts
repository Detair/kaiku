/**
 * Pins and favorites types.
 */

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
