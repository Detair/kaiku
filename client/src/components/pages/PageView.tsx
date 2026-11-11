/**
 * Page View Component
 *
 * Full page display with markdown content.
 */

import { Show, createSignal } from "solid-js";
import { ArrowLeft, Edit, Trash2, Clock, History } from "lucide-solid";
import type { Page } from "@/lib/types";
import MarkdownPreview from "./MarkdownPreview";
import { createConfirm } from "@/components/admin/confirmHost";
import { actionErrorText } from "@/components/admin/actionErrors";
import { t } from "@/i18n";

interface PageViewProps {
  page: Page;
  canEdit?: boolean;
  onEdit?: () => void;
  onDelete?: () => void;
  onBack?: () => void;
  onViewHistory?: () => void;
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

  const confirmDialog = createConfirm();

  const handleDelete = () =>
    confirmDialog.ask({
      title: t("guildSettings.pages.deleteTitle", { title: props.page.title }),
      message: t("guildSettings.pages.deleteBody", { title: props.page.title }),
      confirmLabel: t("guildSettings.pages.delete"),
      danger: true,
      run: deleteNow,
    });

  const deleteNow = async () => {
    setIsDeleting(true);
    setDeleteError(null);
    try {
      await props.onDelete?.();
    } catch (err) {
      console.error("Failed to delete page:", err);
      setDeleteError(`${t("guildSettings.pages.deleteFailed")} ${actionErrorText(err)}`);
    } finally {
      setIsDeleting(false);
    }
  };

  return (
    <div class="flex flex-col h-full bg-surface-base">
      {/* Header */}
      <div class="flex items-center justify-between px-6 py-4 border-b border-white/10">
        <div class="flex items-center gap-4">
          <Show when={props.onBack}>
            <button
              type="button"
              onClick={props.onBack}
              class="p-2 text-text-secondary hover:text-text-primary hover:bg-white/10 rounded transition-colors"
              title="Go back"
            >
              <ArrowLeft class="w-5 h-5" />
            </button>
          </Show>
          <div>
            <h1 class="text-2xl font-bold text-text-primary">{props.page.title}</h1>
            <div class="flex items-center gap-4 mt-1 text-sm text-text-secondary">
              <span class="flex items-center gap-1">
                <Clock class="w-4 h-4" />
                Updated {formatDate(props.page.updated_at)}
              </span>
              <Show when={props.page.requires_acceptance}>
                <span class="px-2 py-0.5 bg-status-warning/20 text-text-primary rounded text-xs font-medium">
                  Requires Acceptance
                </span>
              </Show>
            </div>
          </div>
        </div>

        <div class="flex items-center gap-2">
          <Show when={props.onViewHistory}>
            <button
              type="button"
              onClick={props.onViewHistory}
              class="px-3 py-1.5 text-sm font-medium text-text-primary hover:text-text-primary hover:bg-white/10 rounded-md flex items-center gap-2 transition-colors"
            >
              <History class="w-4 h-4" />
              History
            </button>
          </Show>
          <Show when={props.canEdit}>
            <button
              type="button"
              onClick={props.onEdit}
              class="px-3 py-1.5 text-sm font-medium text-text-primary hover:text-text-primary hover:bg-white/10 rounded-md flex items-center gap-2 transition-colors"
            >
              <Edit class="w-4 h-4" />
              Edit
            </button>
            <button
              type="button"
              onClick={handleDelete}
              disabled={isDeleting()}
              class="px-3 py-1.5 text-sm font-medium text-text-primary hover:bg-status-error/15 rounded-md flex items-center gap-2 transition-colors state-disabled-quiet"
            >
              <Trash2 class="w-4 h-4" />
              {isDeleting() ? "Deleting..." : "Delete"}
            </button>
          </Show>
        </div>
      </div>

      {/* Delete error */}
      <Show when={deleteError()}>
        <div role="alert" class="mx-6 mt-2 px-3 py-2 bg-status-error/15 border border-status-error/50 rounded text-sm text-text-primary">
          {deleteError()}
        </div>
      </Show>
      <confirmDialog.Host />

      {/* Content */}
      <div class="flex-1 overflow-auto">
        <div class="max-w-4xl mx-auto px-6 py-8">
          <MarkdownPreview content={props.page.content} />
        </div>
      </div>
    </div>
  );
}
