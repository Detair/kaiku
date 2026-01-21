import { Component, For, Show, createSignal } from "solid-js";
import { ChevronDown, Plus, Mic } from "lucide-solid";
import {
  channelsState,
  textChannels,
  voiceChannels,
  selectChannel,
} from "@/stores/channels";
import { guildsState, isGuildOwner } from "@/stores/guilds";
import { authState } from "@/stores/auth";
import { joinVoice, leaveVoice, isInChannel } from "@/stores/voice";
import { memberHasPermission } from "@/stores/permissions";
import { PermissionBits } from "@/lib/permissionConstants";
import ChannelItem from "./ChannelItem";
import CreateChannelModal from "./CreateChannelModal";
import ChannelSettingsModal from "./ChannelSettingsModal";
import MicrophoneTest from "../voice/MicrophoneTest";
import VoiceParticipants from "../voice/VoiceParticipants";

const ChannelList: Component = () => {
  const [showMicTest, setShowMicTest] = createSignal(false);
  const [showCreateModal, setShowCreateModal] = createSignal(false);
  const [createModalType, setCreateModalType] = createSignal<"text" | "voice">("text");
  const [settingsChannelId, setSettingsChannelId] = createSignal<string | null>(null);

  // Check if current user can manage channels
  const canManageChannels = () => {
    const guildId = guildsState.activeGuildId;
    const userId = authState.user?.id;
    if (!guildId || !userId) return false;

    const isOwner = isGuildOwner(guildId, userId);
    return isOwner || memberHasPermission(guildId, userId, isOwner, PermissionBits.MANAGE_CHANNELS);
  };

  const handleVoiceChannelClick = async (channelId: string) => {
    if (isInChannel(channelId)) {
      // Already in this channel, leave it
      await leaveVoice();
    } else {
      // Join the voice channel
      try {
        await joinVoice(channelId);
      } catch (err) {
        console.error("Failed to join voice:", err);
      }
    }
  };

  const openCreateModal = (type: "text" | "voice") => {
    setCreateModalType(type);
    setShowCreateModal(true);
  };

  const handleChannelCreated = (channelId: string) => {
    // Auto-select text channels after creation
    if (createModalType() === "text") {
      selectChannel(channelId);
    }
  };

  return (
    <nav class="flex-1 overflow-y-auto px-2 py-2">
      {/* Text Channels */}
      <div class="mb-4">
        <div class="flex items-center justify-between px-2 py-1 mb-1 rounded-lg hover:bg-white/5 transition-colors group">
          <div class="flex items-center gap-1.5">
            <ChevronDown class="w-3 h-3 text-text-secondary transition-transform duration-200 group-hover:text-text-primary" />
            <span class="text-xs font-bold text-text-secondary uppercase tracking-wider group-hover:text-text-primary transition-colors">
              Text Channels
            </span>
          </div>
          <button
            class="p-1 text-text-secondary hover:text-text-primary rounded-lg hover:bg-white/10 transition-all duration-200 opacity-0 group-hover:opacity-100"
            title="Create Text Channel"
            onClick={() => openCreateModal("text")}
          >
            <Plus class="w-4 h-4" />
          </button>
        </div>
        <div class="space-y-0.5">
          <For each={textChannels()}>
            {(channel) => (
              <ChannelItem
                channel={channel}
                isSelected={channelsState.selectedChannelId === channel.id}
                onClick={() => selectChannel(channel.id)}
                onSettings={canManageChannels() ? () => setSettingsChannelId(channel.id) : undefined}
              />
            )}
          </For>
        </div>
      </div>

      {/* Separator */}
      <div class="mx-3 my-2 border-t border-white/10" />

      {/* Voice Channels */}
      <div class="mb-4">
        <div class="flex items-center justify-between px-2 py-1 mb-1 rounded-lg hover:bg-white/5 transition-colors group">
          <div class="flex items-center gap-1.5">
            <ChevronDown class="w-3 h-3 text-text-secondary transition-transform duration-200 group-hover:text-text-primary" />
            <span class="text-xs font-bold text-text-secondary uppercase tracking-wider group-hover:text-text-primary transition-colors">
              Voice Channels
            </span>
          </div>
          <div class="flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity">
            <button
              class="p-1 text-text-secondary hover:text-accent-primary rounded-lg hover:bg-white/10 transition-all duration-200"
              title="Test Microphone"
              onClick={() => setShowMicTest(true)}
            >
              <Mic class="w-4 h-4" />
            </button>
            <button
              class="p-1 text-text-secondary hover:text-text-primary rounded-lg hover:bg-white/10 transition-all duration-200"
              title="Create Voice Channel"
              onClick={() => openCreateModal("voice")}
            >
              <Plus class="w-4 h-4" />
            </button>
          </div>
        </div>
        <div class="space-y-0.5">
          <For each={voiceChannels()}>
            {(channel) => (
              <div>
                <ChannelItem
                  channel={channel}
                  isSelected={false}
                  onClick={() => handleVoiceChannelClick(channel.id)}
                  onSettings={canManageChannels() ? () => setSettingsChannelId(channel.id) : undefined}
                />
                <VoiceParticipants channelId={channel.id} />
              </div>
            )}
          </For>
        </div>
      </div>

      {/* Empty state */}
      <Show
        when={
          !channelsState.isLoading &&
          channelsState.channels.length === 0 &&
          !channelsState.error
        }
      >
        <div class="px-2 py-4 text-center text-text-secondary text-sm">
          No channels yet
        </div>
      </Show>

      {/* Error state */}
      <Show when={channelsState.error}>
        <div class="px-2 py-4 text-center text-sm" style="color: var(--color-error-text)">
          {channelsState.error}
        </div>
      </Show>

      {/* Microphone Test Modal */}
      <Show when={showMicTest()}>
        <MicrophoneTest onClose={() => setShowMicTest(false)} />
      </Show>

      {/* Create Channel Modal */}
      <Show when={showCreateModal() && guildsState.activeGuildId}>
        <CreateChannelModal
          guildId={guildsState.activeGuildId!}
          initialType={createModalType()}
          onClose={() => setShowCreateModal(false)}
          onCreated={handleChannelCreated}
        />
      </Show>

      {/* Channel Settings Modal */}
      <Show when={settingsChannelId() && guildsState.activeGuildId}>
        <ChannelSettingsModal
          channelId={settingsChannelId()!}
          guildId={guildsState.activeGuildId!}
          onClose={() => setSettingsChannelId(null)}
        />
      </Show>
    </nav>
  );
};

export default ChannelList;
