/**
 * Forced password change (contract C4).
 *
 * Shown instead of the app while an admin-created account still uses its
 * temporary password. The server refuses everything else until the change
 * is done, so the only ways out are saving a new password or signing out.
 */

import { Component, Show, createSignal, onCleanup } from "solid-js";
import PasswordInput from "@/components/ui/PasswordInput";
import { t, type MessageKey } from "@/i18n";
import {
  completeForcedPasswordChange,
  hasPendingTemporaryPassword,
  isForcedPasswordChangeSaved,
  logout,
  PASSWORD_SAVED_REFRESH_FAILED,
  retryForcedPasswordChangeFinish,
} from "@/stores/auth";

const MIN_LENGTH = 8;
const MAX_LENGTH = 128;

/** Map a failed change to a message key; the server text is never shown. */
function errorKey(err: unknown): MessageKey {
  const message = err instanceof Error ? err.message : String(err);
  if (message === PASSWORD_SAVED_REFRESH_FAILED) {
    return "auth.forcedChange.savedOffline";
  }
  const status =
    typeof err === "object" && err !== null && "status" in err
      ? (err as { status: unknown }).status
      : undefined;
  if (status === 401 || /invalid credentials/i.test(message)) {
    return "auth.forcedChange.wrongTemporary";
  }
  if (/must differ from the temporary password/i.test(message)) {
    return "auth.forcedChange.sameAsTemporary";
  }
  return "auth.forcedChange.failed";
}

const ForcedPasswordChange: Component = () => {
  document.title = `${t("auth.forcedChange.title")} | Kaiku`;
  onCleanup(() => {
    document.title = "Kaiku";
  });

  // The field appears when this sign-in did not hand us the temporary
  // password (e.g. a restored session), or the remembered one was rejected.
  const [askForCurrent, setAskForCurrent] = createSignal(
    !hasPendingTemporaryPassword(),
  );
  // The new password is saved; only starting the session is left.
  const [saved, setSaved] = createSignal(isForcedPasswordChangeSaved());

  const [current, setCurrent] = createSignal("");
  const [newPassword, setNewPassword] = createSignal("");
  const [confirm, setConfirm] = createSignal("");
  const [error, setError] = createSignal<MessageKey | null>(null);
  const [saving, setSaving] = createSignal(false);

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    setError(null);

    if (saved()) {
      setSaving(true);
      try {
        await retryForcedPasswordChangeFinish();
      } catch (err) {
        setError(errorKey(err));
      } finally {
        setSaving(false);
        setSaved(isForcedPasswordChangeSaved());
      }
      return;
    }

    if (askForCurrent() && current().length === 0) {
      setError("auth.forcedChange.currentRequired");
      return;
    }
    const next = newPassword();
    if (next.length < MIN_LENGTH || next.length > MAX_LENGTH) {
      setError("auth.forcedChange.length");
      return;
    }
    if (next !== confirm()) {
      setError("auth.forcedChange.mismatch");
      return;
    }
    if (askForCurrent() && next === current()) {
      setError("auth.forcedChange.sameAsTemporary");
      return;
    }

    setSaving(true);
    try {
      await completeForcedPasswordChange(
        askForCurrent() ? current() : null,
        next,
      );
    } catch (err) {
      console.warn("[ForcedPasswordChange] Password change failed");
      setError(errorKey(err));
    } finally {
      setSaving(false);
      setSaved(isForcedPasswordChangeSaved());
      // The store drops a rejected remembered password: ask for it now.
      if (!hasPendingTemporaryPassword()) setAskForCurrent(true);
    }
  };

  return (
    <div class="flex items-center justify-center min-h-screen overflow-y-auto bg-background-primary px-4">
      <div class="w-full max-w-md bg-background-secondary rounded-lg shadow-lg p-8">
        <h1 class="text-2xl font-bold mb-2 text-center text-text-primary">
          {t("auth.forcedChange.title")}
        </h1>
        <p class="text-text-secondary text-center mb-6">
          {t("auth.forcedChange.intro")}
        </p>

        <form
          onSubmit={handleSubmit}
          method="post"
          noValidate
          class="space-y-4"
        >
          <Show when={!saved() && askForCurrent()}>
            <div>
              <label
                for="fpc-current"
                class="block text-sm font-medium text-text-secondary mb-1"
              >
                {t("auth.forcedChange.currentPassword")}
              </label>
              <PasswordInput
                id="fpc-current"
                class="input-field"
                name="current-password"
                autocomplete="current-password"
                placeholder={t("auth.forcedChange.currentPasswordPlaceholder")}
                value={current()}
                onInput={(e) => setCurrent(e.currentTarget.value)}
                disabled={saving()}
                required
              />
            </div>
          </Show>

          <Show when={!saved()}>
            <div>
              <label
                for="fpc-new"
                class="block text-sm font-medium text-text-secondary mb-1"
              >
                {t("auth.forcedChange.newPassword")}
              </label>
              <PasswordInput
                id="fpc-new"
                class="input-field"
                name="new-password"
                autocomplete="new-password"
                placeholder={t("auth.forcedChange.newPasswordPlaceholder")}
                value={newPassword()}
                onInput={(e) => setNewPassword(e.currentTarget.value)}
                disabled={saving()}
                required
                minLength={MIN_LENGTH}
                maxLength={MAX_LENGTH}
              />
            </div>

            <div>
              <label
                for="fpc-confirm"
                class="block text-sm font-medium text-text-secondary mb-1"
              >
                {t("auth.forcedChange.confirmPassword")}
              </label>
              <PasswordInput
                id="fpc-confirm"
                class="input-field"
                name="confirm-password"
                autocomplete="new-password"
                placeholder={t("auth.forcedChange.confirmPasswordPlaceholder")}
                value={confirm()}
                onInput={(e) => setConfirm(e.currentTarget.value)}
                disabled={saving()}
                required
                maxLength={MAX_LENGTH}
              />
            </div>
          </Show>

          <Show when={error()}>
            {(key) => (
              <div
                role="alert"
                class="p-3 rounded-md text-sm"
                style={{
                  "background-color": "var(--color-error-bg)",
                  border: "1px solid var(--color-error-border)",
                  color: "var(--color-error-text)",
                }}
              >
                {t(key())}
              </div>
            )}
          </Show>

          <button
            type="submit"
            class="btn-primary w-full flex items-center justify-center gap-2"
            disabled={saving()}
          >
            {saving()
              ? t("auth.forcedChange.saving")
              : saved()
                ? t("auth.forcedChange.retry")
                : t("auth.forcedChange.submit")}
          </button>

          <button
            type="button"
            onClick={() => void logout()}
            disabled={saving()}
            class="w-full px-4 py-2 rounded-lg text-text-secondary hover:text-text-primary transition-colors"
          >
            {t("auth.forcedChange.signOut")}
          </button>
        </form>
      </div>
    </div>
  );
};

export default ForcedPasswordChange;
