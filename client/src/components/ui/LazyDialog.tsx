/**
 * Mount point for a window that opens on demand and so loads on first open:
 * its code stays out of the startup payload (bundle budget). Define the
 * window with `lazyWindow`, so a failed download is told apart reliably.
 *
 * - While the code downloads, nothing shows at first; after a moment a
 *   polite "Loading…" appears (one for all windows loading at once), so a
 *   slow connection does not look like a click that did nothing (and invite
 *   a second click that closes a toggle).
 * - If the code cannot be loaded, a small dialog says so with Try again
 *   (loads it again) and Reload.
 * - If the window loaded but failed while rendering, it says that instead,
 *   with Try again.
 * Close (and Escape) call `onClose`, the window's own close, so opening the
 * window again tries again rather than a click that silently does nothing.
 * Without `onClose` (startup wizards the app cannot go on without) there is
 * no Close and Escape does nothing.
 */

import { Component, ParentProps, Show, Suspense, createSignal, onCleanup } from "solid-js";
import { Portal } from "solid-js/web";
import { t } from "@/i18n";
import { createDialog } from "@/lib/a11y/dialog";
import { createModalLayer, MODAL_LAYER } from "@/lib/a11y/modalLayer";
import { isChunkLoadError } from "@/lib/chunkLoadError";
import { LazyErrorBoundary } from "./LazyFallback";

/** How long a download may take before "Loading…" shows (no flicker below). */
export const LOADING_DELAY_MS = 350;

// Windows still downloading, oldest first, and how many of them have taken
// longer than the delay. One status region serves them all.
const [pending, setPending] = createSignal<symbol[]>([]);
const [late, setLate] = createSignal(0);

/**
 * The live region is there from the start, empty, so screen readers announce
 * the text when it appears. On `body`, so a transformed drawer cannot clip it.
 */
const DelayedLoading: Component = () => {
  const id = Symbol();
  let counted = false;
  setPending((p) => [...p, id]);
  const timer = setTimeout(() => {
    counted = true;
    setLate((n) => n + 1);
  }, LOADING_DELAY_MS);
  onCleanup(() => {
    clearTimeout(timer);
    setPending((p) => p.filter((x) => x !== id));
    if (counted) setLate((n) => n - 1);
  });
  return (
    <Show when={pending()[0] === id}>
      <Portal>
        <div
          role="status"
          aria-live="polite"
          class="fixed bottom-4 left-1/2 -translate-x-1/2 pointer-events-none"
          style={{ "z-index": MODAL_LAYER.notice }}
        >
          <Show when={late() > 0}>
            <span class="block rounded-full border border-border-subtle bg-surface-layer2 px-4 py-2 text-sm text-text-primary shadow-lg">
              {t("common.loading")}
            </span>
          </Show>
        </div>
      </Portal>
    </Show>
  );
};

const FailureDialog: Component<{
  loadFailed: boolean;
  onRetry: () => void;
  onClose?: () => void;
  /** What had focus when the window was asked for: focus goes back there. */
  opener: Element | null;
}> = (props) => {
  let closeButton: HTMLButtonElement | undefined;
  let retryButton: HTMLButtonElement | undefined;
  // Without a close the dialog stays: Escape does nothing. Focus starts on
  // Close, else on Try again, so Enter never reloads the page by accident.
  const dialog = createDialog({
    onClose: () => props.onClose?.(),
    initialFocus: () => closeButton ?? retryButton,
    // eslint-disable-next-line solid/reactivity
    opener: props.opener,
  });
  const layer = createModalLayer(MODAL_LAYER.prompt);
  const button = "min-h-11 rounded-lg px-4 text-base text-text-primary hover:bg-white/10";
  return (
    <Portal>
      <div
        class="fixed inset-0 flex items-center justify-center bg-black/60 p-4"
        style={{ "z-index": layer }}
      >
        <div
          {...dialog.dialogProps}
          role="alertdialog"
          ref={dialog.ref}
          class="flex w-[min(28rem,100%)] flex-col gap-4 rounded-xl border border-status-warning/60 bg-surface-layer2 p-4 shadow-lg"
        >
          <p id={dialog.titleId} class="text-base text-text-primary">
            {t(props.loadFailed ? "common.loadFailed" : "common.windowFailed")}
          </p>
          <div class="flex flex-wrap justify-end gap-2">
            <Show when={props.onClose}>
              {(close) => (
                <button ref={closeButton} type="button" class={button} onClick={() => close()()}>
                  {t("common.close")}
                </button>
              )}
            </Show>
            <Show when={props.loadFailed}>
              <button type="button" class={button} onClick={() => window.location.reload()}>
                {t("common.reload")}
              </button>
            </Show>
            <button
              ref={retryButton}
              type="button"
              class="btn-primary min-h-11 px-4 text-base"
              onClick={() => props.onRetry()}
            >
              {t("common.tryAgain")}
            </button>
          </div>
        </div>
      </div>
    </Portal>
  );
};

const LazyDialog: Component<ParentProps<{ name: string; onClose?: () => void }>> = (props) => {
  // The opener: by the time a slow download fails, focus may be elsewhere.
  const opener = document.activeElement;
  return (
    <LazyErrorBoundary
      name={props.name}
      fallback={(err, reset) => (
        <FailureDialog
          loadFailed={isChunkLoadError(err)}
          onRetry={reset}
          onClose={props.onClose}
          opener={opener}
        />
      )}
    >
      <Suspense fallback={<DelayedLoading />}>{props.children}</Suspense>
    </LazyErrorBoundary>
  );
};

export default LazyDialog;
