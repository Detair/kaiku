/**
 * Scheduled guild events API. Dual-mode via `httpRequest` (no native command).
 */

import { httpRequest } from "./common";

export interface GuildEvent {
  id: string;
  guild_id: string;
  channel_id: string | null;
  name: string;
  description: string | null;
  location: string | null;
  starts_at: string;
  ends_at: string | null;
  status: string;
  created_by: string | null;
  interested_count: number;
  going_count: number;
  my_response: string | null;
}

export async function listGuildEvents(
  guildId: string,
  scope?: "past",
): Promise<GuildEvent[]> {
  const q = scope ? `?scope=${scope}` : "";
  return httpRequest<GuildEvent[]>("GET", `/api/guilds/${guildId}/events${q}`);
}

export async function createGuildEvent(
  guildId: string,
  body: {
    name: string;
    description?: string;
    channel_id?: string;
    location?: string;
    starts_at: string;
    ends_at?: string;
  },
): Promise<GuildEvent> {
  return httpRequest<GuildEvent>("POST", `/api/guilds/${guildId}/events`, body);
}

export async function cancelGuildEvent(
  guildId: string,
  eventId: string,
): Promise<void> {
  return httpRequest<void>("DELETE", `/api/guilds/${guildId}/events/${eventId}`);
}

export async function rsvpGuildEvent(
  guildId: string,
  eventId: string,
  response: "interested" | "going",
): Promise<GuildEvent> {
  return httpRequest<GuildEvent>(
    "PUT",
    `/api/guilds/${guildId}/events/${eventId}/rsvp`,
    { response },
  );
}

export async function clearGuildEventRsvp(
  guildId: string,
  eventId: string,
): Promise<GuildEvent> {
  return httpRequest<GuildEvent>(
    "DELETE",
    `/api/guilds/${guildId}/events/${eventId}/rsvp`,
  );
}
