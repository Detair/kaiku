/**
 * ChannelSettingsModal - Channel settings with permissions tab
 */

import { Component, createSignal, onMount, Show } from "solid-js";
import ConfirmActionDialog from "@/components/admin/ConfirmActionDialog";
import { renameChannel, deleteChannel, canManageChannel as canEditChannelFor } from "@/stores/guildAdmin";
import { reportFailure, reportSuccess } from "@/components/guilds/settingsAction";
import { Portal } from "solid-js/web";
import { X, Hash, Settings, Shield, Check, Bell, BellOff, Trash2 } from "lucide-solid";
import { channelsState } from "@/stores/channels";
import { memberHasPermission, loadChannelOverrides } from "@/stores/permissions";
import { authState } from "@/stores/auth";
import { isGuildOwner } from "@/stores/guilds";
import { PermissionBits } from "@/lib/permissionConstants";
import { createDialog } from "@/lib/a11y/dialog";
import { createModalLayer, MODAL_LAYER } from "@/lib/a11y/modalLayer";
import { createTabs } from "@/lib/a11y/tabs";
import { t } from "@/i18n";
import ChannelPermissions from "./ChannelPermissions";
import {
  getChannelNotificationLevel,
  setChannelNotificationLevel,
  type NotificationLevel,
} from "@/stores/sound";

interface ChannelSettingsModalProps {
  channelId: string;
  guildId: string;
  onClose: () => void;
}

type TabId = "overview" | "permissions";

