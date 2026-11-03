/** "Set up a phone" from a full PC session (F5a): the own-issuer panel in a dialog. */

import { Component } from "solid-js";
import { t } from "@/i18n";
import DialogShell from "@/components/auth/DialogShell";
import { refreshMeSecurity } from "@/stores/security";
import EnrollmentIssuerPanel from "./EnrollmentIssuerPanel";

const SetupPhoneModal: Component<{ onClose: () => void }> = (props) => (
  <DialogShell title={t("security.setupPhone.title")} onClose={() => props.onClose()} wide>
    <EnrollmentIssuerPanel
      issuer={{ kind: "own" }}
      onDone={() => void refreshMeSecurity()}
      onCancel={() => props.onClose()}
    />
  </DialogShell>
);

export default SetupPhoneModal;
