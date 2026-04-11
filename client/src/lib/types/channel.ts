/**
 * Channel, category, and channel override types.
 */

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
  /** ID of the last message the user has read (null = never read). */
  last_read_message_id: string | null;
  /** ID of the most recent message in the channel (null = no messages). */
  last_message_id: string | null;
}

export type CategoryType = "mixed" | "text" | "voice";

export interface ChannelCategory {
  id: string;
  guild_id: string;
  name: string;
  position: number;
  parent_id: string | null;
  category_type: CategoryType;
  collapsed: boolean;
  created_at: string;
}

/** ChannelCategory with nested channels for UI rendering */
export interface ChannelCategoryWithChannels extends ChannelCategory {
  channels: ChannelWithUnread[];
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
