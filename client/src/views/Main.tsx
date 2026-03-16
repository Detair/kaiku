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
  Switch,
  Match,
  lazy,
  Suspense,
  onMount,
  createEffect,
  createSignal,
  onCleanup,
} from "solid-js";
import { useLocation } from "@solidjs/router";
import { Hash, Volume2, Pin } from "lucide-solid";
import AppShell from "@/components/layout/AppShell";
import CommandPalette from "@/components/layout/CommandPalette";
import MessageList from "@/components/messages/MessageList";
import MessageInput from "@/components/messages/MessageInput";
import TypingIndicator from "@/components/messages/TypingIndicator";
import ThreadSidebar from "@/components/messages/ThreadSidebar";
import HomeView from "@/components/home/HomeView";
import HomeSidebar from "@/components/home/HomeSidebar";
import SearchPanel from "@/components/search/SearchPanel";
import KeyboardShortcutsDialog from "@/components/ui/KeyboardShortcutsDialog";
import { LazyErrorBoundary } from "@/components/ui/LazyFallback";
import { selectedChannel } from "@/stores/channels";
import { loadGuilds, guildsState, isDiscoveryActive, isGuildOwner } from "@/stores/guilds";
import { memberHasPermission } from "@/stores/permissions";
import { PermissionBits } from "@/lib/permissionConstants";
import { authState } from "@/stores/auth";
import { threadsState } from "@/stores/threads";
import {
  showGlobalSearch,
  setShowGlobalSearch,
  clearSearch,
} from "@/stores/search";
import { loadChannelPins, pinCount, clearChannelPins } from "@/stores/channelPins";
import PinDrawer from "@/components/channels/PinDrawer";

const DiscoveryView = lazy(
  () => import("@/components/discovery/DiscoveryView"),
);
const LibraryViewRoute = lazy(
  () => import("@/views/LibraryViewRoute"),
);
const PageViewRoute = lazy(
  () => import("@/views/PageViewRoute"),
);

