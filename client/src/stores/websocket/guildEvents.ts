/**
 * Guild-level WebSocket event handlers: emoji updates, patch (state sync).
 */

import type { GuildEmoji } from "@/lib/types";

// Guild emoji event handler

export async function handleGuildEmojiUpdated(
  guildId: string,
  emojis: GuildEmoji[],
): Promise<void> {
  const { setGuildEmojis } = await import("@/stores/emoji");
  setGuildEmojis(guildId, emojis);
}

// State sync (patch) event handler

export async function handlePatchEvent(
  entityType: string,
  entityId: string,
  diff: Record<string, unknown>,
): Promise<void> {
  console.log(`[WebSocket] Patch event: ${entityType}/${entityId}`, diff);

  switch (entityType) {
    case "user":
      {
        const { patchUser } = await import("@/stores/presence");
        patchUser(entityId, diff);
      }
      break;

    case "guild":
      {
        const { patchGuild } = await import("@/stores/guilds");
        patchGuild(entityId, diff);
      }
      break;

    case "member":
      {
        const { patchMember } = await import("@/stores/members");
        patchMember(entityId, diff);
      }
      break;

    case "channel":
      {
        const { patchChannel } = await import("@/stores/channels");
        patchChannel(entityId, diff);
      }
      break;

    default:
      console.warn(`[WebSocket] Unknown patch entity type: ${entityType}`);
  }
}
