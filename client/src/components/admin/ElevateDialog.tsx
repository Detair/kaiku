/**
 * ElevateDialog — turns on admin mode (F7, C27): the `admin.elevate`
 * step-up through `withStepUp` (the step-up dialog with a passkey; on a PC
 * without one it points to the Kaiku app on the phone, since a PC session
 * cannot sign `device_stepup`), then LP's `elevateWithStepUp` posts only the
 * reason. Password and single sign-on re-authentication are gone.
 *
 * It renders nothing itself: the step-up dialog host in `App.tsx` shows the
 * confirmation. Unmounting it cancels a confirmation still waiting.
 */

import { Component, onCleanup, onMount } from "solid-js";
import { elevateWithStepUp } from "@/stores/admin";
import { pendingStepUp, StepUpCancelled, withStepUp, type StepUpRequest } from "@/lib/stepUp";
import { reauthErrorText } from "@/lib/reauth";
import { showToast } from "@/components/ui/Toast";
import { t } from "@/i18n";

const ElevateDialog: Component<{ onClose: () => void; onElevated?: () => void }> = (props) => {
  const req: StepUpRequest = { action: "admin.elevate", target: "", targetLabel: "" };
  let done = false;

  onMount(() => {
    void withStepUp(req, () => elevateWithStepUp(null))
      .then((response) => {
        if (response.elevated) props.onElevated?.();
      })
      .catch((err: unknown) => {
        if (err instanceof StepUpCancelled) return;
        showToast({
          type: "error",
          title: t("admin.reauth.elevateTitle"),
          message: reauthErrorText(err),
          duration: 6000,
        });
      })
      .finally(() => {
        done = true;
        props.onClose();
      });
  });

  onCleanup(() => {
    if (done) return;
    const pending = pendingStepUp();
    if (pending?.req === req) pending.cancel();
  });

  return null;
};

export default ElevateDialog;
