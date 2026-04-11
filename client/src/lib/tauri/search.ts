/**
 * Full-text search commands: guild, DM, and global scopes.
 */

import type {
  GlobalSearchResponse,
  SearchFilters,
  SearchResponse,
} from "../types";
import { httpRequest } from "./common";

/**
 * Search messages in a guild using full-text search.
 */
export async function searchGuildMessages(
  guildId: string,
  query: string,
  limit: number = 25,
  offset: number = 0,
  filters?: SearchFilters,
): Promise<SearchResponse> {
  // Always use HTTP for search - no Tauri command needed since search is server-side
  const params = new URLSearchParams({
    q: query,
    limit: limit.toString(),
    offset: offset.toString(),
  });
  if (filters?.date_from) params.set("date_from", filters.date_from);
  if (filters?.date_to) params.set("date_to", filters.date_to);
  if (filters?.channel_id) params.set("channel_id", filters.channel_id);
  if (filters?.author_id) params.set("author_id", filters.author_id);
  if (filters?.has) params.set("has", filters.has);
  if (filters?.sort) params.set("sort", filters.sort);
  return httpRequest<SearchResponse>(
    "GET",
    `/api/guilds/${guildId}/search?${params}`,
  );
}

/**
 * Search messages in DM channels using full-text search.
 */
export async function searchDMMessages(
  query: string,
  limit: number = 25,
  offset: number = 0,
  filters?: SearchFilters,
): Promise<SearchResponse> {
  const params = new URLSearchParams({
    q: query,
    limit: limit.toString(),
    offset: offset.toString(),
  });
  if (filters?.date_from) params.set("date_from", filters.date_from);
  if (filters?.date_to) params.set("date_to", filters.date_to);
  if (filters?.channel_id) params.set("channel_id", filters.channel_id);
  if (filters?.author_id) params.set("author_id", filters.author_id);
  if (filters?.has) params.set("has", filters.has);
  if (filters?.sort) params.set("sort", filters.sort);
  return httpRequest<SearchResponse>("GET", `/api/dm/search?${params}`);
}

/**
 * Search messages across all guilds and DMs using full-text search.
 */
export async function searchGlobalMessages(
  query: string,
  limit: number = 25,
  offset: number = 0,
  filters?: SearchFilters,
): Promise<GlobalSearchResponse> {
  const params = new URLSearchParams({
    q: query,
    limit: limit.toString(),
    offset: offset.toString(),
  });
  if (filters?.date_from) params.set("date_from", filters.date_from);
  if (filters?.date_to) params.set("date_to", filters.date_to);
  if (filters?.channel_id) params.set("channel_id", filters.channel_id);
  if (filters?.author_id) params.set("author_id", filters.author_id);
  if (filters?.has) params.set("has", filters.has);
  if (filters?.sort) params.set("sort", filters.sort);
  return httpRequest<GlobalSearchResponse>("GET", `/api/search?${params}`);
}
