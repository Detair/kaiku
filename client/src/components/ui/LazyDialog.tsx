/**
 * Mount point for a window that opens on demand and so loads on first open:
 * its code stays out of the startup payload (bundle budget). Define the
 * window with `lazyWindow`, so a failed download is told apart reliably.
 *
 * - While the code downloads, nothing shows at first; after a moment a
 *   polite "Loading…" appears, so a slow connection does not look like a
 *   click that did nothing (and invite a second click that closes a toggle).
 * - If the code cannot be loaded, a small dialog says so with Reload.
 * - If the window loaded but failed while rendering, it says that instead,
 *   with Try again.
 * Close (and Escape) call `onClose`, the window's own close, so opening the
 * window again tries again rather than a click that silently does nothing.
 * Without `onClose` (startup wizards the app cannot go on without) the
 * dialog offers only Reload and cannot be dismissed.
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

/**
 * The live region is there from the start, empty, so screen readers announce
 * the text when it appears. On `body`, so a transformed drawer cannot clip it.
 */
const DelayedLoading: Component = () => {
  const [visible, setVisible] = createSignal(false);
  const timer = setTimeout(() => setVisible(true), LOADING_DELAY_MS);
  onCleanup(() => clearTimeout(timer));
  return (
    <Portal>
      <div
        role="status"
        aria-live="polite"
        class="fixed bottom-4 left-1/2 -translate-x-1/2 pointer-events-none"
        style={{ "z-index": MODAL_LAYER.notice }}
      >
        <Show when={visible()}>
          <span class="block rounded-full border border-border-subtle bg-surface-layer2 px-4 py-2 text-sm text-text-primary shadow-lg">
            {t("common.loading")}
          </span>
        </Show>
      </div>
    </Portal>
  );
};

const FailureDialog: Component<{
  loadFailed: boolean;
  onRetry: () => void;
  onClose?: () => void;
}> = (props) => {
  // Without a close the dialog stays: Escape does nothing.
  const dialog = createDialog({ onClose: () => props.onClose?.() });
  const layer = createModalLayer(MODAL_LAYER.prompt);
  // Startup wizards cannot be retried in place either: the app needs them.
  const reload = () => props.loadFailed || !props.onClose;
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
                <button
                  type="button"
                  class="min-h-11 rounded-lg px-4 text-base text-text-primary hover:bg-white/10"
                  onClick={() => close()()}
                >
                  {t("common.close")}
                </button>
              )}
            </Show>
            <button
              type="button"
              class="btn-primary min-h-11 px-4 text-base"
              onClick={() => (reload() ? window.location.reload() : props.onRetry())}
            >
              {t(reload() ? "common.reload" : "common.tryAgain")}
            </button>
          </div>
        </div>
      </div>
    </Portal>
  );
};

const LazyDialog: Component<ParentProps<{ name: string; onClose?: () => void }>> = (props) => (
  <LazyErrorBoundary
    name={props.name}
    fallback={(err, reset) => (
      <FailureDialog loadFailed={isChunkLoadError(err)} onRetry={reset} onClose={props.onClose} />
    )}
  >
    <Suspense fallback={<DelayedLoading />}>{props.children}</Suspense>
  </LazyErrorBoundary>
);

export default LazyDialog;
