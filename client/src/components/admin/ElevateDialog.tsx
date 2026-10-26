/**
 * ElevateDialog - re-authentication dialog that unlocks admin actions.
 *
 * Asks the server how this session can confirm (password and/or sign-in
 * provider) and offers only those methods.
 */

import { Component, createResource, Show } from "solid-js";
import ReauthDialog from "@/components/auth/ReauthDialog";
import { confirmElevation, elevateSession } from "@/stores/admin";
import { HttpError } from "@/lib/tauri";
import { defaultReauthStrategy, reauthErrorText } from "@/lib/reauth";
import { showToast } from "@/components/ui/Toast";
import { t } from "@/i18n";

const ElevateDialog: Component<{ onClose: () => void; onElevated?: () => void }> = (props) => {
  const [info] = createResource(async () => {
    try {
      return await defaultReauthStrategy.loadInfo();
    } catch (err) {
      showToast({
        type: "error",
        title: t("admin.reauth.loadFailed"),
        message: reauthErrorText(err),
        duration: 6000,
      });
      props.onClose();
      return undefined;
    }
  });

  return (
    <Show
      when={info()}
      fallback={
        <div class="fixed inset-0 z-[60] flex items-center justify-center bg-black/60">
          <p role="status" class="px-4 py-3 rounded-lg bg-background-secondary text-sm text-text-primary">
            {t("admin.reauth.loading")}
          </p>
        </div>
      }
    >
      {(i) => (
        <ReauthDialog
          title={t("admin.reauth.elevateTitle")}
          mode="password"
          mfaEnabled={i().mfaRequired}
          methods={i().methods}
          onSubmit={async (req) => {
            await elevateSession(req);
            props.onElevated?.();
            props.onClose();
          }}
          onOidc={async (m, mfaCode, signal) => {
            await defaultReauthStrategy.reauthWithOidc(m.providerId, { mfaCode, signal });
            if (signal.aborted) return;
            // The provider's answer is only a hint: a fresh, successful
            // status answer from the server must say the session is elevated.
            const elevated = await confirmElevation(signal);
            if (signal.aborted) return;
            if (!elevated) {
              throw new HttpError(0, "REAUTH_NOT_CONFIRMED", "REAUTH_NOT_CONFIRMED");
            }
            props.onElevated?.();
            props.onClose();
          }}
          onClose={props.onClose}
        />
      )}
    </Show>
  );
};

export default ElevateDialog;
