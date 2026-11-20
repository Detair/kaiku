/**
 * The one mount point of the "Set up this PC" dialog (with the other session
 * dialogs). It stays open while the session is chat-only, whatever the
 * banners show meanwhile, and loads its code on first open.
 */

import { Component, Show, lazy } from "solid-js";
import LazyDialog from "@/components/ui/LazyDialog";
import { isProbation } from "@/stores/auth";
import { closeSetUpThisPc, setUpThisPcOpen } from "./SetUpThisPcButton";

const SetUpThisPc = lazy(() => import("./SetUpThisPc"));

const SetUpThisPcHost: Component = () => (
  <Show when={setUpThisPcOpen() && isProbation()}>
    <LazyDialog name="SetUpThisPc">
      <SetUpThisPc onClose={closeSetUpThisPc} />
    </LazyDialog>
  </Show>
);

export default SetUpThisPcHost;
