import { Component, createSignal, Show, onCleanup } from "solid-js";
import { A } from "@solidjs/router";
import { getThemeImage } from "@/lib/themeImage";
import { HttpError } from "@/lib/tauri/common";
import { t } from "@/i18n";
import { errorText } from "@/i18n/errors";

const ForgotPassword: Component = () => {
  document.title = `${t("auth.forgot.title")} | Kaiku`;
  onCleanup(() => { document.title = "Kaiku"; });
  const defaultServerUrl = import.meta.env.VITE_SERVER_URL || window.location.origin;
  const [serverUrl] = createSignal(defaultServerUrl);
  const [email, setEmail] = createSignal("");
  const [error, setError] = createSignal("");
  const [success, setSuccess] = createSignal(false);
  const [isLoading, setIsLoading] = createSignal(false);

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    setError("");
    setSuccess(false);

    if (!email().trim() || !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email())) {
      setError(t("auth.forgot.invalidEmail"));
      return;
    }

    setIsLoading(true);
    try {
      const res = await fetch(
        `${serverUrl().replace(/\/+$/, "")}/auth/forgot-password`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ email: email() }),
        },
      );

      if (!res.ok) {
        const data = await res.json().catch((e) => {
          console.warn("[ForgotPassword] Could not parse error response:", e.message);
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
      setError(errorText(err, "auth.forgot.failed"));
    } finally {
      setIsLoading(false);
    }
  };

  return (
    <div class="flex items-center justify-center min-h-dvh overflow-y-auto bg-background-primary">
      <div class="flex w-full max-w-4xl mx-4 bg-background-secondary rounded-lg shadow-lg overflow-hidden">
        <div class="hidden lg:flex w-1/2 items-center justify-center p-8 bg-surface-base">
          <img src={getThemeImage("floki_auth_forgot.png")} alt={t("auth.forgot.illustrationAlt")} class="w-full max-w-xs object-contain" loading="eager" decoding="async" fetchpriority="high" />
        </div>
        <div class="w-full lg:w-1/2 p-8">
        <h1 class="text-2xl font-bold mb-2 text-center text-text-primary">
          {t("auth.forgot.title")}
        </h1>
        <p class="text-text-secondary text-center mb-6">
          {t("auth.forgot.subtitle")}
        </p>

        <Show when={!success()}>
          <form onSubmit={handleSubmit} method="post" noValidate class="space-y-4">
            {/* Server URL input hidden during beta — hardcoded to kaiku.pmind.de */}

            <div>
              <label
                for="fp-email"
                class="block text-sm font-medium text-text-secondary mb-1"
              >
                {t("auth.forgot.email")}
              </label>
              <input
                id="fp-email"
                type="email"
                class="input-field"
                name="email"
                autocomplete="email"
                placeholder={t("auth.forgot.emailPlaceholder")}
                value={email()}
                onInput={(e) => setEmail(e.currentTarget.value)}
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
                    {t("auth.forgot.submitting")}
                  </>
                }
              >
                {t("auth.forgot.submit")}
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
            {t("auth.forgot.sent")}
          </div>
          <div class="space-y-3">
            <A
              href={`/reset-password?serverUrl=${encodeURIComponent(serverUrl())}`}
              class="btn-primary w-full flex items-center justify-center"
            >
              {t("auth.forgot.enterCode")}
            </A>
            <p class="text-center text-sm text-text-secondary">
              <A href="/login" class="text-primary auth-link">
                {t("auth.reset.backToLogin")}
              </A>
            </p>
          </div>
        </Show>
        </div>
      </div>
    </div>
  );
};

export default ForgotPassword;