const ChannelSettingsModal: Component<ChannelSettingsModalProps> = (props) => {
  const [activeTab, setActiveTab] = createSignal<TabId>("overview");
  const [nameDraft, setNameDraft] = createSignal<string | null>(null);
  const [savingName, setSavingName] = createSignal(false);
  const [confirmDelete, setConfirmDelete] = createSignal(false);

  const dialog = createDialog({ onClose: () => props.onClose() });
  const layer = createModalLayer(MODAL_LAYER.dialog);

  const channel = () =>
    channelsState.channels.find((c) => c.id === props.channelId);

  const isOwner = () => isGuildOwner(props.guildId, authState.user?.id || "");

  const canManageChannel = () =>
    isOwner() ||
    memberHasPermission(
      props.guildId,
      authState.user?.id || "",
      isOwner(),
      PermissionBits.MANAGE_CHANNELS,
    );

  // Real tabs (KONTO-10).
  const tabsA11y = createTabs<TabId>({
    ids: () => (canManageChannel() ? ["overview", "permissions"] : ["overview"]),
    active: activeTab,
    onSelect: setActiveTab,
    label: t("guildSettings.channel.title"),
  });

  const name = () => nameDraft() ?? channel()?.name ?? "";
  const nameChanged = () => {
    const draft = nameDraft();
    return draft !== null && draft.trim() !== "" && draft.trim() !== channel()?.name;
  };

  // Rename (ADMIN-5): says whether it worked.
  const saveName = async (e: Event) => {
    e.preventDefault();
    if (!nameChanged() || savingName()) return;
    setSavingName(true);
    try {
      await renameChannel(props.guildId, props.channelId, name());
      reportSuccess(t("guildSettings.channel.renamed", { name: name().trim() }));
      setNameDraft(null);
    } catch (err) {
      reportFailure(t("guildSettings.channel.renameFailed"), err);
    } finally {
      setSavingName(false);
    }
  };

  let deletedName = "";
  const removeChannel = async () => {
    deletedName = channel()?.name ?? "";
    await deleteChannel(props.guildId, props.channelId);
  };

  // Rename and delete follow the server's per-channel rule (K8).
  const canEditChannel = () =>
    canEditChannelFor(props.guildId, props.channelId, authState.user?.id || "", isOwner());
  onMount(() => {
    if (!isOwner()) void loadChannelOverrides(props.channelId);
  });

  const handleBackdropClick = (e: MouseEvent) => {
    if (e.target === e.currentTarget) {
      props.onClose();
    }
  };

  return (
    <Portal>
      <div
        class="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center"
        style={{ "z-index": layer }}
        onClick={handleBackdropClick}
      >
        <div
          {...dialog.dialogProps}
          ref={dialog.ref}
          class="border border-white/10 rounded-2xl w-[550px] max-w-[calc(100vw-2rem)] max-h-[80vh] flex flex-col shadow-2xl"
          style={{"background-color":"var(--color-surface-base)"}}
        >
          {/* Header */}
          <div class="flex items-center justify-between px-6 py-4 border-b border-white/10">
            <div class="flex items-center gap-3">
              <Hash class="w-5 h-5 text-text-secondary" aria-hidden="true" />
              <div>
                <h2 id={dialog.titleId} class="text-lg font-bold text-text-primary">
                  {channel()?.name}
                </h2>
                <p class="text-sm text-text-secondary">{t("guildSettings.channel.title")}</p>
              </div>
            </div>
            <button
              type="button"
              aria-label={t("common.close")}
              onClick={props.onClose}
              class="p-1.5 tap-target text-text-secondary hover:text-text-primary hover:bg-white/10 rounded-lg transition-colors"
            >
              <X class="w-5 h-5" aria-hidden="true" />
            </button>
          </div>

          {/* Tabs */}
          <div {...tabsA11y.listProps} class="flex border-b border-white/10">
            <button
              {...tabsA11y.tabProps("overview")}
              onClick={() => setActiveTab("overview")}
              class="flex items-center gap-2 px-6 py-3 font-medium transition-colors"
              classList={{
                "text-text-primary border-b-2 border-accent-primary":
                  activeTab() === "overview",
                "text-text-secondary hover:text-text-primary":
                  activeTab() !== "overview",
              }}
            >
              <Settings class="w-4 h-4" aria-hidden="true" />
              {t("guildSettings.channel.overview")}
            </button>
            <Show when={canManageChannel()}>
              <button
                {...tabsA11y.tabProps("permissions")}
                onClick={() => setActiveTab("permissions")}
                data-testid="channel-settings-permissions-tab"
                class="flex items-center gap-2 px-6 py-3 font-medium transition-colors"
                classList={{
                  "text-text-primary border-b-2 border-accent-primary":
                    activeTab() === "permissions",
                  "text-text-secondary hover:text-text-primary":
                    activeTab() !== "permissions",
                }}
              >
                <Shield class="w-4 h-4" aria-hidden="true" />
                {t("guildSettings.channel.permissions")}
              </button>
            </Show>
          </div>

          {/* Content */}
          <div {...tabsA11y.panelProps()} class="flex-1 overflow-y-auto" data-testid="channel-settings-content">
            <Show when={activeTab() === "overview"}>
              <div class="p-6 space-y-6">
                {/* Channel Name: editable for people who manage channels (ADMIN-5). */}
                <Show
                  when={canEditChannel()}
                  fallback={
                    <div>
                      <div class="block text-sm font-medium text-text-secondary mb-2">
                        {t("guildSettings.channel.name")}
                      </div>
                      <div
                        class="px-3 py-2 rounded-lg border border-white/10 text-text-primary"
                        style={{"background-color":"var(--color-surface-layer1)"}}
                      >
                        {channel()?.name}
                      </div>
                    </div>
                  }
                >
                  <form onSubmit={saveName}>
                    <label for="channel-name-input" class="block text-sm font-medium text-text-secondary mb-2">
                      {t("guildSettings.channel.name")}
                    </label>
                    <div class="flex gap-2">
                      <input
                        id="channel-name-input"
                        type="text"
                        maxLength={64}
                        value={name()}
                        onInput={(e) => setNameDraft(e.currentTarget.value)}
                        class="flex-1 px-3 py-2 rounded-lg border border-white/10 text-text-primary focus:outline-none focus:border-accent-primary"
                        style={{"background-color":"var(--color-surface-layer1)"}}
                      />
                      <button
                        type="submit"
                        disabled={!nameChanged() || savingName()}
                        class="px-4 py-2 rounded-lg bg-accent-primary text-on-accent font-medium hover:bg-accent-primary/90 state-disabled"
                      >
                        {savingName() ? t("guildSettings.working") : t("common.save")}
                      </button>
                    </div>
                  </form>
                </Show>

                {/* Notification Settings */}
                <div>
                  <div class="flex items-center gap-2 mb-3">
                    <Bell class="w-4 h-4 text-text-secondary" />
                    <h3 class="text-base font-medium text-text-primary">
                      {t("common.notifications")}
                    </h3>
                  </div>
                  <p class="text-sm text-text-secondary mb-4">
                    {t("guildSettings.channel.notifyHint")}
                  </p>

                  <div class="space-y-2">
                    <NotificationOption
                      channelId={props.channelId}
                      isDm={channel()?.channel_type === "dm"}
                      level="all"
                      label={t("guildSettings.channel.notifyAll")}
                      description={t("guildSettings.channel.notifyAllHint")}
                    />
                    <NotificationOption
                      channelId={props.channelId}
                      isDm={channel()?.channel_type === "dm"}
                      level="mentions"
                      label={t("guildSettings.channel.notifyMentions")}
                      description={t("guildSettings.channel.notifyMentionsHint")}
                    />
                    <NotificationOption
                      channelId={props.channelId}
                      isDm={channel()?.channel_type === "dm"}
                      level="none"
                      label={t("guildSettings.channel.notifyNone")}
                      description={t("guildSettings.channel.notifyNoneHint")}
                    />
                  </div>
                </div>

                {/* Delete (ADMIN-5): asks first, names the channel. */}
                <Show when={canEditChannel()}>
                  <div class="pt-4 border-t border-white/10">
                    <button
                      type="button"
                      onClick={() => setConfirmDelete(true)}
                      class="flex items-center gap-2 px-4 py-2 rounded-lg border border-status-error/50 text-text-primary font-medium hover:bg-status-error/10 transition-colors"
                    >
                      <Trash2 class="w-4 h-4" />
                      {t("guildSettings.channel.delete")}
                    </button>
                  </div>
                </Show>
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
      <Show when={confirmDelete()}>
        <ConfirmActionDialog
          title={t("guildSettings.channel.deleteTitle", { name: channel()?.name ?? "" })}
          message={t("guildSettings.channel.deleteBody", { name: channel()?.name ?? "" })}
          confirmLabel={t("guildSettings.channel.delete")}
          danger
          onConfirm={removeChannel}
          onDone={() => {
            setConfirmDelete(false);
            reportSuccess(t("guildSettings.channel.deleted", { name: deletedName }));
            props.onClose();
          }}
          onCancel={() => setConfirmDelete(false)}
          testId="channel-delete-dialog"
        />
      </Show>
    </Portal>
  );
};

