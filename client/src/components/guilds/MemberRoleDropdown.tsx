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
  memberHasPermission,
} from "@/stores/permissions";
import { authState } from "@/stores/auth";
import { isGuildOwner, kickMember } from "@/stores/guilds";
import { PermissionBits } from "@/lib/permissionConstants";

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
    return memberHasPermission(props.guildId, currentUserId(), isOwner(), PermissionBits.KICK_MEMBERS);
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
