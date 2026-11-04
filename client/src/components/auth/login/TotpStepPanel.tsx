/**
 * TotpStepPanel — the TOTP step of a password or SSO sign-in (F4). Never
 * submits by itself; a wrong code keeps the step.
 */

import { Component, Show, createSignal } from "solid-js";
import { t } from "@/i18n";

export interface TotpStepPanelProps {
  error: string | null;
  busy: boolean;
  onSubmit: (code: string) => void;
  onBack: () => void;
}

const TotpStepPanel: Component<TotpStepPanelProps> = (props) => {
  const [code, setCode] = createSignal("");
  const submit = (e: Event) => {
    e.preventDefault();
    const digits = code().replace(/\s+/g, "");
    if (!digits || props.busy) return;
    props.onSubmit(digits);
  };
  return (
    <form class="space-y-4" onSubmit={submit} noValidate>
      <h2 class="text-xl font-semibold text-text-primary text-center">{t("auth.totp.heading")}</h2>
      <div>
        <label for="login-totp-code" class="block text-base font-medium text-text-primary mb-1">
          {t("auth.totp.label")}
        </label>
        <input
          id="login-totp-code"
          type="text"
          inputmode="numeric"
          autocomplete="one-time-code"
          class="input-field font-mono text-center text-2xl tracking-widest min-h-12"
          value={code()}
          onInput={(e) => setCode(e.currentTarget.value)}
          disabled={props.busy}
          maxLength={12}
        />
        <p class="text-sm text-text-secondary mt-1">{t("auth.totp.hint")}</p>
      </div>
      <Show when={props.error}>
        <div
          role="alert"
          class="p-3 rounded-md text-sm"
          style={{ "background-color": "var(--color-error-bg)", border: "1px solid var(--color-error-border)", color: "var(--color-error-text)" }}
        >
          {props.error}
        </div>
      </Show>
      <button type="submit" class="btn-primary w-full min-h-12 text-base" disabled={props.busy}>
        {t("auth.totp.submit")}
      </button>
      <button
        type="button"
        class="w-full min-h-12 rounded-lg text-base text-text-secondary hover:text-text-primary"
        onClick={() => props.onBack()}
      >
        {t("auth.flow.back")}
      </button>
    </form>
  );
};

export default TotpStepPanel;
