/**
 * Message commands: CRUD, threads, attachments, reactions, pins, channel pins,
 * unread aggregation, and signed URL fetching.
 */

import type {
  ChannelPin,
  CreatePinRequest,
  Message,
  PaginatedMessages,
  Pin,
  UpdatePinRequest,
} from "../types";
import {
  browserState,
  fetchApi,
  getUploadAuth,
  httpRequest,
  isTauri,
  validateFileSize,
} from "./common";

// ============================================================================
// Unread aggregation types
// ============================================================================

export interface ChannelUnread {
  channel_id: string;
  channel_name: string;
  unread_count: number;
}

export interface GuildUnreadSummary {
  guild_id: string;
  guild_name: string;
  channels: ChannelUnread[];
  total_unread: number;
}

export interface UnreadAggregate {
  guilds: GuildUnreadSummary[];
  dms: ChannelUnread[];
  total: number;
}

// ============================================================================
// Messages
// ============================================================================

export async function getMessages(
  channelId: string,
  before?: string,
  limit?: number,
  around?: string,
): Promise<PaginatedMessages> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_messages", { channelId, before, limit, around });
  }

  const params = new URLSearchParams();
  if (around) params.set("around", around);
  else if (before) params.set("before", before);
  if (limit) params.set("limit", limit.toString());
  const query = params.toString();

  return httpRequest<PaginatedMessages>(
    "GET",
    `/api/messages/channel/${channelId}${query ? `?${query}` : ""}`,
  );
}

export async function sendMessage(
  channelId: string,
  content: string,
  options?: { encrypted?: boolean; nonce?: string },
): Promise<Message> {
  const result = await sendMessageWithStatus(channelId, content, options);
  return result.message;
}

export interface SendMessageResult {
  message: Message;
  status: number;
}

export async function sendMessageWithStatus(
  channelId: string,
  content: string,
  options?: { encrypted?: boolean; nonce?: string },
): Promise<SendMessageResult> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    const message = await invoke<Message>("send_message", {
      channelId,
      content,
      encrypted: options?.encrypted,
      nonce: options?.nonce,
    });

    // Tauri command interface currently does not expose HTTP status.
    return { message, status: 201 };
  }

  const token = browserState.accessToken;
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
  };

  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  const baseUrl = browserState.serverUrl.replace(/\/+$/, "");
  const response = await fetch(`${baseUrl}/api/messages/channel/${channelId}`, {
    method: "POST",
    headers,
    body: JSON.stringify({
      content,
      encrypted: options?.encrypted ?? false,
      nonce: options?.nonce,
    }),
  });

  if (!response.ok) {
    let errorMessage = `HTTP ${response.status}: ${response.statusText}`;
    const rawErrorBody = await response.text();

    try {
      const errorBody = rawErrorBody ? JSON.parse(rawErrorBody) : null;
      errorMessage = errorBody.message || errorBody.error || errorMessage;
    } catch (_parseError) {
      if (rawErrorBody.length > 0 && rawErrorBody.length < 500) {
        errorMessage = rawErrorBody;
      }
    }

    throw new Error(errorMessage);
  }

  const message = (await response.json()) as Message;
  return { message, status: response.status };
}

// ============================================================================
// Thread API Functions
// ============================================================================

export async function getThreadReplies(
  parentId: string,
  after?: string,
  limit?: number,
): Promise<PaginatedMessages> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_thread_replies", { parentId, after, limit });
  }

  const params = new URLSearchParams();
  if (after) params.set("after", after);
  if (limit) params.set("limit", limit.toString());
  const query = params.toString();

  return httpRequest<PaginatedMessages>(
    "GET",
    `/api/messages/${parentId}/thread${query ? `?${query}` : ""}`,
  );
}

export async function sendThreadReply(
  parentId: string,
  channelId: string,
  content: string,
  options?: { encrypted?: boolean; nonce?: string },
): Promise<Message> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("send_thread_reply", {
      parentId,
      channelId,
      content,
      encrypted: options?.encrypted,
      nonce: options?.nonce,
    });
  }

  return httpRequest<Message>("POST", `/api/messages/channel/${channelId}`, {
    content,
    encrypted: options?.encrypted ?? false,
    nonce: options?.nonce,
    parent_id: parentId,
  });
}

