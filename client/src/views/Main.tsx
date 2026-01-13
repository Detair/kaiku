/**
 * Main View - Primary Application Interface
 *
 * Uses the AppShell layout with:
 * - ServerRail (for guild/server switching)
 * - Sidebar (channel list and user panel)
 * - Main stage (chat messages)
 * - VoiceIsland (floating voice controls)
 * - CommandPalette (Ctrl+K quick actions)
 */

import { Component, Show, onMount } from "solid-js";
import { Hash, Volume2 } from "lucide-solid";
import AppShell from "@/components/layout/AppShell";
import CommandPalette from "@/components/layout/CommandPalette";
import MessageList from "@/components/messages/MessageList";
import MessageInput from "@/components/messages/MessageInput";
import TypingIndicator from "@/components/messages/TypingIndicator";
import { selectedChannel } from "@/stores/channels";
import { loadGuilds } from "@/stores/guilds";

const Main: Component = () => {
  const channel = selectedChannel;

  // Load guilds on mount
  onMount(() => {
    loadGuilds();
  });

  return (
    <>
      {/* Command Palette (Global) */}
      <CommandPalette />

      {/* Main Application Shell */}
      <AppShell showServerRail={true}>
        {/* Main Content Area */}
        <Show
          when={channel()}
          fallback={
            <div class="flex-1 flex items-center justify-center bg-surface-layer1">
              <div class="text-center text-text-secondary">
                <Hash class="w-12 h-12 mx-auto mb-4 opacity-30" />
                <p class="text-lg font-medium">Select a channel to start chatting</p>
                <p class="text-sm mt-2 opacity-60">Or press Ctrl+K to search</p>
              </div>
            </div>
          }
        >
          {/* Channel Header */}
          <header class="h-12 px-4 flex items-center border-b border-white/5 bg-surface-layer1 shadow-sm">
            <Show
              when={channel()?.channel_type === "voice"}
              fallback={<Hash class="w-5 h-5 text-text-secondary mr-2" />}
            >
              <Volume2 class="w-5 h-5 text-text-secondary mr-2" />
            </Show>
            <span class="font-semibold text-text-primary">{channel()?.name}</span>
            <Show when={channel()?.topic}>
              <div class="ml-4 pl-4 border-l border-white/10 text-text-secondary text-sm truncate">
                {channel()?.topic}
              </div>
            </Show>
          </header>

          {/* Messages */}
          <MessageList channelId={channel()!.id} />

          {/* Typing Indicator */}
          <TypingIndicator channelId={channel()!.id} />

          {/* Message Input */}
          <MessageInput channelId={channel()!.id} channelName={channel()!.name} />
        </Show>
      </AppShell>
    </>
  );
};

export default Main;
