/**
 * Part2FactorDialog — owner rule part 2 (B.2): with no phone or passkey, a
 * "That wasn't me" or a cancel is confirmed with a fresh authenticator code or
 * a recovery code (which is then used up). A wrong code counts as a failed try.
 */

import { Component, Show, createSignal, createUniqueId } from "solid-js";
import { t } from "@/i18n";
import type { Part2Factor } from "@/lib/tauri/security";
import { normalizeRecoveryCode } from "@/components/auth/RecoveryCodeInput";
import DialogShell from "@/components/auth/DialogShell";

export interface Part2FactorDialogProps {
  onSubmit: (factor: Part2Factor) => void;
  onClose: () => void;
}

const Part2FactorDialog: Component<Part2FactorDialogProps> = (props) => {
  const [type, setType] = createSignal<Part2Factor["type"]>("totp");
  const [code, setCode] = createSignal("");
  const id = createUniqueId();
  const submit = (e: Event) => {
    e.preventDefault();
    const value = type() === "totp" ? code().replace(/\D+/g, "") : normalizeRecoveryCode(code());
    if (!value) return;
    props.onSubmit({ type: type(), code: value });
  };
  return (
    <DialogShell title={t("security.part2.title")} onClose={() => props.onClose()}>
      <form class="space-y-4" onSubmit={submit} noValidate>
        <p class="text-base text-text-primary">{t("security.part2.intro")}</p>
        <fieldset class="space-y-2">
          <legend class="sr-only">{t("security.part2.title")}</legend>
          <label class="flex items-center gap-3 min-h-12 text-base text-text-primary">
            <input type="radio" name={`${id}-type`} checked={type() === "totp"} onClick={() => setType("totp")} />
            {t("security.part2.totp")}
          </label>
          <label class="flex items-center gap-3 min-h-12 text-base text-text-primary">
            <input type="radio" name={`${id}-type`} checked={type() === "recovery_code"} onClick={() => setType("recovery_code")} />
            {t("security.part2.recovery")}
          </label>
        </fieldset>
        <label for={id} class="block text-base text-text-primary">
          {t("security.part2.code")}
        </label>
        <input
          id={id}
          class="input-field font-mono text-xl min-h-12"
          autocomplete="one-time-code"
          inputmode={type() === "totp" ? "numeric" : "text"}
          value={code()}
          onInput={(e) => setCode(e.currentTarget.value)}
        />
        <Show when={type() === "totp"}>
          <p class="text-sm text-text-secondary">{t("security.part2.reuse")}</p>
        </Show>
        <p class="text-sm text-text-secondary">{t("security.part2.counts")}</p>
        <button type="submit" class="btn-primary w-full min-h-12 text-base">
          {t("security.part2.send")}
        </button>
      </form>
    </DialogShell>
  );
};

export default Part2FactorDialog;
