/**
 * The one mount point of the "Set up this PC" dialog (with the other session
 * dialogs). It is shown only for the signed-in chat-only session it was
 * opened for, whatever the banners show meanwhile, and loads its code on
 * first open. Its state (attempt, QR code, code field) lives in the dialog
 * and goes with it.
 */

import { Component, Show, createEffect, lazy } from "solid-js";
import LazyDialog from "@/components/ui/LazyDialog";
import { authState, isProbation, sessionInfo } from "@/stores/auth";
import { closeSetUpThisPc, setUpThisPcOpenFor } from "./SetUpThisPcButton";

const SetUpThisPc = lazy(() => import("./SetUpThisPc"));

/**
 * Open only for the signed-in chat-only session it was opened for. A
 * sign-out, a session end or another user or session closes it for good; it
 * never reopens by itself.
 */
const isOpen = (): boolean => {
  const id = setUpThisPcOpenFor();
  return id !== null && id === sessionInfo()?.id && authState.user !== null && isProbation();
};

const SetUpThisPcHost: Component = () => {
  // The session it was opened for is gone: forget the flag, so no later
  // sign-in in this window opens it by itself.
  createEffect(() => {
    if (setUpThisPcOpenFor() !== null && !isOpen()) closeSetUpThisPc();
  });
  return (
    <Show when={isOpen()}>
      <LazyDialog name="SetUpThisPc">
        <SetUpThisPc onClose={closeSetUpThisPc} />
      </LazyDialog>
    </Show>
  );
};

export default SetUpThisPcHost;
