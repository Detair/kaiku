/**
 * Guild commands: CRUD, invites, categories, emojis, roles, members, banner,
 * settings, discovery, and guild-scoped channel listings.
 */

import type {
  AssignRoleResponse,
  ChannelCategory,
  ChannelWithUnread,
  CreateRoleRequest,
  DeleteRoleResponse,
  DiscoverResponse,
  Guild,
  GuildEmoji,
  GuildInvite,
  GuildMember,
  GuildRole,
  GuildSettings,
  GuildUsageStats,
  InviteExpiry,
  InviteResponse,
  JoinDiscoverableResponse,
  RemoveRoleResponse,
  UpdateRoleRequest,
} from "../types";
import {
  fetchApi,
  getUploadAuth,
  httpRequest,
  isTauri,
  validateFileSize,
} from "./common";

// ============================================================================
// Guild CRUD
// ============================================================================

export async function getGuilds(): Promise<Guild[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_guilds");
  }

  return httpRequest<Guild[]>("GET", "/api/guilds");
}

export async function getGuild(guildId: string): Promise<Guild> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_guild", { guildId });
  }

  return httpRequest<Guild>("GET", `/api/guilds/${guildId}`);
}

export async function createGuild(
  name: string,
  description?: string,
  discovery?: {
    discoverable: boolean;
    tags?: string[];
    banner_url?: string;
  },
): Promise<Guild> {
  const body: Record<string, unknown> = { name, description };
  if (discovery) {
    body.discoverable = discovery.discoverable;
    if (discovery.tags && discovery.tags.length > 0) {
      body.tags = discovery.tags;
    }
    if (discovery.banner_url) {
      body.banner_url = discovery.banner_url;
    }
  }

  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("create_guild", body);
  }

  return httpRequest<Guild>("POST", "/api/guilds", body);
}

export async function updateGuild(
  guildId: string,
  name?: string,
  description?: string,
  icon_url?: string,
): Promise<Guild> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("update_guild", {
      guildId,
      name,
      description,
      iconUrl: icon_url,
    });
  }

  return httpRequest<Guild>("PATCH", `/api/guilds/${guildId}`, {
    name,
    description,
    icon_url,
  });
}

export async function deleteGuild(guildId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("delete_guild", { guildId });
  }

  await httpRequest<void>("DELETE", `/api/guilds/${guildId}`);
}

export async function joinGuild(
  _guildId: string,
  inviteCode: string,
): Promise<void> {
  // Guild join always requires a valid invite code — route through the invite endpoint
  await joinViaInvite(inviteCode);
}

export async function leaveGuild(guildId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("leave_guild", { guildId });
  }

  await httpRequest<void>("POST", `/api/guilds/${guildId}/leave`);
}

export async function getGuildMembers(guildId: string): Promise<GuildMember[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_guild_members", { guildId });
  }

  return httpRequest<GuildMember[]>("GET", `/api/guilds/${guildId}/members`);
}

export async function getGuildChannels(
  guildId: string,
): Promise<ChannelWithUnread[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_guild_channels", { guildId });
  }

  return httpRequest<ChannelWithUnread[]>(
    "GET",
    `/api/guilds/${guildId}/channels`,
  );
}

/**
 * Get guild settings.
 */
export async function getGuildSettings(
  guildId: string,
): Promise<GuildSettings> {
  return fetchApi<GuildSettings>(`/api/guilds/${guildId}/settings`);
}

/**
 * Get guild resource usage stats (members, channels, roles, emojis, bots).
 */
export async function getGuildUsage(guildId: string): Promise<GuildUsageStats> {
  return fetchApi<GuildUsageStats>(`/api/guilds/${guildId}/usage`);
}

/**
 * Update guild settings (requires MANAGE_GUILD).
 */
export async function updateGuildSettings(
  guildId: string,
  settings: {
    threads_enabled?: boolean;
    discoverable?: boolean;
    tags?: string[];
    banner_url?: string | null;
  },
): Promise<GuildSettings> {
  return fetchApi<GuildSettings>(`/api/guilds/${guildId}/settings`, {
    method: "PATCH",
    body: settings,
  });
}

