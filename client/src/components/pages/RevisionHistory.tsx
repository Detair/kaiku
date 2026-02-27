/**
 * RevisionHistory - Side panel component displaying the revision history of a page.
 *
 * Shows a chronological list of revisions (newest first) with options to view
 * or restore previous versions. Includes a preview section when a revision is
 * currently being viewed.
 */

import { For, Show } from "solid-js";
import { Clock, Eye, History, RotateCcw, X } from "lucide-solid";
import type { PageRevision, RevisionListItem } from "@/lib/types";

interface RevisionHistoryProps {
  revisions: RevisionListItem[];
  currentRevision: PageRevision | null;
  canRestore?: boolean;
  onViewRevision: (revisionNumber: number) => void;
  onRestore: (revisionNumber: number) => void;
  onClose: () => void;
}

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

export default function RevisionHistory(props: RevisionHistoryProps) {
  const latestRevisionNumber = () => {
    if (props.revisions.length === 0) return -1;
    return Math.max(...props.revisions.map((r) => r.revision_number));
  };

  const isViewing = (revisionNumber: number) =>
    props.currentRevision?.revision_number === revisionNumber;

  const handleRestore = (revisionNumber: number) => {
    const confirmed = confirm(
      `Are you sure you want to restore revision #${revisionNumber}? This will create a new revision with the content from that version.`,
    );
    if (confirmed) {
      props.onRestore(revisionNumber);
    }
  };

  return (
    <div class="flex flex-col h-full bg-zinc-800 border-l border-zinc-700">
      {/* Header */}
      <div class="flex items-center justify-between px-4 py-3 border-b border-zinc-700">
        <div class="flex items-center gap-2 text-zinc-200 font-medium">
          <History class="size-4" />
          <span>Revision History</span>
        </div>
        <button
          type="button"
          onClick={() => props.onClose()}
          class="p-1 text-zinc-400 hover:text-white rounded hover:bg-zinc-700 transition-colors"
          aria-label="Close revision history"
        >
          <X class="size-4" />
        </button>
      </div>

      {/* Revision List */}
      <div class="flex-1 overflow-y-auto">
        <Show
          when={props.revisions.length > 0}
          fallback={
            <div class="flex flex-col items-center justify-center h-full gap-3 text-zinc-500 px-4">
              <History class="size-8" />
              <p class="text-sm text-center">No revisions yet.</p>
            </div>
          }
        >
          <For each={props.revisions}>
            {(revision) => {
              const isLatest = () =>
                revision.revision_number === latestRevisionNumber();
              const viewing = () => isViewing(revision.revision_number);

              return (
                <div
                  class={`px-4 py-3 border-b border-zinc-700/50 transition-colors ${
                    viewing() ? "bg-zinc-700/50" : "hover:bg-zinc-700/30"
                  }`}
                >
                  {/* Revision header line */}
                  <div class="flex items-center justify-between mb-1">
                    <div class="flex items-center gap-2">
                      <span class="text-sm font-medium text-zinc-200">
                        #{revision.revision_number}
                      </span>
                      <Show when={isLatest()}>
                        <span class="px-2 py-0.5 bg-emerald-900/40 text-emerald-400 rounded text-xs font-medium">
                          Current
                        </span>
                      </Show>
                    </div>
                    <Show when={revision.content_hash}>
                      <span class="font-mono text-xs text-zinc-500 bg-zinc-900 px-1.5 py-0.5 rounded">
                        {revision.content_hash!.slice(0, 8)}
                      </span>
                    </Show>
                  </div>

                  {/* Title if available */}
                  <Show when={revision.title}>
                    <p class="text-sm text-zinc-300 truncate mb-1">
                      {revision.title}
                    </p>
                  </Show>

                  {/* Date and author */}
                  <div class="flex items-center gap-1.5 text-xs text-zinc-500 mb-2">
                    <Clock class="size-3" />
                    <span>{formatDate(revision.created_at)}</span>
                    <Show when={revision.created_by}>
                      <span class="text-zinc-600">by</span>
                      <span
                        class="font-mono text-zinc-400"
                        title={revision.created_by!}
                      >
                        {revision.created_by!.slice(0, 8)}
                      </span>
                    </Show>
                  </div>

                  {/* Actions */}
                  <div class="flex items-center gap-3">
                    <button
                      type="button"
                      onClick={() =>
                        props.onViewRevision(revision.revision_number)
                      }
                      class={`flex items-center gap-1 text-sm transition-colors ${
                        viewing()
                          ? "text-white"
                          : "text-zinc-400 hover:text-white"
                      }`}
                    >
                      <Eye class="size-3.5" />
                      <span>{viewing() ? "Viewing" : "View"}</span>
                    </button>
                    <Show when={props.canRestore && !isLatest()}>
                      <button
                        type="button"
                        onClick={() => handleRestore(revision.revision_number)}
                        class="flex items-center gap-1 text-sm text-amber-400 hover:text-amber-300 transition-colors"
                      >
                        <RotateCcw class="size-3.5" />
                        <span>Restore</span>
                      </button>
                    </Show>
                  </div>
                </div>
              );
            }}
          </For>
        </Show>
      </div>

      {/* Preview section when viewing a revision */}
      <Show when={props.currentRevision}>
        {(revision) => (
          <div class="border-t border-zinc-700 px-4 py-3 bg-zinc-900/50">
            <div class="flex items-center gap-2 mb-2">
              <Eye class="size-3.5 text-zinc-400" />
              <span class="text-xs font-medium text-zinc-400 uppercase tracking-wide">
                Viewing Revision #{revision().revision_number}
              </span>
            </div>
            <Show when={revision().title}>
              <p class="text-sm text-zinc-300 font-medium truncate">
                {revision().title}
              </p>
            </Show>
            <p class="text-xs text-zinc-500 mt-1">
              {formatDate(revision().created_at)}
            </p>
          </div>
        )}
      </Show>
    </div>
  );
}
