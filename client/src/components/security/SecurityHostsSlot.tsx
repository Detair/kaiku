/**
 * Where `Main` mounts the account-security overlays (chat-only banner,
 * open-request prompt, migration prompts): a lazy chunk off the startup
 * path. If it cannot load, the page says so with a reload instead of
 * silently dropping them (Fable r2 R1).
 */

import { Component, Suspense, lazy } from "solid-js";
import { LazyErrorBoundary } from "@/components/ui/LazyFallback";
import { ChunkLoadNotice } from "@/components/ui/ChunkLoadNotice";

const SecurityHosts = lazy(() => import("./SecurityHosts"));

const SecurityHostsSlot: Component<{ onReload?: () => void }> = (props) => (
  <LazyErrorBoundary name="SecurityHosts" fallback={<ChunkLoadNotice position="top" onReload={props.onReload} />}>
    <Suspense fallback={null}>
      <SecurityHosts />
    </Suspense>
  </LazyErrorBoundary>
);

export default SecurityHostsSlot;
