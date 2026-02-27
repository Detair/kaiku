/**
 * Channels Store
 *
 * Manages channel list and selection state, including unread tracking.
 */

import { createStore } from "solid-js/store";
import type { ChannelWithUnread } from "@/lib/types";
import * as tauri from "@/lib/tauri";
import { subscribeChannel } from "@/stores/websocket";
import { showToast } from "@/components/ui/Toast";

// Channels state interface
interface ChannelsState {
  channels: ChannelWithUnread[];
  selectedChannelId: string | null;
  isLoading: boolean;
  error: string | null;
}

// Create the store
const [channelsState, setChannelsState] = createStore<ChannelsState>({
  channels: [],
  selectedChannelId: null,
  isLoading: false,
  error: null,
});

// Derived state

export const selectedChannel = () =>
  channelsState.channels.find((c) => c.id === channelsState.selectedChannelId);

export const textChannels = () =>
  channelsState.channels
    .filter((c) => c.channel_type === "text")
    .sort((a, b) => a.position - b.position);

export const voiceChannels = () =>
  channelsState.channels
    .filter((c) => c.channel_type === "voice")
    .sort((a, b) => a.position - b.position);

// Actions

/**
 * Load channels from server (all channels - legacy).
 * Use loadChannelsForGuild for guild-scoped loading.
 */
export async function loadChannels(): Promise<void> {
  setChannelsState({ isLoading: true, error: null });

  try {
    const rawChannels = await tauri.getChannels();
    // Map to ChannelWithUnread (legacy endpoint doesn't return unread counts)
    const channels: ChannelWithUnread[] = rawChannels.map((c) => ({
      ...c,
      unread_count: 0,
    }));
    setChannelsState({
      channels,
      isLoading: false,
      error: null,
    });

    // Auto-select first text channel if none selected
    if (!channelsState.selectedChannelId && channels.length > 0) {
      const firstText = channels.find((c) => c.channel_type === "text");
      if (firstText) {
        setChannelsState({ selectedChannelId: firstText.id });
      }
    }
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to load channels:", error);
    setChannelsState({ isLoading: false, error });
  }
}

/**
 * Load channels for a specific guild.
 * This replaces the current channel list with the guild's channels.
 */
export async function loadChannelsForGuild(guildId: string): Promise<void> {
  setChannelsState({ isLoading: true, error: null });

  try {
    const channels = await tauri.getGuildChannels(guildId);
    setChannelsState({
      channels,
      isLoading: false,
      error: null,
    });

    // Auto-select first text channel in this guild
    if (channels.length > 0) {
      const firstText = channels.find((c) => c.channel_type === "text");
      if (firstText) {
        setChannelsState({ selectedChannelId: firstText.id });
      } else {
        // No text channels, clear selection
        setChannelsState({ selectedChannelId: null });
      }
    } else {
      // No channels in this guild, clear selection
      setChannelsState({ selectedChannelId: null });
    }

    // Subscribe to all text channels for real-time message updates
    // Wait for WebSocket to be connected first
    const maxWaitMs = 5000;
    const pollIntervalMs = 100;
    let waited = 0;

    while (waited < maxWaitMs) {
      const status = await tauri.wsStatus();
      if (status.type === "connected") {
        break;
      }
      await new Promise((resolve) => setTimeout(resolve, pollIntervalMs));
      waited += pollIntervalMs;
    }

    const finalStatus = await tauri.wsStatus();
    if (finalStatus.type !== "connected") {
      console.warn(
        "[Channels] WebSocket not connected, skipping subscriptions",
      );
      return;
    }

    // Subscribe to text channels
    for (const channel of channels.filter((c) => c.channel_type === "text")) {
      try {
        await subscribeChannel(channel.id);
        console.log(`[Channels] Subscribed to channel ${channel.name}`);
      } catch (err) {
        console.warn(`Failed to subscribe to channel ${channel.id}:`, err);
      }
    }
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to load guild channels:", error);
    setChannelsState({ isLoading: false, error });
  }
}

/**
 * Load DM channels (for Home view).
 * This will load channels where guild_id is null.
 */
