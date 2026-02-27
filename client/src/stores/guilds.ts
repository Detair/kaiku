/**
 * Guild Store
 *
 * Manages guild (server) state for multi-server support.
 */

import { createStore } from "solid-js/store";
import type { Guild, GuildMember, GuildInvite, Channel } from "@/lib/types";
import * as tauri from "@/lib/tauri";
import { showToast } from "@/components/ui/Toast";

/**
 * Guild store state
 */
interface GuildStoreState {
  // All guilds the user is a member of
  guilds: Guild[];
  // Currently active/selected guild ID
  activeGuildId: string | null;
  // Members of the active guild
  members: Record<string, GuildMember[]>;
  // Invites for guilds (owner only)
  invites: Record<string, GuildInvite[]>;
  // Channels of the active guild
  guildChannels: Record<string, Channel[]>;
  // Unread message counts per guild (for sidebar badges)
  guildUnreadCounts: Record<string, number>;
  // Map channel IDs to guild IDs (for routing WebSocket events)
  channelGuildMap: Record<string, string>;
  // Loading states
  isLoading: boolean;
  isMembersLoading: boolean;
  isInvitesLoading: boolean;
  // Error state
  error: string | null;
}

// Sentinel value representing the discovery view in activeGuildId
const DISCOVERY_SENTINEL = "__discovery__";

// Create the store
const [guildsState, setGuildsState] = createStore<GuildStoreState>({
  guilds: [],
  activeGuildId: null,
  members: {},
  invites: {},
  guildChannels: {},
  guildUnreadCounts: {},
  channelGuildMap: {},
  isLoading: false,
  isMembersLoading: false,
  isInvitesLoading: false,
  error: null,
});

/**
 * Load all guilds for the current user
 */
export async function loadGuilds(): Promise<void> {
  setGuildsState({ isLoading: true, error: null });

  try {
    const guilds = await tauri.getGuilds();
    setGuildsState({ guilds, isLoading: false });

    // Prefetch channels for all guilds to populate channel→guild map and unread counts
    await loadAllGuildUnreadCounts(guilds);

    // If a real guild was active (not discovery sentinel), reload its data
    if (
      guildsState.activeGuildId &&
      guildsState.activeGuildId !== DISCOVERY_SENTINEL
    ) {
      await loadGuildMembers(guildsState.activeGuildId);
      await loadGuildChannels(guildsState.activeGuildId);
    }
  } catch (err) {
    console.error("Failed to load guilds:", err);
    setGuildsState({
      error: err instanceof Error ? err.message : "Failed to load servers",
      isLoading: false,
    });
  }
}

/**
 * Load guild members
 */
export async function loadGuildMembers(guildId: string): Promise<void> {
  setGuildsState({ isMembersLoading: true });

  try {
    const members = await tauri.getGuildMembers(guildId);
    setGuildsState("members", guildId, members);
    setGuildsState({ isMembersLoading: false });
  } catch (err) {
    console.error("Failed to load guild members:", err);
    setGuildsState({ isMembersLoading: false });
  }
}

/**
 * Load guild channels
 */
export async function loadGuildChannels(guildId: string): Promise<void> {
  try {
    const channels = await tauri.getGuildChannels(guildId);
    setGuildsState("guildChannels", guildId, channels);
    // Update channel→guild mapping
    for (const ch of channels) {
      setGuildsState("channelGuildMap", ch.id, guildId);
    }
  } catch (err) {
    console.error("Failed to load guild channels:", err);
  }
}

/**
 * Select/activate a guild
 * This will trigger channel list reload scoped to guild
 */
export async function selectGuild(guildId: string): Promise<void> {
  const previousGuildId = guildsState.activeGuildId;
  setGuildsState({ activeGuildId: guildId });
  // Clear guild-level unread badge when entering the guild
  clearGuildUnread(guildId);

  // Load channels for this guild (this will update the channels store)
  const { loadChannelsForGuild } = await import("./channels");
  await loadChannelsForGuild(guildId);

  // Load guild members
  await loadGuildMembers(guildId);

  // Check if we need to disconnect from voice
  // If user is in a voice channel from a different guild, disconnect
  if (previousGuildId && previousGuildId !== guildId) {
    const { voiceState } = await import("./voice");
    const { channelsState } = await import("./channels");

    if (voiceState.channelId) {
      const currentChannel = channelsState.channels.find(
        (c) => c.id === voiceState.channelId,
      );
      if (currentChannel && currentChannel.guild_id !== guildId) {
        const { leaveVoice } = await import("./voice");
        await leaveVoice();
      }
    }
  }
}

/**
 * Select "Home" view (no guild selected)
 * This shows DMs, mentions, and cross-server activity
 */
