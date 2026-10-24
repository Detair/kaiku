/**
 * CreateAccountModal - admin creates a local account with a temporary password.
 *
 * Needs an elevated admin session. After success it shows the details the
 * admin hands to the new user.
 */

import { Component, createSignal, onCleanup, onMount, Show } from "solid-js";
import { X } from "lucide-solid";
import { adminState, createUser } from "@/stores/admin";
import { getServerUrl, HttpError } from "@/lib/tauri";
import { generateTemporaryPassword } from "@/lib/passwords";
import { showToast } from "@/components/ui/Toast";
import { t } from "@/i18n";

const USERNAME_RE = /^[a-z0-9_]{3,32}$/;

interface CreateAccountModalProps {
  onClose: () => void;
}

const CreateAccountModal: Component<CreateAccountModalProps> = (props) => {
  const [username, setUsername] = createSignal("");
  const [displayName, setDisplayName] = createSignal("");
  const [password, setPassword] = createSignal(generateTemporaryPassword());
  const [error, setError] = createSignal("");
  const [submitting, setSubmitting] = createSignal(false);
  const [created, setCreated] = createSignal<{
    name: string;
    username: string;
    password: string;
    server: string;
  } | null>(null);

  onMount(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") props.onClose();
    };
    document.addEventListener("keydown", handleKeyDown);
    onCleanup(() => document.removeEventListener("keydown", handleKeyDown));
  });

  const copy = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      showToast({ type: "success", title: t("admin.createAccount.copied"), duration: 2000 });
    } catch {
      showToast({
        type: "error",
        title: t("admin.createAccount.copyFailed"),
        duration: 5000,
      });
    }
  };

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    if (!adminState.isElevated || submitting()) return;
    const name = username().trim();
    if (!USERNAME_RE.test(name)) {
      setError(t("admin.createAccount.usernameInvalid"));
      return;
    }
    const pw = password();
    if (pw.length < 8 || pw.length > 128) {
      setError(t("admin.createAccount.passwordLength"));
      return;
    }
    setError("");
    setSubmitting(true);
    try {
      const user = await createUser({
        username: name,
        display_name: displayName().trim() || null,
        temporary_password: pw,
      });
      setCreated({
        name: user.display_name || user.username,
        username: user.username,
        password: pw,
        server: getServerUrl(),
      });
    } catch (err) {
      if (err instanceof HttpError && err.status === 409) {
        setError(t("admin.createAccount.usernameTaken"));
      } else {
        setError(t("admin.createAccount.failed"));
      }
    } finally {
      setSubmitting(false);
    }
  };

  const inputClass =
    "w-full px-3 py-2 rounded-lg bg-white/10 text-text-primary text-sm border border-white/10 focus:outline-none focus:border-accent-primary disabled:opacity-60";
  const secondaryBtn =
    "px-3 py-2 rounded-lg bg-white/10 hover:bg-white/20 transition-colors text-text-primary text-sm disabled:opacity-60 disabled:cursor-not-allowed";

  return (
    <div
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      onClick={props.onClose}
    >
      <div
        class="bg-background-secondary rounded-xl shadow-2xl w-full max-w-md mx-4"
        role="dialog"
        aria-modal="true"
        onClick={(e) => e.stopPropagation()}
      >
        <div class="flex items-center justify-between p-4 border-b border-white/10">
          <h2 class="text-lg font-semibold text-text-primary">
            {created() ? t("admin.createAccount.created") : t("admin.createAccount.title")}
          </h2>
          <button
            type="button"
            onClick={props.onClose}
            aria-label={t("admin.createAccount.close")}
            class="p-1.5 hover:bg-white/10 rounded-lg transition-colors text-text-secondary"
          >
            <X class="w-5 h-5" />
          </button>
        </div>

        <Show
          when={created()}
          fallback={
            <form onSubmit={handleSubmit} class="p-4 space-y-4">
              <Show when={!adminState.isElevated}>
                <div class="p-3 rounded-lg bg-status-warning/10 border border-status-warning/30 text-text-primary text-sm">
                  {t("admin.createAccount.needsElevation")}
                </div>
              </Show>

              <label class="block space-y-1">
                <span class="text-sm text-text-secondary">{t("admin.createAccount.username")}</span>
                <input
                  type="text"
                  value={username()}
                  onInput={(e) => setUsername(e.currentTarget.value)}
                  disabled={!adminState.isElevated}
                  autocomplete="off"
                  autocapitalize="none"
                  spellcheck={false}
                  class={inputClass}
                />
                <span class="block text-xs text-text-secondary">
                  {t("admin.createAccount.usernameHint")}
                </span>
              </label>

              <label class="block space-y-1">
                <span class="text-sm text-text-secondary">
                  {t("admin.createAccount.displayName")}
                </span>
                <input
                  type="text"
                  value={displayName()}
                  onInput={(e) => setDisplayName(e.currentTarget.value)}
                  disabled={!adminState.isElevated}
                  maxLength={64}
                  class={inputClass}
                />
              </label>

              <div class="space-y-1">
                <label for="create-account-password" class="text-sm text-text-secondary">
                  {t("admin.createAccount.password")}
                </label>
                <input
                  id="create-account-password"
                  type="text"
                  value={password()}
                  onInput={(e) => setPassword(e.currentTarget.value)}
                  disabled={!adminState.isElevated}
                  autocomplete="off"
                  spellcheck={false}
                  class={`${inputClass} font-mono`}
                />
                <div class="flex gap-2 pt-1">
                  <button
                    type="button"
                    class={secondaryBtn}
                    disabled={!adminState.isElevated}
                    onClick={() => setPassword(generateTemporaryPassword())}
                  >
                    {t("admin.createAccount.generate")}
                  </button>
                  <button type="button" class={secondaryBtn} onClick={() => copy(password())}>
                    {t("admin.createAccount.copy")}
                  </button>
                </div>
              </div>

              <Show when={error()}>
                <p role="alert" class="text-sm text-status-error">
                  {error()}
                </p>
              </Show>

              <div class="flex justify-end gap-2 pt-2">
                <button type="button" class={secondaryBtn} onClick={props.onClose}>
                  {t("admin.createAccount.cancel")}
                </button>
                <button
                  type="submit"
                  disabled={!adminState.isElevated || submitting()}
                  class="px-4 py-2 bg-accent-primary hover:bg-accent-primary/80 rounded-lg transition-colors text-on-accent text-sm font-medium disabled:opacity-60 disabled:cursor-not-allowed"
                >
                  {t("admin.createAccount.submit")}
                </button>
              </div>
            </form>
          }
        >
          {(c) => (
            <div class="p-4 space-y-4">
              <dl class="space-y-3 text-sm">
                <div>
                  <dt class="text-text-secondary">{t("admin.createAccount.server")}</dt>
                  <dd class="text-text-primary font-mono break-all">{c().server}</dd>
                </div>
                <div>
                  <dt class="text-text-secondary">{t("admin.createAccount.username")}</dt>
                  <dd class="text-text-primary font-mono">{c().username}</dd>
                </div>
                <div>
                  <dt class="text-text-secondary">{t("admin.createAccount.password")}</dt>
                  <dd class="text-text-primary font-mono break-all">{c().password}</dd>
                </div>
              </dl>
              <p class="text-sm text-text-primary">
                {t("admin.createAccount.handover", { name: c().name })}
              </p>
              <div class="flex justify-end gap-2">
                <button
                  type="button"
                  class={secondaryBtn}
                  onClick={() =>
                    copy(
                      `${c().server}\n${c().username}\n${c().password}`,
                    )
                  }
                >
                  {t("admin.createAccount.copyAll")}
                </button>
                <button
                  type="button"
                  onClick={props.onClose}
                  class="px-4 py-2 bg-accent-primary hover:bg-accent-primary/80 rounded-lg transition-colors text-on-accent text-sm font-medium"
                >
                  {t("admin.createAccount.close")}
                </button>
              </div>
            </div>
          )}
        </Show>
      </div>
    </div>
  );
};

export default CreateAccountModal;
