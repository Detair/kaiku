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
import { getChannelOverrides, getGuildRoles, getMemberRoleIds } from "@/stores/permissions";
import { PermissionBits } from "@/lib/permissionConstants";

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

/**
 * May this member rename or delete this channel? The same rule as the
 * server's `compute_guild_permissions` with channel overrides (K8):
 * @everyone and role permissions, then the @everyone override's deny, then
 * the member's role overrides (deny, then allow). An @everyone allow is
 * masked by the server and not mirrored here. Overrides that are not loaded
 * count as none, which matches a channel without overrides.
 */
export function canManageChannel(
  guildId: string,
  channelId: string,
  userId: string,
  isOwner: boolean,
): boolean {
  if (isOwner) return true;
  const roles = getGuildRoles(guildId);
  const mine = getMemberRoleIds(guildId, userId);
  const everyone = roles.find((r) => r.is_default);
  let perms = everyone?.permissions ?? 0;
  for (const role of roles) {
    if (mine.includes(role.id)) perms |= role.permissions;
  }
  const overrides = getChannelOverrides(channelId);
  const everyoneOverride = everyone ? overrides.find((o) => o.role_id === everyone.id) : undefined;
  if (everyoneOverride) perms &= ~everyoneOverride.deny_permissions;
  let allow = 0;
  let deny = 0;
  for (const o of overrides) {
    if (o === everyoneOverride || !mine.includes(o.role_id)) continue;
    allow |= o.allow_permissions;
    deny |= o.deny_permissions;
  }
  perms &= ~deny;
  perms |= allow;
  return (perms & PermissionBits.MANAGE_CHANNELS) !== 0;
}
