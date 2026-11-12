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
  createEffect,
  onCleanup,
  createMemo,
  untrack,
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
import { getActiveGuild, guildsState } from "@/stores/guilds";
import { dmsState } from "@/stores/dms";
import { plural, t } from "@/i18n";
import type {
  SearchFilters,
  SearchResult,
  GlobalSearchResult,
} from "@/lib/types";
import Avatar from "@/components/ui/Avatar";
import { formatTimestamp } from "@/lib/utils";
import flokiThinking from "@/assets/emotes/floki_emote_2.webp";
import DOMPurify from "dompurify";
import SearchSyntaxHelp from "./SearchSyntaxHelp";
import { createVirtualizer } from "@/lib/virtualizer";

interface SearchPanelProps {
  onClose: () => void;
  mode?: "guild" | "dm" | "global";
  initialScope?: "channel" | "guild" | "all";
  channelId?: string;
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
  const [scope, setScope] = createSignal<"channel" | "guild" | "all">(
    props.initialScope ?? (props.mode === "global" ? "all" : "guild"),
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
    const currentScope = scope();

    if (currentScope === "channel" && props.channelId) {
      searchGlobal(value, filters, props.channelId);
    } else if (currentScope === "all") {
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

  // Re-trigger search when scope changes (if there's already a query).
  // Only scope() is tracked; inputValue is read via untrack to avoid
  // double-firing on every keystroke.
  createEffect(() => {
    scope(); // track scope changes
    untrack(() => {
      if (inputValue().trim().length >= 2) {
        triggerSearch();
      }
    });
  });

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
    if (mode() === "dm") return t("chat.search.placeholder.dm");
    switch (scope()) {
      case "channel":
        return t("chat.search.placeholder.channel");
      case "all":
        return t("chat.search.placeholder.all");
      default:
        return t("chat.search.placeholder.server");
    }
  };

  /**
   * People to filter by: members of the open server, or the people in your
   * direct messages. Nobody has to know an internal user id.
   */
  const authorOptions = createMemo(() => {
    const byId = new Map<string, string>();
    const guild = mode() === "dm" ? undefined : getActiveGuild();
    if (guild) {
      for (const m of guildsState.members[guild.id] ?? []) {
        byId.set(m.user_id, m.nickname || m.display_name || m.username);
      }
    }
    if (mode() !== "guild" || !guild) {
      for (const dm of dmsState.dms) {
        for (const p of dm.participants) {
          if (!p.is_system && !byId.has(p.user_id)) byId.set(p.user_id, p.display_name || p.username);
        }
      }
    }
    return [...byId.entries()]
      .map(([id, name]) => ({ id, name }))
      .sort((a, b) => a.name.localeCompare(b.name));
  });

  return (
    <div
      class="absolute inset-0 z-50 flex flex-col bg-surface-layer2"
      onKeyDown={(e) => {
        // The panel over the sidebar closes with Escape too (A11Y-1); in the
        // global search dialog the dialog handles Escape first.
        if (e.key === "Escape" && !e.defaultPrevented) {
          e.preventDefault();
          props.onClose();
        }
      }}
    >
      {/* Search Header */}
      <div class="flex items-center justify-between px-3 py-2 border-b border-white/10">
        <div class="relative flex-1 max-w-md">
          <Search class="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-text-secondary" />
          <input
            data-testid="search-input"
            type="text"
            placeholder={placeholderText()}
            aria-label={placeholderText()}
            value={inputValue()}
            onInput={handleInput}
            autofocus
            class="w-full pl-8 pr-3 py-1.5 rounded-md text-sm text-text-primary placeholder:text-text-placeholder bg-surface-layer1 border border-white/10 outline-none focus:ring-1 focus:ring-accent-primary/30"
          />
        </div>
        <button
          onClick={() => setShowFilters(!showFilters())}
          class="ml-2 p-1.5 rounded transition-colors"
          classList={{
            "text-text-primary bg-accent-primary/20": showFilters(),
            "text-text-secondary hover:text-text-primary": !showFilters(),
          }}
          title={t("chat.search.filters")}
          aria-label={t("chat.search.filters")}
          aria-pressed={showFilters()}
        >
          <Filter class="w-4 h-4" />
        </button>
        <SearchSyntaxHelp />
        <button
          onClick={() => props.onClose()}
          class="ml-1 p-1.5 text-text-secondary hover:text-text-primary rounded transition-colors"
          title={t("chat.search.close")}
          aria-label={t("chat.search.close")}
        >
          <X class="w-4 h-4" />
        </button>
      </div>

      {/* Scope Selector */}
      <Show when={mode() !== "dm"}>
        <div class="flex gap-1 px-3 py-1.5 border-b border-white/10">
          <button
            data-testid="scope-channel"
            onClick={() => setScope("channel")}
            aria-pressed={scope() === "channel"}
            disabled={!props.channelId}
            class="px-2.5 py-1 rounded text-xs font-medium transition-colors state-disabled-quiet"
            classList={{
              "bg-accent-primary/30 text-text-primary ring-1 ring-accent-primary": scope() === "channel",
              "bg-surface-layer1 text-text-secondary hover:text-text-primary":
                scope() !== "channel",
            }}
          >
            {t("chat.search.scope.channel")}
          </button>
          <button
            data-testid="scope-guild"
            onClick={() => setScope("guild")}
            aria-pressed={scope() === "guild"}
            class="px-2.5 py-1 rounded text-xs font-medium transition-colors"
            classList={{
              "bg-accent-primary/30 text-text-primary ring-1 ring-accent-primary": scope() === "guild",
              "bg-surface-layer1 text-text-secondary hover:text-text-primary":
                scope() !== "guild",
            }}
          >
            {t("chat.search.scope.server")}
          </button>
          <button
            data-testid="scope-all"
            onClick={() => setScope("all")}
            aria-pressed={scope() === "all"}
            class="px-2.5 py-1 rounded text-xs font-medium transition-colors"
            classList={{
              "bg-accent-primary/30 text-text-primary ring-1 ring-accent-primary": scope() === "all",
              "bg-surface-layer1 text-text-secondary hover:text-text-primary":
                scope() !== "all",
            }}
          >
            {t("chat.search.scope.all")}
          </button>
        </div>
      </Show>

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
                "bg-accent-primary/20 text-text-primary":
                  sortOrder() === "relevance",
                "bg-surface-layer1 text-text-secondary hover:text-text-primary":
                  sortOrder() !== "relevance",
              }}
            >
              {t("chat.search.sort.relevance")}
            </button>
            <button
              onClick={() => {
                setSortOrder("date");
                triggerSearch();
              }}
              class="px-2 py-1 rounded text-xs transition-colors"
              classList={{
                "bg-accent-primary/20 text-text-primary":
                  sortOrder() === "date",
                "bg-surface-layer1 text-text-secondary hover:text-text-primary":
                  sortOrder() !== "date",
              }}
            >
              {t("chat.search.sort.date")}
            </button>
          </div>
          <div class="flex gap-2">
            <div class="flex-1">
              <label class="text-xs text-text-secondary block mb-1" for="search-date-from">
                {t("chat.search.dateFrom")}
              </label>
              <input
                id="search-date-from"
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
              <label class="text-xs text-text-secondary block mb-1" for="search-date-to">
                {t("chat.search.dateTo")}
              </label>
              <input
                id="search-date-to"
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
          <Show when={authorOptions().length > 0}>
            <div>
              <label class="text-xs text-text-secondary block mb-1" for="search-author">
                {t("chat.search.author")}
              </label>
              <select
                id="search-author"
                data-testid="search-author"
                value={authorFilter()}
                onChange={(e) => {
                  setAuthorFilter(e.currentTarget.value);
                  triggerSearch();
                }}
                class="w-full px-2 py-1 rounded text-xs text-text-primary bg-surface-layer1 border border-white/10 outline-none"
              >
                <option value="">{t("chat.search.authorAnyone")}</option>
                <For each={authorOptions()}>
                  {(option) => <option value={option.id}>{option.name}</option>}
                </For>
              </select>
            </div>
          </Show>
          <div class="flex gap-2">
            <button
              onClick={() => {
                setHasFilter(hasFilter() === "link" ? "" : "link");
                triggerSearch();
              }}
              class="flex items-center gap-1 px-2 py-1 rounded text-xs transition-colors"
              classList={{
                "bg-accent-primary/20 text-text-primary":
                  hasFilter() === "link",
                "bg-surface-layer1 text-text-secondary hover:text-text-primary":
                  hasFilter() !== "link",
              }}
            >
              <Link class="w-3 h-3" />
              {t("chat.search.hasLink")}
            </button>
            <button
              onClick={() => {
                setHasFilter(hasFilter() === "file" ? "" : "file");
                triggerSearch();
              }}
              class="flex items-center gap-1 px-2 py-1 rounded text-xs transition-colors"
              classList={{
                "bg-accent-primary/20 text-text-primary":
                  hasFilter() === "file",
                "bg-surface-layer1 text-text-secondary hover:text-text-primary":
                  hasFilter() !== "file",
              }}
            >
              <Paperclip class="w-3 h-3" />
              {t("chat.search.hasFile")}
            </button>
          </div>
        </div>
      </Show>

