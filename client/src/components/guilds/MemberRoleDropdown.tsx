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
  canModerateMember,
} from "@/stores/permissions";
import { authState } from "@/stores/auth";
import { isGuildOwner, kickMember, getGuildMembers } from "@/stores/guilds";
import { PermissionBits } from "@/lib/permissionConstants";
import { t } from "@/i18n";
import ConfirmActionDialog from "@/components/admin/ConfirmActionDialog";
import { reportFailure, reportSuccess } from "./settingsAction";

interface MemberRoleDropdownProps {
  guildId: string;
  userId: string;
  onClose?: () => void;
}

const MemberRoleDropdown: Component<MemberRoleDropdownProps> = (props) => {
  const [isOpen, setIsOpen] = createSignal(false);
  const [kickConfirm, setKickConfirm] = createSignal(false);

  const closeDropdown = () => {
    setIsOpen(false);
    props.onClose?.();
  };

  let kickedName = "";
  const kick = async () => {
    kickedName = memberName();
    await kickMember(props.guildId, props.userId);
  };
  const memberName = () => {
    const m = getGuildMembers(props.guildId).find((x) => x.user_id === props.userId);
    return m?.display_name || m?.username || "";
  };

  const isOwner = () => isGuildOwner(props.guildId, authState.user?.id || "");
  const isMemberOwner = () => isGuildOwner(props.guildId, props.userId);
  const currentUserId = () => authState.user?.id || "";

  const roles = () => getGuildRoles(props.guildId).filter((r) => !r.is_default);
  const memberRoleIds = () => getMemberRoleIds(props.guildId, props.userId);

  const canManageThisRole = (rolePosition: number): boolean => {
    return canManageRole(
      props.guildId,
      currentUserId(),
      isOwner(),
      rolePosition,
    );
  };

  const canManageRolesPermission = () => {
    if (isOwner()) return true;
    return memberHasPermission(
      props.guildId,
      currentUserId(),
      isOwner(),
      PermissionBits.MANAGE_ROLES,
    );
  };

  const handleToggleRole = async (roleId: string, roleName: string, hasRole: boolean) => {
    try {
      if (hasRole) {
        await removeMemberRole(props.guildId, props.userId, roleId);
        reportSuccess(t("guildSettings.roles.memberRemoved", { role: roleName, name: memberName() }));
      } else {
        await assignMemberRole(props.guildId, props.userId, roleId);
        reportSuccess(t("guildSettings.roles.memberAdded", { role: roleName, name: memberName() }));
      }
    } catch (err) {
      reportFailure(
        hasRole ? t("guildSettings.roles.removeFailed") : t("guildSettings.roles.assignFailed"),
        err,
      );
    }
  };

  // Can only kick if owner or has kick permission and target is below us
  const canKick = () => {
    if (isMemberOwner()) return false;
    if (props.userId === currentUserId()) return false;
    if (isOwner()) return true;

    return canModerateMember(
      props.guildId,
      currentUserId(),
      props.userId,
      isOwner(),
      PermissionBits.KICK_MEMBERS,
    );
  };

  return (
    <div class="relative">
      <button
        onClick={() => setIsOpen(!isOpen())}
        data-testid="member-role-dropdown-trigger"
        aria-expanded={isOpen()}
        aria-label={t("guildSettings.members.manageNamed", { name: memberName() })}
        class="flex items-center gap-1 px-2 py-1 text-sm text-text-secondary hover:text-text-primary hover:bg-white/10 rounded-lg transition-colors"
      >
        {t("guildSettings.members.manage")}
        <ChevronDown class="w-4 h-4" />
      </button>

      <Show when={isOpen()}>
        <div
          data-testid="member-role-dropdown"
          class="absolute right-0 top-full mt-1 py-1 rounded-lg border border-white/10 shadow-xl z-20 min-w-[200px]"
          style={{"background-color":"var(--color-surface-layer2)"}}
        >
          <Show when={canManageRolesPermission()}>
            <>
              {/* Role assignment section */}
              <div class="px-3 py-1.5 text-xs font-semibold text-text-secondary uppercase">
                {t("guildSettings.members.roles")}
              </div>
              <Show
                when={roles().length > 0}
                fallback={
                  <div class="px-3 py-2 text-sm text-text-secondary">
                    {t("guildSettings.members.noRoles")}
                  </div>
                }
              >
                <For each={roles()}>
                  {(role) => {
                    const hasRole = memberRoleIds().includes(role.id);
                    const canManage = canManageThisRole(role.position);

                    return (
                      <label
                        data-role-id={role.id}
                        data-role-name={role.name}
                        class="flex items-center gap-2 px-3 py-2 cursor-pointer hover:bg-white/10 transition-colors"
                        classList={{
                          "state-unavailable": !canManage,
                        }}
                      >
                        <input
                          type="checkbox"
                          checked={hasRole}
                          data-testid="member-role-checkbox"
                          disabled={!canManage}
                          onChange={() =>
                            canManage && handleToggleRole(role.id, role.name, hasRole)
                          }
                          class="w-4 h-4 rounded border-white/20 text-accent-primary"
                        />
                        <div
                          class="w-2.5 h-2.5 rounded-full flex-shrink-0"
                          style={{
                            "background-color": role.color || "transparent",
                            border: role.color
                              ? "none"
                              : "1px solid var(--color-text-secondary)",
                          }}
                        />
                        <span class="text-sm text-text-primary">
                          {role.name}
                        </span>
                      </label>
                    );
                  }}
                </For>
              </Show>
            </>
          </Show>

          {/* Kick section */}
          <Show when={canKick()}>
            <div
              classList={{
                "border-t border-white/10 mt-1 pt-1":
                  canManageRolesPermission(),
              }}
            >
              <button
                onClick={() => setKickConfirm(true)}
                class="w-full flex items-center gap-2 px-3 py-2 text-sm transition-colors text-text-primary hover:bg-white/10"
              >
                <UserX class="w-4 h-4" />
                {t("guildSettings.members.kick")}
              </button>
            </div>
          </Show>
        </div>
      </Show>

      {/* Click outside to close */}
      <Show when={isOpen()}>
        <div
          data-testid="member-role-dropdown-backdrop"
          class="fixed inset-0 z-10"
          onClick={closeDropdown}
        />
      </Show>

      <Show when={kickConfirm()}>
        <ConfirmActionDialog
          title={t("guildSettings.members.kickTitle", { name: memberName() })}
          message={t("guildSettings.members.kickBody", { name: memberName() })}
          confirmLabel={t("guildSettings.members.kick")}
          danger
          onConfirm={kick}
          onDone={() => {
            setKickConfirm(false);
            reportSuccess(t("guildSettings.members.kicked", { name: kickedName }));
            closeDropdown();
          }}
          onCancel={() => setKickConfirm(false)}
        />
      </Show>
    </div>
  );
};

export default MemberRoleDropdown;
