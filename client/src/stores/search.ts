/**
 * Search Store
 *
 * Manages message search state including results, loading, and pagination.
 * Supports guild, DM, and global search contexts.
 */

import { createSignal } from "solid-js";
import { createStore } from "solid-js/store";
import type {
  SearchResult,
  SearchFilters,
  GlobalSearchResult,
} from "@/lib/types";
import {
  searchGuildMessages,
  searchDMMessages,
  searchGlobalMessages,
} from "@/lib/tauri";

// ============================================================================
// Types
// ============================================================================

interface SearchState {
  /** Current search query */
  query: string;
  /** Search results */
  results: (SearchResult | GlobalSearchResult)[];
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
  /** Guild ID being searched (for guild context) */
  guildId: string | null;
  /** Search context: guild, dm, or global */
  context: "guild" | "dm" | "global";
  /** Active search filters */
  filters: SearchFilters;
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
  context: "guild",
  filters: {},
});

// Global search visibility signal
const [showGlobalSearch, setShowGlobalSearch] = createSignal(false);

// ============================================================================
// Actions
// ============================================================================

/**
 * Search messages in a guild.
 */
export async function search(
  guildId: string,
  query: string,
  filters: SearchFilters = {},
): Promise<void> {
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
      context: "guild",
      filters,
    });
    return;
  }

  setSearchState({
    query: query.trim(),
    isSearching: true,
    error: null,
    guildId,
    context: "guild",
    offset: 0,
    filters,
  });

  try {
    const response = await searchGuildMessages(
      guildId,
      query.trim(),
      searchState.limit,
      0,
      filters,
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
 * Search messages in DM channels.
 */
export async function searchDMs(
  query: string,
  filters: SearchFilters = {},
): Promise<void> {
  // Skip if query is too short
  if (query.trim().length < 2) {
    setSearchState({
      query: query.trim(),
      results: [],
      total: 0,
      offset: 0,
      error: null,
      isSearching: false,
      guildId: null,
      context: "dm",
      filters,
    });
    return;
  }

  setSearchState({
    query: query.trim(),
    isSearching: true,
    error: null,
    guildId: null,
    context: "dm",
    offset: 0,
    filters,
  });

  try {
    const response = await searchDMMessages(
      query.trim(),
      searchState.limit,
      0,
      filters,
    );

    setSearchState({
      results: response.results,
      total: response.total,
      offset: 0,
      isSearching: false,
    });
  } catch (err) {
    console.error("DM search failed:", err);
    setSearchState({
      results: [],
      total: 0,
      error: err instanceof Error ? err.message : "Search failed",
      isSearching: false,
    });
  }
}

/**
 * Search messages across all guilds and DMs.
 */
export async function searchGlobal(
  query: string,
  filters: SearchFilters = {},
): Promise<void> {
  // Skip if query is too short
  if (query.trim().length < 2) {
    setSearchState({
      query: query.trim(),
      results: [],
      total: 0,
      offset: 0,
      error: null,
      isSearching: false,
      guildId: null,
      context: "global",
      filters,
    });
    return;
  }

  setSearchState({
    query: query.trim(),
    isSearching: true,
    error: null,
    guildId: null,
    context: "global",
    offset: 0,
    filters,
  });

  try {
    const response = await searchGlobalMessages(
      query.trim(),
      searchState.limit,
      0,
      filters,
    );

    setSearchState({
      results: response.results,
      total: response.total,
      offset: 0,
      isSearching: false,
    });
  } catch (err) {
    console.error("Global search failed:", err);
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
  if (!searchState.query || searchState.isSearching) {
    return;
  }

  // Check if there are more results
  if (searchState.offset + searchState.limit >= searchState.total) {
    return;
  }

  const newOffset = searchState.offset + searchState.limit;

  setSearchState({ isSearching: true, error: null });

  try {
    let response;
    if (searchState.context === "guild" && searchState.guildId) {
      response = await searchGuildMessages(
        searchState.guildId,
        searchState.query,
        searchState.limit,
        newOffset,
        searchState.filters,
      );
    } else if (searchState.context === "global") {
      response = await searchGlobalMessages(
        searchState.query,
        searchState.limit,
        newOffset,
        searchState.filters,
      );
    } else {
      response = await searchDMMessages(
        searchState.query,
        searchState.limit,
        newOffset,
        searchState.filters,
      );
    }

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
    context: "guild",
    filters: {},
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

export { searchState, showGlobalSearch, setShowGlobalSearch };
