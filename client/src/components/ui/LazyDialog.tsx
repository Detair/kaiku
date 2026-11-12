/**
 * Mount point for a window that opens on demand and so loads on first open:
 * its code stays out of the startup payload (bundle budget). Nothing shows
 * while the chunk loads. If it cannot load, the page says so with a reload,
 * never a click that does nothing.
 */

import { Component, ParentProps, Suspense } from "solid-js";
import { LazyErrorBoundary } from "./LazyFallback";
import { ChunkLoadNotice } from "./ChunkLoadNotice";

const LazyDialog: Component<ParentProps<{ name: string }>> = (props) => (
  <LazyErrorBoundary
    name={props.name}
    fallback={<ChunkLoadNotice position="bottom" />}
  >
    <Suspense fallback={null}>{props.children}</Suspense>
  </LazyErrorBoundary>
);

export default LazyDialog;