/**
 * Dismiss the discovery setup prompt for the current user in a guild.
 */
export async function dismissDiscoveryPrompt(guildId: string): Promise<void> {
  await fetchApi<void>(`/api/guilds/${guildId}/dismiss-discovery-prompt`, {
    method: "POST",
  });
}

/**
 * Browse discoverable guilds (public, no auth required).
 */
export async function discoverGuilds(params?: {
  q?: string;
  tags?: string[];
  sort?: "members" | "newest";
  limit?: number;
  offset?: number;
}): Promise<DiscoverResponse> {
  const searchParams = new URLSearchParams();
  if (params?.q) searchParams.set("q", params.q);
  if (params?.tags?.length) searchParams.set("tags", params.tags.join(","));
  if (params?.sort) searchParams.set("sort", params.sort);
  if (params?.limit != null) searchParams.set("limit", String(params.limit));
  if (params?.offset != null) searchParams.set("offset", String(params.offset));
  const qs = searchParams.toString();
  return fetchApi<DiscoverResponse>(
    `/api/discover/guilds${qs ? `?${qs}` : ""}`,
  );
}

/**
 * Join a discoverable guild (requires auth).
 */
export async function joinDiscoverable(
  guildId: string,
): Promise<JoinDiscoverableResponse> {
  return fetchApi<JoinDiscoverableResponse>(
    `/api/discover/guilds/${guildId}/join`,
    {
      method: "POST",
    },
  );
}

// ============================================================================
// Guild Invite Commands
// ============================================================================

/**
 * Get invites for a guild (owner only)
 */
export async function getGuildInvites(guildId: string): Promise<GuildInvite[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_guild_invites", { guildId });
  }

  return httpRequest<GuildInvite[]>("GET", `/api/guilds/${guildId}/invites`);
}

/**
 * Create a new invite for a guild (owner only)
 */
export async function createGuildInvite(
  guildId: string,
  expiresIn: InviteExpiry = "7d",
): Promise<GuildInvite> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("create_guild_invite", { guildId, expiresIn });
  }

  return httpRequest<GuildInvite>("POST", `/api/guilds/${guildId}/invites`, {
    expires_in: expiresIn,
  });
}

/**
 * Delete/revoke an invite (owner only)
 */
export async function deleteGuildInvite(
  guildId: string,
  code: string,
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("delete_guild_invite", { guildId, code });
  }

  await httpRequest<void>("DELETE", `/api/guilds/${guildId}/invites/${code}`);
}

/**
 * Join a guild via invite code
 */
export async function joinViaInvite(code: string): Promise<InviteResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("join_via_invite", { code });
  }

  return httpRequest<InviteResponse>("POST", `/api/invites/${code}/join`);
}

/**
 * Kick a member from a guild (owner only)
 */
export async function kickGuildMember(
  guildId: string,
  userId: string,
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("kick_guild_member", { guildId, userId });
  }

  await httpRequest<void>("DELETE", `/api/guilds/${guildId}/members/${userId}`);
}

// ============================================================================
// Guild Category Commands
// ============================================================================

/**
 * Get all categories for a guild.
 */
export async function getGuildCategories(
  guildId: string,
): Promise<ChannelCategory[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_guild_categories", { guildId });
  }

  return httpRequest<ChannelCategory[]>(
    "GET",
    `/api/guilds/${guildId}/categories`,
  );
}

/**
 * Create a new category in a guild.
 */
export async function createGuildCategory(
  guildId: string,
  name: string,
  parentId?: string,
  categoryType?: string,
): Promise<ChannelCategory> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("create_guild_category", { guildId, name, parentId, categoryType });
  }

  return httpRequest<ChannelCategory>(
    "POST",
    `/api/guilds/${guildId}/categories`,
    {
      name,
      parent_id: parentId,
      category_type: categoryType ?? "mixed",
    },
  );
}

