/**
 * Membership screening (rules gate) API. Dual-mode via `httpRequest`.
 */

import { httpRequest } from "./common";

export interface ScreeningConfig {
  screening_enabled: boolean;
  rules_md: string | null;
  my_state: string; // 'active' | 'pending'
}

export async function getScreening(guildId: string): Promise<ScreeningConfig> {
  return httpRequest<ScreeningConfig>("GET", `/api/guilds/${guildId}/screening`);
}

export async function updateScreening(
  guildId: string,
  body: { enabled: boolean; rules_md?: string },
): Promise<void> {
  return httpRequest<void>("PUT", `/api/guilds/${guildId}/screening`, body);
}

export async function acceptScreening(guildId: string): Promise<void> {
  return httpRequest<void>("POST", `/api/guilds/${guildId}/screening/accept`);
}