export async function loadDMChannels(): Promise<void> {
  setChannelsState({ isLoading: true, error: null });

  try {
    // For now, use the generic getChannels and filter for DMs
    // In Phase 3 Task 5, this will use a dedicated /api/dm endpoint
    const allChannels = await tauri.getChannels();
    // Map to ChannelWithUnread (legacy endpoint doesn't return unread counts)
    const dmChannels: ChannelWithUnread[] = allChannels
      .filter((c) => c.guild_id === null)
      .map((c) => ({ ...c, unread_count: 0 }));

    setChannelsState({
      channels: dmChannels,
      isLoading: false,
      error: null,
    });

    // Auto-select first DM channel
    if (dmChannels.length > 0) {
      const firstDM = dmChannels.find((c) => c.channel_type === "dm");
      if (firstDM) {
        setChannelsState({ selectedChannelId: firstDM.id });
      } else {
        setChannelsState({ selectedChannelId: null });
      }
    } else {
      setChannelsState({ selectedChannelId: null });
    }
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to load DM channels:", error);
    setChannelsState({ isLoading: false, error });
  }
}

/**
 * Select a channel.
 */
export function selectChannel(channelId: string): void {
  setChannelsState({ selectedChannelId: channelId });
}

/**
 * Clear channel selection.
 */
export function clearSelection(): void {
  setChannelsState({ selectedChannelId: null });
}

/**
 * Find a channel by ID.
 */
export function getChannel(channelId: string): ChannelWithUnread | undefined {
  return channelsState.channels.find((c) => c.id === channelId);
}

/**
 * Get unread count for a specific channel.
 */
export function getUnreadCount(channelId: string): number {
  const channel = channelsState.channels.find((c) => c.id === channelId);
  return channel?.unread_count ?? 0;
}

/**
 * Get total unread count across all text channels.
 */
export function getTotalUnreadCount(): number {
  return channelsState.channels
    .filter((c) => c.channel_type === "text")
    .reduce((sum, c) => sum + (c.unread_count ?? 0), 0);
}

/**
 * Increment unread count for a channel (called when new message arrives).
 */
export function incrementUnreadCount(channelId: string): void {
  const idx = channelsState.channels.findIndex((c) => c.id === channelId);
  if (idx !== -1) {
    setChannelsState(
      "channels",
      idx,
      "unread_count",
      (count) => (count ?? 0) + 1,
    );
  }
}

/**
 * Mark a channel as read (reset unread count to 0).
 */
export async function markChannelAsRead(channelId: string): Promise<void> {
  const idx = channelsState.channels.findIndex((c) => c.id === channelId);
  if (idx !== -1 && channelsState.channels[idx].unread_count > 0) {
    // Optimistic update
    setChannelsState("channels", idx, "unread_count", 0);

    try {
      await tauri.markChannelAsRead(channelId);
    } catch (err) {
      console.error("[Channels] Failed to mark channel as read:", err);
      showToast({
        type: "error",
        title: "Failed to Mark as Read",
        message: "Could not mark channel as read. Will retry on next message.",
        duration: 8000,
      });
      // Could revert here, but the server state is source of truth
    }
  }
}

/**
 * Mark all guild channels as read (optimistic update + API call).
 */
export async function markAllGuildChannelsAsRead(
  guildId: string,
): Promise<void> {
  // Optimistic update: zero out all unread counts for this guild's text channels
  const indices: number[] = [];
  channelsState.channels.forEach((c, idx) => {
    if (
      c.guild_id === guildId &&
      c.channel_type === "text" &&
      c.unread_count > 0
    ) {
      indices.push(idx);
    }
  });
  for (const idx of indices) {
    setChannelsState("channels", idx, "unread_count", 0);
  }

  try {
    await tauri.markAllGuildChannelsRead(guildId);
  } catch (err) {
    console.error("[Channels] Failed to mark all guild channels as read:", err);
    showToast({
      type: "error",
      title: "Mark All Read Failed",
      message: "Could not mark all channels as read. Please try again.",
      duration: 8000,
    });
  }
}

/**
 * Handle channel_read event from WebSocket (cross-device sync).
 */
export function handleChannelReadEvent(channelId: string): void {
  const idx = channelsState.channels.findIndex((c) => c.id === channelId);
  if (idx !== -1) {
    setChannelsState("channels", idx, "unread_count", 0);
  }
}

/**
 * Create a new channel in a guild.
 */
export async function createChannel(
  name: string,
  channelType: "text" | "voice",
  guildId?: string,
  topic?: string,
  categoryId?: string,
): Promise<ChannelWithUnread> {
  const channel = await tauri.createChannel(
    name,
    channelType,
    guildId,
    topic,
    categoryId,
  );
  const channelWithUnread: ChannelWithUnread = { ...channel, unread_count: 0 };
  setChannelsState("channels", (prev) => [...prev, channelWithUnread]);
  return channelWithUnread;
}

