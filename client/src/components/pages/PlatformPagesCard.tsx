/**
 * Platform Pages Card Component
 *
 * Card for Home view showing platform pages with "Action Required" badge
 * for pages pending acceptance.
 */

import { Show, For, onMount } from "solid-js";
import { FileText, AlertCircle, ChevronRight } from "lucide-solid";
import type { PageListItem } from "@/lib/types";
import {
  pagesState,
  loadPlatformPages,
  loadPendingAcceptance,
} from "@/stores/pages";

interface PlatformPagesCardProps {
  /** Called when a page is selected */
  onSelectPage?: (page: PageListItem) => void;
}

export default function PlatformPagesCard(props: PlatformPagesCardProps) {
  onMount(async () => {
    await Promise.all([loadPlatformPages(), loadPendingAcceptance()]);
  });

  const pendingIds = () => new Set(pagesState.pendingAcceptance.map((p) => p.id));

  const hasPendingPages = () => {
    return pagesState.platformPages.some((p) => pendingIds().has(p.id));
  };

  const pendingCount = () => {
    return pagesState.platformPages.filter((p) => pendingIds().has(p.id)).length;
  };

  return (
    <Show when={pagesState.platformPages.length > 0 || pagesState.isPlatformLoading || pagesState.error}>
      <div class="mx-2 mb-2">
        <div class="bg-zinc-800/50 rounded-lg overflow-hidden">
          {/* Header */}
          <div class="flex items-center gap-2 px-3 py-2 border-b border-zinc-700/50">
            <FileText class="w-4 h-4 text-zinc-400" />
            <span class="text-xs font-semibold text-zinc-400 uppercase tracking-wide flex-1">
              Platform Info
            </span>
            <Show when={hasPendingPages()}>
              <span class="flex items-center gap-1 px-1.5 py-0.5 bg-amber-900/40 text-amber-400 rounded text-xs font-medium">
                <AlertCircle class="w-3 h-3" />
                {pendingCount()}
              </span>
            </Show>
          </div>

          {/* Error state */}
          <Show when={pagesState.error && !pagesState.isPlatformLoading && pagesState.platformPages.length === 0}>
            <div class="px-3 py-4 text-center">
              <span class="text-red-400 text-sm">Failed to load pages</span>
            </div>
          </Show>

          {/* Content */}
          <Show
            when={!pagesState.isPlatformLoading}
            fallback={
              <div class="px-3 py-4 text-center">
                <span class="text-zinc-500 text-sm">Loading...</span>
              </div>
            }
          >
            <div class="py-1">
              <For each={pagesState.platformPages}>
                {(page) => {
                  const isPending = () => pendingIds().has(page.id);

                  return (
                    <button
                      type="button"
                      onClick={() => props.onSelectPage?.(page)}
                      class="w-full flex items-center gap-2 px-3 py-2 text-left hover:bg-white/5 transition-colors group"
                    >
                      <span
                        class="flex-1 text-sm truncate"
                        classList={{
                          "text-zinc-300": !isPending(),
                          "text-amber-300 font-medium": isPending(),
                        }}
                      >
                        {page.title}
                      </span>
                      <Show when={isPending()}>
                        <span class="px-1.5 py-0.5 bg-amber-900/30 text-amber-400 rounded text-xs">
                          Action Required
                        </span>
                      </Show>
                      <ChevronRight class="w-4 h-4 text-zinc-500 opacity-0 group-hover:opacity-100 transition-opacity" />
                    </button>
                  );
                }}
              </For>
            </div>
          </Show>
        </div>
      </div>
    </Show>
  );
}
