# Permission System UI Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement UI components for guild role management, channel permission overrides, and system admin dashboard.

**Architecture:** Tab-based modals following existing GuildSettingsModal patterns, using Solid.js signals and stores. Permission checks via `permissionsState` store. Slide-out panels for editors within modals.

**Tech Stack:** Solid.js, TypeScript, UnoCSS, lucide-solid icons, existing `permissions.ts` store

---

## Task 1: RolesTab Component

**Files:**
- Create: `client/src/components/guilds/RolesTab.tsx`

**Step 1: Create the basic RolesTab component**

```tsx
/**
 * RolesTab - Role list with create/edit/delete functionality
 */

import { Component, createSignal, For, Show, onMount } from "solid-js";
import { Plus, Settings, MoreVertical, Trash2, Users } from "lucide-solid";
import {
  permissionsState,
  loadGuildRoles,
  loadMemberRoles,
  getGuildRoles,
  deleteRole,
  memberHasPermission,
} from "@/stores/permissions";
import { authState } from "@/stores/auth";
import { isGuildOwner } from "@/stores/guilds";
import { PermissionBits, hasPermission } from "@/lib/permissionConstants";
import type { GuildRole } from "@/lib/types";

interface RolesTabProps {
  guildId: string;
  onEditRole: (role: GuildRole) => void;
  onCreateRole: () => void;
}

const RolesTab: Component<RolesTabProps> = (props) => {
  const [menuOpen, setMenuOpen] = createSignal<string | null>(null);
  const [deleteConfirm, setDeleteConfirm] = createSignal<string | null>(null);

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
      PermissionBits.MANAGE_ROLES
    );

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
                class="flex items-center gap-3 p-3 rounded-lg border border-white/10 hover:bg-white/5 transition-colors group"
                style="background-color: var(--color-surface-layer1)"
              >
                {/* Color dot */}
                <div
                  class="w-3 h-3 rounded-full flex-shrink-0"
                  style={{
                    "background-color": role.color || "transparent",
                    border: role.color ? "none" : "2px solid var(--color-text-secondary)",
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
                          onClick={() => setMenuOpen(menuOpen() === role.id ? null : role.id)}
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
                                "text-accent-danger bg-accent-danger/10": deleteConfirm() === role.id,
                                "text-text-primary hover:bg-white/10": deleteConfirm() !== role.id,
                              }}
                            >
                              <Trash2 class="w-4 h-4" />
                              {deleteConfirm() === role.id ? "Confirm Delete" : "Delete Role"}
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
```

**Step 2: Verify the component has no TypeScript errors**

Run: `cd client && bun tsc --noEmit src/components/guilds/RolesTab.tsx 2>&1 | head -20`

**Step 3: Commit**

```bash
git add client/src/components/guilds/RolesTab.tsx
git commit -m "feat(ui): add RolesTab component for guild settings"
```

---

## Task 2: RoleEditor Component

**Files:**
- Create: `client/src/components/guilds/RoleEditor.tsx`

**Step 1: Create the RoleEditor component**

```tsx
/**
 * RoleEditor - Slide-out panel for editing role permissions
 */

import { Component, createSignal, createEffect, For, Show, onMount } from "solid-js";
import { ArrowLeft, Plus, X } from "lucide-solid";
import {
  permissionsState,
  createRole,
  updateRole,
  getMemberRoles,
  assignMemberRole,
  removeMemberRole,
  memberHasPermission,
  loadMemberRoles,
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
  PermissionBits,
} from "@/lib/permissionConstants";
import type { GuildRole, GuildMember } from "@/lib/types";

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
      } else if (props.role) {
        await updateRole(props.guildId, props.role.id, {
          name: isEveryoneRole() ? undefined : name(),
          color: color() || undefined,
          permissions: permissions(),
        });
      }
      props.onSave();
    } catch (err) {
      console.error("Failed to save role:", err);
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
          disabled={isSaving() || !hasChanges() || (!isNewRole() && !name().trim())}
          class="px-4 py-2 rounded-lg bg-accent-primary text-white font-medium hover:bg-accent-primary/90 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        >
          {isSaving() ? "Saving..." : isNewRole() ? "Create Role" : "Save Changes"}
        </button>
      </div>
    </div>
  );
};

export default RoleEditor;
```

