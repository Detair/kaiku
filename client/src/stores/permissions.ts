/**
 * Permissions Store
 *
 * Manages roles and channel overrides state.
 */

import { createStore } from "solid-js/store";
import type {
  GuildRole,
  ChannelOverride,
  CreateRoleRequest,
  UpdateRoleRequest,
  SetChannelOverrideRequest,
} from "@/lib/types";
import * as tauri from "@/lib/tauri";
import { hasPermission, PermissionBits } from "@/lib/permissionConstants";

/**
 * Permissions store state
 */
interface PermissionsStoreState {
  // Roles per guild
  roles: Record<string, GuildRole[]>;
  // Member role assignments: guildId -> userId -> roleIds
  memberRoles: Record<string, Record<string, string[]>>;
  // Channel overrides: channelId -> overrides
  channelOverrides: Record<string, ChannelOverride[]>;
  // Loading states
  isRolesLoading: boolean;
  isOverridesLoading: boolean;
  // Error state
  error: string | null;
}

// Create the store
const [permissionsState, setPermissionsState] =
  createStore<PermissionsStoreState>({
    roles: {},
    memberRoles: {},
    channelOverrides: {},
    isRolesLoading: false,
    isOverridesLoading: false,
    error: null,
  });

// ============================================================================
// Role Functions
// ============================================================================

/**
 * Load roles for a guild
 */
export async function loadGuildRoles(guildId: string): Promise<void> {
  setPermissionsState({ isRolesLoading: true, error: null });

  try {
    const roles = await tauri.getGuildRoles(guildId);
    // Sort by position (lower position = higher rank)
    roles.sort((a, b) => a.position - b.position);
    setPermissionsState("roles", guildId, roles);
    setPermissionsState({ isRolesLoading: false });
  } catch (err) {
    console.error("[Permissions] Failed to load guild roles:", err);
    setPermissionsState({
      error: err instanceof Error ? err.message : "Failed to load roles",
      isRolesLoading: false,
    });
  }
}

/**
 * Get roles for a guild
 */
export function getGuildRoles(guildId: string): GuildRole[] {
  return permissionsState.roles[guildId] || [];
}

/**
 * Get a specific role by ID
 */
export function getRole(guildId: string, roleId: string): GuildRole | undefined {
  return getGuildRoles(guildId).find((r) => r.id === roleId);
}

/**
 * Get the @everyone role for a guild
 */
export function getEveryoneRole(guildId: string): GuildRole | undefined {
  return getGuildRoles(guildId).find((r) => r.is_default);
}

/**
 * Create a new role
 */
export async function createRole(
  guildId: string,
  request: CreateRoleRequest
): Promise<GuildRole> {
  const role = await tauri.createGuildRole(guildId, request);
  setPermissionsState("roles", guildId, (prev) => {
    const roles = [...(prev || []), role];
    return roles.sort((a, b) => a.position - b.position);
  });
  return role;
}

/**
 * Update a role
 */
export async function updateRole(
  guildId: string,
  roleId: string,
  request: UpdateRoleRequest
): Promise<GuildRole> {
  const updated = await tauri.updateGuildRole(guildId, roleId, request);
  setPermissionsState("roles", guildId, (prev) => {
    const roles = (prev || []).map((r) => (r.id === roleId ? updated : r));
    return roles.sort((a, b) => a.position - b.position);
  });
  return updated;
}

/**
 * Delete a role
 */
export async function deleteRole(guildId: string, roleId: string): Promise<void> {
  await tauri.deleteGuildRole(guildId, roleId);
  setPermissionsState("roles", guildId, (prev) =>
    (prev || []).filter((r) => r.id !== roleId)
  );
}

/**
 * Reorder a role to a new position.
 * Moves the role and adjusts other roles' positions accordingly.
 */
