import { Component, Show } from "solid-js";
import { Hash, Volume2 } from "lucide-solid";
import type { Channel } from "@/lib/types";
import { voiceState, isInChannel } from "@/stores/voice";

interface ChannelItemProps {
  channel: Channel;
  isSelected: boolean;
  onClick: () => void;
}

const ChannelItem: Component<ChannelItemProps> = (props) => {
  const isVoice = () => props.channel.channel_type === "voice";
  const isConnected = () => isInChannel(props.channel.id);

  return (
    <button
      class={`w-full flex items-center gap-2 px-2 py-1.5 rounded text-sm transition-colors ${
        props.isSelected
          ? "bg-background-tertiary/50 text-text-primary"
          : "text-text-secondary hover:text-text-primary hover:bg-background-tertiary/30"
      }`}
      onClick={props.onClick}
    >
      {isVoice() ? (
        <Volume2 class={`w-4 h-4 shrink-0 ${isConnected() ? "text-success" : ""}`} />
      ) : (
        <Hash class="w-4 h-4 shrink-0" />
      )}
      <span class="truncate">{props.channel.name}</span>
      <Show when={isVoice() && isConnected()}>
        <span class="ml-auto text-xs text-success">Connected</span>
      </Show>
    </button>
  );
};

export default ChannelItem;