/**
 * Update a category.
 */
export async function updateGuildCategory(
  guildId: string,
  categoryId: string,
  updates: {
    name?: string;
    position?: number;
    parentId?: string | null;
  },
): Promise<ChannelCategory> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("update_guild_category", { guildId, categoryId, ...updates });
  }

  return httpRequest<ChannelCategory>(
    "PATCH",
    `/api/guilds/${guildId}/categories/${categoryId}`,
    {
      name: updates.name,
      position: updates.position,
      parent_id: updates.parentId,
    },
  );
}

// ============================================================================
// Guild Emoji Commands
// ============================================================================

export async function getGuildEmojis(guildId: string): Promise<GuildEmoji[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_guild_emojis", { guildId });
  }

  return httpRequest<GuildEmoji[]>("GET", `/api/guilds/${guildId}/emojis`);
}

export async function uploadGuildEmoji(
  guildId: string,
  name: string,
  file: File,
): Promise<GuildEmoji> {
  // Frontend validation
  const validationError = validateFileSize(file, "emoji");
  if (validationError) {
    console.warn(
      "[uploadGuildEmoji] Frontend validation failed:",
      validationError,
    );
    throw new Error(validationError);
  }

  const { token, baseUrl } = await getUploadAuth();

  const headers: Record<string, string> = {};
  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  const formData = new FormData();
  formData.append("name", name);
  formData.append("file", file);

  const response = await fetch(`${baseUrl}/api/guilds/${guildId}/emojis`, {
    method: "POST",
    headers,
    body: formData,
  });

  if (!response.ok) {
    let errorMessage = `Upload failed (HTTP ${response.status})`;

    try {
      const errorBody = await response.json();
      errorMessage = errorBody.message || errorBody.error || errorMessage;
    } catch (parseError) {
      console.warn(
        "[uploadGuildEmoji] Failed to parse error response:",
        parseError,
      );
      errorMessage = response.statusText || errorMessage;
    }

    console.error("[uploadGuildEmoji] Upload failed:", {
      status: response.status,
      error: errorMessage,
      guildId,
      emojiName: name,
      fileSize: file.size,
      fileName: file.name,
    });

    throw new Error(errorMessage);
  }

  try {
    return await response.json();
  } catch (parseError) {
    console.error(
      "[uploadGuildEmoji] Failed to parse success response:",
      parseError,
    );
    throw new Error("Server returned invalid response", { cause: parseError });
  }
}

export async function updateGuildEmoji(
  guildId: string,
  emojiId: string,
  name: string,
): Promise<GuildEmoji> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("update_guild_emoji", { guildId, emojiId, name });
  }

  return httpRequest<GuildEmoji>(
    "PATCH",
    `/api/guilds/${guildId}/emojis/${emojiId}`,
    {
      name,
    },
  );
}

export async function deleteGuildEmoji(
  guildId: string,
  emojiId: string,
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("delete_guild_emoji", { guildId, emojiId });
  }

  await httpRequest<void>("DELETE", `/api/guilds/${guildId}/emojis/${emojiId}`);
}

/**
 * Delete a category.
 */
export async function deleteGuildCategory(
  guildId: string,
  categoryId: string,
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("delete_guild_category", { guildId, categoryId });
  }

  await httpRequest<void>(
    "DELETE",
    `/api/guilds/${guildId}/categories/${categoryId}`,
  );
}

/**
 * Reorder categories in a guild.
 */
export async function reorderGuildCategories(
  guildId: string,
  categories: Array<{ id: string; position: number; parentId?: string | null }>,
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("reorder_guild_categories", { guildId, categories });
  }

  await httpRequest<void>("POST", `/api/guilds/${guildId}/categories/reorder`, {
    categories: categories.map((c) => ({
      id: c.id,
      position: c.position,
      parent_id: c.parentId,
    })),
  });
}

/**
 * Position specification for channel reorder.
 */
export interface ChannelPosition {
  id: string;
  position: number;
  category_id: string | null;
}