const Main: Component = () => {
  const location = useLocation();
  const channel = selectedChannel;
  const [showShortcuts, setShowShortcuts] = createSignal(false);

  // Detect library and page routes within the guild context
  const isLibraryRoute = () => {
    const guildId = guildsState.activeGuildId;
    return guildId && location.pathname === `/guilds/${guildId}/library`;
  };
  const pageSlug = () => {
    const guildId = guildsState.activeGuildId;
    if (!guildId) return null;
    const match = location.pathname.match(`/guilds/${guildId}/pages/(.+)`);
    return match ? match[1] : null;
  };
  const [channelSearchScope, setChannelSearchScope] = createSignal(false);
  const [showPinDrawer, setShowPinDrawer] = createSignal(false);

  // Load guilds on mount
  onMount(() => {
    loadGuilds();
  });

  // Load pins when channel changes
  createEffect(() => {
    const ch = channel();
    if (ch) {
      loadChannelPins(ch.id);
    } else {
      clearChannelPins();
    }
    setShowPinDrawer(false);
  });

  // Combined global keyboard shortcut handler
  const handleGlobalKeydown = (e: KeyboardEvent) => {
    // Ctrl+Shift+F → toggle global search
    if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key === "F") {
      e.preventDefault();
      setShowGlobalSearch(!showGlobalSearch());
      return;
    }

    // Ctrl+F → channel-scoped search (must be checked after Ctrl+Shift+F)
    if ((e.ctrlKey || e.metaKey) && !e.shiftKey && e.key === "f") {
      e.preventDefault();
      setChannelSearchScope(!!channel());
      setShowGlobalSearch(true);
      return;
    }

    // Ctrl+/ → toggle shortcuts dialog
    if ((e.ctrlKey || e.metaKey) && e.key === "/") {
      e.preventDefault();
      setShowShortcuts((prev) => !prev);
      return;
    }

    // ? → toggle shortcuts dialog (only when not in an input field)
    if (e.key === "?" && !e.ctrlKey && !e.metaKey && !e.altKey) {
      const el = document.activeElement;
      const isInput =
        el instanceof HTMLInputElement ||
        el instanceof HTMLTextAreaElement ||
        (el instanceof HTMLElement && el.isContentEditable);
      if (!isInput) {
        e.preventDefault();
        setShowShortcuts((prev) => !prev);
      }
    }
  };

  createEffect(() => {
    window.addEventListener("keydown", handleGlobalKeydown);

    // Listen for custom event from /? slash command
    const handleOpenShortcuts = () => setShowShortcuts(true);
    window.addEventListener("open-shortcuts-dialog", handleOpenShortcuts);

    onCleanup(() => {
      window.removeEventListener("keydown", handleGlobalKeydown);
      window.removeEventListener("open-shortcuts-dialog", handleOpenShortcuts);
    });
  });

  return (
    <>
      {/* Command Palette (Global) */}
      <CommandPalette />

      {/* Keyboard Shortcuts Dialog */}
      <Show when={showShortcuts()}>
        <KeyboardShortcutsDialog onClose={() => setShowShortcuts(false)} />
      </Show>

      {/* Global Search Overlay */}
      <Show when={showGlobalSearch()}>
        <div class="fixed inset-0 z-[90] flex items-start justify-center pt-[10vh]">
          <div
            class="absolute inset-0 bg-black/50"
            onClick={() => {
              setShowGlobalSearch(false);
              setChannelSearchScope(false);
              clearSearch();
            }}
          />
          <div
            class="relative w-[640px] h-[70vh] rounded-xl border border-white/10 shadow-2xl overflow-hidden"
            style="background-color: var(--color-surface-layer2)"
          >
            <SearchPanel
              mode="global"
              initialScope={channelSearchScope() ? "channel" : "all"}
              channelId={channel()?.id}
              onClose={() => {
                setShowGlobalSearch(false);
                setChannelSearchScope(false);
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
          guildsState.activeGuildId === null ? (
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
          <Switch>
            {/* Home View - No guild selected */}
            <Match when={guildsState.activeGuildId === null}>
              <HomeView />
            </Match>

            {/* Guild Library */}
            <Match when={isLibraryRoute()}>
              <LazyErrorBoundary name="LibraryView">
                <Suspense fallback={<div class="flex-1 bg-surface-layer1" />}>
                  <LibraryViewRoute />
                </Suspense>
              </LazyErrorBoundary>
            </Match>

            {/* Guild Page */}
            <Match when={pageSlug()}>
              <Suspense fallback={<div class="flex-1 bg-surface-layer1" />}>
                <PageViewRoute />
              </Suspense>
            </Match>

            {/* Channel Chat */}
            <Match when={channel()}>
              <div class="flex flex-1 min-w-0 h-full overflow-hidden">
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
                    <div class="ml-auto flex items-center">
                      <button
                        onClick={() => setShowPinDrawer(!showPinDrawer())}
                        class="p-1.5 rounded hover:bg-white/10 text-text-secondary hover:text-text-primary transition-colors relative"
                        title="Pinned Messages"
                        aria-label="Pinned Messages"
                      >
                        <Pin class="w-4 h-4" />
                        <Show when={pinCount() > 0}>
                          <span class="absolute -top-1 -right-1 bg-accent-primary text-on-accent text-[10px] rounded-full w-4 h-4 flex items-center justify-center font-bold">
                            {pinCount()}
                          </span>
                        </Show>
                      </button>
                    </div>
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

                {/* Pin Drawer */}
                <Show when={showPinDrawer()}>
                  <PinDrawer
                    channelId={channel()!.id}
                    canUnpin={(() => {
                      const guildId = guildsState.activeGuildId;
                      const userId = authState.user?.id;
                      if (!guildId || !userId) return false;
                      const isOwner = isGuildOwner(guildId, userId);
                      return isOwner || memberHasPermission(guildId, userId, isOwner, PermissionBits.PIN_MESSAGES);
                    })()}
                    onClose={() => setShowPinDrawer(false)}
                    onJumpToMessage={(_messageId) => {
                      // Scroll-to-message works for search results; pin drawer dismisses on jump
                      setShowPinDrawer(false);
                    }}
                  />
                </Show>
              </div>
            </Match>

            {/* No channel selected fallback */}
            <Match when={guildsState.activeGuildId !== null}>
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
            </Match>
          </Switch>
        </Show>
      </AppShell>
    </>
  );
};

export default Main;
