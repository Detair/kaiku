/** "Set up a phone" from a full PC session (F5a): the own-issuer panel in a dialog. */

import { Component } from "solid-js";
import { t } from "@/i18n";
import DialogShell from "@/components/auth/DialogShell";
import { refreshMeSecurity } from "@/stores/security";
import { AUTHENTICATORS_CHANGED_EVENT } from "@/lib/types/events";
import EnrollmentIssuerPanel from "./EnrollmentIssuerPanel";

const SetupPhoneModal: Component<{ onClose: () => void }> = (props) => (
  <DialogShell title={t("security.setupPhone.title")} onClose={() => props.onClose()} wide>
    <EnrollmentIssuerPanel
      issuer={{ kind: "own" }}
      onDone={() => {
        void refreshMeSecurity();
        // The phone list refetches now, not only on the server's hint.
        window.dispatchEvent(new CustomEvent(AUTHENTICATORS_CHANGED_EVENT));
      }}
      onCancel={() => props.onClose()}
    />
  </DialogShell>
);

export default SetupPhoneModal;
