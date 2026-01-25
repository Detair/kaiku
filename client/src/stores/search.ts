/**
 * Search Store
 *
 * Manages message search state including results, loading, and pagination.
 */

import { createStore } from "solid-js/store";
import type { SearchResult } from "@/lib/types";
import { searchGuildMessages } from "@/lib/tauri";

// ============================================================================
// Types
// ============================================================================

interface SearchState {
  /** Current search query */
  query: string;
  /** Search results */
  results: SearchResult[];
  /** Total number of results (for pagination) */
  total: number;
  /** Current offset */
  offset: number;
  /** Results per page */
  limit: number;
  /** Loading state */
  isSearching: boolean;
  /** Error message if search failed */
  error: string | null;
  /** Guild ID being searched */
  guildId: string | null;
}

// ============================================================================
// Store
// ============================================================================

const [searchState, setSearchState] = createStore<SearchState>({
  query: "",
  results: [],
  total: 0,
  offset: 0,
  limit: 25,
  isSearching: false,
  error: null,
  guildId: null,
});

// ============================================================================
// Actions
// ============================================================================

/**
 * Search messages in a guild.
 */
export async function search(guildId: string, query: string): Promise<void> {
  // Skip if query is too short
  if (query.trim().length < 2) {
    setSearchState({
      query: query.trim(),
      results: [],
      total: 0,
      offset: 0,
      error: null,
      isSearching: false,
      guildId,
    });
    return;
  }

  setSearchState({
    query: query.trim(),
    isSearching: true,
    error: null,
    guildId,
    offset: 0,
  });

  try {
    const response = await searchGuildMessages(
      guildId,
      query.trim(),
      searchState.limit,
      0
    );

    setSearchState({
      results: response.results,
      total: response.total,
      offset: 0,
      isSearching: false,
    });
  } catch (err) {
    console.error("Search failed:", err);
    setSearchState({
      results: [],
      total: 0,
      error: err instanceof Error ? err.message : "Search failed",
      isSearching: false,
    });
  }
}

/**
 * Load more results (pagination).
 */
export async function loadMore(): Promise<void> {
  if (!searchState.guildId || !searchState.query || searchState.isSearching) {
    return;
  }

  // Check if there are more results
  if (searchState.offset + searchState.limit >= searchState.total) {
    return;
  }

  const newOffset = searchState.offset + searchState.limit;

  setSearchState({ isSearching: true, error: null });

  try {
    const response = await searchGuildMessages(
      searchState.guildId,
      searchState.query,
      searchState.limit,
      newOffset
    );

    setSearchState({
      results: [...searchState.results, ...response.results],
      total: response.total,
      offset: newOffset,
      isSearching: false,
    });
  } catch (err) {
    console.error("Load more failed:", err);
    setSearchState({
      error: err instanceof Error ? err.message : "Load more failed",
      isSearching: false,
    });
  }
}

/**
 * Clear search results.
 */
export function clearSearch(): void {
  setSearchState({
    query: "",
    results: [],
    total: 0,
    offset: 0,
    error: null,
    isSearching: false,
    guildId: null,
  });
}

/**
 * Check if there are more results to load.
 */
export function hasMore(): boolean {
  return searchState.offset + searchState.limit < searchState.total;
}

// ============================================================================
// Export
// ============================================================================

export { searchState };
