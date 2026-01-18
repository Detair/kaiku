/**
 * GuildSettingsModal - Guild management modal with tabs
 *
 * Provides invite management (owner only), member list, and role management.
 */

import { Component, createSignal, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { X, Link, Users, Shield } from "lucide-solid";
import { guildsState, isGuildOwner } from "@/stores/guilds";
import { authState } from "@/stores/auth";
import InvitesTab from "./InvitesTab";
import MembersTab from "./MembersTab";
import RolesTab from "./RolesTab";
import RoleEditor from "./RoleEditor";
import { memberHasPermission } from "@/stores/permissions";
import { PermissionBits } from "@/lib/permissionConstants";
import type { GuildRole } from "@/lib/types";

interface GuildSettingsModalProps {
  guildId: string;
  onClose: () => void;
}

type TabId = "invites" | "members" | "roles";

const GuildSettingsModal: Component<GuildSettingsModalProps> = (props) => {
  const guild = () => guildsState.guilds.find((g) => g.id === props.guildId);
  const isOwner = () => isGuildOwner(props.guildId, authState.user?.id || "");

  // Default to members tab for non-owners
  const [activeTab, setActiveTab] = createSignal<TabId>(isOwner() ? "invites" : "members");
  const [editingRole, setEditingRole] = createSignal<GuildRole | null>(null);
  const [isCreatingRole, setIsCreatingRole] = createSignal(false);

  const canManageRoles = () =>
    isOwner() ||
    memberHasPermission(
      props.guildId,
      authState.user?.id || "",
      isOwner(),
      PermissionBits.MANAGE_ROLES
    );

  const handleBackdropClick = (e: MouseEvent) => {
    if (e.target === e.currentTarget) {
      props.onClose();
    }
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      props.onClose();
    }
  };

  return (
    <Portal>
      <div
        class="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50"
        onClick={handleBackdropClick}
        onKeyDown={handleKeyDown}
        tabIndex={-1}
      >
        <div
          class="border border-white/10 rounded-2xl w-[600px] max-h-[80vh] flex flex-col shadow-2xl"
          style="background-color: var(--color-surface-base)"
        >
          {/* Header */}
          <div class="flex items-center justify-between px-6 py-4 border-b border-white/10">
            <div class="flex items-center gap-3">
              <div class="w-10 h-10 rounded-xl bg-accent-primary/20 flex items-center justify-center">
                <span class="text-lg font-bold text-accent-primary">
                  {guild()?.name.charAt(0).toUpperCase()}
                </span>
              </div>
              <div>
                <h2 class="text-lg font-bold text-text-primary">{guild()?.name}</h2>
                <p class="text-sm text-text-secondary">Server Settings</p>
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
            <Show when={isOwner()}>
              <button
                onClick={() => setActiveTab("invites")}
                class="flex items-center gap-2 px-6 py-3 font-medium transition-colors"
                classList={{
                  "text-accent-primary border-b-2 border-accent-primary": activeTab() === "invites",
                  "text-text-secondary hover:text-text-primary": activeTab() !== "invites",
                }}
              >
                <Link class="w-4 h-4" />
                Invites
              </button>
            </Show>
            <button
              onClick={() => setActiveTab("members")}
              class="flex items-center gap-2 px-6 py-3 font-medium transition-colors"
              classList={{
                "text-accent-primary border-b-2 border-accent-primary": activeTab() === "members",
                "text-text-secondary hover:text-text-primary": activeTab() !== "members",
              }}
            >
              <Users class="w-4 h-4" />
              Members
            </button>
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
          </div>

          {/* Content */}
          <div class="flex-1 overflow-y-auto">
            <Show when={activeTab() === "invites" && isOwner()}>
              <InvitesTab guildId={props.guildId} />
            </Show>
            <Show when={activeTab() === "members"}>
              <MembersTab guildId={props.guildId} isOwner={isOwner()} />
            </Show>
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
          </div>
        </div>
      </div>
    </Portal>
  );
};

export default GuildSettingsModal;
