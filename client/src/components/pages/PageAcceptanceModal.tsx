/**
 * Page Acceptance Modal Component
 *
 * Modal for accepting pages with scroll-to-bottom requirement.
 */

import { Show, createSignal, onMount } from "solid-js";
import { X, Check, ChevronDown } from "lucide-solid";
import type { Page } from "@/lib/types";
import { SCROLL_TOLERANCE } from "@/lib/pageConstants";
import MarkdownPreview from "./MarkdownPreview";

interface PageAcceptanceModalProps {
  page: Page;
  isBlocking?: boolean;
  onAccept: () => Promise<void>;
  onDefer?: () => void;
  onClose?: () => void;
}

export default function PageAcceptanceModal(props: PageAcceptanceModalProps) {
  const [hasScrolledToBottom, setHasScrolledToBottom] = createSignal(false);
  const [isAccepting, setIsAccepting] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  let contentRef: HTMLDivElement | undefined;

  // Check if content is scrollable and track scroll position
  const checkScroll = () => {
    if (!contentRef) return;

    const { scrollTop, scrollHeight, clientHeight } = contentRef;
    // Consider "bottom" reached if within SCROLL_TOLERANCE of the end
    const isAtBottom = scrollTop + clientHeight >= scrollHeight - SCROLL_TOLERANCE;

    if (isAtBottom) {
      setHasScrolledToBottom(true);
    }
  };

  // Check on mount if content doesn't need scrolling.
  // Delayed re-check accounts for async markdown rendering changing scroll height.
  onMount(() => {
    const doCheck = () => {
      if (!contentRef) return;
      const { scrollHeight, clientHeight } = contentRef;
      if (scrollHeight <= clientHeight) {
        setHasScrolledToBottom(true);
      }
    };

    doCheck();
    // Re-check after markdown finishes rendering
    setTimeout(doCheck, 500);
  });

  const handleAccept = async () => {
    if (!hasScrolledToBottom()) return;

    setIsAccepting(true);
    setError(null);

    try {
      await props.onAccept();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to accept page");
    } finally {
      setIsAccepting(false);
    }
  };

  const handleDefer = () => {
    if (props.isBlocking) return;
    props.onDefer?.();
  };

  const canAccept = () => hasScrolledToBottom() && !isAccepting();

  return (
    <div class="fixed inset-0 z-50 flex items-center justify-center p-4" role="dialog" aria-modal="true">
      {/* Backdrop */}
      <div
        class="absolute inset-0 bg-black/70"
        onClick={props.isBlocking ? undefined : props.onClose}
      />

      {/* Modal */}
      <div class="relative w-full max-w-2xl max-h-[90vh] bg-zinc-800 rounded-lg shadow-xl flex flex-col">
        {/* Header */}
        <div class="flex items-center justify-between px-6 py-4 border-b border-zinc-700">
          <div>
            <h2 class="text-xl font-semibold text-white">{props.page.title}</h2>
            <p class="text-sm text-zinc-400 mt-1">
              {props.isBlocking
                ? "You must accept this to continue using the platform"
                : "Please review and accept this page"}
            </p>
          </div>
          <Show when={!props.isBlocking && props.onClose}>
            <button
              type="button"
              onClick={props.onClose}
              class="p-2 text-zinc-400 hover:text-white hover:bg-zinc-700 rounded transition-colors"
            >
              <X class="w-5 h-5" />
            </button>
          </Show>
        </div>

        {/* Content */}
        <div
          ref={contentRef}
          onScroll={checkScroll}
          class="flex-1 overflow-auto px-6 py-4 min-h-0"
        >
          <MarkdownPreview content={props.page.content} />
        </div>

        {/* Scroll indicator */}
        <Show when={!hasScrolledToBottom()}>
          <div class="flex items-center justify-center py-2 bg-zinc-700/50 text-zinc-400 text-sm">
            <ChevronDown class="w-4 h-4 mr-1 animate-bounce" />
            Scroll to bottom to enable acceptance
          </div>
        </Show>

        {/* Error */}
        <Show when={error()}>
          <div class="mx-6 mb-2 px-3 py-2 bg-red-900/30 border border-red-700 rounded text-sm text-red-300">
            {error()}
          </div>
        </Show>

        {/* Footer */}
        <div class="flex items-center justify-end gap-3 px-6 py-4 border-t border-zinc-700">
          <Show when={!props.isBlocking && props.onDefer}>
            <button
              type="button"
              onClick={handleDefer}
              class="px-4 py-2 text-sm font-medium text-zinc-300 hover:text-white hover:bg-zinc-700 rounded-md transition-colors"
            >
              Remind Me Later
            </button>
          </Show>
          <button
            type="button"
            onClick={handleAccept}
            disabled={!canAccept()}
            class={`px-6 py-2 text-sm font-medium rounded-md flex items-center gap-2 transition-colors ${
              canAccept()
                ? "bg-emerald-600 hover:bg-emerald-500 text-white"
                : "bg-zinc-600 text-zinc-400 cursor-not-allowed"
            }`}
          >
            <Check class="w-4 h-4" />
            {isAccepting() ? "Accepting..." : "I Accept"}
          </button>
        </div>
      </div>
    </div>
  );
}
