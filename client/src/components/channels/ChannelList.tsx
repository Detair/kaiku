import { Component, For, Show } from "solid-js";
import { ChevronDown } from "lucide-solid";
import {
  channelsState,
  textChannels,
  voiceChannels,
  selectChannel,
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

  return (
    <nav class="flex-1 overflow-y-auto px-2 py-2">
      {/* Text Channels */}
      <Show when={textChannels().length > 0}>
        <div class="mb-4">
          <div class="flex items-center gap-1 px-1 mb-1">
            <ChevronDown class="w-3 h-3 text-text-muted" />
            <span class="text-xs font-semibold text-text-muted uppercase tracking-wide">
              Text Channels
            </span>
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
      </Show>

      {/* Voice Channels */}
      <Show when={voiceChannels().length > 0}>
        <div class="mb-4">
          <div class="flex items-center gap-1 px-1 mb-1">
            <ChevronDown class="w-3 h-3 text-text-muted" />
            <span class="text-xs font-semibold text-text-muted uppercase tracking-wide">
              Voice Channels
            </span>
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
      </Show>

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
