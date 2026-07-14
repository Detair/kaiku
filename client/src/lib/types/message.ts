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
  embeds?: MessageEmbed[];
  components?: MessageActionRow[];
  /** Incoming webhook that authored this message (renders an APP badge). */
  webhook_id?: string | null;
}

/** A row of interactive components (bot-authored). */
export interface MessageActionRow {
  components: MessageComponent[];
}

export type MessageComponent =
  | {
      type: "button";
      style: "primary" | "secondary" | "success" | "danger" | "link";
      label?: string;
      custom_id?: string;
      url?: string;
      disabled?: boolean;
    }
  | {
      type: "select_menu";
      custom_id: string;
      placeholder?: string;
      disabled?: boolean;
      options: {
        label: string;
        value: string;
        description?: string;
        default?: boolean;
      }[];
    };

/** A rich embed card (bot-authored). Mirrors the server `Embed` shape. */
export interface MessageEmbed {
  title?: string;
  description?: string;
  url?: string;
  color?: number;
  author?: { name: string; url?: string; icon_url?: string };
  fields?: { name: string; value: string; inline?: boolean }[];
  image?: string;
  thumbnail?: string;
  footer?: { text: string; icon_url?: string };
  timestamp?: string;
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
