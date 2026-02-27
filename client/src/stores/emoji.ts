/**
 * Emoji Store
 *
 * Manages emoji state including recents, guild emojis, and search functionality.
 */

import { createStore } from "solid-js/store";
import type { GuildEmoji } from "@/lib/types";
import * as tauri from "@/lib/tauri";

// ============================================================================
// Types
// ============================================================================

export interface Emoji {
  /** Unicode emoji character or custom emoji ID */
  id: string;
  /** Display name */
  name: string;
  /** Category (e.g., "smileys", "people", "animals") */
  category: string;
  /** Keywords for search */
  keywords?: string[];
  /** Whether this is a custom guild emoji */
  isCustom?: boolean;
  /** URL for custom emoji image */
  imageUrl?: string;
}

// ============================================================================
// Constants
// ============================================================================

/** Maximum number of recent emojis to store */
const MAX_RECENTS = 20;

/** Local storage key for recent emojis */
const RECENTS_STORAGE_KEY = "emoji_recents";

// ============================================================================
// State
// ============================================================================

interface EmojiState {
  /** Recently used emojis */
  recents: Emoji[];
  /** Favorite emojis */
  favorites: string[];
  /** Search query */
  searchQuery: string;
  /** Loading state */
  isLoading: boolean;
  /** Guild custom emojis by guild ID */
  guildEmojis: Record<string, GuildEmoji[]>;
}

const [emojiState, setEmojiState] = createStore<EmojiState>({
  recents: [],
  favorites: [],
  searchQuery: "",
  isLoading: false,
  guildEmojis: {},
});

// ============================================================================
// Pure Helper Functions (Testable)
// ============================================================================

/**
 * Add an emoji to recents array (pure function for testing).
 * Returns a new array with the emoji at the front, limited to MAX_RECENTS.
 * If the emoji already exists, it's moved to the front.
 */
export function addEmojiToRecentsArray(
  recents: Emoji[],
  emoji: Emoji,
  maxRecents: number = MAX_RECENTS,
): Emoji[] {
  // Filter out existing instance of this emoji
  const filtered = recents.filter((e) => e.id !== emoji.id);
  // Add to front and limit size
  return [emoji, ...filtered].slice(0, maxRecents);
}

/**
 * Search emojis by name or keywords (pure function for testing).
 * Returns emojis that match the query.
 */
export function searchEmojisInArray(emojis: Emoji[], query: string): Emoji[] {
  if (!query.trim()) {
    return emojis;
  }

  const normalizedQuery = query.toLowerCase().trim();

  return emojis.filter((emoji) => {
    // Check name
    if (emoji.name.toLowerCase().includes(normalizedQuery)) {
      return true;
    }
    // Check keywords
    if (
      emoji.keywords?.some((kw) => kw.toLowerCase().includes(normalizedQuery))
    ) {
      return true;
    }
    return false;
  });
}

// ============================================================================
// Store Actions
// ============================================================================

/**
 * Load recents from local storage.
 */
export function loadRecentsFromStorage(): void {
  try {
    const stored = localStorage.getItem(RECENTS_STORAGE_KEY);
    if (stored) {
      const parsed = JSON.parse(stored) as Emoji[];
      setEmojiState("recents", parsed.slice(0, MAX_RECENTS));
    }
  } catch (err) {
    console.warn("Failed to load emoji recents from storage:", err);
  }
}

/**
 * Save recents to local storage.
 */
function saveRecentsToStorage(recents: Emoji[]): void {
  try {
    localStorage.setItem(RECENTS_STORAGE_KEY, JSON.stringify(recents));
  } catch (err) {
    console.warn("Failed to save emoji recents to storage:", err);
  }
}

/**
 * Add an emoji to recents.
 */
export function addToRecents(emoji: Emoji): void {
  const newRecents = addEmojiToRecentsArray(
    emojiState.recents,
    emoji,
    MAX_RECENTS,
  );
  setEmojiState("recents", newRecents);
  saveRecentsToStorage(newRecents);
}

/**
 * Add an emoji string to recents (convenience function for simple emoji characters).
 */
export function addEmojiStringToRecents(emojiChar: string): void {
  const emoji: Emoji = {
    id: emojiChar,
    name: emojiChar,
    category: "recent",
  };
  addToRecents(emoji);
}

/**
 * Clear all recents.
 */
export function clearRecents(): void {
  setEmojiState("recents", []);
  try {
    localStorage.removeItem(RECENTS_STORAGE_KEY);
  } catch (err) {
    console.warn("Failed to clear emoji recents from storage:", err);
  }
}

/**
 * Get recents.
 */
export function getRecents(): Emoji[] {
  return emojiState.recents;
}

/**
 * Set search query.
 */
export function setSearchQuery(query: string): void {
  setEmojiState("searchQuery", query);
}

/**
 * Get search query.
 */
export function getSearchQuery(): string {
  return emojiState.searchQuery;
}

// ============================================================================
// Guild Emojis
// ============================================================================

/**
 * Set custom emojis for a guild.
 */
export function setGuildEmojis(guildId: string, emojis: GuildEmoji[]): void {
  setEmojiState("guildEmojis", guildId, emojis);
}

/**
 * Get custom emojis for a guild.
 */
export function getGuildEmojis(guildId: string): GuildEmoji[] {
  return emojiState.guildEmojis[guildId] ?? [];
}

/**
 * Load emojis for a guild from API
 */
export async function loadGuildEmojis(guildId: string): Promise<void> {
  const emojis = await tauri.getGuildEmojis(guildId);
  setGuildEmojis(guildId, emojis);
}

/**
 * Upload a new guild emoji
 */
export async function uploadEmoji(
  guildId: string,
  name: string,
  file: File,
): Promise<void> {
  const emoji = await tauri.uploadGuildEmoji(guildId, name, file);
  setEmojiState("guildEmojis", guildId, (prev) => [emoji, ...(prev || [])]);
}

/**
 * Update a guild emoji
 */
export async function updateEmoji(
  guildId: string,
  emojiId: string,
  name: string,
): Promise<void> {
  const updated = await tauri.updateGuildEmoji(guildId, emojiId, name);
  setEmojiState("guildEmojis", guildId, (prev) =>
    (prev || []).map((e) => (e.id === emojiId ? updated : e)),
  );
}

/**
 * Delete a guild emoji
 */
export async function deleteEmoji(
  guildId: string,
  emojiId: string,
): Promise<void> {
  await tauri.deleteGuildEmoji(guildId, emojiId);
  setEmojiState("guildEmojis", guildId, (prev) =>
    (prev || []).filter((e) => e.id !== emojiId),
  );
}

// ============================================================================
// Export
// ============================================================================

export { emojiState, MAX_RECENTS };