export async function markThreadRead(parentId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("mark_thread_read", { parentId });
  }

  return httpRequest<void>("POST", `/api/messages/${parentId}/thread/read`);
}

// ============================================================================
// File upload
// ============================================================================

export async function uploadFile(messageId: string, file: File): Promise<any> {
  // Frontend validation
  const error = validateFileSize(file, "attachment");
  if (error) {
    console.warn("[uploadFile] Frontend validation failed:", error);
    throw new Error(error);
  }

  const { token, baseUrl } = await getUploadAuth();

  const headers: Record<string, string> = {};
  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  const formData = new FormData();
  formData.append("message_id", messageId);
  formData.append("file", file);

  const response = await fetch(`${baseUrl}/api/messages/upload`, {
    method: "POST",
    headers,
    body: formData,
  });

  if (!response.ok) {
    let errorMessage = `Upload failed (HTTP ${response.status})`;

    try {
      const errorBody = await response.json();
      errorMessage = errorBody.message || errorBody.error || errorMessage;
    } catch (parseError) {
      console.warn("[uploadFile] Failed to parse error response:", parseError);
      errorMessage = response.statusText || errorMessage;
    }

    console.error("[uploadFile] Upload failed:", {
      status: response.status,
      error: errorMessage,
      messageId,
      fileSize: file.size,
      fileName: file.name,
    });

    throw new Error(errorMessage);
  }

  try {
    return await response.json();
  } catch (parseError) {
    console.error("[uploadFile] Failed to parse success response:", parseError);
    throw new Error("Server returned invalid response", { cause: parseError });
  }
}

/**
 * Upload a file and create a message in one request.
 * Uses the combined endpoint that creates the message and attaches the file.
 */
export async function uploadMessageWithFile(
  channelId: string,
  file: File,
  content?: string,
): Promise<Message> {
  // Frontend validation
  const error = validateFileSize(file, "attachment");
  if (error) {
    console.warn("[uploadMessageWithFile] Frontend validation failed:", error);
    throw new Error(error);
  }

  const { token, baseUrl } = await getUploadAuth();

  const headers: Record<string, string> = {};
  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  const formData = new FormData();
  formData.append("file", file);
  if (content) {
    formData.append("content", content);
  }

  const response = await fetch(
    `${baseUrl}/api/messages/channel/${channelId}/upload`,
    {
      method: "POST",
      headers,
      body: formData,
    },
  );

  if (!response.ok) {
    let errorMessage = `Upload failed (HTTP ${response.status})`;

    try {
      const errorBody = await response.json();
      errorMessage = errorBody.message || errorBody.error || errorMessage;
    } catch (parseError) {
      console.warn(
        "[uploadMessageWithFile] Failed to parse error response:",
        parseError,
      );
      errorMessage = response.statusText || errorMessage;
    }

    console.error("[uploadMessageWithFile] Upload failed:", {
      status: response.status,
      error: errorMessage,
      channelId,
      fileSize: file.size,
      fileName: file.name,
    });

    throw new Error(errorMessage);
  }

  try {
    return await response.json();
  } catch (parseError) {
    console.error(
      "[uploadMessageWithFile] Failed to parse success response:",
      parseError,
    );
    throw new Error("Server returned invalid response", { cause: parseError });
  }
}

// ============================================================================
// Pins (user-level pin items, not message pinning)
// ============================================================================

export async function fetchPins(): Promise<Pin[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("fetch_pins");
  }

  return httpRequest<Pin[]>("GET", "/api/me/pins");
}

export async function createPin(request: CreatePinRequest): Promise<Pin> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("create_pin", { request });
  }

  return httpRequest<Pin>("POST", "/api/me/pins", request);
}

export async function updatePin(
  pinId: string,
  request: UpdatePinRequest,
): Promise<Pin> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("update_pin", { pin_id: pinId, request });
  }

  return httpRequest<Pin>("PUT", `/api/me/pins/${pinId}`, request);
}

