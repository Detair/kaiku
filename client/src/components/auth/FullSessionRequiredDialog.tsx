/**
 * FullSessionRequiredDialog — a request was refused with 403
 * `FULL_SESSION_REQUIRED` (C27): this needs a passkey sign-in (offered when
 * the account has a passkey), or the Kaiku app on the phone. Its open state
 * is `lib/fullSessionRequired` (`FULL_SESSION_REQUIRED_EVENT`, or
 * `withStepUp` for a chat-only session); mounted by `SessionDialogHosts`.
 */

import { Component, Show, createEffect, createSignal, on, onCleanup } from "solid-js";
import { t } from "@/i18n";
import { passkeyFailureText } from "@/lib/authErrors";
import { isTauri } from "@/lib/tauri/common";
import { closeFullSessionRequired, fullSessionRequiredOpen as open } from "@/lib/fullSessionRequired";
import { replaceSessionWithPasskey } from "@/stores/auth";
import { canUsePasskeys } from "@/lib/tauri/passkeys";
import { meSecurity, refreshMeSecurity } from "@/stores/security";
import DialogShell from "./DialogShell";
import LinuxPasskeyHint from "./LinuxPasskeyHint";

export { showFullSessionRequired } from "@/lib/fullSessionRequired";

const FullSessionRequiredDialog: Component = () => {
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  // The passkey sign-in is offered only when the account has a passkey
  // (C27 `sign_in_with_passkey`); without an overview yet, load it.
  createEffect(() => {
    if (open() && meSecurity() === null) void refreshMeSecurity();
  });
  const canSignIn = () => (meSecurity()?.passkeys ?? 0) > 0 && canUsePasskeys();
  // A fresh dialog shows no error of an earlier attempt (Argon review 1 #1).
  createEffect(on(open, (o) => o && setError(null)));

  /** The running sign-in; closing the dialog aborts it (Fable m6). */
  let attempt: AbortController | null = null;
  onCleanup(() => attempt?.abort());

  const close = () => {
    attempt?.abort();
    attempt = null;
    setBusy(false);
    closeFullSessionRequired();
    setError(null);
  };
  const signIn = async () => {
    attempt?.abort();
    const ctl = new AbortController();
    attempt = ctl;
    setBusy(true);
    setError(null);
    try {
      await replaceSessionWithPasskey(ctl.signal);
      if (!ctl.signal.aborted) close();
    } catch (err) {
      if (!ctl.signal.aborted) setError(passkeyFailureText(err, "auth.probation.signInFailed") || null);
    } finally {
      if (attempt === ctl) {
        attempt = null;
        setBusy(false);
      }
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
