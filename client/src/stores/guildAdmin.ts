/**
 * Server and channel admin actions for the settings windows (ADMIN-5).
 * Kept apart from `stores/guilds.ts` and `stores/channels.ts`: these only
 * call their exported functions to refresh what the sidebar shows.
 * Every function throws on failure so the dialog can say why.
 */
import { renameChannel as apiRenameChannel, deleteChannel as apiDeleteChannel } from "@/lib/tauri/channelAdmin";
import { channelsState, clearSelection, refreshChannelsForGuild } from "@/stores/channels";
import { deleteGuild, leaveGuild, updateGuild } from "@/stores/guilds";

export async function renameChannel(guildId: string, channelId: string, name: string): Promise<void> {
  await apiRenameChannel(channelId, name.trim());
  await refreshChannelsForGuild(guildId);
}

export async function deleteChannel(guildId: string, channelId: string): Promise<void> {
  await apiDeleteChannel(channelId);
  if (channelsState.selectedChannelId === channelId) clearSelection();
  await refreshChannelsForGuild(guildId);
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