**Step 2: Commit**

```bash
git add client/src/components/guilds/RoleEditor.tsx
git commit -m "feat(ui): add RoleEditor component for permission management"
```

---

## Task 3: Integrate Roles Tab into GuildSettingsModal

**Files:**
- Modify: `client/src/components/guilds/GuildSettingsModal.tsx`

**Step 1: Update imports and add Roles tab**

```tsx
// Add to imports at top:
import { Shield } from "lucide-solid";
import RolesTab from "./RolesTab";
import RoleEditor from "./RoleEditor";
import { memberHasPermission } from "@/stores/permissions";
import { PermissionBits } from "@/lib/permissionConstants";
import type { GuildRole } from "@/lib/types";

// Change TabId type:
type TabId = "invites" | "members" | "roles";

// Add signals after activeTab:
const [editingRole, setEditingRole] = createSignal<GuildRole | null>(null);
const [isCreatingRole, setIsCreatingRole] = createSignal(false);

// Add permission check:
const canManageRoles = () =>
  isOwner() ||
  memberHasPermission(
    props.guildId,
    authState.user?.id || "",
    isOwner(),
    PermissionBits.MANAGE_ROLES
  );

// Add to tabs section (after Members tab button):
<Show when={canManageRoles()}>
  <button
    onClick={() => setActiveTab("roles")}
    class="flex items-center gap-2 px-6 py-3 font-medium transition-colors"
    classList={{
      "text-accent-primary border-b-2 border-accent-primary": activeTab() === "roles",
      "text-text-secondary hover:text-text-primary": activeTab() !== "roles",
    }}
  >
    <Shield class="w-4 h-4" />
    Roles
  </button>
</Show>

// Add to content section:
<Show when={activeTab() === "roles" && canManageRoles()}>
  <Show
    when={editingRole() || isCreatingRole()}
    fallback={
      <RolesTab
        guildId={props.guildId}
        onEditRole={(role) => setEditingRole(role)}
        onCreateRole={() => setIsCreatingRole(true)}
      />
    }
  >
    <RoleEditor
      guildId={props.guildId}
      role={editingRole()}
      onBack={() => {
        setEditingRole(null);
        setIsCreatingRole(false);
      }}
      onSave={() => {
        setEditingRole(null);
        setIsCreatingRole(false);
      }}
    />
  </Show>
</Show>
```

**Step 2: Commit**

```bash
git add client/src/components/guilds/GuildSettingsModal.tsx
git commit -m "feat(ui): integrate Roles tab into GuildSettingsModal"
```

---

## Task 4: MemberRoleDropdown Component

**Files:**
- Create: `client/src/components/guilds/MemberRoleDropdown.tsx`

**Step 1: Create the dropdown component**

