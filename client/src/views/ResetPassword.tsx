import { Component, createSignal, Show, onCleanup } from "solid-js";
import { A } from "@solidjs/router";
import PasswordInput from "@/components/ui/PasswordInput";
import { getThemeImage } from "@/lib/themeImage";
import { HttpError } from "@/lib/tauri/common";
import { t, type MessageKey } from "@/i18n";
import { errorText } from "@/i18n/errors";

/** Server codes for a reset code that cannot be used. */
const RESET_ERRORS: Partial<Record<string, MessageKey>> = {
  INVALID_TOKEN: "auth.reset.codeInvalid",
  TOKEN_EXPIRED: "auth.reset.codeInvalid",
};

const ResetPassword: Component = () => {
  document.title = `${t("auth.reset.title")} | Kaiku`;
  onCleanup(() => { document.title = "Kaiku"; });
  const defaultServerUrl = import.meta.env.VITE_SERVER_URL || window.location.origin;
  const [serverUrl] = createSignal(defaultServerUrl);
  const [token, setToken] = createSignal("");
  const [newPassword, setNewPassword] = createSignal("");
  const [confirmPassword, setConfirmPassword] = createSignal("");
  const [error, setError] = createSignal("");
  const [success, setSuccess] = createSignal(false);
  const [isLoading, setIsLoading] = createSignal(false);

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    setError("");
    setSuccess(false);

    if (!token().trim()) {
      setError(t("auth.reset.codeRequired"));
      return;
    }
    if (newPassword().length < 8 || newPassword().length > 128) {
      setError(t("auth.reset.length"));
      return;
    }
    if (newPassword() !== confirmPassword()) {
      setError(t("auth.reset.mismatch"));
      return;
    }

    setIsLoading(true);
    try {
      const res = await fetch(
        `${serverUrl().replace(/\/+$/, "")}/auth/reset-password`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ token: token(), new_password: newPassword() }),
        },
      );

      if (!res.ok) {
        const data = await res.json().catch((e) => {
          console.warn("[ResetPassword] Could not parse error response:", e.message);
          return null;
        });
        throw new HttpError(
          res.status,
          (typeof data?.message === "string" && data.message) || `Request failed (${res.status})`,
          typeof data?.error === "string" ? data.error : undefined,
        );
      }

      setSuccess(true);
    } catch (err: unknown) {
      setError(errorText(err, "auth.reset.failed", RESET_ERRORS));
    } finally {
      setIsLoading(false);
    }
  };

  return (
    <div class="flex items-center justify-center min-h-dvh overflow-y-auto bg-background-primary">
      <div class="flex w-full max-w-4xl mx-4 bg-background-secondary rounded-lg shadow-lg overflow-hidden">
        <div class="hidden lg:flex w-1/2 items-center justify-center p-8 bg-surface-base">
          <img src={getThemeImage("floki_auth_forgot.png")} alt={t("auth.reset.illustrationAlt")} class="w-full max-w-xs object-contain" loading="eager" decoding="async" fetchpriority="high" />
        </div>
        <div class="w-full lg:w-1/2 p-8">
        <h1 class="text-2xl font-bold mb-2 text-center text-text-primary">
          {t("auth.reset.title")}
        </h1>
        <p class="text-text-secondary text-center mb-6">
          {t("auth.reset.subtitle")}
        </p>

        <Show when={!success()}>
          <form onSubmit={handleSubmit} method="post" noValidate class="space-y-4">
            {/* Server URL input hidden during beta — hardcoded to kaiku.pmind.de */}

            <div>
              <label
                for="rp-token"
                class="block text-sm font-medium text-text-secondary mb-1"
              >
                {t("auth.reset.code")}
              </label>
              <input
                id="rp-token"
                type="text"
                class="input-field"
                name="token"
                autocomplete="off"
                placeholder={t("auth.reset.codePlaceholder")}
                value={token()}
                onInput={(e) => setToken(e.currentTarget.value)}
                disabled={isLoading()}
                required
              />
            </div>

            <div>
              <label
                for="rp-new-password"
                class="block text-sm font-medium text-text-secondary mb-1"
              >
                {t("auth.reset.newPassword")}
              </label>
              <PasswordInput
                id="rp-new-password"
                class="input-field"
                name="new-password"
                autocomplete="new-password"
                placeholder={t("auth.reset.newPasswordPlaceholder")}
                value={newPassword()}
                onInput={(e) => setNewPassword(e.currentTarget.value)}
                disabled={isLoading()}
                required
                minLength={8}
                maxLength={128}
              />
            </div>

            <div>
              <label
                for="rp-confirm-password"
                class="block text-sm font-medium text-text-secondary mb-1"
              >
                {t("auth.reset.confirmPassword")}
              </label>
              <PasswordInput
                id="rp-confirm-password"
                class="input-field"
                name="confirm-password"
                autocomplete="new-password"
                placeholder={t("auth.reset.confirmPasswordPlaceholder")}
                value={confirmPassword()}
                onInput={(e) => setConfirmPassword(e.currentTarget.value)}
                disabled={isLoading()}
                required
              />
            </div>

            <Show when={error()}>
              <div
                role="alert"
                class="p-3 rounded-md text-sm"
                style={{"background-color":"var(--color-error-bg)","border":"1px solid var(--color-error-border)","color":"var(--color-error-text)"}}
              >
                {error()}
              </div>
            </Show>

            <button
              type="submit"
              class="btn-primary w-full flex items-center justify-center gap-2"
              disabled={isLoading()}
            >
              <Show
                when={!isLoading()}
                fallback={
                  <>
                    <span class="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                    {t("auth.reset.submitting")}
                  </>
                }
              >
                {t("auth.reset.submit")}
              </Show>
            </button>

            <p class="text-center text-sm text-text-secondary mt-4">
              <A href="/login" class="text-primary auth-link">
                {t("auth.reset.backToLogin")}
              </A>
            </p>
          </form>
        </Show>

        <Show when={success()}>
          <div
            class="p-4 rounded-md text-sm mb-4 bg-status-success/10 border border-status-success/30 text-status-success"
          >
            {t("auth.reset.success")}
          </div>
          <A
            href="/login"
            class="btn-primary w-full flex items-center justify-center"
          >
            {t("auth.reset.goToLogin")}
          </A>
        </Show>
        </div>
      </div>
    </div>
  );
};

export default ResetPassword;
