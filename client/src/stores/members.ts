/**
 * Members Store
 *
 * Handles member-related operations and patches.
 * Member data is stored in the guilds store.
 */

import { guildsState, setGuildsState } from "./guilds";
import type { GuildMember } from "@/lib/types";

/**
 * Apply a partial patch to a member's data.
 * Server sends: entity_id = user_id (UUID), diff = { guild_id, updates }
 */
export function patchMember(
  entityId: string,
  diff: Record<string, unknown>,
): void {
  // Extract guild_id from diff (server wraps member patches with guild context)
  const guildId = diff.guild_id as string | undefined;
  const updates = diff.updates as Record<string, unknown> | undefined;

  if (!guildId || !updates) {
    console.warn(
      "[Members] Invalid member patch format, expected { guild_id, updates }:",
      diff,
    );
    return;
  }

  // entityId is the user's UUID
  const userId = entityId;

  const members = guildsState.members[guildId];
  if (!members) {
    // No members loaded for this guild, ignore patch
    return;
  }

  const memberIndex = members.findIndex((m) => m.user_id === userId);
  if (memberIndex === -1) {
    // Member not in store, ignore patch
    return;
  }

  // Filter to only valid GuildMember fields
  const validFields: (keyof GuildMember)[] = [
    "username",
    "display_name",
    "avatar_url",
    "nickname",
    "status",
    "last_seen_at",
  ];
  const fieldUpdates: Partial<GuildMember> = {};
  for (const field of validFields) {
    if (field in updates) {
      (fieldUpdates as Record<string, unknown>)[field] = updates[field];
    }
  }

  if (Object.keys(fieldUpdates).length > 0) {
    setGuildsState("members", guildId, memberIndex, (prev) => ({
      ...prev,
      ...fieldUpdates,
    }));
  }
}

/**
 * Get members for a guild (re-export from guilds store).
 */
export function getMembers(guildId: string): GuildMember[] {
  return guildsState.members[guildId] || [];
}

/**
 * Get a specific member.
 */
export function getMember(
  guildId: string,
  userId: string,
): GuildMember | undefined {
  return guildsState.members[guildId]?.find((m) => m.user_id === userId);
}

// Re-export setGuildsState for internal use
export { setGuildsState };
