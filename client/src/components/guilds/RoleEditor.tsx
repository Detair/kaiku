/**
 * RoleEditor - Slide-out panel for editing role permissions
 */

import { Component, createSignal, createEffect, For, Show } from "solid-js";
import { ArrowLeft, Plus, X } from "lucide-solid";
import {
  permissionsState,
  createRole,
  updateRole,
  getMemberRoles,
  assignMemberRole,
  removeMemberRole,
} from "@/stores/permissions";
import { authState } from "@/stores/auth";
import { isGuildOwner, getGuildMembers } from "@/stores/guilds";
import {
  PERMISSIONS,
  CATEGORY_NAMES,
  PermissionCategory,
  hasPermission,
  addPermission,
  removePermission,
} from "@/lib/permissionConstants";
import type { GuildRole } from "@/lib/types";
import { showToast } from "@/components/ui/Toast";

interface RoleEditorProps {
  guildId: string;
  role: GuildRole | null; // null = creating new role
  onBack: () => void;
  onSave: () => void;
}

const RoleEditor: Component<RoleEditorProps> = (props) => {
  const [name, setName] = createSignal("");
  const [color, setColor] = createSignal<string | null>(null);
  const [permissions, setPermissions] = createSignal(0);
  const [isSaving, setIsSaving] = createSignal(false);
  const [showMemberPicker, setShowMemberPicker] = createSignal(false);
  const [hasChanges, setHasChanges] = createSignal(false);

  const isOwner = () => isGuildOwner(props.guildId, authState.user?.id || "");
  const isNewRole = () => !props.role;
  const isEveryoneRole = () => props.role?.is_default ?? false;

  // User's own permissions (for escalation prevention)
  const userPermissions = () => {
    if (isOwner()) return 0xffffffff;
    const userId = authState.user?.id || "";
    const userRoles = getMemberRoles(props.guildId, userId);
    let perms = 0;
    for (const r of userRoles) {
      perms |= r.permissions;
    }
    return perms;
  };

  // Initialize form when role changes
  createEffect(() => {
    if (props.role) {
      setName(props.role.is_default ? "@everyone" : props.role.name);
      setColor(props.role.color);
      setPermissions(props.role.permissions);
    } else {
      setName("");
      setColor(null);
      setPermissions(0);
    }
    setHasChanges(false);
  });

  // Track changes
  createEffect(() => {
    if (props.role) {
      const changed =
        name() !== (props.role.is_default ? "@everyone" : props.role.name) ||
        color() !== props.role.color ||
        permissions() !== props.role.permissions;
      setHasChanges(changed);
    } else {
      setHasChanges(name().trim() !== "");
    }
  });

  const handlePermissionToggle = (bit: number) => {
    if (hasPermission(permissions(), bit)) {
      setPermissions(removePermission(permissions(), bit));
    } else {
      setPermissions(addPermission(permissions(), bit));
    }
  };

  const canEditPermission = (bit: number): boolean => {
    if (isOwner()) return true;
    return hasPermission(userPermissions(), bit);
  };

  const handleSave = async () => {
    if (isSaving()) return;
    setIsSaving(true);

    try {
      if (isNewRole()) {
        await createRole(props.guildId, {
          name: name(),
          color: color() || undefined,
          permissions: permissions(),
        });
        showToast({
          type: "success",
          title: "Role Created",
          message: `Role "${name()}" has been created successfully.`,
          duration: 3000,
        });
      } else if (props.role) {
        await updateRole(props.guildId, props.role.id, {
          name: isEveryoneRole() ? undefined : name(),
          color: color() || undefined,
          permissions: permissions(),
        });
        showToast({
          type: "success",
          title: "Role Updated",
          message: "Role permissions have been saved.",
          duration: 3000,
        });
      }
      props.onSave();
    } catch (err) {
      console.error("Failed to save role:", err);
      showToast({
        type: "error",
        title: "Failed to Save Role",
        message: "Could not save role changes. Please try again.",
      });
    } finally {
      setIsSaving(false);
    }
  };

  const handleBack = () => {
    if (hasChanges()) {
      if (!confirm("You have unsaved changes. Discard them?")) {
        return;
      }
    }
    props.onBack();
  };

  // Group permissions by category
  const permissionsByCategory = () => {
    const categories: Record<PermissionCategory, typeof PERMISSIONS> = {
      content: [],
      voice: [],
      moderation: [],
      guild_management: [],
      invites: [],
      pages: [],
    };
    for (const perm of PERMISSIONS) {
      // For @everyone, skip forbidden permissions entirely
      if (isEveryoneRole() && perm.forbiddenForEveryone) continue;
      categories[perm.category].push(perm);
    }
    return categories;
  };

  // Get members with this role
  const membersWithRole = () => {
    if (!props.role || props.role.is_default) return [];
    const members = getGuildMembers(props.guildId);
    const memberRoles = permissionsState.memberRoles[props.guildId] || {};
    return members.filter((m) =>
      memberRoles[m.user_id]?.includes(props.role!.id)
    );
  };

  // Get members without this role (for picker)
  const availableMembers = () => {
    if (!props.role) return [];
    const members = getGuildMembers(props.guildId);
    const memberRoles = permissionsState.memberRoles[props.guildId] || {};
    return members.filter(
      (m) => !memberRoles[m.user_id]?.includes(props.role!.id)
    );
  };

  const handleAddMember = async (userId: string) => {
    if (!props.role) return;
    try {
      await assignMemberRole(props.guildId, userId, props.role.id);
    } catch (err) {
      console.error("Failed to assign role:", err);
    }
    setShowMemberPicker(false);
  };

  const handleRemoveMember = async (userId: string) => {
    if (!props.role) return;
    try {
      await removeMemberRole(props.guildId, userId, props.role.id);
    } catch (err) {
      console.error("Failed to remove role:", err);
    }
  };

  const colorPresets = [
    "#ef4444", "#f97316", "#eab308", "#22c55e",
    "#06b6d4", "#3b82f6", "#8b5cf6", "#ec4899",
  ];

  return (
    <div class="flex flex-col h-full">
      {/* Header */}
      <div class="flex items-center gap-3 px-6 py-4 border-b border-white/10">
        <button
          onClick={handleBack}
          class="p-1.5 text-text-secondary hover:text-text-primary hover:bg-white/10 rounded-lg transition-colors"
        >
          <ArrowLeft class="w-5 h-5" />
        </button>
        <h3 class="text-lg font-semibold text-text-primary">
          {isNewRole() ? "Create Role" : `Edit Role: ${name()}`}
        </h3>
      </div>

      {/* Content */}
      <div class="flex-1 overflow-y-auto p-6 space-y-6">
        {/* Name */}
        <div>
          <label class="block text-sm font-medium text-text-secondary mb-2">
            Role Name
          </label>
          <input
            type="text"
            value={name()}
            onInput={(e) => setName(e.currentTarget.value)}
            disabled={isEveryoneRole()}
            placeholder="Enter role name..."
            class="w-full px-3 py-2 rounded-lg border border-white/10 text-text-primary placeholder-text-secondary disabled:opacity-50"
            style="background-color: var(--color-surface-layer1)"
          />
        </div>

        {/* Color */}
        <div>
          <label class="block text-sm font-medium text-text-secondary mb-2">
            Color
          </label>
          <div class="flex items-center gap-2 flex-wrap">
            <button
              onClick={() => setColor(null)}
              class="w-8 h-8 rounded-full border-2 transition-colors"
              classList={{
                "border-accent-primary": color() === null,
                "border-white/20": color() !== null,
              }}
              style="background-color: var(--color-surface-layer1)"
              title="No color"
            />
            <For each={colorPresets}>
              {(preset) => (
                <button
                  onClick={() => setColor(preset)}
                  class="w-8 h-8 rounded-full border-2 transition-colors"
                  classList={{
                    "border-white": color() === preset,
                    "border-transparent": color() !== preset,
                  }}
                  style={{ "background-color": preset }}
                />
              )}
            </For>
          </div>
        </div>

        {/* Permissions */}
        <div>
          <label class="block text-sm font-medium text-text-secondary mb-3">
            Permissions
          </label>
          <div class="space-y-4">
            <For each={Object.entries(permissionsByCategory())}>
              {([category, perms]) => (
                <Show when={perms.length > 0}>
                  <div>
                    <h4 class="text-xs font-semibold text-text-secondary uppercase tracking-wider mb-2">
                      {CATEGORY_NAMES[category as PermissionCategory]}
                    </h4>
                    <div class="space-y-1">
                      <For each={perms}>
                        {(perm) => {
                          const canEdit = canEditPermission(perm.bit);
                          const isEnabled = hasPermission(permissions(), perm.bit);

                          return (
                            <label
                              class="flex items-start gap-3 p-2 rounded-lg cursor-pointer hover:bg-white/5 transition-colors"
                              classList={{ "opacity-50 cursor-not-allowed": !canEdit }}
                            >
                              <input
                                type="checkbox"
                                checked={isEnabled}
                                disabled={!canEdit}
                                onChange={() => canEdit && handlePermissionToggle(perm.bit)}
                                class="mt-1 w-4 h-4 rounded border-white/20 text-accent-primary focus:ring-accent-primary focus:ring-offset-0"
                                style="background-color: var(--color-surface-layer1)"
                              />
                              <div class="flex-1">
                                <div class="text-sm font-medium text-text-primary">
                                  {perm.name}
                                </div>
                                <div class="text-xs text-text-secondary">
                                  {perm.description}
                                </div>
                                <Show when={!canEdit}>
                                  <div class="text-xs text-accent-warning mt-1">
                                    You don't have this permission
                                  </div>
                                </Show>
                              </div>
                            </label>
                          );
                        }}
                      </For>
                    </div>
                  </div>
                </Show>
              )}
            </For>
          </div>
        </div>

        {/* Members with this role */}
        <Show when={!isNewRole() && !isEveryoneRole()}>
          <div>
            <div class="flex items-center justify-between mb-3">
              <label class="text-sm font-medium text-text-secondary">
                Members with this role ({membersWithRole().length})
              </label>
              <div class="relative">
                <button
                  onClick={() => setShowMemberPicker(!showMemberPicker())}
                  class="flex items-center gap-1 px-2 py-1 text-sm text-accent-primary hover:bg-accent-primary/10 rounded-lg transition-colors"
                >
                  <Plus class="w-4 h-4" />
                  Add Member
                </button>
                <Show when={showMemberPicker()}>
                  <div
                    class="absolute right-0 top-full mt-1 py-1 rounded-lg border border-white/10 shadow-xl z-10 w-64 max-h-48 overflow-y-auto"
                    style="background-color: var(--color-surface-layer2)"
                  >
                    <Show
                      when={availableMembers().length > 0}
                      fallback={
                        <div class="px-3 py-2 text-sm text-text-secondary">
                          All members have this role
                        </div>
                      }
                    >
                      <For each={availableMembers()}>
                        {(member) => (
                          <button
                            onClick={() => handleAddMember(member.user_id)}
                            class="w-full flex items-center gap-2 px-3 py-2 text-sm text-text-primary hover:bg-white/10 transition-colors"
                          >
                            <div class="w-6 h-6 rounded-full bg-accent-primary/20 flex items-center justify-center text-xs">
                              {member.display_name.charAt(0).toUpperCase()}
                            </div>
                            {member.display_name}
                          </button>
                        )}
                      </For>
                    </Show>
                  </div>
                </Show>
              </div>
            </div>
            <div class="space-y-1">
              <For each={membersWithRole()}>
                {(member) => (
                  <div class="flex items-center gap-2 p-2 rounded-lg hover:bg-white/5 transition-colors group">
                    <div class="w-8 h-8 rounded-full bg-accent-primary/20 flex items-center justify-center text-sm">
                      {member.display_name.charAt(0).toUpperCase()}
                    </div>
                    <div class="flex-1">
                      <div class="text-sm text-text-primary">{member.display_name}</div>
                      <div class="text-xs text-text-secondary">@{member.username}</div>
                    </div>
                    <button
                      onClick={() => handleRemoveMember(member.user_id)}
                      class="p-1 text-text-secondary hover:text-accent-danger opacity-0 group-hover:opacity-100 transition-all"
                    >
                      <X class="w-4 h-4" />
                    </button>
                  </div>
                )}
              </For>
              <Show when={membersWithRole().length === 0}>
                <div class="text-sm text-text-secondary py-2">
                  No members have this role yet
                </div>
              </Show>
            </div>
          </div>
        </Show>
      </div>

      {/* Footer */}
      <div class="flex items-center justify-end gap-3 px-6 py-4 border-t border-white/10">
        <button
          onClick={handleBack}
          class="px-4 py-2 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors"
        >
          Cancel
        </button>
        <button
          onClick={handleSave}
          disabled={isSaving() || !hasChanges() || !name().trim()}
          class="px-4 py-2 rounded-lg bg-accent-primary text-white font-medium hover:bg-accent-primary/90 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        >
          {isSaving() ? "Saving..." : isNewRole() ? "Create Role" : "Save Changes"}
        </button>
      </div>
    </div>
  );
};

export default RoleEditor;
