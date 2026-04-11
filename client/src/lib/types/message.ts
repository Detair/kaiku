/**
 * Message, attachment, reaction, thread, and pagination types.
 */

import type { UserProfile } from "./user";

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
  users?: string[]; // User IDs (for tooltip, optional)
  me: boolean; // Did current user react
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
  pinned: boolean;
  message_type: string; // "user" | "system"
  nonce?: string | null;
}

export interface ChannelPin {
  message: Message;
  pinned_by: string;
  pinned_at: string;
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