```tsx
/**
 * MemberRoleDropdown - Dropdown for managing member roles
 */

import { Component, createSignal, For, Show } from "solid-js";
import { ChevronDown, UserX } from "lucide-solid";
import {
  getGuildRoles,
  getMemberRoleIds,
  assignMemberRole,
  removeMemberRole,
  canManageRole,
  getUserHighestRolePosition,
} from "@/stores/permissions";
import { authState } from "@/stores/auth";
import { isGuildOwner, kickMember } from "@/stores/guilds";

interface MemberRoleDropdownProps {
  guildId: string;
  userId: string;
  onClose?: () => void;
}

const MemberRoleDropdown: Component<MemberRoleDropdownProps> = (props) => {
  const [isOpen, setIsOpen] = createSignal(false);
  const [kickConfirm, setKickConfirm] = createSignal(false);

  const isOwner = () => isGuildOwner(props.guildId, authState.user?.id || "");
  const isMemberOwner = () => isGuildOwner(props.guildId, props.userId);
  const currentUserId = () => authState.user?.id || "";

  const roles = () => getGuildRoles(props.guildId).filter((r) => !r.is_default);
  const memberRoleIds = () => getMemberRoleIds(props.guildId, props.userId);

  const userHighestPosition = () =>
    getUserHighestRolePosition(props.guildId, currentUserId());

  const canManageThisRole = (rolePosition: number): boolean => {
    return canManageRole(props.guildId, currentUserId(), isOwner(), rolePosition);
  };

  const handleToggleRole = async (roleId: string, hasRole: boolean) => {
    try {
      if (hasRole) {
        await removeMemberRole(props.guildId, props.userId, roleId);
      } else {
        await assignMemberRole(props.guildId, props.userId, roleId);
      }
    } catch (err) {
      console.error("Failed to toggle role:", err);
    }
  };

  const handleKick = async () => {
    if (kickConfirm()) {
      try {
        await kickMember(props.guildId, props.userId);
        setIsOpen(false);
        props.onClose?.();
      } catch (err) {
        console.error("Failed to kick member:", err);
      }
      setKickConfirm(false);
    } else {
      setKickConfirm(true);
      setTimeout(() => setKickConfirm(false), 3000);
    }
  };

  // Can only kick if owner or has kick permission and target is below us
  const canKick = () => {
    if (isMemberOwner()) return false;
    if (props.userId === currentUserId()) return false;
    if (isOwner()) return true;
    // TODO: Check KICK_MEMBERS permission
    return false;
  };

  return (
    <div class="relative">
      <button
        onClick={() => setIsOpen(!isOpen())}
        class="flex items-center gap-1 px-2 py-1 text-sm text-text-secondary hover:text-text-primary hover:bg-white/10 rounded-lg transition-colors"
      >
        Manage
        <ChevronDown class="w-4 h-4" />
      </button>

      <Show when={isOpen()}>
        <div
          class="absolute right-0 top-full mt-1 py-1 rounded-lg border border-white/10 shadow-xl z-20 min-w-[200px]"
          style="background-color: var(--color-surface-layer2)"
        >
          {/* Role assignment section */}
          <div class="px-3 py-1.5 text-xs font-semibold text-text-secondary uppercase">
            Assign Role
          </div>
          <Show
            when={roles().length > 0}
            fallback={
              <div class="px-3 py-2 text-sm text-text-secondary">No roles available</div>
            }
          >
            <For each={roles()}>
              {(role) => {
                const hasRole = memberRoleIds().includes(role.id);
                const canManage = canManageThisRole(role.position);

                return (
                  <label
                    class="flex items-center gap-2 px-3 py-2 cursor-pointer hover:bg-white/10 transition-colors"
                    classList={{ "opacity-50 cursor-not-allowed": !canManage }}
                  >
                    <input
                      type="checkbox"
                      checked={hasRole}
                      disabled={!canManage}
                      onChange={() => canManage && handleToggleRole(role.id, hasRole)}
                      class="w-4 h-4 rounded border-white/20 text-accent-primary"
                    />
                    <div
                      class="w-2.5 h-2.5 rounded-full flex-shrink-0"
                      style={{
                        "background-color": role.color || "transparent",
                        border: role.color ? "none" : "1px solid var(--color-text-secondary)",
                      }}
                    />
                    <span class="text-sm text-text-primary">{role.name}</span>
                  </label>
                );
              }}
            </For>
          </Show>

          {/* Kick section */}
          <Show when={canKick()}>
            <div class="border-t border-white/10 mt-1 pt-1">
              <button
                onClick={handleKick}
                class="w-full flex items-center gap-2 px-3 py-2 text-sm transition-colors"
                classList={{
                  "text-accent-danger bg-accent-danger/10": kickConfirm(),
                  "text-text-primary hover:bg-white/10": !kickConfirm(),
                }}
              >
                <UserX class="w-4 h-4" />
                {kickConfirm() ? "Confirm Kick" : "Kick from Server"}
              </button>
            </div>
          </Show>
        </div>
      </Show>

      {/* Click outside to close */}
      <Show when={isOpen()}>
        <div
          class="fixed inset-0 z-10"
          onClick={() => setIsOpen(false)}
        />
      </Show>
    </div>
  );
};

export default MemberRoleDropdown;
```

**Step 2: Commit**

