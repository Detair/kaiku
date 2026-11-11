/**
 * GuildSettingsModal - Guild management modal with tabs
 *
 * Tabs are split into everyday ones and "Advanced" (ADMIN-22). Closing
 * with unsaved role or filter changes asks first (ADMIN-17). At the bottom
 * the owner can delete the server, everyone else can leave it (ADMIN-5).
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
  SmilePlus,
  Bot,
  Settings,
  BarChart3,
  LayoutList,
  Webhook,
  LogOut,
  Trash2,
} from "lucide-solid";
import { t } from "@/i18n";
import ConfirmActionDialog from "@/components/admin/ConfirmActionDialog";
import { deleteServer, leaveServer } from "@/stores/guildAdmin";
import { reportSuccess } from "./settingsAction";
import { guildsState, isGuildOwner } from "@/stores/guilds";
import { authState } from "@/stores/auth";
import GeneralTab from "./GeneralTab";
import InvitesTab from "./InvitesTab";
import MembersTab from "./MembersTab";
import RolesTab from "./RolesTab";
import ReactionRolesTab from "./ReactionRolesTab";
import EmojisTab from "./EmojisTab";
import BotsTab from "./BotsTab";
import WebhooksTab from "./WebhooksTab";
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
  | "reaction-roles"
  | "emojis"
  | "bots"
  | "webhooks"
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
  // Unsaved edits in the role editor or the filter settings (ADMIN-17).
  const [roleDirty, setRoleDirty] = createSignal(false);
  const [safetyDirty, setSafetyDirty] = createSignal(false);
  const [nameDirty, setNameDirty] = createSignal(false);
  // What to do after "Discard changes": close the window or open another tab.
  const [discardThen, setDiscardThen] = createSignal<(() => void) | null>(null);
  const confirmClose = () => discardThen() !== null;
  const [confirmExit, setConfirmExit] = createSignal(false);
  const [deleteName, setDeleteName] = createSignal("");
  const dirty = () => roleDirty() || safetyDirty() || nameDirty();

  const requestClose = () => {
    if (dirty()) setDiscardThen(() => props.onClose);
    else props.onClose();
  };

  const openTab = (tab: TabId) => {
    setRoleDirty(false);
    setSafetyDirty(false);
    setNameDirty(false);
    setEditingRole(null);
    setIsCreatingRole(false);
    setActiveTab(tab);
  };

  // Another tab drops the open edits: ask first (W3), switch only after "Discard".
  const switchTab = (tab: TabId) => {
    if (tab === activeTab()) return;
    if (dirty()) setDiscardThen(() => () => openTab(tab));
    else openTab(tab);
  };


  const exitServer = async () => {
    const name = guild()?.name ?? "";
    if (isOwner()) await deleteServer(props.guildId);
    else await leaveServer(props.guildId);
    reportSuccess(
      isOwner() ? t("guildSettings.server.deleted", { name }) : t("guildSettings.server.left", { name }),
    );
  };

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

  const canManageWebhooks = () =>
    isOwner() ||
    memberHasPermission(
      props.guildId,
      authState.user?.id || "",
      isOwner(),
      PermissionBits.MANAGE_WEBHOOKS,
    );

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
      onClick={() => switchTab(tab)}
      aria-current={activeTab() === tab ? "page" : undefined}
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
      requestClose();
    }
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape" && !confirmClose() && !confirmExit()) {
      requestClose();
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
          class="border border-white/10 rounded-2xl w-[90vw] max-w-[900px] overflow-x-hidden max-h-[85vh] flex flex-col shadow-2xl"
          style={{"background-color":"var(--color-surface-base)"}}
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
                <p class="text-sm text-text-secondary">{t("guildSettings.title")}</p>
              </div>
            </div>
            <button
              onClick={requestClose}
              aria-label={t("common.close")}
              class="p-2.5 text-text-secondary hover:text-text-primary hover:bg-white/10 rounded-lg transition-colors"
            >
              <X class="w-5 h-5" />
            </button>
          </div>

          {/* Sidebar + Content */}
          <div class="flex flex-1 min-h-0">
            {/* Sidebar Navigation */}
            <nav
              aria-label={t("guildSettings.title")}
              class="min-w-48 max-w-56 shrink-0 border-r border-white/10 py-3 px-2 overflow-y-auto flex flex-col"
            >
              {/* Everyday tasks first (ADMIN-22). */}
              <Show when={canManageGuild()}>
                {navItem("general", Settings, t("guildSettings.tab.general"))}
              </Show>
              <Show when={isOwner()}>
                {navItem("invites", Link, t("guildSettings.tab.invites"))}
              </Show>
              {navItem("members", Users, t("guildSettings.tab.members"), "guild-settings-tab-members")}
              <Show when={canManageRoles()}>
                {navItem("roles", Shield, t("guildSettings.tab.roles"), "guild-settings-tab-roles")}
              </Show>
              <Show when={canManageChannels()}>
                {navItem("categories", LayoutList, t("guildSettings.tab.categories"))}
              </Show>
              <Show when={canManageEmojis()}>
                {navItem("emojis", Smile, t("guildSettings.tab.emojis"))}
              </Show>

              {/* Advanced: only for those who look for it. */}
              <div
                class="mt-4 mb-1 px-3 text-xs font-semibold uppercase tracking-wide text-text-secondary"
                data-testid="guild-settings-advanced"
              >
                {t("guildSettings.tab.advanced")}
              </div>
              {navItem("usage", BarChart3, t("guildSettings.tab.usage"))}
              <Show when={canManageGuild()}>
                {navItem("safety", ShieldAlert, t("guildSettings.tab.safety"))}
              </Show>
              <Show when={canManageRoles()}>
                {navItem(
                  "reaction-roles",
                  SmilePlus,
                  t("guildSettings.tab.reactionRoles"),
                  "guild-settings-tab-reaction-roles",
                )}
              </Show>
              <Show when={canManageBots()}>
                {navItem("bots", Bot, t("guildSettings.tab.bots"))}
              </Show>
              <Show when={canManageWebhooks()}>
                {navItem(
                  "webhooks",
                  Webhook,
                  t("guildSettings.tab.webhooks"),
                  "guild-settings-tab-webhooks",
                )}
              </Show>

              {/* Leave or delete (ADMIN-5). */}
              <div class="mt-auto pt-4">
                <button
                  onClick={() => setConfirmExit(true)}
                  data-testid="guild-settings-exit"
                  class="flex items-center gap-2.5 w-full px-3 py-2 rounded-lg text-sm font-medium text-text-primary hover:bg-status-error/15 transition-colors"
                >
                  <Show when={isOwner()} fallback={<LogOut class="w-4 h-4 shrink-0" />}>
                    <Trash2 class="w-4 h-4 shrink-0" />
                  </Show>
                  {isOwner() ? t("guildSettings.server.delete") : t("guildSettings.server.leave")}
                </button>
              </div>
            </nav>

            {/* Content */}
            <div class="flex-1 overflow-y-auto min-h-0">
            <Show when={activeTab() === "general" && canManageGuild()}>
              <GeneralTab guildId={props.guildId} onDirtyChange={setNameDirty} />
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
            <Show when={activeTab() === "webhooks" && canManageWebhooks()}>
              <WebhooksTab guildId={props.guildId} />
            </Show>
            <Show when={activeTab() === "safety" && canManageGuild()}>
              <SafetyTab guildId={props.guildId} onDirtyChange={setSafetyDirty} />
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
                    setRoleDirty(false);
                    setEditingRole(null);
                    setIsCreatingRole(false);
                  }}
                  onSave={() => {
                    setRoleDirty(false);
                    setEditingRole(null);
                    setIsCreatingRole(false);
                  }}
                  onDirtyChange={setRoleDirty}
                />
              </Show>
            </Show>
            <Show when={activeTab() === "reaction-roles" && canManageRoles()}>
              <ReactionRolesTab guildId={props.guildId} />
            </Show>
            </div>
          </div>
        </div>
      </div>

      <Show when={discardThen()}>
        {(then) => (
          <ConfirmActionDialog
            title={t("guildSettings.unsaved.title")}
            message={t("guildSettings.unsaved.body")}
            confirmLabel={t("guildSettings.unsaved.discard")}
            danger
            onConfirm={() => Promise.resolve()}
            onDone={() => {
              const next = then();
              setDiscardThen(null);
              next();
            }}
            onCancel={() => setDiscardThen(null)}
            testId="guild-settings-unsaved"
          />
        )}
      </Show>

      <Show when={confirmExit()}>
        <ConfirmActionDialog
          title={
            isOwner()
              ? t("guildSettings.server.deleteTitle", { name: guild()?.name ?? "" })
              : t("guildSettings.server.leaveTitle", { name: guild()?.name ?? "" })
          }
          message={
            isOwner()
              ? t("guildSettings.server.deleteBody", { name: guild()?.name ?? "" })
              : t("guildSettings.server.leaveBody", { name: guild()?.name ?? "" })
          }
          confirmLabel={isOwner() ? t("guildSettings.server.delete") : t("guildSettings.server.leave")}
          danger
          confirmDisabled={isOwner() && deleteName() !== guild()?.name}
          onConfirm={exitServer}
          onDone={() => {
            setConfirmExit(false);
            props.onClose();
          }}
          onCancel={() => {
            setConfirmExit(false);
            setDeleteName("");
          }}
          testId="guild-settings-exit-dialog"
        >
          <Show when={isOwner()}>
            <label class="block space-y-2">
              <span class="text-sm text-text-secondary">
                {t("guildSettings.server.typeName", { name: guild()?.name ?? "" })}
              </span>
              <input
                type="text"
                value={deleteName()}
                onInput={(e) => setDeleteName(e.currentTarget.value)}
                class="w-full px-3 py-2 rounded-lg bg-white/5 border border-white/10 text-text-primary focus:outline-none focus:border-status-error text-sm"
              />
            </label>
          </Show>
        </ConfirmActionDialog>
      </Show>
    </Portal>
  );
};

export default GuildSettingsModal;
