/**
 * Content Filter API
 *
 * Guild content filter configuration, custom patterns, moderation log.
 */

import { httpRequest } from "../tauri/common";

// ============================================================================
// Types
// ============================================================================

export type FilterCategory =
  | "slurs"
  | "hate_speech"
  | "spam"
  | "abusive_language"
  | "custom";
export type FilterAction = "block" | "log" | "warn";

export interface GuildFilterConfig {
  id: string;
  guild_id: string;
  category: FilterCategory;
  enabled: boolean;
  action: FilterAction;
  created_at: string;
  updated_at: string;
}

export interface GuildFilterPattern {
  id: string;
  guild_id: string;
  pattern: string;
  is_regex: boolean;
  description?: string;
  enabled: boolean;
  created_by: string;
  created_at: string;
  updated_at: string;
}

export interface ModerationAction {
  id: string;
  guild_id: string;
  user_id: string;
  channel_id: string;
  action: FilterAction;
  category?: FilterCategory;
  matched_pattern: string;
  original_content: string;
  custom_pattern_id?: string;
  created_at: string;
}

export interface PaginatedModerationLog {
  items: ModerationAction[];
  total: number;
  limit: number;
  offset: number;
}

export interface FilterConfigEntry {
  category: FilterCategory;
  enabled: boolean;
  action: FilterAction;
}

export interface CreatePatternRequest {
  pattern: string;
  is_regex?: boolean;
  description?: string;
}

export interface UpdatePatternRequest {
  pattern?: string;
  is_regex?: boolean;
  description?: string;
  enabled?: boolean;
}

export interface TestFilterResponse {
  blocked: boolean;
  matches: Array<{
    category: FilterCategory;
    action: FilterAction;
    matched_pattern: string;
  }>;
}

// ============================================================================
// API Functions
// ============================================================================

/**
 * List filter category configs for a guild.
 */
export async function listFilterConfigs(
  guildId: string,
): Promise<GuildFilterConfig[]> {
  return httpRequest<GuildFilterConfig[]>(
    "GET",
    `/api/guilds/${guildId}/filters`,
  );
}

/**
 * Update filter category configs (bulk upsert).
 */
export async function updateFilterConfigs(
  guildId: string,
  configs: FilterConfigEntry[],
): Promise<GuildFilterConfig[]> {
  return httpRequest<GuildFilterConfig[]>(
    "PUT",
    `/api/guilds/${guildId}/filters`,
    { configs },
  );
}

/**
 * List custom filter patterns for a guild.
 */
export async function listCustomPatterns(
  guildId: string,
): Promise<GuildFilterPattern[]> {
  return httpRequest<GuildFilterPattern[]>(
    "GET",
    `/api/guilds/${guildId}/filters/patterns`,
  );
}

/**
 * Create a custom filter pattern.
 */
export async function createCustomPattern(
  guildId: string,
  data: CreatePatternRequest,
): Promise<GuildFilterPattern> {
  return httpRequest<GuildFilterPattern>(
    "POST",
    `/api/guilds/${guildId}/filters/patterns`,
    data,
  );
}

/**
 * Update a custom filter pattern.
 */
export async function updateCustomPattern(
  guildId: string,
  patternId: string,
  data: UpdatePatternRequest,
): Promise<GuildFilterPattern> {
  return httpRequest<GuildFilterPattern>(
    "PUT",
    `/api/guilds/${guildId}/filters/patterns/${patternId}`,
    data,
  );
}

/**
 * Delete a custom filter pattern.
 */
export async function deleteCustomPattern(
  guildId: string,
  patternId: string,
): Promise<void> {
  await httpRequest<void>(
    "DELETE",
    `/api/guilds/${guildId}/filters/patterns/${patternId}`,
  );
}

/**
 * List moderation log entries (paginated).
 */
export async function listModerationLog(
  guildId: string,
  limit = 50,
  offset = 0,
): Promise<PaginatedModerationLog> {
  return httpRequest<PaginatedModerationLog>(
    "GET",
    `/api/guilds/${guildId}/filters/log?limit=${limit}&offset=${offset}`,
  );
}

/**
 * Test content against active filters (dry-run).
 */
export async function testFilter(
  guildId: string,
  content: string,
): Promise<TestFilterResponse> {
  return httpRequest<TestFilterResponse>(
    "POST",
    `/api/guilds/${guildId}/filters/test`,
    { content },
  );
}