// Notification level option component
const NotificationOption: Component<{
  channelId: string;
  isDm: boolean;
  level: NotificationLevel;
  label: string;
  description: string;
}> = (props) => {
  const currentLevel = () =>
    getChannelNotificationLevel(props.channelId, props.isDm);
  const isSelected = () => currentLevel() === props.level;

  return (
    <button
      type="button"
      onClick={() => setChannelNotificationLevel(props.channelId, props.level)}
      class="w-full text-left p-3 rounded-xl border-2 transition-all duration-200"
      classList={{
        "border-accent-primary bg-accent-primary/20": isSelected(),
        "border-white/10 hover:border-accent-primary/50 hover:bg-white/5":
          !isSelected(),
      }}
    >
      <div class="flex items-start gap-3">
        {/* Radio indicator */}
        <div
          class="w-5 h-5 rounded-full border-2 flex items-center justify-center flex-shrink-0 mt-0.5 transition-colors"
          classList={{
            "border-accent-primary bg-accent-primary": isSelected(),
            "border-white/30": !isSelected(),
          }}
        >
          {isSelected() && <Check class="w-3 h-3 text-on-accent" />}
        </div>

        {/* Content */}
        <div class="flex-1">
          <span class="font-semibold text-text-primary">{props.label}</span>
          <div class="text-sm text-text-secondary mt-0.5">
            {props.description}
          </div>
        </div>

        {/* Icon for muted */}
        {props.level === "none" && (
          <BellOff class="w-4 h-4 text-text-secondary mt-0.5" />
        )}
      </div>
    </button>
  );
};

export default ChannelSettingsModal;
