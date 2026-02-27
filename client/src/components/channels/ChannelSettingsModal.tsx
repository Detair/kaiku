/**
 * ChannelSettingsModal - Channel settings with permissions tab
 */

import { Component, createSignal, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { X, Hash, Settings, Shield, Check, Bell, BellOff } from "lucide-solid";
import { channelsState } from "@/stores/channels";
import { memberHasPermission } from "@/stores/permissions";
import { authState } from "@/stores/auth";
import { isGuildOwner } from "@/stores/guilds";
import { PermissionBits } from "@/lib/permissionConstants";
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
                <h2 class="text-lg font-bold text-text-primary">
                  {channel()?.name}
                </h2>
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
                "text-accent-primary border-b-2 border-accent-primary":
                  activeTab() === "overview",
                "text-text-secondary hover:text-text-primary":
                  activeTab() !== "overview",
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
                  "text-accent-primary border-b-2 border-accent-primary":
                    activeTab() === "permissions",
                  "text-text-secondary hover:text-text-primary":
                    activeTab() !== "permissions",
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
              <div class="p-6 space-y-6">
                {/* Channel Name */}
                <div>
                  <label class="block text-sm font-medium text-text-secondary mb-2">
                    Channel Name
                  </label>
                  <div
                    class="px-3 py-2 rounded-lg border border-white/10 text-text-primary"
                    style="background-color: var(--color-surface-layer1)"
                  >
                    {channel()?.name}
                  </div>
                </div>

                {/* Notification Settings */}
                <div>
                  <div class="flex items-center gap-2 mb-3">
                    <Bell class="w-4 h-4 text-text-secondary" />
                    <h3 class="text-base font-medium text-text-primary">
                      Notifications
                    </h3>
                  </div>
                  <p class="text-sm text-text-secondary mb-4">
                    Choose when to receive sound notifications for this channel
                  </p>

                  <div class="space-y-2">
                    <NotificationOption
                      channelId={props.channelId}
                      isDm={channel()?.channel_type === "dm"}
                      level="all"
                      label="All messages"
                      description="Get notified for every message"
                    />
                    <NotificationOption
                      channelId={props.channelId}
                      isDm={channel()?.channel_type === "dm"}
                      level="mentions"
                      label="Mentions only"
                      description="Only when you're @mentioned"
                    />
                    <NotificationOption
                      channelId={props.channelId}
                      isDm={channel()?.channel_type === "dm"}
                      level="none"
                      label="None"
                      description="Mute all notifications"
                    />
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
      onClick={() => setChannelNotificationLevel(props.channelId, props.level)}
      class="w-full text-left p-3 rounded-xl border-2 transition-all duration-200"
      classList={{
        "border-accent-primary bg-accent-primary/10": isSelected(),
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
          {isSelected() && <Check class="w-3 h-3 text-white" />}
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
