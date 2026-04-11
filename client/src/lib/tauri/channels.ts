/**
 * Channel commands: CRUD, reorder, permission overrides, and read tracking.
 */

import type {
  Channel,
  ChannelOverride,
  SetChannelOverrideRequest,
} from "../types";
import { httpRequest, isTauri } from "./common";

export async function getChannels(): Promise<Channel[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_channels");
  }

  return httpRequest<Channel[]>("GET", "/api/channels");
}

export async function createChannel(
  name: string,
  channelType: "text" | "voice",
  guildId?: string,
  topic?: string,
  categoryId?: string,
): Promise<Channel> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("create_channel", {
      name,
      channelType,
      guildId,
      topic,
      categoryId,
    });
  }

  return httpRequest<Channel>("POST", "/api/channels", {
    name,
    channel_type: channelType,
    guild_id: guildId,
    topic,
    category_id: categoryId,
  });
}

/**
 * Mark a guild channel as read.
 * @param channelId - Channel ID to mark as read
 * @param lastReadMessageId - ID of the last read message
 */
export async function markChannelAsRead(
  channelId: string,
  lastReadMessageId: string,
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("mark_channel_as_read", { channelId, lastReadMessageId });
  }

  await httpRequest<void>("POST", `/api/channels/${channelId}/read`, {
    last_read_message_id: lastReadMessageId,
  });
}

// ============================================================================
// Channel Override Commands
// ============================================================================

/**
 * Get permission overrides for a channel.
 */
export async function getChannelOverrides(
  channelId: string,
): Promise<ChannelOverride[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_channel_overrides", { channelId });
  }

  return httpRequest<ChannelOverride[]>(
    "GET",
    `/api/channels/${channelId}/overrides`,
  );
}

/**
 * Set a permission override for a role in a channel.
 */
export async function setChannelOverride(
  channelId: string,
  roleId: string,
  request: SetChannelOverrideRequest,
): Promise<ChannelOverride> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("set_channel_override", { channelId, roleId, request });
  }

  return httpRequest<ChannelOverride>(
    "PUT",
    `/api/channels/${channelId}/overrides/${roleId}`,
    request,
  );
}

/**
 * Delete a permission override for a role in a channel.
 */
export async function deleteChannelOverride(
  channelId: string,
  roleId: string,
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("delete_channel_override", { channelId, roleId });
  }

  await httpRequest<void>(
    "DELETE",
    `/api/channels/${channelId}/overrides/${roleId}`,
  );
}
