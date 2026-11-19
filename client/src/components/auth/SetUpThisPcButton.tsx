/**
 * The entry point of "Set up this PC" in the banners and the security
 * settings: whether it is on offer, and the button that opens its dialog.
 * The dialog (QR code, keypad, desktop hand-off) loads on first open, so its
 * code stays out of the startup payload (bundle budget).
 */

import { Component, Show, createSignal, lazy } from "solid-js";
import { t } from "@/i18n";
import { isProbation } from "@/stores/auth";
import { meSecurity } from "@/stores/security";
import LazyDialog from "@/components/ui/LazyDialog";

const SetUpThisPc = lazy(() => import("./SetUpThisPc"));

/** Whether "Set up this PC" is on offer: a chat-only session whose account has a phone that can confirm. */
export const canSetUpThisPc = (): boolean => isProbation() && meSecurity()?.can_set_up_this_pc === true;

/** The button and its dialog. `primary`: the first action of its row. */
export const SetUpThisPcButton: Component<{ primary?: boolean }> = (props) => {
  const [open, setOpen] = createSignal(false);
  return (
    <>
      <button
        type="button"
        class={
          props.primary
            ? "btn-primary min-h-11 text-base"
            : "min-h-11 px-3 rounded-lg text-base text-text-primary bg-surface-layer2 hover:bg-white/10"
        }
        onClick={() => setOpen(true)}
      >
        {t("auth.pcSetup.button")}
      </button>
      <Show when={open()}>
        <LazyDialog name="SetUpThisPc">
          <SetUpThisPc onClose={() => setOpen(false)} />
        </LazyDialog>
      </Show>
    </>
  );
};