export async function reorderRole(
  guildId: string,
  roleId: string,
  newPosition: number
): Promise<void> {
  // Optimistically update the UI first
  const roles = getGuildRoles(guildId);
  const roleIndex = roles.findIndex((r) => r.id === roleId);
  if (roleIndex === -1) return;

  const role = roles[roleIndex];
  const oldPosition = role.position;

  // Can't reorder the @everyone role (always at the bottom)
  if (role.is_default) return;

  // Create new array with updated positions
  const updatedRoles = roles.map((r) => {
    if (r.id === roleId) {
      return { ...r, position: newPosition };
    }
    // Shift roles between old and new position
    if (oldPosition < newPosition) {
      // Moving down: shift roles in between up
      if (r.position > oldPosition && r.position <= newPosition) {
        return { ...r, position: r.position - 1 };
      }
    } else {
      // Moving up: shift roles in between down
      if (r.position >= newPosition && r.position < oldPosition) {
        return { ...r, position: r.position + 1 };
      }
    }
    return r;
  });

  // Sort by position
  updatedRoles.sort((a, b) => a.position - b.position);

  // Update local state optimistically
  setPermissionsState("roles", guildId, updatedRoles);

  try {
    // Send update to server
    await tauri.updateGuildRole(guildId, roleId, { position: newPosition });
  } catch (err) {
    console.error("[Permissions] Failed to reorder role:", err);
    // Revert on failure by reloading
    await loadGuildRoles(guildId);
    throw err;
  }
}

// ============================================================================
// Member Role Functions
// ============================================================================

/**
 * Load member roles for a guild
 */
export async function loadMemberRoles(guildId: string): Promise<void> {
  try {
    const memberRoles = await tauri.getGuildMemberRoles(guildId);
    setPermissionsState("memberRoles", guildId, memberRoles);
  } catch (err) {
    console.error("[Permissions] Failed to load member roles:", err);
  }
}

/**
 * Get role IDs for a specific member
 */
export function getMemberRoleIds(guildId: string, userId: string): string[] {
  return permissionsState.memberRoles[guildId]?.[userId] || [];
}

/**
 * Get role objects for a specific member
 */
export function getMemberRoles(guildId: string, userId: string): GuildRole[] {
  const roleIds = getMemberRoleIds(guildId, userId);
  const guildRoles = getGuildRoles(guildId);
  return roleIds
    .map((id) => guildRoles.find((r) => r.id === id))
    .filter((r): r is GuildRole => r !== undefined)
    .sort((a, b) => a.position - b.position);
}

/**
 * Assign a role to a member
 */
export async function assignMemberRole(
  guildId: string,
  userId: string,
  roleId: string
): Promise<void> {
  await tauri.assignMemberRole(guildId, userId, roleId);
  setPermissionsState("memberRoles", guildId, userId, (prev) => {
    const current = prev || [];
    if (current.includes(roleId)) return current;
    return [...current, roleId];
  });
}

/**
 * Remove a role from a member
 */
export async function removeMemberRole(
  guildId: string,
  userId: string,
  roleId: string
): Promise<void> {
  await tauri.removeMemberRole(guildId, userId, roleId);
  setPermissionsState("memberRoles", guildId, userId, (prev) =>
    (prev || []).filter((id) => id !== roleId)
  );
}

// ============================================================================
// Channel Override Functions
// ============================================================================

/**
 * Load channel overrides
 */
export async function loadChannelOverrides(channelId: string): Promise<void> {
  setPermissionsState({ isOverridesLoading: true });

  try {
    const overrides = await tauri.getChannelOverrides(channelId);
    setPermissionsState("channelOverrides", channelId, overrides);
    setPermissionsState({ isOverridesLoading: false });
  } catch (err) {
    console.error("[Permissions] Failed to load channel overrides:", err);
    setPermissionsState({ isOverridesLoading: false });
  }
}

/**
 * Get channel overrides
 */
export function getChannelOverrides(channelId: string): ChannelOverride[] {
  return permissionsState.channelOverrides[channelId] || [];
}

/**
 * Set a channel override
 */
export async function setChannelOverride(
  channelId: string,
  roleId: string,
  request: SetChannelOverrideRequest
): Promise<ChannelOverride> {
  const override = await tauri.setChannelOverride(channelId, roleId, request);
  setPermissionsState("channelOverrides", channelId, (prev) => {
    const filtered = (prev || []).filter((o) => o.role_id !== roleId);
    return [...filtered, override];
  });
  return override;
}