export async function selectHome(): Promise<void> {
  const previousGuildId = guildsState.activeGuildId;
  setGuildsState({ activeGuildId: null });

  // Load DM channels for home view
  const { loadDMChannels } = await import("./channels");
  await loadDMChannels();

  // Check if we need to disconnect from voice
  // If user is in a voice channel from a guild, disconnect
  if (previousGuildId) {
    const { voiceState } = await import("./voice");
    const { channelsState } = await import("./channels");

    if (voiceState.channelId) {
      const currentChannel = channelsState.channels.find(
        (c) => c.id === voiceState.channelId,
      );
      // If the voice channel belongs to a guild, disconnect
      if (currentChannel && currentChannel.guild_id !== null) {
        const { leaveVoice } = await import("./voice");
        await leaveVoice();
      }
    }
  }
}

/**
 * Get the currently active guild
 */
export function getActiveGuild(): Guild | null {
  if (
    !guildsState.activeGuildId ||
    guildsState.activeGuildId === DISCOVERY_SENTINEL
  )
    return null;
  return (
    guildsState.guilds.find((g) => g.id === guildsState.activeGuildId) ?? null
  );
}

/**
 * Get guild members for a specific guild
 */
export function getGuildMembers(guildId: string): GuildMember[] {
  return guildsState.members[guildId] || [];
}

/**
 * Get channels for a specific guild
 */
export function getGuildChannels(guildId: string): Channel[] {
  return guildsState.guildChannels[guildId] || [];
}

/**
 * Create a new guild
 */
export async function createGuild(
  name: string,
  description?: string,
): Promise<Guild> {
  const guild = await tauri.createGuild(name, description);
  setGuildsState("guilds", (prev) => [...prev, guild]);
  return guild;
}

/**
 * Update a guild
 */
export async function updateGuild(
  guildId: string,
  name?: string,
  description?: string,
  icon_url?: string,
): Promise<Guild> {
  const updated = await tauri.updateGuild(guildId, name, description, icon_url);
  setGuildsState("guilds", (g) => g.id === guildId, updated);
  return updated;
}

/**
 * Delete a guild
 */
export async function deleteGuild(guildId: string): Promise<void> {
  await tauri.deleteGuild(guildId);
  setGuildsState("guilds", (prev) => prev.filter((g) => g.id !== guildId));

  // If the deleted guild was active, select home
  if (guildsState.activeGuildId === guildId) {
    selectHome();
  }
}

/**
 * Join a guild with an invite code
 */
export async function joinGuild(
  guildId: string,
  inviteCode: string,
): Promise<void> {
  await tauri.joinGuild(guildId, inviteCode);
  await loadGuilds(); // Reload guilds to include the newly joined one
}

/**
 * Leave a guild
 */
export async function leaveGuild(guildId: string): Promise<void> {
  await tauri.leaveGuild(guildId);
  setGuildsState("guilds", (prev) => prev.filter((g) => g.id !== guildId));

  // If the left guild was active, select home
  if (guildsState.activeGuildId === guildId) {
    selectHome();
  }
}

// ============================================================================
// Invite Functions
// ============================================================================

/**
 * Load invites for a guild (owner only)
 */
export async function loadGuildInvites(guildId: string): Promise<void> {
  setGuildsState({ isInvitesLoading: true });

  try {
    const invites = await tauri.getGuildInvites(guildId);
    setGuildsState("invites", guildId, invites);
    setGuildsState({ isInvitesLoading: false });
  } catch (err) {
    console.error("Failed to load guild invites:", err);
    setGuildsState({ isInvitesLoading: false });
  }
}

/**
 * Create a new invite
 */
export async function createInvite(
  guildId: string,
  expiresIn: tauri.InviteExpiry = "7d",
): Promise<tauri.GuildInvite> {
  const invite = await tauri.createGuildInvite(guildId, expiresIn);
  setGuildsState("invites", guildId, (prev) => [invite, ...(prev || [])]);
  return invite;
}

/**
 * Delete an invite
 */
export async function deleteInvite(
  guildId: string,
  code: string,
): Promise<void> {
  await tauri.deleteGuildInvite(guildId, code);
  setGuildsState("invites", guildId, (prev) =>
    (prev || []).filter((i) => i.code !== code),
  );
}

/**
 * Join a guild via invite code
 */
export async function joinViaInviteCode(code: string): Promise<void> {
  // Join first — if this fails, the error is genuine
  const response = await tauri.joinViaInvite(code);

  // Post-join UI setup — join already succeeded at this point
  try {
    await loadGuilds();
    await selectGuild(response.guild_id);
  } catch (err) {
    console.error("Post-join UI setup failed (join succeeded):", err);
    showToast({
      type: "warning",
      title: "Joined Guild",
      message:
        "Guild joined successfully. Refresh the page if it doesn't appear.",
    });
    // Don't rethrow — the join worked, guild will appear on next load
  }
}

/**
 * Kick a member from a guild
 */
export async function kickMember(
  guildId: string,
  userId: string,
): Promise<void> {
  await tauri.kickGuildMember(guildId, userId);
  setGuildsState("members", guildId, (prev) =>
    (prev || []).filter((m) => m.user_id !== userId),
  );
}

/**
 * Get invites for a guild
 */
export function getGuildInvites(guildId: string): tauri.GuildInvite[] {
  return guildsState.invites[guildId] || [];
}

