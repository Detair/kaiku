/**
 * PasskeyFirstSteps — what LP's empty passkey list ("No passkeys yet.")
 * offers (A5): the first passkey comes from a key device, a setup link, an
 * account request or a registration ticket, never from this list (DC1). So:
 * "Set up a phone" (Task A7, a full session only) and "Secure my account"
 * (Task A11, a request that adds a phone or passkey after the waiting time).
 */

import { Component, Show } from "solid-js";
import { useNavigate } from "@solidjs/router";
import { t } from "@/i18n";
import { isProbation } from "@/stores/auth";
import { addPasskeyHintKey } from "@/stores/security";
import { secondaryButton } from "./SectionCard";

const PasskeyFirstSteps: Component<{ onSetUpPhone: () => void }> = (props) => {
  const navigate = useNavigate();
  return (
    <div class="space-y-2" data-testid="passkey-first-steps">
      <Show when={isProbation()}>
        <p class="text-sm text-text-primary">{t(addPasskeyHintKey())}</p>
      </Show>
      <div class="flex flex-wrap gap-2">
        <Show when={!isProbation()}>
          <button type="button" class={secondaryButton} onClick={() => props.onSetUpPhone()}>
            {t("security.setupPhone.button")}
          </button>
        </Show>
        <button type="button" class={secondaryButton} onClick={() => navigate("/secure-account")}>
          {t("auth.login.secureAccount")}
        </button>
      </div>
    </div>
  );
};

export default PasskeyFirstSteps;