/**
 * Delete a channel override
 */
export async function deleteChannelOverride(
  channelId: string,
  roleId: string
): Promise<void> {
  await tauri.deleteChannelOverride(channelId, roleId);
  setPermissionsState("channelOverrides", channelId, (prev) =>
    (prev || []).filter((o) => o.role_id !== roleId)
  );
}

// ============================================================================
// Permission Computation Helpers
// ============================================================================

/**
 * Compute effective permissions for a member in a guild
 * Combines @everyone + all assigned roles
 */
export function computeMemberPermissions(
  guildId: string,
  userId: string,
  isOwner: boolean
): number {
  // Owner has all permissions
  if (isOwner) {
    return 0xffffffff; // All bits set
  }

  const guildRoles = getGuildRoles(guildId);
  const memberRoleIds = getMemberRoleIds(guildId, userId);

  // Start with @everyone permissions
  const everyoneRole = guildRoles.find((r) => r.is_default);
  let permissions = everyoneRole?.permissions || 0;

  // Add permissions from assigned roles
  for (const roleId of memberRoleIds) {
    const role = guildRoles.find((r) => r.id === roleId);
    if (role) {
      permissions |= role.permissions;
    }
  }

  return permissions;
}

/**
 * Compute effective permissions for a member in a specific channel
 * Applies channel overrides on top of guild permissions
 */
export function computeChannelPermissions(
  guildId: string,
  channelId: string,
  userId: string,
  isOwner: boolean
): number {
  // Owner bypasses all permission checks
  if (isOwner) {
    return 0xffffffff;
  }

  let permissions = computeMemberPermissions(guildId, userId, isOwner);
  const overrides = getChannelOverrides(channelId);
  const memberRoleIds = getMemberRoleIds(guildId, userId);

  // Apply @everyone channel override first
  const everyoneRole = getEveryoneRole(guildId);
  if (everyoneRole) {
    const everyoneOverride = overrides.find(
      (o) => o.role_id === everyoneRole.id
    );
    if (everyoneOverride) {
      permissions &= ~everyoneOverride.deny_permissions;
      permissions |= everyoneOverride.allow_permissions;
    }
  }

  // Apply role-specific overrides
  for (const roleId of memberRoleIds) {
    const override = overrides.find((o) => o.role_id === roleId);
    if (override) {
      permissions &= ~override.deny_permissions;
      permissions |= override.allow_permissions;
    }
  }

  return permissions;
}

/**
 * Check if a member has a specific permission in a guild
 */
export function memberHasPermission(
  guildId: string,
  userId: string,
  isOwner: boolean,
  permission: number
): boolean {
  const permissions = computeMemberPermissions(guildId, userId, isOwner);
  return hasPermission(permissions, permission);
}

/**
 * Check if the current user can manage a specific role
 * (must have MANAGE_ROLES and role must be below their highest role)
 */
export function canManageRole(
  guildId: string,
  userId: string,
  isOwner: boolean,
  targetRolePosition: number
): boolean {
  // Owner can manage all roles
  if (isOwner) return true;

  // Must have MANAGE_ROLES permission
  if (
    !memberHasPermission(
      guildId,
      userId,
      isOwner,
      PermissionBits.MANAGE_ROLES
    )
  ) {
    return false;
  }

  // Get user's highest role position
  const userRoles = getMemberRoles(guildId, userId);
  const highestPosition =
    userRoles.length > 0
      ? Math.min(...userRoles.map((r) => r.position))
      : Infinity;

  // Can only manage roles below their position (higher number = lower rank)
  return targetRolePosition > highestPosition;
}

/**
 * Get the highest role position for a user
 */
export function getUserHighestRolePosition(
  guildId: string,
  userId: string
): number {
  const userRoles = getMemberRoles(guildId, userId);
  if (userRoles.length === 0) return Infinity;
  return Math.min(...userRoles.map((r) => r.position));
}

// Export the store for reading
export { permissionsState };
