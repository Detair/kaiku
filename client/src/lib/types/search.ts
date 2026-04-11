/**
 * Global and per-scope search types.
 */

export interface SearchAuthor {
  id: string;
  username: string;
  display_name: string;
  avatar_url: string | null;
}

export interface SearchResult {
  id: string;
  channel_id: string;
  channel_name: string;
  author: SearchAuthor;
  content: string;
  created_at: string;
  headline: string;
  rank: number;
}

export interface SearchResponse {
  results: SearchResult[];
  total: number;
  limit: number;
  offset: number;
}

export interface SearchFilters {
  date_from?: string;
  date_to?: string;
  channel_id?: string;
  author_id?: string;
  has?: "link" | "file";
  sort?: "relevance" | "date";
}

// Global Search Types

export interface GlobalSearchSource {
  type: "guild" | "dm";
  guild_id: string | null;
  guild_name: string | null;
}

export interface GlobalSearchResult extends SearchResult {
  source: GlobalSearchSource;
}

export interface GlobalSearchResponse {
  results: GlobalSearchResult[];
  total: number;
  limit: number;
  offset: number;
}
