/**
 * The entry point of "Set up this PC" in the banners and the security
 * settings: whether it is on offer, and the button that opens its dialog.
 * The dialog itself is mounted once, by `SetUpThisPcHost` among the session
 * dialogs, so a refresh of the data the buttons depend on never closes it
 * (closing gives a live attempt up). Its code loads on first open.
 */

import { Component, createSignal } from "solid-js";
import { t } from "@/i18n";
import { isProbation, sessionInfo } from "@/stores/auth";
import { meSecurity } from "@/stores/security";

/** The session the dialog was opened for; `null` when it is closed. */
const [openFor, setOpenFor] = createSignal<string | null>(null);

/** The session the dialog was opened for (read by `SetUpThisPcHost`). */
export const setUpThisPcOpenFor = openFor;

/** Close the dialog (its cleanup gives a live attempt up). */
export function closeSetUpThisPc(): void {
  setOpenFor(null);
}

function openSetUpThisPc(): void {
  const id = sessionInfo()?.id;
  if (id) setOpenFor(id);
}

/** Whether "Set up this PC" is on offer: a chat-only session whose account has a phone that can confirm. */
export const canSetUpThisPc = (): boolean => isProbation() && meSecurity()?.can_set_up_this_pc === true;

/** The button. `primary`: the first action of its row. */
export const SetUpThisPcButton: Component<{ primary?: boolean }> = (props) => (
  <button
    type="button"
    class={
      props.primary
        ? "btn-primary min-h-11 text-base"
        : "min-h-11 px-3 rounded-lg text-base text-text-primary bg-surface-layer2 hover:bg-white/10"
    }
    onClick={openSetUpThisPc}
  >
    {t("auth.pcSetup.button")}
  </button>
);
