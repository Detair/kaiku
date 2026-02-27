/**
 * Sidebar - Context Navigation
 *
 * Middle-left panel containing:
 * - Server/Guild header with settings gear
 * - Search bar
 * - Guild pages section
 * - Channel list
 * - User panel at bottom
 */

import {
  Component,
  createSignal,
  createEffect,
  onMount,
  Show,
  lazy,
  Suspense,
} from "solid-js";
import { useNavigate } from "@solidjs/router";
import { ChevronDown, Settings, Search, BookOpen } from "lucide-solid";
import { loadChannels } from "@/stores/channels";
import { getActiveGuild } from "@/stores/guilds";
import { loadFavorites } from "@/stores/favorites";
import { clearSearch } from "@/stores/search";
import FavoritesSection from "./FavoritesSection";
import {
  pagesState,
  loadGuildPages,
  loadPendingAcceptance,
} from "@/stores/pages";
import ChannelList from "@/components/channels/ChannelList";
import PageSection from "@/components/pages/PageSection";
import SearchPanel from "@/components/search/SearchPanel";
import UserPanel from "./UserPanel";
import { ModalFallback, LazyErrorBoundary } from "@/components/ui/LazyFallback";
import type { PageListItem } from "@/lib/types";

const GuildSettingsModal = lazy(
  () => import("@/components/guilds/GuildSettingsModal"),
);

const Sidebar: Component = () => {
  const navigate = useNavigate();
  const [showGuildSettings, setShowGuildSettings] = createSignal(false);
  const [selectedPageId, setSelectedPageId] = createSignal<string | null>(null);
  const [pagesExpanded, setPagesExpanded] = createSignal(true);
  const [showSearch, setShowSearch] = createSignal(false);

  // Close search panel and clear results
  const handleCloseSearch = () => {
    setShowSearch(false);
    clearSearch();
  };

  // Load channels and favorites when sidebar mounts
  onMount(() => {
    loadChannels();
    loadPendingAcceptance();
    loadFavorites();
  });

  const activeGuild = () => getActiveGuild();

  // Load guild pages when active guild changes
  createEffect(() => {
    const guild = activeGuild();
    if (guild) {
      loadGuildPages(guild.id);
    }
  });

  // Get guild pages for the active guild
  const guildPages = () => {
    const guild = activeGuild();
    if (!guild) return [];
    return pagesState.guildPages[guild.id] || [];
  };

  // Get pending page IDs as a Set
  const pendingPageIds = () =>
    new Set(pagesState.pendingAcceptance.map((p) => p.id));

  // Handle page selection - navigate to page route
  const handleSelectPage = (page: PageListItem) => {
    setSelectedPageId(page.id);
    const guild = activeGuild();
    if (guild) {
      navigate(`/guilds/${guild.id}/pages/${page.slug}`);
    }
  };

  return (
    <aside class="w-[240px] flex flex-col bg-surface-layer2 z-10 transition-all duration-300 border-r border-white/10">
      {/* Server Header with Settings */}
      <header class="h-12 px-4 flex items-center justify-between border-b border-white/10 group">
        <div class="flex items-center gap-2 flex-1 min-w-0 rounded-lg -ml-2 px-2 py-1">
          <h1 class="font-bold text-lg text-text-primary truncate">
            {activeGuild()?.name || "VoiceChat"}
          </h1>
          <ChevronDown class="w-4 h-4 text-text-secondary flex-shrink-0" />
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
        <Show when={activeGuild()}>
          <button
            onClick={() => setShowSearch(true)}
            class="w-full flex items-center gap-2 px-3 py-2 rounded-xl text-sm text-text-secondary/50 border border-white/5 hover:border-white/10 transition-colors"
            style="background-color: var(--color-surface-base)"
          >
            <Search class="w-4 h-4" />
            <span>Search messages...</span>
          </button>
        </Show>
        <Show when={!activeGuild()}>
          <div
            class="w-full px-3 py-2 rounded-xl text-sm text-text-secondary/50 border border-white/5"
            style="background-color: var(--color-surface-base)"
          >
            Search...
          </div>
        </Show>
      </div>

      {/* Separator */}
      <div class="mx-3 my-1 border-t border-white/10" />

      {/* Favorites Section */}
      <FavoritesSection />

      {/* Guild Pages Section */}
      <Show when={activeGuild() && guildPages().length > 0}>
        <PageSection
          title="Information"
          pages={guildPages()}
          pendingPageIds={pendingPageIds()}
          selectedPageId={selectedPageId()}
          isExpanded={pagesExpanded()}
          onToggle={() => setPagesExpanded(!pagesExpanded())}
          onSelectPage={handleSelectPage}
        />
      </Show>

      {/* Library Link */}
      <Show when={activeGuild()}>
        <button
          type="button"
          onClick={() => navigate(`/guilds/${activeGuild()!.id}/library`)}
          class="mx-2 mb-1 flex items-center gap-2 px-3 py-1.5 rounded-md text-sm text-text-secondary hover:text-text-primary hover:bg-white/5 transition-colors"
        >
          <BookOpen class="w-4 h-4" />
          Library
        </button>
      </Show>

      {/* Channel List */}
      <ChannelList />

      {/* User Panel (Bottom) */}
      <UserPanel />

      {/* Guild Settings Modal */}
      <Show when={showGuildSettings() && activeGuild()}>
        <LazyErrorBoundary name="GuildSettingsModal">
          <Suspense fallback={<ModalFallback />}>
            <GuildSettingsModal
              guildId={activeGuild()!.id}
              onClose={() => setShowGuildSettings(false)}
            />
          </Suspense>
        </LazyErrorBoundary>
      </Show>

      {/* Search Panel Overlay */}
      <Show when={showSearch() && activeGuild()}>
        <SearchPanel onClose={handleCloseSearch} />
      </Show>
    </aside>
  );
};

export default Sidebar;