export async function deletePin(pinId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("delete_pin", { pin_id: pinId });
  }

  await httpRequest<void>("DELETE", `/api/me/pins/${pinId}`);
}

export async function reorderPins(pinIds: string[]): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("reorder_pins", { pin_ids: pinIds });
  }

  await httpRequest<void>("PUT", "/api/me/pins/reorder", { pin_ids: pinIds });
}

// ============================================================================
// Channel Pins (message-level pinning)
// ============================================================================

/**
 * List pinned messages for a channel.
 */
export async function listChannelPins(channelId: string): Promise<ChannelPin[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<ChannelPin[]>("list_channel_pins", { channelId });
  }

  return httpRequest<ChannelPin[]>("GET", `/api/channels/${channelId}/pins`);
}

/**
 * Pin a message to a channel.
 */
export async function pinMessage(channelId: string, messageId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("pin_message", { channelId, messageId });
  }

  await httpRequest<void>("PUT", `/api/channels/${channelId}/messages/${messageId}/pin`);
}

/**
 * Unpin a message from a channel.
 */
export async function unpinMessage(channelId: string, messageId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("unpin_message", { channelId, messageId });
  }

  await httpRequest<void>("DELETE", `/api/channels/${channelId}/messages/${messageId}/pin`);
}

// ============================================================================
// Reactions
// ============================================================================

/**
 * Add a reaction to a message.
 */
export async function addReaction(
  channelId: string,
  messageId: string,
  emoji: string,
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("add_reaction", { channelId, messageId, emoji });
  }

  await httpRequest<void>(
    "PUT",
    `/api/channels/${channelId}/messages/${messageId}/reactions`,
    { emoji },
  );
}

/**
 * Remove a reaction from a message.
 */
export async function removeReaction(
  channelId: string,
  messageId: string,
  emoji: string,
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("remove_reaction", { channelId, messageId, emoji });
  }

  await httpRequest<void>(
    "DELETE",
    `/api/channels/${channelId}/messages/${messageId}/reactions/${encodeURIComponent(emoji)}`,
  );
}

/**
 * Delete a message (soft delete, own messages only).
 */
export async function deleteMessage(messageId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("delete_message", { messageId });
  }

  await httpRequest<void>("DELETE", `/api/messages/${messageId}`);
}

/**
 * Edit a message (own messages only).
 */
export async function editMessage(
  messageId: string,
  content: string,
): Promise<Message> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("edit_message", { messageId, content });
  }

  return httpRequest<Message>("PATCH", `/api/messages/${messageId}`, {
    content,
  });
}

// ============================================================================
// Unread aggregation / read-all
// ============================================================================

/**
 * Get aggregate unread counts across all guilds and DMs.
 * Returns unread counts grouped by guild, plus DM unreads.
 */
export async function getUnreadAggregate(): Promise<UnreadAggregate> {
  return fetchApi<UnreadAggregate>("/api/me/unread");
}

/**
 * Mark all text channels in a guild as read.
 */
export async function markAllGuildChannelsRead(guildId: string): Promise<void> {
  await fetchApi<void>(`/api/guilds/${guildId}/read-all`, { method: "POST" });
}

/**
 * Mark all DM channels as read.
 */
export async function markAllDMsRead(): Promise<void> {
  await fetchApi<void>("/api/dm/read-all", { method: "POST" });
}

/**
 * Mark everything (guilds + DMs) as read.
 */
export async function markAllRead(): Promise<void> {
  await fetchApi<void>("/api/me/read-all", { method: "POST" });
}

// ============================================================================
// Signed URLs (for attachments)
// ============================================================================

/**
 * Get a presigned S3 URL for downloading an attachment.
 * Uses Authorization header instead of passing JWT in the URL.
 */
export async function getSignedUrl(
  attachmentId: string,
  variant?: string,
): Promise<{ url: string; expires_in: number }> {
  const path = variant
    ? `/api/messages/attachments/${attachmentId}/url?variant=${encodeURIComponent(variant)}`
    : `/api/messages/attachments/${attachmentId}/url`;
  return httpRequest<{ url: string; expires_in: number }>("GET", path);
}
