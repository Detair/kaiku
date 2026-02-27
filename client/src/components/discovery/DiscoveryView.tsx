/**
 * DiscoveryView - Browse and search for public guilds.
 */

import {
  Component,
  createSignal,
  createEffect,
  createMemo,
  For,
  Show,
  on,
  onCleanup,
  onMount,
} from "solid-js";
import { Search, ChevronLeft, ChevronRight } from "lucide-solid";
import type { DiscoverableGuild } from "@/lib/types";
import { discoverGuilds } from "@/lib/tauri";
import { guildsState } from "@/stores/guilds";
import GuildCard from "./GuildCard";

const PAGE_SIZE = 12;

const DiscoveryView: Component = () => {
  const [query, setQuery] = createSignal("");
  const [sort, setSort] = createSignal<"members" | "newest">("members");
  const [guilds, setGuilds] = createSignal<DiscoverableGuild[]>([]);
  const [total, setTotal] = createSignal(0);
  const [offset, setOffset] = createSignal(0);
  const [loading, setLoading] = createSignal(true);
  const [error, setError] = createSignal<string | null>(null);
  const [isPermanentError, setIsPermanentError] = createSignal(false);

  let debounceTimer: ReturnType<typeof setTimeout>;
  let requestId = 0;

  const memberGuildIds = createMemo(
    () => new Set(guildsState.guilds.map((g) => g.id)),
  );

  const fetchGuilds = async () => {
    const thisRequest = ++requestId;
    setLoading(true);
    setError(null);
    setIsPermanentError(false);
    try {
      const result = await discoverGuilds({
        q: query() || undefined,
        sort: sort(),
        limit: PAGE_SIZE,
        offset: offset(),
      });
      if (thisRequest !== requestId) return; // stale response
      setGuilds(result.guilds);
      setTotal(result.total);
    } catch (err: unknown) {
      if (thisRequest !== requestId) return;
      console.error("Failed to discover guilds:", err);
      // Distinguish discovery-disabled from transient failures
      const isDisabled =
        err instanceof Error && err.message.includes("DISCOVERY_DISABLED");
      setIsPermanentError(isDisabled);
      setError(
        isDisabled
          ? "Guild discovery is not enabled on this server."
          : "Could not load guilds. Please try again.",
      );
    } finally {
      if (thisRequest === requestId) setLoading(false);
    }
  };

  // Initial fetch on mount (no debounce delay)
  onMount(() => fetchGuilds());

  // Fetch on sort or offset change (defer to skip initial run)
  createEffect(on([sort, offset], () => fetchGuilds(), { defer: true }));

  // Debounce search query (defer to skip initial run)
  createEffect(
    on(
      query,
      () => {
        clearTimeout(debounceTimer);
        debounceTimer = setTimeout(() => {
          if (offset() !== 0) {
            setOffset(0); // triggers the [sort, offset] effect which calls fetchGuilds
          } else {
            fetchGuilds(); // offset already 0, effect won't fire, so fetch directly
          }
        }, 300);
      },
      { defer: true },
    ),
  );

  // Clean up debounce timer on unmount
  onCleanup(() => clearTimeout(debounceTimer));

  const totalPages = () => Math.max(1, Math.ceil(total() / PAGE_SIZE));
  const currentPage = () => Math.floor(offset() / PAGE_SIZE) + 1;

  return (
    <div class="flex-1 flex flex-col bg-surface-layer1 overflow-hidden">
      {/* Header */}
      <header class="h-12 px-6 flex items-center border-b border-white/5 bg-surface-layer1 shadow-sm shrink-0">
        <span class="font-semibold text-text-primary">Discover Servers</span>
      </header>

      <div class="flex-1 overflow-y-auto p-6">
        {/* Search and sort controls */}
        <div class="flex items-center gap-3 mb-6">
          <div class="flex-1 relative">
            <Search class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-text-secondary" />
            <input
              type="text"
              placeholder="Search servers..."
              aria-label="Search servers"
              value={query()}
              onInput={(e) => setQuery(e.currentTarget.value)}
              class="w-full pl-9 pr-3 py-2 text-sm rounded-lg bg-surface-layer2 border border-white/5 text-text-primary placeholder-text-secondary focus:outline-none focus:border-accent-primary/50"
            />
          </div>

          <div
            class="flex rounded-lg border border-white/5 overflow-hidden text-xs"
            role="group"
            aria-label="Sort order"
          >
            <button
              onClick={() => {
                setSort("members");
                setOffset(0);
              }}
              class="px-3 py-2 transition-colors"
              aria-pressed={sort() === "members"}
              classList={{
                "bg-accent-primary text-white": sort() === "members",
                "bg-surface-layer2 text-text-secondary hover:text-text-primary":
                  sort() !== "members",
              }}
            >
              Popular
            </button>
            <button
              onClick={() => {
                setSort("newest");
                setOffset(0);
              }}
              class="px-3 py-2 transition-colors"
              aria-pressed={sort() === "newest"}
              classList={{
                "bg-accent-primary text-white": sort() === "newest",
                "bg-surface-layer2 text-text-secondary hover:text-text-primary":
                  sort() !== "newest",
              }}
            >
              Newest
            </button>
          </div>
        </div>

        {/* Loading skeleton */}
        <Show when={loading()}>
          <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
            <For each={Array.from({ length: 6 })}>
              {() => (
                <div class="h-52 rounded-xl bg-surface-layer2 animate-pulse border border-white/5" />
              )}
            </For>
          </div>
        </Show>

        {/* Error state */}
        <Show when={error() && !loading()}>
          <div class="flex flex-col items-center justify-center py-16 text-center">
            <p class="text-text-secondary text-sm">{error()}</p>
            <Show when={!isPermanentError()}>
              <button
                onClick={fetchGuilds}
                class="mt-3 px-4 py-2 text-sm bg-accent-primary text-white rounded-lg hover:bg-accent-hover"
              >
                Retry
              </button>
            </Show>
          </div>
        </Show>

        {/* Empty state */}
        <Show when={!loading() && !error() && guilds().length === 0}>
          <div class="flex flex-col items-center justify-center py-16 text-center">
            <Search class="w-10 h-10 text-text-secondary opacity-30 mb-3" />
            <p class="text-text-secondary text-sm">
              {query()
                ? "No servers found matching your search."
                : "No discoverable servers yet."}
            </p>
          </div>
        </Show>

        {/* Guild grid */}
        <Show when={!loading() && !error() && guilds().length > 0}>
          <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
            <For each={guilds()}>
              {(guild) => (
                <GuildCard
                  guild={guild}
                  isMember={memberGuildIds().has(guild.id)}
                />
              )}
            </For>
          </div>

          {/* Pagination */}
          <Show when={totalPages() > 1}>
            <div class="flex items-center justify-center gap-3 mt-6">
              <button
                onClick={() =>
                  setOffset((prev) => Math.max(0, prev - PAGE_SIZE))
                }
                disabled={offset() === 0}
                aria-label="Previous page"
                class="p-2 rounded-lg bg-surface-layer2 text-text-secondary hover:text-text-primary disabled:opacity-30 disabled:cursor-default transition-colors"
              >
                <ChevronLeft class="w-4 h-4" />
              </button>
              <span class="text-xs text-text-secondary">
                Page {currentPage()} of {totalPages()}
              </span>
              <button
                onClick={() => setOffset((prev) => prev + PAGE_SIZE)}
                disabled={offset() + PAGE_SIZE >= total()}
                aria-label="Next page"
                class="p-2 rounded-lg bg-surface-layer2 text-text-secondary hover:text-text-primary disabled:opacity-30 disabled:cursor-default transition-colors"
              >
                <ChevronRight class="w-4 h-4" />
              </button>
            </div>
          </Show>
        </Show>
      </div>
    </div>
  );
};

export default DiscoveryView;