      <Show when={searchState.total > 0}>
        <span class="ml-3 text-xs text-text-secondary">
          {plural(searchState.total, {
            one: "chat.search.results.one",
            other: "chat.search.results.other",
          })}
        </span>
      </Show>

      {/* Results */}
      <div ref={resultsContainerRef} data-testid="search-results" class="flex-1 overflow-y-auto">
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
            <img src={flokiThinking} alt={""} class="w-14 h-14 object-contain mb-3" loading="lazy" />
            <p class="text-sm">{t("chat.search.noResults")}</p>
            <p class="text-xs mt-1">{t("chat.search.noResultsHint")}</p>
          </div>
        </Show>

        {/* Hint State */}
        <Show when={searchState.query.length < 2}>
          <div class="flex flex-col items-center justify-center py-8 text-text-secondary">
            <Search class="w-12 h-12 mb-3 opacity-50" />
            <p class="text-sm">{t("chat.search.minLength")}</p>
          </div>
        </Show>

        {/* Error State */}
        <Show when={searchState.error}>
          <div role="alert" class="p-4 text-center text-error-text text-sm">
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
                    ref={(el) => {
                      el.setAttribute("data-index", String(virtualItem.index));
                      queueMicrotask(() => virtualizer.measureElement(el));
                    }}
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
                                        <span>{t("chat.search.directMessages")}</span>
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
                            // eslint-disable-next-line solid/no-innerhtml -- sanitizeHeadline() runs DOMPurify with ALLOWED_TAGS: ["mark"] only; defined above
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
                class="px-4 py-2 text-sm text-accent-primary hover:bg-white/5 rounded-lg state-disabled-quiet"
              >
                <Show when={searchState.isSearching} fallback={t("chat.search.loadMore")}>
                  <Loader2 class="w-4 h-4 animate-spin inline mr-2" />
                  {t("chat.search.loading")}
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
