/**
 * Mount point for a window that opens on demand and so loads on first open:
 * its code stays out of the startup payload (bundle budget).
 *
 * - While the code downloads, nothing shows at first; after a moment a
 *   polite "Loading…" appears, so a slow connection does not look like a
 *   click that did nothing (and invite a second click that closes a toggle).
 * - If the code cannot be downloaded, the page says so with a reload.
 * - If the window loaded but failed while rendering, it says that instead,
 *   with a retry.
 * Both notices can be closed. Closing calls `onClose` (the window's own close),
 * so opening the window again tries again and shows the notice again if it
 * still fails, rather than a click that silently does nothing.
 */

import { Component, ParentProps, Show, Suspense, createSignal, onCleanup } from "solid-js";
import { LazyErrorBoundary } from "./LazyFallback";
import { ChunkLoadNotice, LoadingNotice, WindowErrorNotice } from "./ChunkLoadNotice";
import { isChunkLoadError } from "@/lib/chunkLoadError";

/** How long a download may take before "Loading…" shows (no flicker below). */
export const LOADING_DELAY_MS = 350;

const DelayedLoading: Component = () => {
  const [visible, setVisible] = createSignal(false);
  const timer = setTimeout(() => setVisible(true), LOADING_DELAY_MS);
  onCleanup(() => clearTimeout(timer));
  return <LoadingNotice visible={visible()} />;
};

const LazyDialog: Component<ParentProps<{ name: string; onClose?: () => void }>> = (props) => {
  const [dismissed, setDismissed] = createSignal(false);
  const dismiss = () => {
    if (props.onClose) props.onClose();
    else setDismissed(true);
  };
  return (
    <LazyErrorBoundary
      name={props.name}
      fallback={(err, reset) => (
        <Show when={!dismissed()}>
          <Show
            when={isChunkLoadError(err)}
            fallback={<WindowErrorNotice onRetry={reset} onDismiss={dismiss} />}
          >
            <ChunkLoadNotice position="bottom" onDismiss={dismiss} />
          </Show>
        </Show>
      )}
    >
      <Suspense fallback={<DelayedLoading />}>{props.children}</Suspense>
    </LazyErrorBoundary>
  );
};

export default LazyDialog;