/**
 * Reorder channels in a guild.
 * Requires MANAGE_CHANNELS permission.
 */
export async function reorderGuildChannels(
  guildId: string,
  channels: ChannelPosition[],
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("reorder_guild_channels", { guildId, channels });
  }

  await httpRequest<void>("POST", `/api/guilds/${guildId}/channels/reorder`, {
    channels,
  });
}

// ============================================================================
// Role Commands
// ============================================================================

/**
 * Get all roles for a guild.
 */
export async function getGuildRoles(guildId: string): Promise<GuildRole[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_guild_roles", { guildId });
  }

  return httpRequest<GuildRole[]>("GET", `/api/guilds/${guildId}/roles`);
}

/**
 * Create a new role in a guild.
 */
export async function createGuildRole(
  guildId: string,
  request: CreateRoleRequest,
): Promise<GuildRole> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("create_guild_role", { guildId, request });
  }

  return httpRequest<GuildRole>(
    "POST",
    `/api/guilds/${guildId}/roles`,
    request,
  );
}

/**
 * Update an existing role.
 */
export async function updateGuildRole(
  guildId: string,
  roleId: string,
  request: UpdateRoleRequest,
): Promise<GuildRole> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("update_guild_role", { guildId, roleId, request });
  }

  return httpRequest<GuildRole>(
    "PATCH",
    `/api/guilds/${guildId}/roles/${roleId}`,
    request,
  );
}

/**
 * Delete a role from a guild.
 */
export async function deleteGuildRole(
  guildId: string,
  roleId: string,
): Promise<DeleteRoleResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("delete_guild_role", { guildId, roleId });
  }

  return httpRequest<DeleteRoleResponse>(
    "DELETE",
    `/api/guilds/${guildId}/roles/${roleId}`,
  );
}

/**
 * Get all member role assignments for a guild.
 * Returns a map of user_id -> list of role_ids.
 */
export async function getGuildMemberRoles(
  guildId: string,
): Promise<Record<string, string[]>> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_guild_member_roles", { guildId });
  }

  return httpRequest<Record<string, string[]>>(
    "GET",
    `/api/guilds/${guildId}/member-roles`,
  );
}

/**
 * Assign a role to a guild member.
 */
export async function assignMemberRole(
  guildId: string,
  userId: string,
  roleId: string,
): Promise<AssignRoleResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("assign_member_role", { guildId, userId, roleId });
  }

  return httpRequest<AssignRoleResponse>(
    "POST",
    `/api/guilds/${guildId}/members/${userId}/roles/${roleId}`,
  );
}

/**
 * Remove a role from a guild member.
 */
export async function removeMemberRole(
  guildId: string,
  userId: string,
  roleId: string,
): Promise<RemoveRoleResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("remove_member_role", { guildId, userId, roleId });
  }

  return httpRequest<RemoveRoleResponse>(
    "DELETE",
    `/api/guilds/${guildId}/members/${userId}/roles/${roleId}`,
  );
}

// ============================================================================
// Guild Banner Upload
// ============================================================================

export async function uploadGuildBanner(
  guildId: string,
  file: File,
): Promise<Guild> {
  if (file.size > 5 * 1024 * 1024) {
    throw new Error(
      `File too large (${(file.size / 1024 / 1024).toFixed(1)}MB). Maximum size is 5.0MB`,
    );
  }

  const { token, baseUrl } = await getUploadAuth();

  const headers: Record<string, string> = {};
  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  const formData = new FormData();
  formData.append("banner", file);

  const response = await fetch(`${baseUrl}/api/guilds/${guildId}/banner`, {
    method: "POST",
    headers,
    body: formData,
  });

  if (!response.ok) {
    let errorMessage = `Banner upload failed (HTTP ${response.status})`;
    try {
      const errorBody = await response.json();
      errorMessage = errorBody.message || errorBody.error || errorMessage;
    } catch {
      errorMessage = response.statusText || errorMessage;
    }
    throw new Error(errorMessage);
  }

  return response.json();
}
