/**
 * Sidebar - Context Navigation
 *
 * Middle-left panel containing:
 * - Server/Guild header with settings gear
 * - Search bar
 * - Channel list
 * - User panel at bottom
 */

import { Component, createSignal, onMount, Show } from "solid-js";
import { ChevronDown, Settings } from "lucide-solid";
import { loadChannels } from "@/stores/channels";
import { getActiveGuild } from "@/stores/guilds";
import ChannelList from "@/components/channels/ChannelList";
import UserPanel from "./UserPanel";
import GuildSettingsModal from "@/components/guilds/GuildSettingsModal";

const Sidebar: Component = () => {
  const [showGuildSettings, setShowGuildSettings] = createSignal(false);

  // Load channels when sidebar mounts
  onMount(() => {
    loadChannels();
  });

  const activeGuild = () => getActiveGuild();

  return (
    <aside class="w-[240px] flex flex-col bg-surface-layer2 z-10 transition-all duration-300">
      {/* Server Header with Settings */}
      <header class="h-12 px-4 flex items-center justify-between border-b border-white/5 group">
        <div class="flex items-center gap-2 flex-1 min-w-0 cursor-pointer hover:bg-surface-highlight rounded-lg -ml-2 px-2 py-1">
          <h1 class="font-bold text-lg text-text-primary truncate">
            {activeGuild()?.name || "VoiceChat"}
          </h1>
          <ChevronDown class="w-4 h-4 text-text-secondary flex-shrink-0 transition-transform duration-200 group-hover:rotate-180" />
        </div>

        {/* Settings gear - only show when in a guild */}
        <Show when={activeGuild()}>
          <button
            onClick={() => setShowGuildSettings(true)}
            class="p-1.5 text-text-secondary hover:text-text-primary hover:bg-white/10 rounded-lg transition-colors"
            title="Server Settings"
          >
            <Settings class="w-4 h-4" />
          </button>
        </Show>
      </header>

      {/* Search Bar */}
      <div class="px-3 py-2">
        <input
          type="text"
          placeholder="Search..."
          class="w-full px-3 py-2 rounded-xl text-sm text-text-input placeholder:text-text-secondary/50 outline-none focus:ring-2 focus:ring-accent-primary/30 border border-white/5"
          style="background-color: var(--color-surface-base)"
        />
      </div>

      {/* Channel List */}
      <ChannelList />

      {/* User Panel (Bottom) */}
      <UserPanel />

      {/* Guild Settings Modal */}
      <Show when={showGuildSettings() && activeGuild()}>
        <GuildSettingsModal
          guildId={activeGuild()!.id}
          onClose={() => setShowGuildSettings(false)}
        />
      </Show>
    </aside>
  );
};

export default Sidebar;
