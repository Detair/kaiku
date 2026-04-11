/**
 * Friends, friendship, and direct message types.
 */

import type { Channel, ChannelType } from "./channel";

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
  last_seen?: string | null;
  direction?: "incoming" | "outgoing" | null;
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
  /** ID of the last message the user has read (null = never read). */
  last_read_message_id: string | null;
  /** ID of the most recent message in the channel (null = no messages). */
  last_message_id: string | null;
}
