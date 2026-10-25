/**
 * ElevateDialog - re-authentication dialog that unlocks admin actions.
 *
 * Asks the server how this session can confirm (password and/or sign-in
 * provider) and offers only those methods.
 */

import { Component, createResource, Show } from "solid-js";
import ReauthDialog from "@/components/auth/ReauthDialog";
import { adminState, checkAdminStatus, elevateSession } from "@/stores/admin";
import { defaultReauthStrategy, reauthErrorText } from "@/lib/reauth";
import { showToast } from "@/components/ui/Toast";
import { t } from "@/i18n";

const ElevateDialog: Component<{ onClose: () => void }> = (props) => {
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
    <Show when={info()}>
      {(i) => (
        <ReauthDialog
          title={t("admin.reauth.elevateTitle")}
          mode="password"
          mfaEnabled={i().mfaRequired}
          methods={i().methods}
          onSubmit={async (req) => {
            await elevateSession(req);
            props.onClose();
          }}
          onOidc={async (m, mfaCode) => {
            await defaultReauthStrategy.reauthWithOidc(m.providerId, { mfaCode });
            // The provider's answer is only a hint: ask the server.
            await checkAdminStatus();
            if (!adminState.isElevated) {
              throw new Error(t("admin.reauth.notConfirmed"));
            }
            props.onClose();
          }}
          onClose={props.onClose}
        />
      )}
    </Show>
  );
};

export default ElevateDialog;
