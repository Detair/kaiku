/**
 * FullSessionRequiredDialog — a request was refused with 403
 * `FULL_SESSION_REQUIRED` (C27): this needs a passkey sign-in (offered when
 * the account has a passkey), or the Kaiku app on the phone. Opened by `FULL_SESSION_REQUIRED_EVENT` (at most once per
 * 2 s) or directly by `withStepUp` for a chat-only session.
 */

import { Component, Show, createEffect, createSignal, onCleanup, onMount } from "solid-js";
import { t } from "@/i18n";
import { errorText } from "@/i18n/errors";
import { FULL_SESSION_REQUIRED_EVENT } from "@/lib/tauri/common";
import { replaceSessionWithPasskey } from "@/stores/auth";
import { passkeyCeremonies } from "@/lib/passkeySeam";
import { meSecurity, refreshMeSecurity } from "@/stores/security";
import DialogShell from "./DialogShell";

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
  const canSignIn = () => (meSecurity()?.passkeys ?? 0) > 0 && passkeyCeremonies() !== null;

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
      setError(errorText(err, "auth.probation.signInFailed"));
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
          {/* TODO(LP rebase): <LinuxPasskeyHint /> (Task A5) under the button on the desktop. */}
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