```bash
git add client/src/components/guilds/MemberRoleDropdown.tsx
git commit -m "feat(ui): add MemberRoleDropdown for role assignment"
```

---

## Task 5: Enhance MembersTab with Role Badges

**Files:**
- Modify: `client/src/components/guilds/MembersTab.tsx`

**Step 1: Update imports and add role display**

Add to imports:
```tsx
import { loadGuildRoles, loadMemberRoles, getMemberRoles, memberHasPermission } from "@/stores/permissions";
import { PermissionBits } from "@/lib/permissionConstants";
import { authState } from "@/stores/auth";
import MemberRoleDropdown from "./MemberRoleDropdown";
```

Add to onMount:
```tsx
loadGuildRoles(props.guildId);
loadMemberRoles(props.guildId);
```

Add helper function:
```tsx
const canManageRoles = () =>
  props.isOwner ||
  memberHasPermission(
    props.guildId,
    authState.user?.id || "",
    props.isOwner,
    PermissionBits.MANAGE_ROLES
  );
```

Replace the kick button section with role badges + dropdown:
```tsx
{/* Role badges */}
<div class="flex items-center gap-1 flex-wrap">
  <For each={getMemberRoles(props.guildId, member.user_id)}>
    {(role) => (
      <span
        class="px-1.5 py-0.5 text-xs rounded-full"
        style={{
          "background-color": role.color ? `${role.color}20` : "var(--color-surface-layer1)",
          color: role.color || "var(--color-text-secondary)",
          border: `1px solid ${role.color || "var(--color-white-10)"}`,
        }}
      >
        {role.name}
      </span>
    )}
  </For>
  <Show when={getMemberRoles(props.guildId, member.user_id).length === 0}>
    <span class="text-xs text-text-secondary">(no roles)</span>
  </Show>
</div>

{/* Manage dropdown - replaces kick button */}
<Show when={canManageRoles() && !isGuildOwner}>
  <MemberRoleDropdown
    guildId={props.guildId}
    userId={member.user_id}
  />
</Show>
```

**Step 2: Commit**

```bash
git add client/src/components/guilds/MembersTab.tsx
git commit -m "feat(ui): add role badges and management to MembersTab"
```

---

## Task 6: ChannelSettingsModal Component

**Files:**
- Create: `client/src/components/channels/ChannelSettingsModal.tsx`

**Step 1: Create the modal**

