/**
 * Guild Store
 *
 * Manages guild (server) state for multi-server support.
 */

import { createStore } from "solid-js/store";
import type { Guild, GuildMember, Channel } from "@/lib/types";
import * as tauri from "@/lib/tauri";

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
  // Channels of the active guild
  guildChannels: Record<string, Channel[]>;
  // Loading states
  isLoading: boolean;
  isMembersLoading: boolean;
  // Error state
  error: string | null;
}

// Create the store
const [guildsState, setGuildsState] = createStore<GuildStoreState>({
  guilds: [],
  activeGuildId: null,
  members: {},
  guildChannels: {},
  isLoading: false,
  isMembersLoading: false,
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

    // If a guild was active, reload its data
    if (guildsState.activeGuildId) {
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
        (c) => c.id === voiceState.channelId
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
        (c) => c.id === voiceState.channelId
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
  if (!guildsState.activeGuildId) return null;
  return guildsState.guilds.find((g) => g.id === guildsState.activeGuildId) ?? null;
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
  description?: string
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
  icon_url?: string
): Promise<Guild> {
  const updated = await tauri.updateGuild(guildId, name, description, icon_url);
  setGuildsState(
    "guilds",
    (g) => g.id === guildId,
    updated
  );
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
export async function joinGuild(guildId: string, inviteCode: string): Promise<void> {
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

/**
 * Check if a channel belongs to the active guild
 */
export function isChannelInActiveGuild(channel: Channel): boolean {
  if (!guildsState.activeGuildId) {
    // In home view, show DM channels only
    return channel.channel_type === "dm";
  }
  return channel.guild_id === guildsState.activeGuildId;
}

// Export the store for reading
export { guildsState };
