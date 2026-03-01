/**
 * Main View - Primary Application Interface
 *
 * Uses the AppShell layout with:
 * - ServerRail (for guild/server switching)
 * - Sidebar (channel list and user panel)
 * - Main stage (chat messages)
 * - CommandPalette (Ctrl+K quick actions)
 */

import {
  Component,
  Show,
  lazy,
  Suspense,
  onMount,
  createEffect,
  onCleanup,
} from "solid-js";
import { Hash, Volume2 } from "lucide-solid";
import AppShell from "@/components/layout/AppShell";
import CommandPalette from "@/components/layout/CommandPalette";
import MessageList from "@/components/messages/MessageList";
import MessageInput from "@/components/messages/MessageInput";
import TypingIndicator from "@/components/messages/TypingIndicator";
import ThreadSidebar from "@/components/messages/ThreadSidebar";
import HomeView from "@/components/home/HomeView";
import HomeSidebar from "@/components/home/HomeSidebar";
import SearchPanel from "@/components/search/SearchPanel";
import { selectedChannel } from "@/stores/channels";
import { loadGuilds, guildsState, isDiscoveryActive } from "@/stores/guilds";
import { threadsState } from "@/stores/threads";
import {
  showGlobalSearch,
  setShowGlobalSearch,
  clearSearch,
} from "@/stores/search";

const DiscoveryView = lazy(
  () => import("@/components/discovery/DiscoveryView"),
);

const Main: Component = () => {
  const channel = selectedChannel;

  // Load guilds on mount
  onMount(() => {
    loadGuilds();
  });

  // Global search keyboard shortcut: Ctrl+Shift+F
  const handleGlobalSearchShortcut = (e: KeyboardEvent) => {
    if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key === "F") {
      e.preventDefault();
      setShowGlobalSearch(!showGlobalSearch());
    }
  };

  createEffect(() => {
    window.addEventListener("keydown", handleGlobalSearchShortcut);
    onCleanup(() =>
      window.removeEventListener("keydown", handleGlobalSearchShortcut),
    );
  });

  return (
    <>
      {/* Command Palette (Global) */}
      <CommandPalette />

      {/* Global Search Overlay */}
      <Show when={showGlobalSearch()}>
        <div class="fixed inset-0 z-[90] flex items-start justify-center pt-[10vh]">
          <div
            class="absolute inset-0 bg-black/50"
            onClick={() => {
              setShowGlobalSearch(false);
              clearSearch();
            }}
          />
          <div
            class="relative w-[640px] h-[70vh] rounded-xl border border-white/10 shadow-2xl overflow-hidden"
            style="background-color: var(--color-surface-layer2)"
          >
            <SearchPanel
              mode="global"
              onClose={() => {
                setShowGlobalSearch(false);
                clearSearch();
              }}
            />
          </div>
        </div>
      </Show>

      {/* Main Application Shell */}
      <AppShell
        showServerRail={true}
        sidebar={
          isDiscoveryActive() ? (
            <></>
          ) : guildsState.activeGuildId === null ? (
            <HomeSidebar />
          ) : undefined
        }
      >
        {/* Discovery View */}
        <Show when={isDiscoveryActive()}>
          <Suspense fallback={<div class="flex-1 bg-surface-layer1" />}>
            <DiscoveryView />
          </Suspense>
        </Show>

        {/* Main Content Area */}
        <Show when={!isDiscoveryActive()}>
          <Show
            when={guildsState.activeGuildId === null}
            fallback={
              <Show
                when={channel()}
                fallback={
                  <div class="flex-1 flex items-center justify-center bg-surface-layer1">
                    <div class="text-center text-text-secondary">
                      <Hash class="w-12 h-12 mx-auto mb-4 opacity-30" />
                      <p class="text-lg font-medium">
                        Select a channel to start chatting
                      </p>
                      <p class="text-sm mt-2 opacity-60">
                        Or press Ctrl+K to search
                      </p>
                    </div>
                  </div>
                }
              >
                <div class="flex flex-1 min-w-0">
                  <div class="flex-1 flex flex-col min-w-0">
                    {/* Channel Header */}
                    <header class="h-12 px-4 flex items-center border-b border-white/5 bg-surface-layer1 shadow-sm">
                      <Show
                        when={channel()?.channel_type === "voice"}
                        fallback={
                          <Hash class="w-5 h-5 text-text-secondary mr-2" />
                        }
                      >
                        <Volume2 class="w-5 h-5 text-text-secondary mr-2" />
                      </Show>
                      <span class="font-semibold text-text-primary">
                        {channel()?.name}
                      </span>
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
                    <MessageInput
                      channelId={channel()!.id}
                      channelName={channel()!.name}
                      guildId={guildsState.activeGuildId ?? undefined}
                    />
                  </div>

                  {/* Thread Sidebar */}
                  <Show when={threadsState.activeThreadId}>
                    <ThreadSidebar
                      channelId={channel()!.id}
                      guildId={guildsState.activeGuildId ?? undefined}
                    />
                  </Show>
                </div>
              </Show>
            }
          >
            {/* Home View - DMs and Friends */}
            <HomeView />
          </Show>
        </Show>
      </AppShell>
    </>
  );
};

export default Main;
