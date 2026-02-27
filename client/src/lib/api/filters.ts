/**
 * Content Filter API
 *
 * Guild content filter configuration, custom patterns, moderation log.
 */

import { getAccessToken } from "../tauri";

const API_BASE = import.meta.env.VITE_API_URL || "http://localhost:3000";

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
  const token = getAccessToken();
  const response = await fetch(`${API_BASE}/api/guilds/${guildId}/filters`, {
    headers: { Authorization: `Bearer ${token}` },
  });

  if (!response.ok) {
    throw new Error("Failed to load filter configs");
  }

  return response.json();
}

/**
 * Update filter category configs (bulk upsert).
 */
export async function updateFilterConfigs(
  guildId: string,
  configs: FilterConfigEntry[],
): Promise<GuildFilterConfig[]> {
  const token = getAccessToken();
  const response = await fetch(`${API_BASE}/api/guilds/${guildId}/filters`, {
    method: "PUT",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${token}`,
    },
    body: JSON.stringify({ configs }),
  });

  if (!response.ok) {
    throw new Error("Failed to update filter configs");
  }

  return response.json();
}

/**
 * List custom filter patterns for a guild.
 */
export async function listCustomPatterns(
  guildId: string,
): Promise<GuildFilterPattern[]> {
  const token = getAccessToken();
  const response = await fetch(
    `${API_BASE}/api/guilds/${guildId}/filters/patterns`,
    {
      headers: { Authorization: `Bearer ${token}` },
    },
  );

  if (!response.ok) {
    throw new Error("Failed to load custom patterns");
  }

  return response.json();
}

/**
 * Create a custom filter pattern.
 */
export async function createCustomPattern(
  guildId: string,
  data: CreatePatternRequest,
): Promise<GuildFilterPattern> {
  const token = getAccessToken();
  const response = await fetch(
    `${API_BASE}/api/guilds/${guildId}/filters/patterns`,
    {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${token}`,
      },
      body: JSON.stringify(data),
    },
  );

  if (!response.ok) {
    const error = await response.text();
    throw new Error(error || "Failed to create pattern");
  }

  return response.json();
}

/**
 * Update a custom filter pattern.
 */
export async function updateCustomPattern(
  guildId: string,
  patternId: string,
  data: UpdatePatternRequest,
): Promise<GuildFilterPattern> {
  const token = getAccessToken();
  const response = await fetch(
    `${API_BASE}/api/guilds/${guildId}/filters/patterns/${patternId}`,
    {
      method: "PUT",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${token}`,
      },
      body: JSON.stringify(data),
    },
  );

  if (!response.ok) {
    const error = await response.text();
    throw new Error(error || "Failed to update pattern");
  }

  return response.json();
}

/**
 * Delete a custom filter pattern.
 */
export async function deleteCustomPattern(
  guildId: string,
  patternId: string,
): Promise<void> {
  const token = getAccessToken();
  const response = await fetch(
    `${API_BASE}/api/guilds/${guildId}/filters/patterns/${patternId}`,
    {
      method: "DELETE",
      headers: { Authorization: `Bearer ${token}` },
    },
  );

  if (!response.ok) {
    throw new Error("Failed to delete pattern");
  }
}

/**
 * List moderation log entries (paginated).
 */
export async function listModerationLog(
  guildId: string,
  limit = 50,
  offset = 0,
): Promise<PaginatedModerationLog> {
  const token = getAccessToken();
  const response = await fetch(
    `${API_BASE}/api/guilds/${guildId}/filters/log?limit=${limit}&offset=${offset}`,
    {
      headers: { Authorization: `Bearer ${token}` },
    },
  );

  if (!response.ok) {
    throw new Error("Failed to load moderation log");
  }

  return response.json();
}

/**
 * Test content against active filters (dry-run).
 */
export async function testFilter(
  guildId: string,
  content: string,
): Promise<TestFilterResponse> {
  const token = getAccessToken();
  const response = await fetch(
    `${API_BASE}/api/guilds/${guildId}/filters/test`,
    {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${token}`,
      },
      body: JSON.stringify({ content }),
    },
  );

  if (!response.ok) {
    throw new Error("Failed to test filter");
  }

  return response.json();
}
