/**
 * GuildSettingsModal - Guild management modal with tabs
 *
 * Provides invite management (owner only), member list, and role management.
 */

import { Component, createSignal, Show } from "solid-js";
import { Portal } from "solid-js/web";
import {
  X,
  Link,
  Users,
  Shield,
  ShieldAlert,
  Smile,
  Bot,
  Settings,
  BarChart3,
  LayoutList,
} from "lucide-solid";
import { guildsState, isGuildOwner } from "@/stores/guilds";
import { authState } from "@/stores/auth";
import GeneralTab from "./GeneralTab";
import InvitesTab from "./InvitesTab";
import MembersTab from "./MembersTab";
import RolesTab from "./RolesTab";
import EmojisTab from "./EmojisTab";
import BotsTab from "./BotsTab";
import SafetyTab from "./SafetyTab";
import UsageTab from "./UsageTab";
import CategoriesTab from "./CategoriesTab";
import RoleEditor from "./RoleEditor";
import { memberHasPermission } from "@/stores/permissions";
import { PermissionBits } from "@/lib/permissionConstants";
import type { GuildRole } from "@/lib/types";

interface GuildSettingsModalProps {
  guildId: string;
  onClose: () => void;
}

type TabId =
  | "general"
  | "invites"
  | "members"
  | "categories"
  | "roles"
  | "emojis"
  | "bots"
  | "safety"
  | "usage";

const GuildSettingsModal: Component<GuildSettingsModalProps> = (props) => {
  const guild = () => guildsState.guilds.find((g) => g.id === props.guildId);
  const isOwner = () => isGuildOwner(props.guildId, authState.user?.id || "");

  // Default tab: general for managers, invites for owner-only, members for everyone else
  const defaultTab = (): TabId => {
    if (isOwner()) return "general";
    if (
      memberHasPermission(
        props.guildId,
        authState.user?.id || "",
        false,
        PermissionBits.MANAGE_GUILD,
      )
    )
      return "general";
    return "members";
  };
  const [activeTab, setActiveTab] = createSignal<TabId>(defaultTab());
  const [editingRole, setEditingRole] = createSignal<GuildRole | null>(null);
  const [isCreatingRole, setIsCreatingRole] = createSignal(false);

  const canManageRoles = () =>
    isOwner() ||
    memberHasPermission(
      props.guildId,
      authState.user?.id || "",
      isOwner(),
      PermissionBits.MANAGE_ROLES,
    );

  const canManageEmojis = () =>
    isOwner() ||
    memberHasPermission(
      props.guildId,
      authState.user?.id || "",
      isOwner(),
      PermissionBits.MANAGE_EMOJIS_AND_STICKERS,
    );

  const canManageGuild = () =>
    isOwner() ||
    memberHasPermission(
      props.guildId,
      authState.user?.id || "",
      isOwner(),
      PermissionBits.MANAGE_GUILD,
    );

  const canManageBots = () => canManageGuild();

  const canManageChannels = () =>
    isOwner() ||
    memberHasPermission(
      props.guildId,
      authState.user?.id || "",
      isOwner(),
      PermissionBits.MANAGE_CHANNELS,
    );

  const navItem = (tab: TabId, Icon: Component<{ class?: string }>, label: string, testId?: string) => (
    <button
      onClick={() => setActiveTab(tab)}
      data-testid={testId}
      class="flex items-center gap-2.5 w-full px-3 py-2 rounded-lg text-sm font-medium transition-colors"
      classList={{
        "bg-accent-primary/20 text-text-primary font-semibold": activeTab() === tab,
        "text-text-secondary hover:text-text-primary hover:bg-white/5": activeTab() !== tab,
      }}
    >
      <Icon class="w-4 h-4 shrink-0" />
      {label}
    </button>
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
          class="border border-white/10 rounded-2xl w-[90vw] md:w-[900px] max-w-5xl max-h-[85vh] flex flex-col shadow-2xl"
          style="background-color: var(--color-surface-base)"
        >
          {/* Header */}
          <div class="flex items-center justify-between px-6 py-4 border-b border-white/10">
            <div class="flex items-center gap-3">
              <Show
                when={guild()?.icon_url}
                fallback={
                  <div class="w-10 h-10 rounded-xl bg-accent-primary/20 flex items-center justify-center">
                    <span class="text-lg font-bold text-text-primary">
                      {guild()?.name.charAt(0).toUpperCase()}
                    </span>
                  </div>
                }
              >
                <img
                  src={guild()!.icon_url!}
                  alt={guild()?.name}
                  class="w-10 h-10 rounded-xl object-cover"
                />
              </Show>
              <div>
                <h2 class="text-lg font-bold text-text-primary">
                  {guild()?.name}
                </h2>
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

          {/* Sidebar + Content */}
          <div class="flex flex-1 min-h-0">
            {/* Sidebar Navigation */}
            <nav class="w-48 shrink-0 border-r border-white/10 py-3 px-2 overflow-y-auto">
              <Show when={canManageGuild()}>
                {navItem("general", Settings, "General")}
              </Show>
              <Show when={isOwner()}>
                {navItem("invites", Link, "Invites")}
              </Show>
              {navItem("members", Users, "Members", "guild-settings-tab-members")}
              <Show when={canManageChannels()}>
                {navItem("categories", LayoutList, "Categories")}
              </Show>
              {navItem("usage", BarChart3, "Usage")}
              <Show when={canManageEmojis()}>
                {navItem("emojis", Smile, "Emojis")}
              </Show>
              <Show when={canManageBots()}>
                {navItem("bots", Bot, "Bots")}
              </Show>
              <Show when={canManageGuild()}>
                {navItem("safety", ShieldAlert, "Safety")}
              </Show>
              <Show when={canManageRoles()}>
                {navItem("roles", Shield, "Roles", "guild-settings-tab-roles")}
              </Show>
            </nav>

            {/* Content */}
            <div class="flex-1 overflow-y-auto min-h-0">
            <Show when={activeTab() === "general" && canManageGuild()}>
              <GeneralTab guildId={props.guildId} />
            </Show>
            <Show when={activeTab() === "invites" && isOwner()}>
              <InvitesTab guildId={props.guildId} />
            </Show>
            <Show when={activeTab() === "members"}>
              <MembersTab guildId={props.guildId} isOwner={isOwner()} />
            </Show>
            <Show when={activeTab() === "categories" && canManageChannels()}>
              <CategoriesTab guildId={props.guildId} />
            </Show>
            <Show when={activeTab() === "usage"}>
              <UsageTab guildId={props.guildId} />
            </Show>
            <Show when={activeTab() === "emojis" && canManageEmojis()}>
              <EmojisTab guildId={props.guildId} />
            </Show>
            <Show when={activeTab() === "bots" && canManageBots()}>
              <BotsTab guildId={props.guildId} />
            </Show>
            <Show when={activeTab() === "safety" && canManageGuild()}>
              <SafetyTab guildId={props.guildId} />
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
      </div>
    </Portal>
  );
};

export default GuildSettingsModal;