```tsx
/**
 * ChannelSettingsModal - Channel settings with permissions tab
 */

import { Component, createSignal, Show, onMount } from "solid-js";
import { Portal } from "solid-js/web";
import { X, Hash, Settings, Shield } from "lucide-solid";
import { channelsState } from "@/stores/channels";
import { memberHasPermission } from "@/stores/permissions";
import { authState } from "@/stores/auth";
import { isGuildOwner } from "@/stores/guilds";
import { PermissionBits } from "@/lib/permissionConstants";
import ChannelPermissions from "./ChannelPermissions";

interface ChannelSettingsModalProps {
  channelId: string;
  guildId: string;
  onClose: () => void;
}

type TabId = "overview" | "permissions";

const ChannelSettingsModal: Component<ChannelSettingsModalProps> = (props) => {
  const [activeTab, setActiveTab] = createSignal<TabId>("overview");

  const channel = () =>
    channelsState.channels[props.guildId]?.find((c) => c.id === props.channelId);

  const isOwner = () => isGuildOwner(props.guildId, authState.user?.id || "");

  const canManageChannel = () =>
    isOwner() ||
    memberHasPermission(
      props.guildId,
      authState.user?.id || "",
      isOwner(),
      PermissionBits.MANAGE_CHANNELS
    );

  const handleBackdropClick = (e: MouseEvent) => {
    if (e.target === e.currentTarget) {
      props.onClose();
    }
  };

  return (
    <Portal>
      <div
        class="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50"
        onClick={handleBackdropClick}
      >
        <div
          class="border border-white/10 rounded-2xl w-[550px] max-h-[80vh] flex flex-col shadow-2xl"
          style="background-color: var(--color-surface-base)"
        >
          {/* Header */}
          <div class="flex items-center justify-between px-6 py-4 border-b border-white/10">
            <div class="flex items-center gap-3">
              <Hash class="w-5 h-5 text-text-secondary" />
              <div>
                <h2 class="text-lg font-bold text-text-primary">{channel()?.name}</h2>
                <p class="text-sm text-text-secondary">Channel Settings</p>
              </div>
            </div>
            <button
              onClick={props.onClose}
              class="p-1.5 text-text-secondary hover:text-text-primary hover:bg-white/10 rounded-lg transition-colors"
            >
              <X class="w-5 h-5" />
            </button>
          </div>

          {/* Tabs */}
          <div class="flex border-b border-white/10">
            <button
              onClick={() => setActiveTab("overview")}
              class="flex items-center gap-2 px-6 py-3 font-medium transition-colors"
              classList={{
                "text-accent-primary border-b-2 border-accent-primary": activeTab() === "overview",
                "text-text-secondary hover:text-text-primary": activeTab() !== "overview",
              }}
            >
              <Settings class="w-4 h-4" />
              Overview
            </button>
            <Show when={canManageChannel()}>
              <button
                onClick={() => setActiveTab("permissions")}
                class="flex items-center gap-2 px-6 py-3 font-medium transition-colors"
                classList={{
                  "text-accent-primary border-b-2 border-accent-primary": activeTab() === "permissions",
                  "text-text-secondary hover:text-text-primary": activeTab() !== "permissions",
                }}
              >
                <Shield class="w-4 h-4" />
                Permissions
              </button>
            </Show>
          </div>

          {/* Content */}
          <div class="flex-1 overflow-y-auto">
            <Show when={activeTab() === "overview"}>
              <div class="p-6">
                <div class="space-y-4">
                  <div>
                    <label class="block text-sm font-medium text-text-secondary mb-2">
                      Channel Name
                    </label>
                    <div class="px-3 py-2 rounded-lg border border-white/10 text-text-primary" style="background-color: var(--color-surface-layer1)">
                      {channel()?.name}
                    </div>
                  </div>
                  <div class="text-sm text-text-secondary">
                    More channel settings coming soon...
                  </div>
                </div>
              </div>
            </Show>
            <Show when={activeTab() === "permissions" && canManageChannel()}>
              <ChannelPermissions
                channelId={props.channelId}
                guildId={props.guildId}
              />
            </Show>
          </div>
        </div>
      </div>
    </Portal>
  );
};

export default ChannelSettingsModal;
```

**Step 2: Commit**

```bash
git add client/src/components/channels/ChannelSettingsModal.tsx
git commit -m "feat(ui): add ChannelSettingsModal component"
```

---

## Task 7: ChannelPermissions Component

**Files:**
- Create: `client/src/components/channels/ChannelPermissions.tsx`

**Step 1: Create the permissions component**

