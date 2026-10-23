/**
 * Favorites Store
 *
 * Manages user's cross-server channel favorites.
 */

import { createSignal, createMemo } from "solid-js";
import type { FavoriteChannel, Favorite } from "@/lib/types";
import { showToast } from "@/components/ui/Toast";
import { httpRequest } from "@/lib/tauri/common";

// ============================================================================
// State
// ============================================================================

const [favorites, setFavorites] = createSignal<FavoriteChannel[]>([]);
const [isLoading, setIsLoading] = createSignal(false);

// ============================================================================
// Computed
// ============================================================================

/**
 * Favorites grouped by guild, sorted by guild_position then channel_position.
 */
export const favoritesByGuild = createMemo(() => {
  const grouped = new Map<
    string,
    {
      guild: { id: string; name: string; icon: string | null };
      channels: FavoriteChannel[];
    }
  >();

  for (const fav of favorites()) {
    if (!grouped.has(fav.guild_id)) {
      grouped.set(fav.guild_id, {
        guild: { id: fav.guild_id, name: fav.guild_name, icon: fav.guild_icon },
        channels: [],
      });
    }
    grouped.get(fav.guild_id)!.channels.push(fav);
  }

  // Sort channels within each guild by channel_position
  for (const group of grouped.values()) {
    group.channels.sort((a, b) => a.channel_position - b.channel_position);
  }

  // Convert to array and sort by guild_position
  return Array.from(grouped.values()).sort((a, b) => {
    const posA = a.channels[0]?.guild_position ?? 0;
    const posB = b.channels[0]?.guild_position ?? 0;
    return posA - posB;
  });
});

/**
 * Check if a channel is favorited.
 */
export function isFavorited(channelId: string): boolean {
  return favorites().some((f) => f.channel_id === channelId);
}

// ============================================================================
// API Calls
// ============================================================================

async function apiCall<T>(
  method: string,
  endpoint: string,
  body?: unknown,
): Promise<T> {
  return httpRequest<T>(method, endpoint, body);
}

// ============================================================================
// Actions
// ============================================================================

export async function loadFavorites(): Promise<void> {
  setIsLoading(true);
  try {
    const response = await apiCall<
      FavoriteChannel[] | { favorites: FavoriteChannel[] }
    >("GET", "/api/me/favorites");
    // Handle both shapes: a bare array or { favorites: [...] }
    const data = Array.isArray(response) ? response : response.favorites;
    setFavorites(data);
  } catch (error) {
    console.error("Failed to load favorites:", error);
  } finally {
    setIsLoading(false);
  }
}

export async function addFavorite(
  channelId: string,
  _guildId: string,
  guildName: string,
  guildIcon: string | null,
  channelName: string,
  channelType: "text" | "voice",
): Promise<boolean> {
  try {
    const result = await apiCall<Favorite>(
      "POST",
      `/api/me/favorites/${channelId}`,
    );

    // Add to local state
    setFavorites((prev) => [
      ...prev,
      {
        channel_id: result.channel_id,
        channel_name: channelName,
        channel_type: channelType,
        guild_id: result.guild_id,
        guild_name: guildName,
        guild_icon: guildIcon,
        guild_position: result.guild_position,
        channel_position: result.channel_position,
      },
    ]);
    return true;
  } catch (error) {
    const message =
      error instanceof Error ? error.message : "Failed to add favorite";
    console.error("Failed to add favorite:", message);
    showToast({
      type: "error",
      title: "Favorite Failed",
      message: "Could not add favorite. Please try again.",
      duration: 8000,
    });
    throw new Error(message, { cause: error });
  }
}

export async function removeFavorite(channelId: string): Promise<boolean> {
  try {
    await apiCall("DELETE", `/api/me/favorites/${channelId}`);
    setFavorites((prev) => prev.filter((f) => f.channel_id !== channelId));
    return true;
  } catch (error) {
    const message =
      error instanceof Error ? error.message : "Failed to remove favorite";
    console.error("Failed to remove favorite:", message);
    showToast({
      type: "error",
      title: "Unfavorite Failed",
      message: "Could not remove favorite. Please try again.",
      duration: 8000,
    });
    throw new Error(message, { cause: error });
  }
}

export async function toggleFavorite(
  channelId: string,
  guildId: string,
  guildName: string,
  guildIcon: string | null,
  channelName: string,
  channelType: "text" | "voice",
): Promise<boolean> {
  try {
    if (isFavorited(channelId)) {
      await removeFavorite(channelId);
    } else {
      await addFavorite(
        channelId,
        guildId,
        guildName,
        guildIcon,
        channelName,
        channelType,
      );
    }
    return true;
  } catch {
    // Error already logged in add/remove
    return false;
  }
}

export async function reorderChannels(
  guildId: string,
  channelIds: string[],
): Promise<boolean> {
  try {
    await apiCall("PUT", "/api/me/favorites/reorder", {
      guild_id: guildId,
      channel_ids: channelIds,
    });

    // Update local state positions
    setFavorites((prev) => {
      return prev.map((f) => {
        if (f.guild_id === guildId) {
          const newPos = channelIds.indexOf(f.channel_id);
          return newPos >= 0 ? { ...f, channel_position: newPos } : f;
        }
        return f;
      });
    });
    return true;
  } catch (error) {
    console.error("Failed to reorder channels:", error);
    return false;
  }
}

export async function reorderGuilds(guildIds: string[]): Promise<boolean> {
  try {
    await apiCall("PUT", "/api/me/favorites/reorder-guilds", {
      guild_ids: guildIds,
    });

    // Update local state positions
    setFavorites((prev) => {
      return prev.map((f) => {
        const newPos = guildIds.indexOf(f.guild_id);
        return newPos >= 0 ? { ...f, guild_position: newPos } : f;
      });
    });
    return true;
  } catch (error) {
    console.error("Failed to reorder guilds:", error);
    return false;
  }
}

// ============================================================================
// Selectors
// ============================================================================

export { favorites, isLoading };
