/**
 * RolesTab - Role list with create/edit/delete functionality
 */

import { Component, createSignal, For, Show, onMount } from "solid-js";
import {
  Plus,
  Settings,
  MoreVertical,
  Trash2,
  Users,
  GripVertical,
} from "lucide-solid";
import {
  permissionsState,
  loadGuildRoles,
  loadMemberRoles,
  getGuildRoles,
  deleteRole,
  reorderRole,
  memberHasPermission,
  getUserHighestRolePosition,
} from "@/stores/permissions";
import { authState } from "@/stores/auth";
import { isGuildOwner } from "@/stores/guilds";
import { PermissionBits } from "@/lib/permissionConstants";
import type { GuildRole } from "@/lib/types";

interface RolesTabProps {
  guildId: string;
  onEditRole: (role: GuildRole) => void;
  onCreateRole: () => void;
}

const RolesTab: Component<RolesTabProps> = (props) => {
  const [menuOpen, setMenuOpen] = createSignal<string | null>(null);
  const [deleteConfirm, setDeleteConfirm] = createSignal<string | null>(null);
  const [draggedRoleId, setDraggedRoleId] = createSignal<string | null>(null);
  const [dropTargetId, setDropTargetId] = createSignal<string | null>(null);

  onMount(() => {
    loadGuildRoles(props.guildId);
    loadMemberRoles(props.guildId);
  });

  const roles = () => getGuildRoles(props.guildId);
  const isOwner = () => isGuildOwner(props.guildId, authState.user?.id || "");
  const canManageRoles = () =>
    isOwner() ||
    memberHasPermission(
      props.guildId,
      authState.user?.id || "",
      isOwner(),
      PermissionBits.MANAGE_ROLES,
    );

  // Get user's highest role position (lower = higher rank)
  const userHighestPosition = () =>
    isOwner()
      ? -1
      : getUserHighestRolePosition(props.guildId, authState.user?.id || "");

  // Check if user can reorder a specific role (must be below their highest role)
  const canReorderRole = (role: GuildRole) => {
    if (role.is_default) return false; // Can't reorder @everyone
    if (isOwner()) return true;
    return role.position > userHighestPosition();
  };

  // Check if a role can be dropped on target position
  const canDropOnRole = (targetRole: GuildRole) => {
    if (targetRole.is_default) return false; // Can't drop on @everyone
    if (isOwner()) return true;
    return targetRole.position > userHighestPosition();
  };

  // Drag handlers
  const handleDragStart = (e: DragEvent, roleId: string) => {
    if (!e.dataTransfer) return;
    e.dataTransfer.effectAllowed = "move";
    e.dataTransfer.setData("text/plain", roleId);
    setDraggedRoleId(roleId);
  };

  const handleDragOver = (e: DragEvent, role: GuildRole) => {
    e.preventDefault();
    if (!e.dataTransfer) return;

    const draggedId = draggedRoleId();
    if (!draggedId || draggedId === role.id) return;

    const draggedRole = roles().find((r) => r.id === draggedId);
    if (!draggedRole) return;

    if (!canDropOnRole(role)) {
      e.dataTransfer.dropEffect = "none";
      return;
    }

    e.dataTransfer.dropEffect = "move";
    setDropTargetId(role.id);
  };

  const handleDragLeave = () => {
    setDropTargetId(null);
  };

  const handleDrop = async (e: DragEvent, targetRole: GuildRole) => {
    e.preventDefault();
    setDropTargetId(null);

    const draggedId = draggedRoleId();
    if (!draggedId || draggedId === targetRole.id) return;

    const draggedRole = roles().find((r) => r.id === draggedId);
    if (!draggedRole) return;

    if (!canDropOnRole(targetRole)) return;

    try {
      await reorderRole(props.guildId, draggedId, targetRole.position);
    } catch (err) {
      console.error("Failed to reorder role:", err);
    }
  };

  const handleDragEnd = () => {
    setDraggedRoleId(null);
    setDropTargetId(null);
  };

  const countPermissions = (permissions: number): number => {
    let count = 0;
    for (let i = 0; i < 32; i++) {
      if (permissions & (1 << i)) count++;
    }
    return count;
  };

  const handleDelete = async (roleId: string) => {
    if (deleteConfirm() === roleId) {
      try {
        await deleteRole(props.guildId, roleId);
      } catch (err) {
        console.error("Failed to delete role:", err);
      }
      setDeleteConfirm(null);
      setMenuOpen(null);
    } else {
      setDeleteConfirm(roleId);
      setTimeout(() => setDeleteConfirm(null), 3000);
    }
  };

  return (
    <div class="p-6">
      {/* Header */}
      <div class="flex items-center justify-between mb-4">
        <h3 class="text-lg font-semibold text-text-primary">Roles</h3>
        <Show when={canManageRoles()}>
          <button
            onClick={() => props.onCreateRole()}
            class="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-accent-primary text-white text-sm font-medium hover:bg-accent-primary/90 transition-colors"
          >
            <Plus class="w-4 h-4" />
            New Role
          </button>
        </Show>
      </div>

      {/* Loading */}
      <Show when={permissionsState.isRolesLoading}>
        <div class="text-center py-8 text-text-secondary">Loading roles...</div>
      </Show>

      {/* Role List */}
      <Show when={!permissionsState.isRolesLoading}>
        <div class="space-y-2">
          <For each={roles()}>
            {(role) => (
              <div
                class="flex items-center gap-3 p-3 rounded-lg border transition-colors group"
                classList={{
                  "border-white/10 hover:bg-white/5":
                    dropTargetId() !== role.id,
                  "border-accent-primary bg-accent-primary/10":
                    dropTargetId() === role.id,
                  "opacity-50": draggedRoleId() === role.id,
                }}
                style="background-color: var(--color-surface-layer1)"
                draggable={canManageRoles() && canReorderRole(role)}
                onDragStart={(e) => handleDragStart(e, role.id)}
                onDragOver={(e) => handleDragOver(e, role)}
                onDragLeave={handleDragLeave}
                onDrop={(e) => handleDrop(e, role)}
                onDragEnd={handleDragEnd}
              >
                {/* Drag handle */}
                <Show when={canManageRoles() && canReorderRole(role)}>
                  <div class="cursor-grab text-text-secondary hover:text-text-primary opacity-0 group-hover:opacity-100 transition-opacity">
                    <GripVertical class="w-4 h-4" />
                  </div>
                </Show>

                {/* Color dot */}
                <div
                  class="w-3 h-3 rounded-full flex-shrink-0"
                  style={{
                    "background-color": role.color || "transparent",
                    border: role.color
                      ? "none"
                      : "2px solid var(--color-text-secondary)",
                  }}
                />

                {/* Role info */}
                <div class="flex-1 min-w-0">
                  <div class="font-medium text-text-primary">
                    {role.is_default ? "@everyone" : role.name}
                  </div>
                  <div class="text-xs text-text-secondary">
                    {role.is_default
                      ? "Base permissions for all members"
                      : `${countPermissions(role.permissions)} permissions`}
                  </div>
                </div>

                {/* Actions */}
                <Show when={canManageRoles()}>
                  <div class="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                    <button
                      onClick={() => props.onEditRole(role)}
                      class="p-2 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors"
                      title="Edit role"
                    >
                      <Settings class="w-4 h-4" />
                    </button>
                    <Show when={!role.is_default}>
                      <div class="relative">
                        <button
                          onClick={() =>
                            setMenuOpen(menuOpen() === role.id ? null : role.id)
                          }
                          class="p-2 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors"
                        >
                          <MoreVertical class="w-4 h-4" />
                        </button>
                        <Show when={menuOpen() === role.id}>
                          <div
                            class="absolute right-0 top-full mt-1 py-1 rounded-lg border border-white/10 shadow-xl z-10 min-w-[160px]"
                            style="background-color: var(--color-surface-layer2)"
                          >
                            <button
                              onClick={() => {
                                props.onEditRole(role);
                                setMenuOpen(null);
                              }}
                              class="w-full flex items-center gap-2 px-3 py-2 text-sm text-text-primary hover:bg-white/10 transition-colors"
                            >
                              <Users class="w-4 h-4" />
                              Manage Members
                            </button>
                            <button
                              onClick={() => handleDelete(role.id)}
                              class="w-full flex items-center gap-2 px-3 py-2 text-sm transition-colors"
                              classList={{
                                "text-accent-danger bg-accent-danger/10":
                                  deleteConfirm() === role.id,
                                "text-text-primary hover:bg-white/10":
                                  deleteConfirm() !== role.id,
                              }}
                            >
                              <Trash2 class="w-4 h-4" />
                              {deleteConfirm() === role.id
                                ? "Confirm Delete"
                                : "Delete Role"}
                            </button>
                          </div>
                        </Show>
                      </div>
                    </Show>
                  </div>
                </Show>
              </div>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
};

export default RolesTab;
