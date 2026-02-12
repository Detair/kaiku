/**
 * Page Section Component
 *
 * Collapsible sidebar section for pages with admin controls.
 */

import { Show, For, createSignal, createEffect } from "solid-js";
import { ChevronDown, ChevronRight, Plus, FileText } from "lucide-solid";
import type { PageListItem } from "@/lib/types";
import PageItem from "./PageItem";

interface PageSectionProps {
  title: string;
  pages: PageListItem[];
  pendingPageIds?: Set<string>;
  selectedPageId?: string | null;
  canManage?: boolean;
  isExpanded?: boolean;
  onToggle?: () => void;
  onSelectPage?: (page: PageListItem) => void;
  onCreatePage?: () => void;
}

export default function PageSection(props: PageSectionProps) {
  const [isExpanded, setIsExpanded] = createSignal(props.isExpanded ?? true);

  // Update internal state when prop changes
  createEffect(() => {
    if (props.isExpanded !== undefined) {
      setIsExpanded(props.isExpanded);
    }
  });

  const handleToggle = () => {
    const newState = !isExpanded();
    setIsExpanded(newState);
    props.onToggle?.();
  };

  const hasPendingPages = () => {
    if (!props.pendingPageIds || props.pendingPageIds.size === 0) return false;
    return props.pages.some((p) => props.pendingPageIds!.has(p.id));
  };

  const pendingCount = () => {
    if (!props.pendingPageIds) return 0;
    return props.pages.filter((p) => props.pendingPageIds!.has(p.id)).length;
  };

  return (
    <Show when={props.pages.length > 0 || props.canManage}>
      <div class="mb-2">
        {/* Section Header */}
        <button
          type="button"
          onClick={handleToggle}
          class="w-full flex items-center gap-2 px-3 py-2 text-xs font-semibold text-zinc-400 uppercase tracking-wide hover:text-zinc-200 transition-colors"
        >
          <Show when={isExpanded()} fallback={<ChevronRight class="w-4 h-4" />}>
            <ChevronDown class="w-4 h-4" />
          </Show>

          <FileText class="w-4 h-4" />

          <span class="flex-1 text-left">{props.title}</span>

          <Show when={hasPendingPages()}>
            <span class="px-1.5 py-0.5 bg-amber-900/40 text-amber-400 rounded text-xs font-medium">
              {pendingCount()}
            </span>
          </Show>

          <Show when={props.canManage}>
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                props.onCreatePage?.();
              }}
              class="p-1 text-zinc-500 hover:text-white hover:bg-zinc-700 rounded transition-colors"
              title="Add page"
            >
              <Plus class="w-3.5 h-3.5" />
            </button>
          </Show>
        </button>

        {/* Page List */}
        <Show when={isExpanded()}>
          <div class="ml-2 space-y-0.5">
            <Show
              when={props.pages.length > 0}
              fallback={
                <Show when={props.canManage}>
                  <div class="px-3 py-2 text-sm text-zinc-500 italic">
                    No pages yet
                  </div>
                </Show>
              }
            >
              <For each={props.pages}>
                {(page) => (
                  <PageItem
                    page={page}
                    isSelected={props.selectedPageId === page.id}
                    isPending={props.pendingPageIds?.has(page.id)}
                    isDraggable={props.canManage}
                    onClick={() => props.onSelectPage?.(page)}
                  />
                )}
              </For>
            </Show>
          </div>
        </Show>
      </div>
    </Show>
  );
}
