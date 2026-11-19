/**
 * Page Item Component
 *
 * Individual page list item for sidebar display.
 */

import { Show } from "solid-js";
import { FileText, AlertCircle, GripVertical } from "lucide-solid";
import type { PageListItem } from "@/lib/types";

interface PageItemProps {
  page: PageListItem;
  isSelected?: boolean;
  isPending?: boolean;
  isDraggable?: boolean;
  onClick?: () => void;
}

export default function PageItem(props: PageItemProps) {
  return (
    <button
      type="button"
      onClick={props.onClick}
      class={`w-full flex items-center gap-2 px-3 py-2 rounded-md text-left transition-colors group ${
        props.isSelected
          ? "bg-surface-layer2 text-text-primary"
          : "text-text-primary hover:bg-white/10 hover:text-text-primary"
      }`}
    >
      <Show when={props.isDraggable}>
        <GripVertical class="w-4 h-4 text-text-secondary opacity-0 group-hover:opacity-100 cursor-grab transition-opacity flex-shrink-0" />
      </Show>

      <FileText class="w-4 h-4 flex-shrink-0 text-text-secondary" />

      <span class="flex-1 truncate text-sm" title={props.page.title}>
        {props.page.title}
      </span>

      <Show when={props.isPending}>
        <span
          class="flex items-center gap-1 px-1.5 py-0.5 bg-status-warning/20 text-text-primary rounded text-xs font-medium"
          title="Action required"
        >
          <AlertCircle class="w-3 h-3" />
        </span>
      </Show>

      <Show when={props.page.requires_acceptance && !props.isPending}>
        <span
          class="w-2 h-2 bg-status-warning rounded-full flex-shrink-0"
          title="Requires acceptance"
        />
      </Show>
    </button>
  );
}