```tsx
/**
 * ChannelPermissions - Channel permission override editor
 */

import { Component, createSignal, For, Show, onMount, createEffect } from "solid-js";
import { Plus, Settings, Trash2 } from "lucide-solid";
import {
  loadChannelOverrides,
  getChannelOverrides,
  setChannelOverride,
  deleteChannelOverride,
  loadGuildRoles,
  getGuildRoles,
  permissionsState,
} from "@/stores/permissions";
import {
  PERMISSIONS,
  CATEGORY_NAMES,
  PermissionCategory,
  hasPermission,
} from "@/lib/permissionConstants";
import type { GuildRole, ChannelOverride } from "@/lib/types";

interface ChannelPermissionsProps {
  channelId: string;
  guildId: string;
}

type OverrideState = "inherit" | "allow" | "deny";

const ChannelPermissions: Component<ChannelPermissionsProps> = (props) => {
  const [editingRoleId, setEditingRoleId] = createSignal<string | null>(null);
  const [showRolePicker, setShowRolePicker] = createSignal(false);
  const [localOverrides, setLocalOverrides] = createSignal<Record<number, OverrideState>>({});
  const [isSaving, setIsSaving] = createSignal(false);

  onMount(() => {
    loadGuildRoles(props.guildId);
    loadChannelOverrides(props.channelId);
  });

  const roles = () => getGuildRoles(props.guildId);
  const overrides = () => getChannelOverrides(props.channelId);

  const rolesWithOverrides = () => {
    const overrideRoleIds = new Set(overrides().map((o) => o.role_id));
    return roles().filter((r) => overrideRoleIds.has(r.id));
  };

  const rolesWithoutOverrides = () => {
    const overrideRoleIds = new Set(overrides().map((o) => o.role_id));
    return roles().filter((r) => !overrideRoleIds.has(r.id));
  };

  const getOverride = (roleId: string): ChannelOverride | undefined => {
    return overrides().find((o) => o.role_id === roleId);
  };

  const getOverrideState = (override: ChannelOverride | undefined, bit: number): OverrideState => {
    if (!override) return "inherit";
    if (hasPermission(override.allow_permissions, bit)) return "allow";
    if (hasPermission(override.deny_permissions, bit)) return "deny";
    return "inherit";
  };

  const countOverrides = (override: ChannelOverride): { allowed: number; denied: number } => {
    let allowed = 0;
    let denied = 0;
    for (const perm of PERMISSIONS) {
      if (hasPermission(override.allow_permissions, perm.bit)) allowed++;
      if (hasPermission(override.deny_permissions, perm.bit)) denied++;
    }
    return { allowed, denied };
  };

  // Initialize local overrides when editing role changes
  createEffect(() => {
    const roleId = editingRoleId();
    if (!roleId) {
      setLocalOverrides({});
      return;
    }
    const override = getOverride(roleId);
    const states: Record<number, OverrideState> = {};
    for (const perm of PERMISSIONS) {
      states[perm.bit] = getOverrideState(override, perm.bit);
    }
    setLocalOverrides(states);
  });

  const handleStateChange = (bit: number, state: OverrideState) => {
    setLocalOverrides((prev) => ({ ...prev, [bit]: state }));
  };

  const handleSaveOverride = async () => {
    const roleId = editingRoleId();
    if (!roleId || isSaving()) return;

    setIsSaving(true);
    try {
      let allow = 0;
      let deny = 0;
      for (const [bitStr, state] of Object.entries(localOverrides())) {
        const bit = parseInt(bitStr);
        if (state === "allow") allow |= bit;
        if (state === "deny") deny |= bit;
      }
      await setChannelOverride(props.channelId, roleId, { allow, deny });
      setEditingRoleId(null);
    } catch (err) {
      console.error("Failed to save override:", err);
    } finally {
      setIsSaving(false);
    }
  };

  const handleAddRole = async (roleId: string) => {
    try {
      await setChannelOverride(props.channelId, roleId, { allow: 0, deny: 0 });
      setShowRolePicker(false);
      setEditingRoleId(roleId);
    } catch (err) {
      console.error("Failed to add override:", err);
    }
  };

  const handleDeleteOverride = async (roleId: string) => {
    try {
      await deleteChannelOverride(props.channelId, roleId);
      if (editingRoleId() === roleId) {
        setEditingRoleId(null);
      }
    } catch (err) {
      console.error("Failed to delete override:", err);
    }
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
      categories[perm.category].push(perm);
    }
    return categories;
  };

  return (
    <div class="p-6">
      <Show
        when={editingRoleId()}
        fallback={
          <>
            {/* Header */}
            <div class="flex items-center justify-between mb-4">
              <h3 class="text-sm font-semibold text-text-secondary uppercase">Role Overrides</h3>
              <div class="relative">
                <button
                  onClick={() => setShowRolePicker(!showRolePicker())}
                  class="flex items-center gap-2 px-3 py-1.5 text-sm text-accent-primary hover:bg-accent-primary/10 rounded-lg transition-colors"
                >
                  <Plus class="w-4 h-4" />
                  Add Role
                </button>
                <Show when={showRolePicker()}>
                  <div
                    class="absolute right-0 top-full mt-1 py-1 rounded-lg border border-white/10 shadow-xl z-10 w-48"
                    style="background-color: var(--color-surface-layer2)"
                  >
                    <Show
                      when={rolesWithoutOverrides().length > 0}
                      fallback={
                        <div class="px-3 py-2 text-sm text-text-secondary">
                          All roles have overrides
                        </div>
                      }
                    >
                      <For each={rolesWithoutOverrides()}>
                        {(role) => (
                          <button
                            onClick={() => handleAddRole(role.id)}
                            class="w-full flex items-center gap-2 px-3 py-2 text-sm text-text-primary hover:bg-white/10 transition-colors"
                          >
                            <div
                              class="w-2.5 h-2.5 rounded-full"
                              style={{
                                "background-color": role.color || "transparent",
                                border: role.color ? "none" : "1px solid var(--color-text-secondary)",
                              }}
                            />
                            {role.is_default ? "@everyone" : role.name}
                          </button>
                        )}
                      </For>
                    </Show>
                  </div>
                </Show>
              </div>
            </div>

            {/* Override list */}
            <Show when={permissionsState.isOverridesLoading}>
              <div class="text-center py-8 text-text-secondary">Loading...</div>
            </Show>
            <Show when={!permissionsState.isOverridesLoading}>
              <div class="space-y-2">
                <For each={rolesWithOverrides()}>
                  {(role) => {
                    const override = getOverride(role.id);
                    const counts = override ? countOverrides(override) : { allowed: 0, denied: 0 };

                    return (
                      <div
                        class="flex items-center gap-3 p-3 rounded-lg border border-white/10 hover:bg-white/5 transition-colors group"
                        style="background-color: var(--color-surface-layer1)"
                      >
                        <div
                          class="w-3 h-3 rounded-full flex-shrink-0"
                          style={{
                            "background-color": role.color || "transparent",
                            border: role.color ? "none" : "2px solid var(--color-text-secondary)",
                          }}
                        />
                        <div class="flex-1 min-w-0">
                          <div class="font-medium text-text-primary">
                            {role.is_default ? "@everyone" : role.name}
                          </div>
                          <div class="text-xs text-text-secondary">
                            <Show when={counts.allowed > 0}>
                              <span class="text-green-400">+{counts.allowed} allowed</span>
                            </Show>
                            <Show when={counts.allowed > 0 && counts.denied > 0}> &bull; </Show>
                            <Show when={counts.denied > 0}>
                              <span class="text-red-400">-{counts.denied} denied</span>
                            </Show>
                            <Show when={counts.allowed === 0 && counts.denied === 0}>
                              No overrides set
                            </Show>
                          </div>
                        </div>
                        <div class="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                          <button
                            onClick={() => setEditingRoleId(role.id)}
                            class="p-2 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors"
                          >
                            <Settings class="w-4 h-4" />
                          </button>
                          <button
                            onClick={() => handleDeleteOverride(role.id)}
                            class="p-2 rounded-lg text-text-secondary hover:text-accent-danger hover:bg-white/10 transition-colors"
                          >
                            <Trash2 class="w-4 h-4" />
                          </button>
                        </div>
                      </div>
                    );
                  }}
                </For>
                <Show when={rolesWithOverrides().length === 0}>
                  <div class="text-center py-8 text-text-secondary">
                    No permission overrides. All roles use their base permissions.
                  </div>
                </Show>
              </div>
            </Show>
          </>
        }
      >
        {/* Override editor */}
        {(() => {
          const roleId = editingRoleId()!;
          const role = roles().find((r) => r.id === roleId);
          if (!role) return null;

          return (
            <div>
              <div class="flex items-center gap-3 mb-4">
                <button
                  onClick={() => setEditingRoleId(null)}
                  class="text-text-secondary hover:text-text-primary transition-colors"
                >
                  &larr; Back
                </button>
                <h3 class="text-lg font-semibold text-text-primary">
                  {role.is_default ? "@everyone" : role.name} in #{/* channel name */}
                </h3>
              </div>

              <div class="space-y-4 mb-6">
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
                              const state = () => localOverrides()[perm.bit] || "inherit";

                              return (
                                <div class="flex items-center gap-4 p-2 rounded-lg hover:bg-white/5">
                                  <div class="flex-1">
                                    <div class="text-sm text-text-primary">{perm.name}</div>
                                  </div>
                                  <div class="flex items-center gap-2">
                                    <label class="flex items-center gap-1 cursor-pointer">
                                      <input
                                        type="radio"
                                        name={`perm-${perm.bit}`}
                                        checked={state() === "inherit"}
                                        onChange={() => handleStateChange(perm.bit, "inherit")}
                                        class="w-4 h-4"
                                      />
                                      <span class="text-xs text-text-secondary">Inherit</span>
                                    </label>
                                    <label class="flex items-center gap-1 cursor-pointer">
                                      <input
                                        type="radio"
                                        name={`perm-${perm.bit}`}
                                        checked={state() === "allow"}
                                        onChange={() => handleStateChange(perm.bit, "allow")}
                                        class="w-4 h-4 accent-green-500"
                                      />
                                      <span class="text-xs text-green-400">Allow</span>
                                    </label>
                                    <label class="flex items-center gap-1 cursor-pointer">
                                      <input
                                        type="radio"
                                        name={`perm-${perm.bit}`}
                                        checked={state() === "deny"}
                                        onChange={() => handleStateChange(perm.bit, "deny")}
                                        class="w-4 h-4 accent-red-500"
                                      />
                                      <span class="text-xs text-red-400">Deny</span>
                                    </label>
                                  </div>
                                </div>
                              );
                            }}
                          </For>
                        </div>
                      </div>
                    </Show>
                  )}
                </For>
              </div>

              <div class="flex justify-end">
                <button
                  onClick={handleSaveOverride}
                  disabled={isSaving()}
                  class="px-4 py-2 rounded-lg bg-accent-primary text-white font-medium hover:bg-accent-primary/90 transition-colors disabled:opacity-50"
                >
                  {isSaving() ? "Saving..." : "Save"}
                </button>
              </div>
            </div>
          );
        })()}
      </Show>
    </div>
  );
};

export default ChannelPermissions;
```

