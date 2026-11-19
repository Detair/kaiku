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
import { plural, t } from "@/i18n";
import ConfirmActionDialog from "@/components/admin/ConfirmActionDialog";
import { reportFailure } from "./settingsAction";

interface RolesTabProps {
  guildId: string;
  onEditRole: (role: GuildRole) => void;
  onCreateRole: () => void;
}

const RolesTab: Component<RolesTabProps> = (props) => {
  const [menuOpen, setMenuOpen] = createSignal<string | null>(null);
  const [deleting, setDeleting] = createSignal<GuildRole | null>(null);
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
      reportFailure(t("guildSettings.roles.reorderFailed"), err);
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


  return (
    <div class="p-6">
      {/* Header */}
      <div class="flex items-center justify-between mb-4">
        <h3 class="text-lg font-semibold text-text-primary">{t("guildSettings.roles.title")}</h3>
        <Show when={canManageRoles()}>
          <button
            onClick={() => props.onCreateRole()}
            data-testid="roles-tab-create-role"
            class="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-accent-primary text-on-accent text-sm font-medium hover:bg-accent-primary/90 transition-colors"
          >
            <Plus class="w-4 h-4" />
            {t("guildSettings.roles.new")}
          </button>
        </Show>
      </div>

      {/* Loading */}
      <Show when={permissionsState.isRolesLoading}>
        <div class="text-center py-8 text-text-secondary">{t("guildSettings.loading")}</div>
      </Show>

      {/* Role List */}
      <Show when={!permissionsState.isRolesLoading}>
        <div class="space-y-2">
          <For each={roles()}>
            {(role) => (
              <div
                data-testid="roles-tab-role-row"
                data-role-id={role.id}
                data-role-name={role.is_default ? "@everyone" : role.name}
                class="flex items-center gap-3 p-3 rounded-lg border transition-colors group"
                classList={{
                  "border-white/10 hover:bg-white/5":
                    dropTargetId() !== role.id,
                  "border-accent-primary bg-accent-primary/20":
                    dropTargetId() === role.id,
                  "opacity-50": draggedRoleId() === role.id,
                }}
                style={{"background-color":"var(--color-surface-layer1)"}}
                draggable={canManageRoles() && canReorderRole(role)}
                onDragStart={(e) => handleDragStart(e, role.id)}
                onDragOver={(e) => handleDragOver(e, role)}
                onDragLeave={handleDragLeave}
                onDrop={(e) => handleDrop(e, role)}
                onDragEnd={handleDragEnd}
              >
                {/* Drag handle */}
                <Show when={canManageRoles() && canReorderRole(role)}>
                  <div class="cursor-grab text-text-secondary hover:text-text-primary opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 touch:opacity-100 transition-opacity">
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
                      ? t("guildSettings.roles.everyoneHint")
                      : plural(countPermissions(role.permissions), { one: "guildSettings.roles.permissionCount.one", other: "guildSettings.roles.permissionCount.other" })}
                  </div>
                </div>

                {/* Actions */}
                <Show when={canManageRoles()}>
                  <div class="flex items-center gap-1 opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 touch:opacity-100 transition-opacity">
                    <button
                      onClick={() => props.onEditRole(role)}
                      data-testid="roles-tab-role-edit"
                      class="p-2 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors"
                      title={t("guildSettings.roles.editNamed", { name: role.is_default ? "@everyone" : role.name })}
                      aria-label={t("guildSettings.roles.editNamed", { name: role.is_default ? "@everyone" : role.name })}
                    >
                      <Settings class="w-4 h-4" />
                    </button>
                    <Show when={!role.is_default}>
                      <div class="relative">
                        <button
                          onClick={() =>
                            setMenuOpen(menuOpen() === role.id ? null : role.id)
                          }
                          aria-label={t("guildSettings.roles.moreNamed", { name: role.name })}
                          aria-expanded={menuOpen() === role.id}
                          class="p-2 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors"
                        >
                          <MoreVertical class="w-4 h-4" />
                        </button>
                        <Show when={menuOpen() === role.id}>
                          <div
                            class="absolute right-0 top-full mt-1 py-1 rounded-lg border border-white/10 shadow-xl z-10 min-w-[160px]"
                            style={{"background-color":"var(--color-surface-layer2)"}}
                          >
                            <button
                              onClick={() => {
                                props.onEditRole(role);
                                setMenuOpen(null);
                              }}
                              class="w-full flex items-center gap-2 px-3 py-2 text-sm text-text-primary hover:bg-white/10 transition-colors"
                            >
                              <Users class="w-4 h-4" />
                              {t("guildSettings.roles.manageMembers")}
                            </button>
                            <button
                              onClick={() => {
                                setDeleting(role);
                                setMenuOpen(null);
                              }}
                              class="w-full flex items-center gap-2 px-3 py-2 text-sm text-text-primary hover:bg-white/10 transition-colors"
                            >
                              <Trash2 class="w-4 h-4" />
                              {t("guildSettings.roles.delete")}
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

      <Show when={deleting()}>
        {(role) => (
          <ConfirmActionDialog
            title={t("guildSettings.roles.deleteTitle", { name: role().name })}
            message={t("guildSettings.roles.deleteBody", { name: role().name })}
            confirmLabel={t("guildSettings.roles.delete")}
            danger
            onConfirm={() => deleteRole(props.guildId, role().id)}
            onDone={() => setDeleting(null)}
            onCancel={() => setDeleting(null)}
          />
        )}
      </Show>
    </div>
  );
};

export default RolesTab;
