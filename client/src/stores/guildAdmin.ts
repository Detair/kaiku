/**
 * Server and channel admin actions for the settings windows (ADMIN-5).
 * Kept apart from `stores/guilds.ts` and `stores/channels.ts`: these only
 * call their exported functions to refresh what the sidebar shows.
 * Every function throws on failure so the dialog can say why.
 */
import { renameChannel as apiRenameChannel, deleteChannel as apiDeleteChannel } from "@/lib/tauri/channelAdmin";
import {
  channelsState,
  clearSelection,
  patchChannel,
  refreshChannelsForGuild,
  setChannelsState,
} from "@/stores/channels";
import { deleteGuild, leaveGuild, updateGuild } from "@/stores/guilds";

/**
 * The list refresh after a write is best effort: the write already worked,
 * so a failed refresh must not turn into "that did not work" (a retry would
 * then hit a 404). The local list is updated first.
 */
async function refreshQuietly(guildId: string): Promise<void> {
  try {
    await refreshChannelsForGuild(guildId);
  } catch (err) {
    console.warn("[guildAdmin] channel list refresh failed", err);
  }
}

export async function renameChannel(guildId: string, channelId: string, name: string): Promise<void> {
  const trimmed = name.trim();
  await apiRenameChannel(channelId, trimmed);
  patchChannel(channelId, { name: trimmed });
  await refreshQuietly(guildId);
}

export async function deleteChannel(guildId: string, channelId: string): Promise<void> {
  await apiDeleteChannel(channelId);
  if (channelsState.selectedChannelId === channelId) clearSelection();
  setChannelsState("channels", (list) => list.filter((c) => c.id !== channelId));
  await refreshQuietly(guildId);
}

/** Owner only on the server. */
export async function renameServer(guildId: string, name: string): Promise<void> {
  await updateGuild(guildId, name.trim());
}

/** Owner only on the server. */
export async function deleteServer(guildId: string): Promise<void> {
  await deleteGuild(guildId);
}

/** Everyone but the owner (the owner must delete the server instead). */
export async function leaveServer(guildId: string): Promise<void> {
  await leaveGuild(guildId);
}
