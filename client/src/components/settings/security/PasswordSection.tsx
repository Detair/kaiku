/**
 * Password (C27): set or change it (step-up `password.set`), remove it
 * (`password.remove`). The server's blocklist answers 400 `PASSWORD_REJECTED
 * {reason}`. Without TOTP and without `may_approve`, a password has no use
 * (F5d) and the section says so.
 */

import { Component, Show, createSignal } from "solid-js";
import { KeyRound } from "lucide-solid";
import { t } from "@/i18n";
import { removePassword, setPassword } from "@/lib/tauri/security";
import { withStepUp } from "@/lib/stepUp";
import { isProbation } from "@/stores/auth";
import { meSecurity, offerSignOutOthers, refreshMeSecurity } from "@/stores/security";
import PasswordInput from "@/components/ui/PasswordInput";
import SectionCard, { dangerButton, errorBox, errorStyle, primaryButton, secondaryButton } from "./SectionCard";
import { securityErrorText } from "./securityErrors";

const PasswordSection: Component = () => {
  const [editing, setEditing] = createSignal(false);
  const [first, setFirst] = createSignal("");
  const [second, setSecond] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const sec = () => meSecurity();
  const hasPassword = () => sec()?.password != null;
  const noUse = () => {
    const s = sec();
    return !!s && s.totp === null && !s.may_approve;
  };

  const reset = () => {
    setEditing(false);
    setFirst("");
    setSecond("");
    setError(null);
  };

  const save = async (e: Event) => {
    e.preventDefault();
    setError(null);
    if (first().length < 8) {
      setError(t("security.password.rejected.length"));
      return;
    }
    if (first() !== second()) {
      setError(t("security.password.mismatch"));
      return;
    }
    setBusy(true);
    try {
      const pw = first();
      await withStepUp({ action: "password.set", target: "", targetLabel: "" }, () => setPassword(pw));
      reset();
      void refreshMeSecurity();
      offerSignOutOthers();
    } catch (err) {
      setError(securityErrorText(err, "security.password.failed"));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    setBusy(true);
    setError(null);
    try {
      await withStepUp({ action: "password.remove", target: "", targetLabel: "" }, () => removePassword());
      void refreshMeSecurity();
      // A sign-in method changed: offer to end every other session (Argon 3 #7).
      offerSignOutOthers();
    } catch (err) {
      setError(securityErrorText(err, "security.password.failed"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <SectionCard
      title={t("security.password.title")}
      icon={<KeyRound class="w-5 h-5 text-text-secondary" aria-hidden="true" />}
      testId="security-password"
    >
      <p class="text-base text-text-primary">
        {hasPassword() ? t("security.password.isSet") : t("security.password.notSet")}
      </p>
      <Show when={sec()?.password?.state === "compromised"}>
        <p class="text-base text-text-primary" role="status">{t("security.password.compromised")}</p>
      </Show>
      <Show when={sec()?.password?.state === "throttled"}>
        <p class="text-base text-text-primary" role="status">{t("security.password.throttled")}</p>
      </Show>
      <Show when={noUse()}>
        <p class="text-base text-text-secondary">{t("security.password.noUse")}</p>
      </Show>
      <Show when={!isProbation()}>
        <Show
          when={editing()}
          fallback={
            <div class="flex flex-wrap gap-2">
              <button type="button" class={secondaryButton} onClick={() => setEditing(true)}>
                {hasPassword() ? t("security.password.change") : t("security.password.set")}
              </button>
              <Show when={hasPassword()}>
                <button type="button" class={dangerButton} disabled={busy()} onClick={() => void remove()}>
                  {t("security.password.remove")}
                </button>
              </Show>
            </div>
          }
        >
          <form class="space-y-3" onSubmit={save} noValidate>
            <label class="block space-y-1">
              <span class="text-base text-text-primary">{t("security.password.new")}</span>
              <PasswordInput
                class="input-field"
                autocomplete="new-password"
                value={first()}
                onInput={(e) => setFirst(e.currentTarget.value)}
                aria-label={t("security.password.new")}
              />
            </label>
            <label class="block space-y-1">
              <span class="text-base text-text-primary">{t("security.password.repeat")}</span>
              <PasswordInput
                class="input-field"
                autocomplete="new-password"
                value={second()}
                onInput={(e) => setSecond(e.currentTarget.value)}
                aria-label={t("security.password.repeat")}
              />
            </label>
            <div class="flex flex-wrap justify-end gap-2">
              <button type="button" class={secondaryButton} onClick={reset}>
                {t("common.cancel")}
              </button>
              <button type="submit" class={primaryButton} disabled={busy()}>
                {t("security.password.save")}
              </button>
            </div>
          </form>
        </Show>
      </Show>
      <Show when={error()}>
        <p role="alert" class={errorBox} style={errorStyle}>{error()}</p>
      </Show>
    </SectionCard>
  );
};

export default PasswordSection;
