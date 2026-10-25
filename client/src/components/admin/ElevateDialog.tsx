/**
 * ElevateDialog - re-authentication dialog that unlocks admin actions.
 */

import { Component } from "solid-js";
import ReauthDialog from "@/components/auth/ReauthDialog";
import { elevateSession } from "@/stores/admin";
import { currentUser } from "@/stores/auth";
import { defaultReauthStrategy } from "@/lib/reauth";
import { t } from "@/i18n";

const ElevateDialog: Component<{ onClose: () => void }> = (props) => (
  <ReauthDialog
    title={t("admin.reauth.elevateTitle")}
    mode="password"
    mfaEnabled={currentUser()?.mfa_enabled ?? false}
    methods={defaultReauthStrategy.availableMethods()}
    onSubmit={async (req) => {
      await elevateSession(req);
      props.onClose();
    }}
    onOidc={async (m) => {
      await defaultReauthStrategy.reauthWithOidc(m.providerId);
      props.onClose();
    }}
    onClose={props.onClose}
  />
);

export default ElevateDialog;
