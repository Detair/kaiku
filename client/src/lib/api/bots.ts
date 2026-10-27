/**
 * Bot Applications API
 */

import { httpRequest } from "../tauri/common";

export interface BotApplication {
  id: string;
  name: string;
  description?: string;
  bot_user_id?: string;
  public: boolean;
  gateway_intents: string[];
  created_at: string;
}

export interface BotTokenResponse {
  token: string;
  bot_user_id: string;
}

export interface SlashCommand {
  id: string;
  application_id: string;
  guild_id?: string;
  name: string;
  description: string;
  options: CommandOption[];
  created_at: string;
}

export interface CommandOption {
  name: string;
  description: string;
  type: "string" | "integer" | "boolean" | "user" | "channel" | "role";
  required: boolean;
}

export interface GuildCommand {
  name: string;
  description: string;
  bot_name: string;
  application_id: string;
  is_ambiguous: boolean;
}

export interface InstalledBot {
  application_id: string;
  bot_user_id: string;
  name: string;
  description?: string;
  installed_by: string;
  installed_at: string;
}

export interface CreateApplicationRequest {
  name: string;
  description?: string;
}

export interface RegisterCommandsRequest {
  commands: Array<{
    name: string;
    description: string;
    options?: CommandOption[];
  }>;
}

/**
 * Create a new bot application.
 */
export async function createBotApplication(
  data: CreateApplicationRequest,
): Promise<BotApplication> {
  return httpRequest<BotApplication>("POST", "/api/applications", data);
}

/**
 * List all bot applications for the current user.
 */
export async function listBotApplications(): Promise<BotApplication[]> {
  return httpRequest<BotApplication[]>("GET", "/api/applications");
}

/**
 * Get a specific bot application by ID.
 */
export async function getBotApplication(id: string): Promise<BotApplication> {
  return httpRequest<BotApplication>("GET", `/api/applications/${id}`);
}

/**
 * Delete a bot application.
 */
export async function deleteBotApplication(id: string): Promise<void> {
  await httpRequest<void>("DELETE", `/api/applications/${id}`);
}

/**
 * Create a bot user for an application and get the token.
 * **WARNING: The token is only shown once!**
 */
export async function createBotUser(
  applicationId: string,
): Promise<BotTokenResponse> {
  return httpRequest<BotTokenResponse>(
    "POST",
    `/api/applications/${applicationId}/bot`,
  );
}

/**
 * Reset the bot token for an application.
 * **WARNING: The new token is only shown once!**
 */
export async function resetBotToken(
  applicationId: string,
): Promise<BotTokenResponse> {
  return httpRequest<BotTokenResponse>(
    "POST",
    `/api/applications/${applicationId}/reset-token`,
  );
}

/**
 * Register slash commands for an application.
 */
export async function registerSlashCommands(
  applicationId: string,
  data: RegisterCommandsRequest,
  guildId?: string,
): Promise<SlashCommand[]> {
  const query = guildId ? `?guild_id=${encodeURIComponent(guildId)}` : "";
  return httpRequest<SlashCommand[]>(
    "PUT",
    `/api/applications/${applicationId}/commands${query}`,
    data,
  );
}

/**
 * List slash commands for an application.
 */
export async function listSlashCommands(
  applicationId: string,
  guildId?: string,
): Promise<SlashCommand[]> {
  const query = guildId ? `?guild_id=${encodeURIComponent(guildId)}` : "";
  return httpRequest<SlashCommand[]>(
    "GET",
    `/api/applications/${applicationId}/commands${query}`,
  );
}

/**
 * Delete a specific slash command.
 */
export async function deleteSlashCommand(
  applicationId: string,
  commandId: string,
): Promise<void> {
  await httpRequest<void>(
    "DELETE",
    `/api/applications/${applicationId}/commands/${commandId}`,
  );
}

/**
 * Delete all slash commands for an application (in a specific scope).
 */
export async function deleteAllSlashCommands(
  applicationId: string,
  guildId?: string,
): Promise<void> {
  const query = guildId ? `?guild_id=${encodeURIComponent(guildId)}` : "";
  await httpRequest<void>(
    "DELETE",
    `/api/applications/${applicationId}/commands${query}`,
  );
}

/**
 * List bots installed in a guild.
 */
export async function listInstalledBots(
  guildId: string,
): Promise<InstalledBot[]> {
  return httpRequest<InstalledBot[]>("GET", `/api/guilds/${guildId}/bots`);
}

/**
 * Remove a bot from a guild.
 */
export async function removeInstalledBot(
  guildId: string,
  botId: string,
): Promise<void> {
  await httpRequest<void>("DELETE", `/api/guilds/${guildId}/bots/${botId}`);
}

/**
 * List available slash commands in a guild (from installed bots).
 */
export async function listGuildCommands(
  guildId: string,
): Promise<GuildCommand[]> {
  return httpRequest<GuildCommand[]>("GET", `/api/guilds/${guildId}/commands`);
}

/**
 * Update gateway intents for an application.
 */
export async function updateGatewayIntents(
  applicationId: string,
  intents: string[],
): Promise<BotApplication> {
  return httpRequest<BotApplication>(
    "PUT",
    `/api/applications/${applicationId}/intents`,
    { intents },
  );
}
