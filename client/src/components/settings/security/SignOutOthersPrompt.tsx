/** "Sign out everywhere else?" after a sign-in method changed (ASVS 7.4.3). */

import { Component, Show, createSignal } from "solid-js";
import { t } from "@/i18n";
import { revokeAllOtherSessions } from "@/lib/tauri";
import { withStepUp } from "@/lib/stepUp";
import { closeSignOutOthers, signOutOthersPrompt } from "@/stores/security";
import DialogShell from "@/components/auth/DialogShell";
import { errorBox, errorStyle, primaryButton, secondaryButton } from "./SectionCard";
import { securityErrorText } from "./securityErrors";

const SignOutOthersPrompt: Component = () => {
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const close = () => {
    setError(null);
    closeSignOutOthers();
  };
  const confirm = async () => {
    setBusy(true);
    setError(null);
    try {
      await withStepUp({ action: "sessions.end", target: "others", targetLabel: "" }, () => revokeAllOtherSessions());
      close();
    } catch (err) {
      setError(securityErrorText(err, "security.sessions.endAllFailed"));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Show when={signOutOthersPrompt()}>
      <DialogShell title={t("security.signOutOthers.title")} onClose={close}>
        <p class="text-base text-text-primary">{t("security.signOutOthers.text")}</p>
        <Show when={error()}>
          <p role="alert" class={errorBox} style={errorStyle}>{error()}</p>
        </Show>
        <div class="flex flex-wrap justify-end gap-2">
          <button type="button" class={secondaryButton} onClick={close}>
            {t("security.signOutOthers.keep")}
          </button>
          <button type="button" class={primaryButton} disabled={busy()} onClick={() => void confirm()}>
            {t("security.signOutOthers.confirm")}
          </button>
        </div>
      </DialogShell>
    </Show>
  );
};

export default SignOutOthersPrompt;