**Step 2: Commit**

```bash
git add client/src/components/channels/ChannelPermissions.tsx
git commit -m "feat(ui): add ChannelPermissions component for override editing"
```

---

## Task 8-12: Admin Dashboard Components

Due to the size of this plan, the Admin Dashboard components (AdminQuickModal, AdminDashboard, AdminSidebar, UsersPanel, GuildsPanel, AuditLogPanel) will be implemented in a follow-up plan. The guild permission UI is the priority.

---

## Task 8: Final Integration - Add channel context menu

**Files:**
- Modify: Appropriate channel component (find the channel list item)

Find the channel item component and add context menu option:

```tsx
// Add context menu option for "Edit Channel"
// Opens ChannelSettingsModal when clicked
```

**Step 1: Commit all remaining changes**

```bash
git add -A
git commit -m "feat(ui): permission system UI - Batch 3 complete

- RolesTab: List, create, delete roles
- RoleEditor: Edit permissions, manage members
- MembersTab: Role badges, assignment dropdown
- ChannelSettingsModal: Channel settings with permissions tab
- ChannelPermissions: Override editor with inherit/allow/deny states

Admin dashboard deferred to separate batch."
```

---

## Summary

**Components created:**
1. `RolesTab.tsx` - Role list with CRUD
2. `RoleEditor.tsx` - Permission editor with member management
3. `MemberRoleDropdown.tsx` - Role assignment dropdown
4. `ChannelSettingsModal.tsx` - Channel settings modal
5. `ChannelPermissions.tsx` - Channel override editor

**Components modified:**
1. `GuildSettingsModal.tsx` - Added Roles tab
2. `MembersTab.tsx` - Added role badges and manage dropdown

**Admin dashboard (deferred):**
- AdminQuickModal
- AdminDashboard + panels

---

Plan complete and saved to `docs/plans/2026-01-19-permission-ui-implementation.md`. Two execution options:

**1. Subagent-Driven (this session)** - I dispatch fresh subagent per task, review between tasks, fast iteration

**2. Parallel Session (separate)** - Open new session with executing-plans, batch execution with checkpoints

Which approach?
