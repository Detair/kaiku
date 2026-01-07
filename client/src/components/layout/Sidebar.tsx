import { Component, onMount } from "solid-js";
import { loadChannels } from "@/stores/channels";
import ChannelList from "@/components/channels/ChannelList";
import VoicePanel from "@/components/voice/VoicePanel";
import UserPanel from "./UserPanel";

const Sidebar: Component = () => {
  // Load channels when sidebar mounts
  onMount(() => {
    loadChannels();
  });

  return (
    <div class="w-60 bg-background-secondary flex flex-col h-full">
      {/* Server header */}
      <div class="h-12 px-4 flex items-center border-b border-background-tertiary shadow-sm">
        <h1 class="font-semibold text-text-primary truncate">VoiceChat</h1>
      </div>

      {/* Channel list */}
      <ChannelList />

      {/* Voice panel (shown when in voice) */}
      <VoicePanel />

      {/* User panel */}
      <UserPanel />
    </div>
  );
};

export default Sidebar;
