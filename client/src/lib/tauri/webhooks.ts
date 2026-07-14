/**
 * Incoming (Discord-compatible) webhooks management API. Dual-mode via
 * `httpRequest`. Not to be confused with the bot outgoing webhooks in
 * `lib/api/webhooks.ts`.
 */

import { httpRequest } from "./common";

export interface IncomingWebhook {
  id: string;
  /** Discord webhook type discriminator — always 1 (Incoming). */
  type: number;
  guild_id: string;
  channel_id: string;
  name: string;
  avatar: string | null;
  token: string;
  application_id: string | null;
  /** Fully-qualified execute URL for copy-paste into integrations. */
  url: string;
  user?: {
    id: string;
    username: string;
    display_name: string;
    avatar_url: string | null;
  };
}

export async function listGuildWebhooks(
  guildId: string,
): Promise<IncomingWebhook[]> {
  return httpRequest<IncomingWebhook[]>(
    "GET",
    `/api/guilds/${guildId}/webhooks`,
  );
}

export async function listChannelWebhooks(
  channelId: string,
): Promise<IncomingWebhook[]> {
  return httpRequest<IncomingWebhook[]>(
    "GET",
    `/api/channels/${channelId}/webhooks`,
  );
}

export async function createWebhook(
  channelId: string,
  body: { name: string; avatar_url?: string },
): Promise<IncomingWebhook> {
  return httpRequest<IncomingWebhook>(
    "POST",
    `/api/channels/${channelId}/webhooks`,
    body,
  );
}

export async function updateWebhook(
  webhookId: string,
  body: { name?: string; avatar_url?: string; channel_id?: string },
): Promise<IncomingWebhook> {
  return httpRequest<IncomingWebhook>(
    "PATCH",
    `/api/webhooks/${webhookId}`,
    body,
  );
}

export async function deleteWebhook(webhookId: string): Promise<void> {
  return httpRequest<void>("DELETE", `/api/webhooks/${webhookId}`);
}
