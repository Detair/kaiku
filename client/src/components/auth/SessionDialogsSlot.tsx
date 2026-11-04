/**
 * Where `App.tsx` mounts the session dialogs (full session, step-up, sign
 * out others): a lazy chunk off the startup path. If it cannot load, the
 * page says so with a reload, and step-ups fail instead of waiting for a
 * dialog that never comes (Fable r2 R1).
 */

import { Component, Suspense, lazy } from "solid-js";
import { LazyErrorBoundary } from "@/components/ui/LazyFallback";
import { ChunkLoadNotice } from "@/components/ui/ChunkLoadNotice";
import { markSessionDialogsFailed } from "@/lib/fullSessionRequired";

const SessionDialogHosts = lazy(() => import("./SessionDialogHosts"));

const SessionDialogsSlot: Component<{ onReload?: () => void }> = (props) => (
  <LazyErrorBoundary
    name="SessionDialogHosts"
    fallback={<ChunkLoadNotice position="bottom" onReload={props.onReload} onShown={markSessionDialogsFailed} />}
  >
    <Suspense fallback={null}>
      <SessionDialogHosts />
    </Suspense>
  </LazyErrorBoundary>
);

export default SessionDialogsSlot;
