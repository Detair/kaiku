/**
 * Page Acceptance Modal Component
 *
 * Modal for accepting pages with scroll-to-bottom requirement.
 */

import { Show, createSignal, createUniqueId, onMount } from "solid-js";
import { X, Check, ChevronDown } from "lucide-solid";
import type { Page } from "@/lib/types";
import { SCROLL_TOLERANCE } from "@/lib/pageConstants";
import MarkdownPreview from "./MarkdownPreview";
import Modal from "@/components/ui/Modal";
import Button from "@/components/ui/Button";
import { t } from "@/i18n";
import { errorText } from "@/i18n/errors";

interface PageAcceptanceModalProps {
  page: Page;
  isBlocking?: boolean;
  onAccept: () => Promise<void>;
  onDefer?: () => void;
  onClose?: () => void;
}

export default function PageAcceptanceModal(props: PageAcceptanceModalProps) {
  const titleId = createUniqueId();
  // Escape and the backdrop close it only when it may be closed.
  const close = () => {
    if (!props.isBlocking) props.onClose?.();
  };

  const [hasScrolledToBottom, setHasScrolledToBottom] = createSignal(false);
  const [isAccepting, setIsAccepting] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  let contentRef: HTMLDivElement | undefined;

  // Check if content is scrollable and track scroll position
  const checkScroll = () => {
    if (!contentRef) return;

    const { scrollTop, scrollHeight, clientHeight } = contentRef;
    // Consider "bottom" reached if within SCROLL_TOLERANCE of the end
    const isAtBottom =
      scrollTop + clientHeight >= scrollHeight - SCROLL_TOLERANCE;

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
      setError(errorText(err, "pages.accept.failed"));
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
    <Modal
      onClose={close}
      labelledBy={titleId}
      closeOnBackdrop={!props.isBlocking && !!props.onClose}
      backdropClass="fixed inset-0 flex items-center justify-center p-4 bg-black/70"
      class="relative w-full max-w-2xl max-h-[90vh] bg-surface-layer1 rounded-lg shadow-xl flex flex-col"
      testId="page-acceptance-modal"
    >
        {/* Header */}
        <div class="flex items-center justify-between px-6 py-4 border-b border-white/10">
          <div>
            <h2 id={titleId} class="text-xl font-semibold text-text-primary">{props.page.title}</h2>
            <p class="text-sm text-text-secondary mt-1">
              {props.isBlocking ? t("pages.accept.required") : t("pages.accept.review")}
            </p>
          </div>
          <Show when={!props.isBlocking && props.onClose}>
            <button
              type="button"
              onClick={close}
              aria-label={t("common.close")}
              class="p-2 tap-target text-text-secondary hover:text-text-primary hover:bg-white/10 rounded transition-colors"
            >
              <X class="w-5 h-5" aria-hidden="true" />
            </button>
          </Show>
        </div>

        {/* Content */}
        <div
          ref={contentRef}
          onScroll={checkScroll}
          data-testid="page-acceptance-content"
          // Keyboard users scroll the text with the arrow keys to reach the end.
          tabIndex={0}
          role="region"
          aria-label={props.page.title}
          class="flex-1 overflow-auto px-6 py-4 min-h-0"
        >
          <MarkdownPreview content={props.page.content} />
        </div>

        {/* Scroll indicator */}
        <Show when={!hasScrolledToBottom()}>
          <div class="flex items-center justify-center py-2 bg-surface-layer2/50 text-text-secondary text-sm">
            <ChevronDown class="w-4 h-4 mr-1 animate-bounce" aria-hidden="true" />
            {t("pages.accept.scroll")}
          </div>
        </Show>

        {/* Error */}
        <Show when={error()}>
          <div role="alert" class="mx-6 mb-2 px-3 py-2 bg-status-error/15 border border-status-error/50 rounded text-sm text-text-primary">
            {error()}
          </div>
        </Show>

        {/* Footer */}
        <div class="flex items-center justify-end gap-3 px-6 py-4 border-t border-white/10">
          <Show when={!props.isBlocking && props.onDefer}>
            <Button variant="ghost" onClick={handleDefer} data-testid="page-acceptance-remind-later">
              {t("pages.accept.later")}
            </Button>
          </Show>
          <Button
            onClick={handleAccept}
            data-testid="page-acceptance-accept"
            disabled={!canAccept()}
            disabledReason={!hasScrolledToBottom() && t("pages.accept.scroll")}
            showReason={false}
          >
            <Check class="w-4 h-4" aria-hidden="true" />
            {isAccepting() ? t("pages.accept.accepting") : t("pages.accept.accept")}
          </Button>
        </div>
    </Modal>
  );
}