/**
 * Move a channel to a different position within its category or to another category.
 * Performs optimistic local update and persists to server.
 *
 * @param channelId - The channel being moved
 * @param targetChannelId - The target channel (to drop before/after)
 * @param position - 'before' or 'after' relative to target
 * @param newCategoryId - Optional new category ID (for moving between categories)
 */
export async function moveChannel(
  channelId: string,
  targetChannelId: string,
  position: "before" | "after",
  newCategoryId?: string | null,
): Promise<void> {
  const channel = channelsState.channels.find((c) => c.id === channelId);
  const targetChannel = channelsState.channels.find(
    (c) => c.id === targetChannelId,
  );

  if (!channel || !targetChannel) {
    console.error("Channel not found for move");
    return;
  }

  const guildId = channel.guild_id;
  if (!guildId) {
    console.error("Cannot reorder channel without guild_id");
    return;
  }

  // Determine the target category
  const targetCategoryId =
    newCategoryId !== undefined ? newCategoryId : targetChannel.category_id;

  // Get channels in the target category
  const categoryChannels = channelsState.channels
    .filter((c) => c.category_id === targetCategoryId && c.id !== channelId)
    .sort((a, b) => a.position - b.position);

  // Find where to insert
  const targetIndex = categoryChannels.findIndex(
    (c) => c.id === targetChannelId,
  );
  const insertIndex = position === "before" ? targetIndex : targetIndex + 1;

  // Insert the moved channel
  categoryChannels.splice(insertIndex, 0, {
    ...channel,
    category_id: targetCategoryId,
  });

  // Build updated channels with new positions
  const updatedChannels = channelsState.channels.map((c) => {
    const newIndex = categoryChannels.findIndex((cat) => cat.id === c.id);
    if (newIndex !== -1) {
      return {
        ...c,
        position: newIndex,
        category_id: c.id === channelId ? targetCategoryId : c.category_id,
      };
    }
    return c;
  });

  // Optimistic local update
  setChannelsState("channels", updatedChannels);

  // Prepare channel positions for server - only channels in the affected category
  const channelPositions: tauri.ChannelPosition[] = categoryChannels.map(
    (c, idx) => ({
      id: c.id,
      position: idx,
      category_id: c.id === channelId ? targetCategoryId : c.category_id,
    }),
  );

  // Persist to server
  try {
    await tauri.reorderGuildChannels(guildId, channelPositions);
    console.log("[Channels] Channel reordered and persisted to server");
  } catch (err) {
    console.error("[Channels] Failed to persist channel reorder:", err);
    // Reload channels to sync with server state
    if (guildId) {
      await loadChannelsForGuild(guildId);
    }
  }
}

/**
 * Move a channel to a different category.
 * Performs optimistic local update and persists to server.
 *
 * @param channelId - The channel being moved
 * @param newCategoryId - The new category ID (null for uncategorized)
 */
export async function moveChannelToCategory(
  channelId: string,
  newCategoryId: string | null,
): Promise<void> {
  const channel = channelsState.channels.find((c) => c.id === channelId);

  if (!channel) {
    console.error("Channel not found for category move");
    return;
  }

  const guildId = channel.guild_id;
  if (!guildId) {
    console.error("Cannot reorder channel without guild_id");
    return;
  }

  // Get channels in the target category to determine position
  const categoryChannels = channelsState.channels
    .filter((c) => c.category_id === newCategoryId)
    .sort((a, b) => a.position - b.position);

  // Add to end of category
  const newPosition = categoryChannels.length;

  const updatedChannels = channelsState.channels.map((c) => {
    if (c.id === channelId) {
      return {
        ...c,
        category_id: newCategoryId,
        position: newPosition,
      };
    }
    return c;
  });

  // Optimistic local update
  setChannelsState("channels", updatedChannels);

  // Persist to server - just the moved channel
  const channelPositions: tauri.ChannelPosition[] = [
    {
      id: channelId,
      position: newPosition,
      category_id: newCategoryId,
    },
  ];

  try {
    await tauri.reorderGuildChannels(guildId, channelPositions);
    console.log("[Channels] Channel moved to category and persisted to server");
  } catch (err) {
    console.error("[Channels] Failed to persist category move:", err);
    // Reload channels to sync with server state
    if (guildId) {
      await loadChannelsForGuild(guildId);
    }
  }
}

// Export the store for reading
export { channelsState, setChannelsState };
