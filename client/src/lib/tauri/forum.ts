/**
 * Forum channel API (posts + tags). Uses the dual-mode `httpRequest` helper so
 * it works in both browser and Tauri without a dedicated native command.
 */

import { httpRequest } from "./common";

export interface ForumPost {
  id: string;
  channel_id: string;
  root_message_id: string;
  title: string;
  author_id: string | null;
  pinned: boolean;
  locked: boolean;
  reply_count: number;
  tag_ids: string[];
  created_at: string;
  last_activity_at: string;
}

export interface ForumTag {
  id: string;
  channel_id: string;
  name: string;
  emoji: string | null;
}

export async function listForumPosts(
  channelId: string,
  tag?: string,
): Promise<ForumPost[]> {
  const q = tag ? `?tag=${encodeURIComponent(tag)}` : "";
  return httpRequest<ForumPost[]>("GET", `/api/channels/${channelId}/posts${q}`);
}

export async function createForumPost(
  channelId: string,
  body: { title: string; content: string; tag_ids?: string[] },
): Promise<ForumPost> {
  return httpRequest<ForumPost>("POST", `/api/channels/${channelId}/posts`, body);
}

export async function updateForumPost(
  postId: string,
  body: { pinned?: boolean; locked?: boolean },
): Promise<void> {
  return httpRequest<void>("PATCH", `/api/forum/posts/${postId}`, body);
}

export async function deleteForumPost(postId: string): Promise<void> {
  return httpRequest<void>("DELETE", `/api/forum/posts/${postId}`);
}

export async function listForumTags(channelId: string): Promise<ForumTag[]> {
  return httpRequest<ForumTag[]>("GET", `/api/channels/${channelId}/tags`);
}

export async function createForumTag(
  channelId: string,
  body: { name: string; emoji?: string },
): Promise<ForumTag> {
  return httpRequest<ForumTag>("POST", `/api/channels/${channelId}/tags`, body);
}
