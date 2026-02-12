/**
 * Page View Component
 *
 * Full page display with markdown content.
 */

import { Show, createSignal } from "solid-js";
import { ArrowLeft, Edit, Trash2, Clock } from "lucide-solid";
import type { Page } from "@/lib/types";
import MarkdownPreview from "./MarkdownPreview";

interface PageViewProps {
  page: Page;
  canEdit?: boolean;
  onEdit?: () => void;
  onDelete?: () => void;
  onBack?: () => void;
}

/**
 * Format a date string for display.
 */
function formatDate(dateStr: string): string {
  const date = new Date(dateStr);
  return date.toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export default function PageView(props: PageViewProps) {
  const [isDeleting, setIsDeleting] = createSignal(false);
  const [deleteError, setDeleteError] = createSignal<string | null>(null);

  const handleDelete = async () => {
    if (!confirm(`Are you sure you want to delete "${props.page.title}"?`)) {
      return;
    }

    setIsDeleting(true);
    setDeleteError(null);
    try {
      await props.onDelete?.();
    } catch (err) {
      console.error("Failed to delete page:", err);
      setDeleteError(err instanceof Error ? err.message : "Failed to delete page");
    } finally {
      setIsDeleting(false);
    }
  };

  return (
    <div class="flex flex-col h-full bg-zinc-900">
      {/* Header */}
      <div class="flex items-center justify-between px-6 py-4 border-b border-zinc-700">
        <div class="flex items-center gap-4">
          <Show when={props.onBack}>
            <button
              type="button"
              onClick={props.onBack}
              class="p-2 text-zinc-400 hover:text-white hover:bg-zinc-700 rounded transition-colors"
              title="Go back"
            >
              <ArrowLeft class="w-5 h-5" />
            </button>
          </Show>
          <div>
            <h1 class="text-2xl font-bold text-white">{props.page.title}</h1>
            <div class="flex items-center gap-4 mt-1 text-sm text-zinc-400">
              <span class="flex items-center gap-1">
                <Clock class="w-4 h-4" />
                Updated {formatDate(props.page.updated_at)}
              </span>
              <Show when={props.page.requires_acceptance}>
                <span class="px-2 py-0.5 bg-amber-900/40 text-amber-400 rounded text-xs font-medium">
                  Requires Acceptance
                </span>
              </Show>
            </div>
          </div>
        </div>

        <Show when={props.canEdit}>
          <div class="flex items-center gap-2">
            <button
              type="button"
              onClick={props.onEdit}
              class="px-3 py-1.5 text-sm font-medium text-zinc-300 hover:text-white hover:bg-zinc-700 rounded-md flex items-center gap-2 transition-colors"
            >
              <Edit class="w-4 h-4" />
              Edit
            </button>
            <button
              type="button"
              onClick={handleDelete}
              disabled={isDeleting()}
              class="px-3 py-1.5 text-sm font-medium text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded-md flex items-center gap-2 transition-colors disabled:opacity-50"
            >
              <Trash2 class="w-4 h-4" />
              {isDeleting() ? "Deleting..." : "Delete"}
            </button>
          </div>
        </Show>
      </div>

      {/* Delete error */}
      <Show when={deleteError()}>
        <div class="mx-6 mt-2 px-3 py-2 bg-red-900/30 border border-red-700 rounded text-sm text-red-300">
          {deleteError()}
        </div>
      </Show>

      {/* Content */}
      <div class="flex-1 overflow-auto">
        <div class="max-w-4xl mx-auto px-6 py-8">
          <MarkdownPreview content={props.page.content} />
        </div>
      </div>
    </div>
  );
}
