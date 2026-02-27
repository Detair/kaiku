/**
 * SearchPanel Component
 *
 * Displays message search results with pagination and advanced filters.
 * Supports guild, DM, and global search modes.
 * Uses server-side ts_headline for highlighting and ts_rank for relevance sorting.
 */

import {
  Component,
  Show,
  For,
  createSignal,
  onCleanup,
  createMemo,
} from "solid-js";
import { useNavigate } from "@solidjs/router";
import {
  Search,
  X,
  Loader2,
  Hash,
  Filter,
  Link,
  Paperclip,
  Globe,
  MessageSquare,
} from "lucide-solid";
import {
  searchState,
  search,
  searchDMs,
  searchGlobal,
  loadMore,
  clearSearch,
  hasMore,
} from "@/stores/search";
import { getActiveGuild } from "@/stores/guilds";
import type {
  SearchFilters,
  SearchResult,
  GlobalSearchResult,
} from "@/lib/types";
import Avatar from "@/components/ui/Avatar";
import { formatTimestamp } from "@/lib/utils";
import DOMPurify from "dompurify";
import SearchSyntaxHelp from "./SearchSyntaxHelp";
import { createVirtualizer } from "@/lib/virtualizer";

interface SearchPanelProps {
  onClose: () => void;
  mode?: "guild" | "dm" | "global";
}