/**
 * Check if current user is guild owner
 */
export function isGuildOwner(guildId: string, userId: string): boolean {
  const guild = guildsState.guilds.find((g) => g.id === guildId);
  return guild?.owner_id === userId;
}

/**
 * Check if a channel belongs to the active guild
 */
export function isChannelInActiveGuild(channel: Channel): boolean {
  if (
    !guildsState.activeGuildId ||
    guildsState.activeGuildId === DISCOVERY_SENTINEL
  ) {
    // In home or discovery view, show DM channels only
    return channel.channel_type === "dm";
  }
  return channel.guild_id === guildsState.activeGuildId;
}

/**
 * Apply a partial patch to a guild's data.
 * Updates the guild in the store if it exists.
 */
export function patchGuild(
  guildId: string,
  diff: Record<string, unknown>,
): void {
  const guildIndex = guildsState.guilds.findIndex((g) => g.id === guildId);
  if (guildIndex === -1) {
    // Guild not in store, ignore patch
    return;
  }

  // Filter to only valid Guild fields
  const validFields: (keyof Guild)[] = [
    "name",
    "icon_url",
    "description",
    "owner_id",
    "threads_enabled",
    "discoverable",
    "tags",
    "banner_url",
  ];
  const updates: Partial<Guild> = {};
  for (const field of validFields) {
    if (field in diff) {
      (updates as Record<string, unknown>)[field] = diff[field];
    }
  }

  if (Object.keys(updates).length > 0) {
    setGuildsState("guilds", guildIndex, (prev) => ({ ...prev, ...updates }));
  }
}

/**
 * Check if threads are enabled for a guild.
 */
export function areThreadsEnabled(guildId: string | undefined | null): boolean {
  if (!guildId) return true; // DMs always allow threads
  const guild = guildsState.guilds.find((g) => g.id === guildId);
  return guild?.threads_enabled ?? true;
}

// ============================================================================
// Guild Unread Count Functions
// ============================================================================

/**
 * Load unread counts for all guilds by fetching their channels.
 */
async function loadAllGuildUnreadCounts(guilds: Guild[]): Promise<void> {
  await Promise.all(
    guilds.map(async (guild) => {
      try {
        const channels = await tauri.getGuildChannels(guild.id);
        setGuildsState("guildChannels", guild.id, channels);
        // Build channel→guild map
        for (const ch of channels) {
          setGuildsState("channelGuildMap", ch.id, guild.id);
        }
        // Sum unread counts from text channels
        const total = channels
          .filter((c) => c.channel_type === "text")
          .reduce((sum, c) => sum + (c.unread_count ?? 0), 0);
        setGuildsState("guildUnreadCounts", guild.id, total);
      } catch (err) {
        console.error(`Failed to load channels for guild ${guild.id}:`, err);
      }
    }),
  );
}

/**
 * Get unread count for a guild.
 */
export function getGuildUnreadCount(guildId: string): number {
  return guildsState.guildUnreadCounts[guildId] ?? 0;
}

/**
 * Increment unread count for a guild (called from WebSocket handler).
 */
export function incrementGuildUnread(guildId: string): void {
  setGuildsState("guildUnreadCounts", guildId, (prev) => (prev ?? 0) + 1);
}

/**
 * Clear unread count for a guild (called when entering the guild).
 */
export function clearGuildUnread(guildId: string): void {
  setGuildsState("guildUnreadCounts", guildId, 0);
}

/**
 * Look up which guild a channel belongs to.
 */
export function getGuildIdForChannel(channelId: string): string | undefined {
  return guildsState.channelGuildMap[channelId];
}

// ============================================================================
// Discovery Helpers
// ============================================================================

/**
 * Select the discovery view (browse public guilds).
 */
export async function selectDiscovery(): Promise<void> {
  const previousGuildId = guildsState.activeGuildId;
  setGuildsState({ activeGuildId: DISCOVERY_SENTINEL });

  // Disconnect from voice only if in a guild voice channel (preserve DM calls)
  if (previousGuildId && previousGuildId !== DISCOVERY_SENTINEL) {
    const { voiceState } = await import("./voice");
    const { channelsState } = await import("./channels");

    if (voiceState.channelId) {
      const currentChannel = channelsState.channels.find(
        (c) => c.id === voiceState.channelId,
      );
      if (currentChannel && currentChannel.guild_id !== null) {
        try {
          const { leaveVoice } = await import("./voice");
          await leaveVoice();
        } catch (err) {
          console.error(
            "Failed to leave voice when switching to discovery:",
            err,
          );
          showToast({
            type: "warning",
            title: "Voice Disconnect Failed",
            message:
              "Could not disconnect from voice. You may still be in the voice channel.",
          });
        }
      }
    }
  }
}

/**
 * Check if discovery view is currently active.
 */
export function isDiscoveryActive(): boolean {
  return guildsState.activeGuildId === DISCOVERY_SENTINEL;
}

// Export the store for reading and modifying (for members store)
export { guildsState, setGuildsState };
