/**
 * ChannelPermissions - Channel permission override editor
 */

import {
  Component,
  createSignal,
  For,
  Show,
  onMount,
  createEffect,
} from "solid-js";
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
  CHANNEL_OVERRIDE_PERMISSIONS,
  CATEGORY_NAMES,
  PermissionCategory,
  hasPermission,
} from "@/lib/permissionConstants";
import type { ChannelOverride } from "@/lib/types";

interface ChannelPermissionsProps {
  channelId: string;
  guildId: string;
}

type OverrideState = "inherit" | "allow" | "deny";

const ChannelPermissions: Component<ChannelPermissionsProps> = (props) => {
  const [editingRoleId, setEditingRoleId] = createSignal<string | null>(null);
  const [showRolePicker, setShowRolePicker] = createSignal(false);
  const [localOverrides, setLocalOverrides] = createSignal<
    Record<number, OverrideState>
  >({});
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

  const getOverrideState = (
    override: ChannelOverride | undefined,
    bit: number,
  ): OverrideState => {
    if (!override) return "inherit";
    if (hasPermission(override.allow_permissions, bit)) return "allow";
    if (hasPermission(override.deny_permissions, bit)) return "deny";
    return "inherit";
  };

  const countOverrides = (
    override: ChannelOverride,
  ): { allowed: number; denied: number } => {
    let allowed = 0;
    let denied = 0;
    for (const perm of CHANNEL_OVERRIDE_PERMISSIONS) {
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
    for (const perm of CHANNEL_OVERRIDE_PERMISSIONS) {
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
    const categories: Record<
      PermissionCategory,
      typeof CHANNEL_OVERRIDE_PERMISSIONS
    > = {
      content: [],
      voice: [],
      moderation: [],
      guild_management: [],
      invites: [],
      pages: [],
    };
    for (const perm of CHANNEL_OVERRIDE_PERMISSIONS) {
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
              <h3 class="text-sm font-semibold text-text-secondary uppercase">
                Role Overrides
              </h3>
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
                                border: role.color
                                  ? "none"
                                  : "1px solid var(--color-text-secondary)",
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
                    const counts = override
                      ? countOverrides(override)
                      : { allowed: 0, denied: 0 };

                    return (
                      <div
                        class="flex items-center gap-3 p-3 rounded-lg border border-white/10 hover:bg-white/5 transition-colors group"
                        style="background-color: var(--color-surface-layer1)"
                      >
                        <div
                          class="w-3 h-3 rounded-full flex-shrink-0"
                          style={{
                            "background-color": role.color || "transparent",
                            border: role.color
                              ? "none"
                              : "2px solid var(--color-text-secondary)",
                          }}
                        />
                        <div class="flex-1 min-w-0">
                          <div class="font-medium text-text-primary">
                            {role.is_default ? "@everyone" : role.name}
                          </div>
                          <div class="text-xs text-text-secondary">
                            <Show when={counts.allowed > 0}>
                              <span class="text-green-400">
                                +{counts.allowed} allowed
                              </span>
                            </Show>
                            <Show
                              when={counts.allowed > 0 && counts.denied > 0}
                            >
                              {" "}
                              •{" "}
                            </Show>
                            <Show when={counts.denied > 0}>
                              <span class="text-red-400">
                                -{counts.denied} denied
                              </span>
                            </Show>
                            <Show
                              when={counts.allowed === 0 && counts.denied === 0}
                            >
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
                    No permission overrides. All roles use their base
                    permissions.
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
                  ← Back
                </button>
                <h3 class="text-lg font-semibold text-text-primary">
                  {role.is_default ? "@everyone" : role.name} permissions
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
                              const state = () =>
                                localOverrides()[perm.bit] || "inherit";

                              return (
                                <div class="flex items-center gap-4 p-2 rounded-lg hover:bg-white/5">
                                  <div class="flex-1">
                                    <div class="text-sm text-text-primary">
                                      {perm.name}
                                    </div>
                                  </div>
                                  <div class="flex items-center gap-2">
                                    <label class="flex items-center gap-1 cursor-pointer">
                                      <input
                                        type="radio"
                                        name={`perm-${perm.bit}`}
                                        checked={state() === "inherit"}
                                        onChange={() =>
                                          handleStateChange(perm.bit, "inherit")
                                        }
                                        class="w-4 h-4"
                                      />
                                      <span class="text-xs text-text-secondary">
                                        Inherit
                                      </span>
                                    </label>
                                    <label class="flex items-center gap-1 cursor-pointer">
                                      <input
                                        type="radio"
                                        name={`perm-${perm.bit}`}
                                        checked={state() === "allow"}
                                        onChange={() =>
                                          handleStateChange(perm.bit, "allow")
                                        }
                                        class="w-4 h-4 accent-green-500"
                                      />
                                      <span class="text-xs text-green-400">
                                        Allow
                                      </span>
                                    </label>
                                    <label class="flex items-center gap-1 cursor-pointer">
                                      <input
                                        type="radio"
                                        name={`perm-${perm.bit}`}
                                        checked={state() === "deny"}
                                        onChange={() =>
                                          handleStateChange(perm.bit, "deny")
                                        }
                                        class="w-4 h-4 accent-red-500"
                                      />
                                      <span class="text-xs text-red-400">
                                        Deny
                                      </span>
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
