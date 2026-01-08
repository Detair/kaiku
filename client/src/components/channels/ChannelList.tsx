import { Component, For, Show } from "solid-js";
import { ChevronDown, Plus } from "lucide-solid";
import {
  channelsState,
  textChannels,
  voiceChannels,
  selectChannel,
  createChannel,
} from "@/stores/channels";
import { joinVoice, leaveVoice, isInChannel } from "@/stores/voice";
import ChannelItem from "./ChannelItem";

const ChannelList: Component = () => {
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

  const handleCreateChannel = async (type: "text" | "voice") => {
    const name = prompt(`Enter ${type} channel name:`);
    if (!name || !name.trim()) return;

    try {
      const channel = await createChannel(name.trim(), type);
      if (type === "text") {
        selectChannel(channel.id);
      }
    } catch (err) {
      console.error("Failed to create channel:", err);
      alert("Failed to create channel: " + (err instanceof Error ? err.message : String(err)));
    }
  };

  return (
    <nav class="flex-1 overflow-y-auto px-2 py-2">
      {/* Text Channels */}
      <div class="mb-4">
        <div class="flex items-center justify-between px-1 mb-1">
          <div class="flex items-center gap-1">
            <ChevronDown class="w-3 h-3 text-text-muted" />
            <span class="text-xs font-semibold text-text-muted uppercase tracking-wide">
              Text Channels
            </span>
          </div>
          <button
            class="p-0.5 text-text-muted hover:text-text-primary rounded transition-colors"
            title="Create Text Channel"
            onClick={() => handleCreateChannel("text")}
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
              />
            )}
          </For>
        </div>
      </div>

      {/* Voice Channels */}
      <div class="mb-4">
        <div class="flex items-center justify-between px-1 mb-1">
          <div class="flex items-center gap-1">
            <ChevronDown class="w-3 h-3 text-text-muted" />
            <span class="text-xs font-semibold text-text-muted uppercase tracking-wide">
              Voice Channels
            </span>
          </div>
          <button
            class="p-0.5 text-text-muted hover:text-text-primary rounded transition-colors"
            title="Create Voice Channel"
            onClick={() => handleCreateChannel("voice")}
          >
            <Plus class="w-4 h-4" />
          </button>
        </div>
        <div class="space-y-0.5">
          <For each={voiceChannels()}>
            {(channel) => (
              <ChannelItem
                channel={channel}
                isSelected={false}
                onClick={() => handleVoiceChannelClick(channel.id)}
              />
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
        <div class="px-2 py-4 text-center text-text-muted text-sm">
          No channels yet
        </div>
      </Show>

      {/* Error state */}
      <Show when={channelsState.error}>
        <div class="px-2 py-4 text-center text-danger text-sm">
          {channelsState.error}
        </div>
      </Show>
    </nav>
  );
};

export default ChannelList;