const SearchPanel: Component<SearchPanelProps> = (props) => {
  const navigate = useNavigate();
  const [inputValue, setInputValue] = createSignal("");
  const [showFilters, setShowFilters] = createSignal(false);
  const [dateFrom, setDateFrom] = createSignal("");
  const [dateTo, setDateTo] = createSignal("");
  const [authorFilter, setAuthorFilter] = createSignal("");
  const [hasFilter, setHasFilter] = createSignal<"link" | "file" | "">("");
  const [sortOrder, setSortOrder] = createSignal<"relevance" | "date">(
    "relevance",
  );
  let searchTimeout: ReturnType<typeof setTimeout> | null = null;
  let resultsContainerRef: HTMLDivElement | undefined;

  const resultCount = createMemo(() => searchState.results.length);

  const virtualizer = createVirtualizer({
    get count() {
      return resultCount();
    },
    getScrollElement: () => resultsContainerRef ?? null,
    estimateSize: () => 100,
    overscan: 5,
  });

  const mode = () => props.mode ?? "guild";

  const buildFilters = (): SearchFilters => {
    const filters: SearchFilters = {};
    if (dateFrom()) filters.date_from = new Date(dateFrom()).toISOString();
    if (dateTo()) {
      const endDate = new Date(dateTo());
      endDate.setHours(23, 59, 59, 999);
      filters.date_to = endDate.toISOString();
    }
    if (authorFilter()) filters.author_id = authorFilter();
    if (hasFilter()) filters.has = hasFilter() as "link" | "file";
    filters.sort = sortOrder();
    return filters;
  };

  const triggerSearch = () => {
    const value = inputValue();
    if (value.trim().length < 2) {
      clearSearch();
      return;
    }

    const filters = buildFilters();
    if (mode() === "global") {
      searchGlobal(value, filters);
    } else if (mode() === "dm") {
      searchDMs(value, filters);
    } else {
      const guild = getActiveGuild();
      if (guild) {
        search(guild.id, value, filters);
      }
    }
  };

  // Debounced search
  const handleInput = (e: Event) => {
    const value = (e.target as HTMLInputElement).value;
    setInputValue(value);

    if (searchTimeout) {
      clearTimeout(searchTimeout);
    }

    searchTimeout = setTimeout(triggerSearch, 300);
  };

  // Navigate to the message's channel when clicked
  const handleResultClick = (result: SearchResult | GlobalSearchResult) => {
    if (mode() === "global" && "source" in result) {
      const globalResult = result as GlobalSearchResult;
      if (
        globalResult.source.type === "guild" &&
        globalResult.source.guild_id
      ) {
        navigate(
          `/guilds/${globalResult.source.guild_id}/channels/${result.channel_id}?highlight=${result.id}`,
        );
      } else {
        navigate(`/home/dm/${result.channel_id}?highlight=${result.id}`);
      }
    } else if (mode() === "dm") {
      navigate(`/home/dm/${result.channel_id}?highlight=${result.id}`);
    } else {
      const guild = getActiveGuild();
      if (guild) {
        navigate(
          `/guilds/${guild.id}/channels/${result.channel_id}?highlight=${result.id}`,
        );
      }
    }
    props.onClose();
  };

  // Sanitize headline HTML (only allow <mark> tags from ts_headline)
  const sanitizeHeadline = (html: string): string => {
    return DOMPurify.sanitize(html, {
      ALLOWED_TAGS: ["mark"],
      ALLOWED_ATTR: [],
      ALLOW_DATA_ATTR: false,
    });
  };

  // Cleanup on unmount
  onCleanup(() => {
    if (searchTimeout) {
      clearTimeout(searchTimeout);
    }
  });

  const placeholderText = () => {
    switch (mode()) {
      case "global":
        return "Search everywhere...";
      case "dm":
        return "Search DMs...";
      default:
        return "Search messages...";
    }
  };

  return (
    <div class="absolute inset-0 z-50 flex flex-col bg-surface-layer2">
      {/* Search Header */}
      <div class="flex items-center justify-between px-3 py-2 border-b border-white/10">
        <div class="relative flex-1 max-w-md">
          <Search class="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-text-secondary" />
          <input
            type="text"
            placeholder={placeholderText()}
            value={inputValue()}
            onInput={handleInput}
            autofocus
            class="w-full pl-8 pr-3 py-1.5 rounded-md text-sm text-text-primary placeholder:text-text-secondary bg-surface-layer1 border border-white/10 outline-none focus:ring-1 focus:ring-accent-primary/30"
          />
        </div>
        <button
          onClick={() => setShowFilters(!showFilters())}
          class="ml-2 p-1.5 rounded transition-colors"
          classList={{
            "text-accent-primary bg-accent-primary/10": showFilters(),
            "text-text-secondary hover:text-text-primary": !showFilters(),
          }}
          title="Toggle filters"
        >
          <Filter class="w-4 h-4" />
        </button>
        <SearchSyntaxHelp />
        <button
          onClick={props.onClose}
          class="ml-1 p-1.5 text-text-secondary hover:text-text-primary rounded transition-colors"
        >
          <X class="w-4 h-4" />
        </button>
      </div>

      {/* Filters Panel */}
      <Show when={showFilters()}>
        <div class="px-3 py-2 border-b border-white/10 space-y-2">
          {/* Sort Toggle */}
          <div class="flex gap-1">
            <button
              onClick={() => {
                setSortOrder("relevance");
                triggerSearch();
              }}
              class="px-2 py-1 rounded text-xs transition-colors"
              classList={{
                "bg-accent-primary/20 text-accent-primary":
                  sortOrder() === "relevance",
                "bg-surface-layer1 text-text-secondary hover:text-text-primary":
                  sortOrder() !== "relevance",
              }}
            >
              Relevance
            </button>
            <button
              onClick={() => {
                setSortOrder("date");
                triggerSearch();
              }}
              class="px-2 py-1 rounded text-xs transition-colors"
              classList={{
                "bg-accent-primary/20 text-accent-primary":
                  sortOrder() === "date",
                "bg-surface-layer1 text-text-secondary hover:text-text-primary":
                  sortOrder() !== "date",
              }}
            >
              Date
            </button>
          </div>
          <div class="flex gap-2">
            <div class="flex-1">
              <label class="text-xs text-text-secondary block mb-1">
                From date
              </label>
              <input
                type="date"
                value={dateFrom()}
                onInput={(e) => {
                  setDateFrom(e.currentTarget.value);
                  triggerSearch();
                }}
                class="w-full px-2 py-1 rounded text-xs text-text-primary bg-surface-layer1 border border-white/10 outline-none"
              />
            </div>
            <div class="flex-1">
              <label class="text-xs text-text-secondary block mb-1">
                To date
              </label>
              <input
                type="date"
                value={dateTo()}
                onInput={(e) => {
                  setDateTo(e.currentTarget.value);
                  triggerSearch();
                }}
                class="w-full px-2 py-1 rounded text-xs text-text-primary bg-surface-layer1 border border-white/10 outline-none"
              />
            </div>
          </div>
          <div>
            <label class="text-xs text-text-secondary block mb-1">
              Author ID
            </label>
            <input
              type="text"
              placeholder="User ID"
              value={authorFilter()}
              onInput={(e) => {
                setAuthorFilter(e.currentTarget.value);
                triggerSearch();
              }}
              class="w-full px-2 py-1 rounded text-xs text-text-primary placeholder:text-text-secondary/50 bg-surface-layer1 border border-white/10 outline-none"
            />
          </div>
          <div class="flex gap-2">
            <button
              onClick={() => {
                setHasFilter(hasFilter() === "link" ? "" : "link");
                triggerSearch();
              }}
              class="flex items-center gap-1 px-2 py-1 rounded text-xs transition-colors"
              classList={{
                "bg-accent-primary/20 text-accent-primary":
                  hasFilter() === "link",
                "bg-surface-layer1 text-text-secondary hover:text-text-primary":
                  hasFilter() !== "link",
              }}
            >
              <Link class="w-3 h-3" />
              Has link
            </button>
            <button
              onClick={() => {
                setHasFilter(hasFilter() === "file" ? "" : "file");
                triggerSearch();
              }}
              class="flex items-center gap-1 px-2 py-1 rounded text-xs transition-colors"
              classList={{
                "bg-accent-primary/20 text-accent-primary":
                  hasFilter() === "file",
                "bg-surface-layer1 text-text-secondary hover:text-text-primary":
                  hasFilter() !== "file",
              }}
            >
              <Paperclip class="w-3 h-3" />
              Has file
            </button>
          </div>
        </div>
      </Show>

      <Show when={searchState.total > 0}>
        <span class="ml-3 text-xs text-text-secondary">
          {searchState.total} result{searchState.total !== 1 ? "s" : ""}
        </span>
      </Show>

      {/* Results */}
      <div ref={resultsContainerRef} class="flex-1 overflow-y-auto">
        {/* Loading State */}
        <Show
          when={searchState.isSearching && searchState.results.length === 0}
        >
          <div class="flex items-center justify-center py-8">
            <Loader2 class="w-6 h-6 text-text-secondary animate-spin" />
          </div>
        </Show>

        {/* Empty State */}
        <Show
          when={
            !searchState.isSearching &&
            searchState.query.length >= 2 &&
            searchState.results.length === 0
          }
        >
          <div class="flex flex-col items-center justify-center py-8 text-text-secondary">
            <Search class="w-12 h-12 mb-3 opacity-50" />
            <p class="text-sm">No results found</p>
            <p class="text-xs mt-1">Try different keywords</p>
          </div>
        </Show>

        {/* Hint State */}
        <Show when={searchState.query.length < 2}>
          <div class="flex flex-col items-center justify-center py-8 text-text-secondary">
            <Search class="w-12 h-12 mb-3 opacity-50" />
            <p class="text-sm">Type at least 2 characters to search</p>
          </div>
        </Show>

        {/* Error State */}
        <Show when={searchState.error}>
          <div class="p-4 text-center text-red-400 text-sm">
            {searchState.error}
          </div>
        </Show>

        {/* Results List */}
        <Show when={searchState.results.length > 0}>
          <div
            style={{
              height: `${virtualizer.getTotalSize()}px`,
              position: "relative",
            }}
          >
            <For each={virtualizer.getVirtualItems()}>
              {(virtualItem) => {
                const result = () => searchState.results[virtualItem.index];
                return (
                  <div
                    data-index={virtualItem.index}
                    ref={(el) => virtualizer.measureElement(el)}
                    style={{
                      position: "absolute",
                      top: `${virtualItem.start}px`,
                      width: "100%",
                    }}
                  >
                    <Show when={result()}>
                      {(r) => (
                        <button
                          onClick={() => handleResultClick(r())}
                          class="w-full p-3 text-left hover:bg-white/5 transition-colors border-b border-white/5"
                        >
                          {/* Source Badge (global mode only) */}
                          <Show when={mode() === "global" && "source" in r()}>
                            {(() => {
                              const globalResult = r() as GlobalSearchResult;
                              return (
                                <div class="flex items-center gap-1 text-xs text-text-secondary mb-1">
                                  <Show
                                    when={globalResult.source.type === "guild"}
                                    fallback={
                                      <>
                                        <MessageSquare class="w-3 h-3" />
                                        <span>Direct Messages</span>
                                      </>
                                    }
                                  >
                                    <Globe class="w-3 h-3" />
                                    <span>
                                      {globalResult.source.guild_name}
                                    </span>
                                  </Show>
                                  <span class="mx-0.5">&gt;</span>
                                  <Hash class="w-3 h-3" />
                                  <span>{r().channel_name}</span>
                                </div>
                              );
                            })()}
                          </Show>

                          {/* Channel Name (non-global mode) */}
                          <Show when={mode() !== "global"}>
                            <div class="flex items-center gap-1 text-xs text-text-secondary mb-1">
                              <Hash class="w-3 h-3" />
                              <span>{r().channel_name}</span>
                            </div>
                          </Show>

                          {/* Author and Time */}
                          <div class="flex items-center gap-2 mb-1">
                            <Avatar
                              src={r().author.avatar_url}
                              alt={r().author.display_name}
                              size="sm"
                            />
                            <span class="text-sm font-medium text-text-primary">
                              {r().author.display_name}
                            </span>
                            <span class="text-xs text-text-secondary">
                              {formatTimestamp(r().created_at)}
                            </span>
                          </div>

                          {/* Content Preview with server-side highlighting */}
                          <p
                            class="text-sm text-text-secondary line-clamp-2 [&_mark]:bg-accent-primary/30 [&_mark]:text-text-primary [&_mark]:rounded [&_mark]:px-0.5"
                            innerHTML={sanitizeHeadline(r().headline)}
                          />
                        </button>
                      )}
                    </Show>
                  </div>
                );
              }}
            </For>
          </div>

          {/* Load More Button — sits after the virtualizer container in normal flow.
              New results append below existing ones; scroll position is preserved. */}
          <Show when={hasMore()}>
            <div class="p-3 text-center">
              <button
                onClick={loadMore}
                disabled={searchState.isSearching}
                class="px-4 py-2 text-sm text-accent-primary hover:bg-white/5 rounded-lg disabled:opacity-50"
              >
                <Show when={searchState.isSearching} fallback="Load more">
                  <Loader2 class="w-4 h-4 animate-spin inline mr-2" />
                  Loading...
                </Show>
              </button>
            </div>
          </Show>
        </Show>
      </div>
    </div>
  );
};

export default SearchPanel;
