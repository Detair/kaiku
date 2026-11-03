/**
 * FullSessionRequiredDialog — a request was refused with 403
 * `FULL_SESSION_REQUIRED` (C27): this needs a passkey sign-in (offered when
 * the account has a passkey), or the Kaiku app on the phone. Opened by `FULL_SESSION_REQUIRED_EVENT` (at most once per
 * 2 s) or directly by `withStepUp` for a chat-only session.
 */

import { Component, Show, createEffect, createSignal, onCleanup, onMount } from "solid-js";
import { t } from "@/i18n";
import { passkeyFailureText } from "@/lib/authErrors";
import { FULL_SESSION_REQUIRED_EVENT, isTauri } from "@/lib/tauri/common";
import { replaceSessionWithPasskey } from "@/stores/auth";
import { canUsePasskeys } from "@/lib/tauri/passkeys";
import { meSecurity, refreshMeSecurity } from "@/stores/security";
import DialogShell from "./DialogShell";
import LinuxPasskeyHint from "./LinuxPasskeyHint";

const [open, setOpen] = createSignal(false);

/** Open the dialog (used by `withStepUp` for a chat-only session). */
export function showFullSessionRequired(): void {
  setOpen(true);
}

const FullSessionRequiredDialog: Component = () => {
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const onEvent = () => setOpen(true);
  onMount(() => window.addEventListener(FULL_SESSION_REQUIRED_EVENT, onEvent));
  onCleanup(() => window.removeEventListener(FULL_SESSION_REQUIRED_EVENT, onEvent));
  // The passkey sign-in is offered only when the account has a passkey
  // (C27 `sign_in_with_passkey`); without an overview yet, load it.
  createEffect(() => {
    if (open() && meSecurity() === null) void refreshMeSecurity();
  });
  const canSignIn = () => (meSecurity()?.passkeys ?? 0) > 0 && canUsePasskeys();

  const close = () => {
    setOpen(false);
    setError(null);
  };
  const signIn = async () => {
    setBusy(true);
    setError(null);
    try {
      await replaceSessionWithPasskey();
      close();
    } catch (err) {
      setError(passkeyFailureText(err, "auth.probation.signInFailed") || null);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Show when={open()}>
      <DialogShell title={t("auth.fullSession.title")} onClose={close}>
        <p class="text-base text-text-primary">{t("auth.fullSession.text")}</p>
        <Show when={canSignIn()}>
          <button type="button" class="btn-primary w-full min-h-12 text-base" disabled={busy()} onClick={() => void signIn()}>
            {t("auth.probation.signIn")}
          </button>
          <Show when={isTauri() && busy()}>
            <p class="text-base text-text-primary" role="status">{t("passkeys.login.browserHint")}</p>
            <LinuxPasskeyHint class="text-base text-text-secondary" />
          </Show>
        </Show>
        <p class="text-base text-text-secondary">{t("auth.fullSession.phone")}</p>
        <Show when={error()}>
          <p role="alert" class="text-sm text-text-primary">{error()}</p>
        </Show>
      </DialogShell>
    </Show>
  );
};

export default FullSessionRequiredDialog;
