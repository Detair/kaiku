/**
 * Bot Applications API
 */

import { getAccessToken } from "../tauri";

const API_BASE = import.meta.env.VITE_API_URL || "http://localhost:3000";

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
  const token = getAccessToken();
  const response = await fetch(`${API_BASE}/api/applications`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${token}`,
    },
    body: JSON.stringify(data),
  });

  if (!response.ok) {
    const error = await response.text();
    throw new Error(error || "Failed to create application");
  }

  return response.json();
}

/**
 * List all bot applications for the current user.
 */
export async function listBotApplications(): Promise<BotApplication[]> {
  const token = getAccessToken();
  const response = await fetch(`${API_BASE}/api/applications`, {
    method: "GET",
    headers: {
      Authorization: `Bearer ${token}`,
    },
  });

  if (!response.ok) {
    throw new Error("Failed to list applications");
  }

  return response.json();
}

/**
 * Get a specific bot application by ID.
 */
export async function getBotApplication(id: string): Promise<BotApplication> {
  const token = getAccessToken();
  const response = await fetch(`${API_BASE}/api/applications/${id}`, {
    method: "GET",
    headers: {
      Authorization: `Bearer ${token}`,
    },
  });

  if (!response.ok) {
    throw new Error("Failed to get application");
  }

  return response.json();
}

/**
 * Delete a bot application.
 */
export async function deleteBotApplication(id: string): Promise<void> {
  const token = getAccessToken();
  const response = await fetch(`${API_BASE}/api/applications/${id}`, {
    method: "DELETE",
    headers: {
      Authorization: `Bearer ${token}`,
    },
  });

  if (!response.ok) {
    throw new Error("Failed to delete application");
  }
}

/**
 * Create a bot user for an application and get the token.
 * **WARNING: The token is only shown once!**
 */
export async function createBotUser(
  applicationId: string,
): Promise<BotTokenResponse> {
  const token = getAccessToken();
  const response = await fetch(
    `${API_BASE}/api/applications/${applicationId}/bot`,
    {
      method: "POST",
      headers: {
        Authorization: `Bearer ${token}`,
      },
    },
  );

  if (!response.ok) {
    const error = await response.text();
    throw new Error(error || "Failed to create bot user");
  }

  return response.json();
}

/**
 * Reset the bot token for an application.
 * **WARNING: The new token is only shown once!**
 */
export async function resetBotToken(
  applicationId: string,
): Promise<BotTokenResponse> {
  const token = getAccessToken();
  const response = await fetch(
    `${API_BASE}/api/applications/${applicationId}/reset-token`,
    {
      method: "POST",
      headers: {
        Authorization: `Bearer ${token}`,
      },
    },
  );

  if (!response.ok) {
    throw new Error("Failed to reset token");
  }

  return response.json();
}

/**
 * Register slash commands for an application.
 */
export async function registerSlashCommands(
  applicationId: string,
  data: RegisterCommandsRequest,
  guildId?: string,
): Promise<SlashCommand[]> {
  const token = getAccessToken();
  const url = new URL(`${API_BASE}/api/applications/${applicationId}/commands`);
  if (guildId) {
    url.searchParams.set("guild_id", guildId);
  }

  const response = await fetch(url.toString(), {
    method: "PUT",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${token}`,
    },
    body: JSON.stringify(data),
  });

  if (!response.ok) {
    const error = await response.text();
    throw new Error(error || "Failed to register commands");
  }

  return response.json();
}

/**
 * List slash commands for an application.
 */
export async function listSlashCommands(
  applicationId: string,
  guildId?: string,
): Promise<SlashCommand[]> {
  const token = getAccessToken();
  const url = new URL(`${API_BASE}/api/applications/${applicationId}/commands`);
  if (guildId) {
    url.searchParams.set("guild_id", guildId);
  }

  const response = await fetch(url.toString(), {
    method: "GET",
    headers: {
      Authorization: `Bearer ${token}`,
    },
  });

  if (!response.ok) {
    throw new Error("Failed to list commands");
  }

  return response.json();
}

/**
 * Delete a specific slash command.
 */
export async function deleteSlashCommand(
  applicationId: string,
  commandId: string,
): Promise<void> {
  const token = getAccessToken();
  const response = await fetch(
    `${API_BASE}/api/applications/${applicationId}/commands/${commandId}`,
    {
      method: "DELETE",
      headers: {
        Authorization: `Bearer ${token}`,
      },
    },
  );

  if (!response.ok) {
    throw new Error("Failed to delete command");
  }
}

/**
 * Delete all slash commands for an application (in a specific scope).
 */
export async function deleteAllSlashCommands(
  applicationId: string,
  guildId?: string,
): Promise<void> {
  const token = getAccessToken();
  const url = new URL(`${API_BASE}/api/applications/${applicationId}/commands`);
  if (guildId) {
    url.searchParams.set("guild_id", guildId);
  }

  const response = await fetch(url.toString(), {
    method: "DELETE",
    headers: {
      Authorization: `Bearer ${token}`,
    },
  });

  if (!response.ok) {
    throw new Error("Failed to delete commands");
  }
}

/**
 * List bots installed in a guild.
 */
export async function listInstalledBots(
  guildId: string,
): Promise<InstalledBot[]> {
  const token = getAccessToken();
  const response = await fetch(`${API_BASE}/api/guilds/${guildId}/bots`, {
    method: "GET",
    headers: {
      Authorization: `Bearer ${token}`,
    },
  });

  if (!response.ok) {
    throw new Error("Failed to list installed bots");
  }

  return response.json();
}

/**
 * Remove a bot from a guild.
 */
export async function removeInstalledBot(
  guildId: string,
  botId: string,
): Promise<void> {
  const token = getAccessToken();
  const response = await fetch(
    `${API_BASE}/api/guilds/${guildId}/bots/${botId}`,
    {
      method: "DELETE",
      headers: {
        Authorization: `Bearer ${token}`,
      },
    },
  );

  if (!response.ok) {
    throw new Error("Failed to remove bot");
  }
}

/**
 * List available slash commands in a guild (from installed bots).
 */
export async function listGuildCommands(
  guildId: string,
): Promise<GuildCommand[]> {
  const token = getAccessToken();
  const response = await fetch(`${API_BASE}/api/guilds/${guildId}/commands`, {
    method: "GET",
    headers: {
      Authorization: `Bearer ${token}`,
    },
  });

  if (!response.ok) {
    throw new Error("Failed to list guild commands");
  }

  return response.json();
}

/**
 * Update gateway intents for an application.
 */
export async function updateGatewayIntents(
  applicationId: string,
  intents: string[],
): Promise<BotApplication> {
  const token = getAccessToken();
  const response = await fetch(
    `${API_BASE}/api/applications/${applicationId}/intents`,
    {
      method: "PUT",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${token}`,
      },
      body: JSON.stringify({ intents }),
    },
  );

  if (!response.ok) {
    const error = await response.text();
    throw new Error(error || "Failed to update intents");
  }

  return response.json();
}
