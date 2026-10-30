import { Component, createSignal, createResource, Show, For, onCleanup } from "solid-js";
import { A, useNavigate, useSearchParams } from "@solidjs/router";
import {
  register,
  loginWithOidc,
  restoreSessionAfterPopup,
  completeNativeOidcLogin,
  authState,
  clearError,
} from "@/stores/auth";
import { fetchServerSettings, oidcAuthorize } from "@/lib/tauri";
import type { OidcProvider } from "@/lib/types";
import { KeyRound, ShieldAlert } from "lucide-solid";
import { Github, Chrome } from "@/components/icons/BrandIcons";
import PasswordInput from "@/components/ui/PasswordInput";
import { getThemeImage } from "@/lib/themeImage";
import { t } from "@/i18n";
import { isTauri } from "@/lib/tauri/common";
import { openBlankSsoPopup, watchSsoPopup } from "@/lib/oidcPopup";
import { safeReturnUrl } from "@/lib/returnUrl";
import { errorText, SSO_ERRORS } from "@/i18n/errors";
import { REGISTER_ERRORS, REGISTER_STATUS_ERRORS } from "@/lib/authErrors";

/** Map icon_hint to a Lucide icon component. */
function providerIcon(hint: string | null) {
  switch (hint) {
    case "github":
      return Github;
    case "chrome":
    case "google":
      return Chrome;
    default:
      return KeyRound;
  }
}

const Register: Component = () => {
  document.title = `${t("auth.register.pageTitle")} | Kaiku`;
  onCleanup(() => { document.title = "Kaiku"; });
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const afterRegister = () => safeReturnUrl(searchParams.returnUrl) ?? "/";
  const defaultServerUrl = import.meta.env.VITE_SERVER_URL || window.location.origin;
  const [serverUrl] = createSignal(defaultServerUrl);
  const [username, setUsername] = createSignal("");
  const [email, setEmail] = createSignal("");
  const [displayName, setDisplayName] = createSignal("");
  const [password, setPassword] = createSignal("");
  const [confirmPassword, setConfirmPassword] = createSignal("");
  const [localError, setLocalError] = createSignal("");
  const [oidcLoading, setOidcLoading] = createSignal<string | null>(null);

  // Fetch server settings from hardcoded beta URL
  const [settings] = createResource(() => defaultServerUrl, async (url) => {
    if (!url.trim()) return null;
    try {
      return await fetchServerSettings(url);
    } catch (err) {
      console.warn("[Register] Failed to fetch server settings:", err instanceof Error ? err.message : err);
      return null;
    }
  });

  const handleRegister = async (e: Event) => {
    e.preventDefault();
    setLocalError("");
    clearError();

    if (!username().trim()) {
      setLocalError(t("auth.register.usernameRequired"));
      return;
    }
    if (username().length < 3) {
      setLocalError(t("auth.register.usernameShort"));
      return;
    }
    if (username().length > 32) {
      setLocalError(t("auth.register.usernameLong"));
      return;
    }
    if (!/^[a-z0-9_]+$/.test(username())) {
      setLocalError(t("auth.register.usernameChars"));
      return;
    }
    if (!password()) {
      setLocalError(t("auth.register.passwordRequired"));
      return;
    }
    if (password().length < 8) {
      setLocalError(t("auth.register.passwordShort"));
      return;
    }
    if (password() !== confirmPassword()) {
      setLocalError(t("auth.register.mismatch"));
      return;
    }

    try {
      await register(
        serverUrl(),
        username(),
        password(),
        email() || undefined,
        displayName() || undefined,
      );
      navigate(afterRegister(), { replace: true });
    } catch (err) {
      if (!authState.error) {
        setLocalError(
          errorText(
            err,
            "auth.register.failed",
            REGISTER_ERRORS,
            REGISTER_STATUS_ERRORS,
          ),
        );
      }
    }
  };

  const handleOidcRegister = async (provider: OidcProvider) => {
    setLocalError("");
    clearError();

    // Browser: open the popup inside the click, before any await, or popup
    // blockers reject it. It is pointed at the provider once the server answers.
    const popup = isTauri() ? null : openBlankSsoPopup("oidc-register");
    if (!isTauri() && !popup) {
      setLocalError(t("auth.register.popupBlocked"));
      return;
    }
    setOidcLoading(provider.slug);
    let stopWatching: (() => void) | undefined;

    try {
      const result = await oidcAuthorize(serverUrl(), provider.slug);

      if (result.mode === "tauri") {
        // Tauri: Rust kept the tokens and returned the user
        await completeNativeOidcLogin(serverUrl(), result.result);
        navigate(afterRegister(), { replace: true });
        setOidcLoading(null);
        return;
      }

      // Browser: send the popup to the provider and wait for its callback
      const expectedOrigin = new URL(serverUrl()).origin;
      stopWatching = watchSsoPopup({
        popup: popup!,
        expectedOrigin,
        onResult: (data) => {
          loginWithOidc(
            serverUrl(),
            data.access_token,
            data.refresh_token,
            data.expires_in || 900,
            data.setup_required ?? false,
          )
            .then(() => {
              navigate(afterRegister(), { replace: true });
            })
            .catch((err) => {
              if (!authState.error) {
                setLocalError(errorText(err, "auth.register.ssoFailed"));
              }
            })
            .finally(() => {
              setOidcLoading(null);
            });
        },
        onClosedWithoutResult: () => {
          // The callback may have lost its opener; the refresh cookie is set
          // anyway, so try to resume the session once.
          void restoreSessionAfterPopup(serverUrl()).then((restored) => {
            if (restored) navigate(afterRegister(), { replace: true });
            setOidcLoading(null);
          });
        },
      });
      popup!.location.href = result.authUrl;
    } catch (err) {
      stopWatching?.();
      popup?.close();
      setLocalError(errorText(err, "auth.register.ssoFailed", SSO_ERRORS.byCode));
      setOidcLoading(null);
    }
  };

  const isClosed = () => {
    const s = settings();
    return s?.registration_policy === "closed";
  };

  const showLocalRegister = () => {
    const s = settings();
    return !s || (s.auth_methods.local && s.registration_policy !== "closed");
  };

  const showOidc = () => {
    const s = settings();
    return (
      s?.oidc_enabled &&
      s.oidc_providers.length > 0 &&
      s.registration_policy !== "closed"
    );
  };

  const error = () => localError() || authState.error;

  return (
    <div class="flex items-center justify-center min-h-screen overflow-y-auto bg-background-primary py-8">
      <div class="flex w-full max-w-4xl mx-4 bg-background-secondary rounded-lg shadow-lg overflow-hidden">
        <div class="hidden lg:flex w-1/2 items-start justify-center pt-16 p-8 bg-surface-base">
          <img src={getThemeImage("floki_auth_register.png")} alt={t("auth.register.illustrationAlt")} class="w-full max-w-xs object-contain" loading="eager" decoding="async" fetchpriority="high" />
        </div>
        <div class="w-full lg:w-1/2 p-8">
        <h1 class="text-2xl font-bold mb-2 text-center text-text-primary">
          {t("auth.register.heading")}
        </h1>
        <p class="text-text-secondary text-center mb-6">
          {t("auth.register.subtitle")}
        </p>

        {/* Server URL input hidden during beta — hardcoded to kaiku.pmind.de */}

        {/* Registration closed message */}
        <Show when={isClosed()}>
          <div class="flex items-center gap-3 p-4 rounded-lg border border-white/10 bg-white/5 text-text-secondary">
            <ShieldAlert class="w-5 h-5 flex-shrink-0" />
            <div>
              <p class="font-medium text-text-primary">
                {t("auth.register.closedTitle")}
              </p>
              <p class="text-sm mt-1">
                {t("auth.register.closed")}
              </p>
            </div>
          </div>
        </Show>

        {/* SSO Buttons */}
        <Show when={showOidc()}>
          <div class="space-y-2 mb-4">
            <For each={settings()!.oidc_providers}>
              {(provider) => {
                const Icon = providerIcon(provider.icon_hint);
                return (
                  <button
                    type="button"
                    class="w-full flex items-center justify-center gap-3 px-4 py-2.5 rounded-lg border border-white/10 bg-white/5 hover:bg-white/10 text-text-primary text-sm font-medium transition-colors"
                    disabled={authState.isLoading || !!oidcLoading()}
                    onClick={() => handleOidcRegister(provider)}
                  >
                    <Show
                      when={oidcLoading() !== provider.slug}
                      fallback={
                        <span class="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                      }
                    >
                      <Icon class="w-4 h-4" />
                    </Show>
                    {t("auth.register.signUpWith", { provider: provider.display_name })}
                  </button>
                );
              }}
            </For>
          </div>

          <Show when={showLocalRegister()}>
            <div class="relative my-5">
              <div class="absolute inset-0 flex items-center">
                <div class="w-full border-t border-white/10" />
              </div>
              <div class="relative flex justify-center text-xs">
                <span class="bg-background-secondary px-3 text-text-muted">
                  {t("auth.register.or")}
                </span>
              </div>
            </div>
          </Show>
        </Show>

        {/* Local Registration Form */}
        <Show when={showLocalRegister()}>
          <form onSubmit={handleRegister} method="post" noValidate class="space-y-4">
            <div>
              <label for="register-username" class="block text-sm font-medium text-text-secondary mb-1">
                {t("common.username")} <span class="text-error-text">*</span>
              </label>
              <input
                id="register-username"
                type="text"
                class="input-field"
                name="username"
                autocomplete="username"
                data-testid="register-username"
                placeholder={t("auth.register.usernamePlaceholder")}
                pattern="^[a-z0-9_]{3,32}$"
                minLength={3}
                maxLength={32}
                value={username()}
                onInput={(e) => setUsername(e.currentTarget.value)}
                disabled={authState.isLoading}
                required
              />
              <p class="text-xs text-text-muted mt-1">
                {t("auth.register.usernameHint")}
              </p>
            </div>

            <div>
              <label for="register-email" class="block text-sm font-medium text-text-secondary mb-1">
                {t("auth.register.email")} <span class="text-text-muted">{t("auth.register.optional")}</span>
              </label>
              <input
                id="register-email"
                type="email"
                class="input-field"
                name="email"
                autocomplete="email"
                placeholder={t("auth.register.emailPlaceholder")}
                value={email()}
                onInput={(e) => setEmail(e.currentTarget.value)}
                disabled={authState.isLoading}
              />
            </div>

            <div>
              <label for="register-display-name" class="block text-sm font-medium text-text-secondary mb-1">
                {t("auth.register.displayName")} <span class="text-text-muted">{t("auth.register.optional")}</span>
              </label>
              <input
                id="register-display-name"
                type="text"
                class="input-field"
                name="name"
                autocomplete="name"
                placeholder={t("auth.register.displayNamePlaceholder")}
                value={displayName()}
                onInput={(e) => setDisplayName(e.currentTarget.value)}
                disabled={authState.isLoading}
              />
            </div>

            <div>
              <label for="register-password" class="block text-sm font-medium text-text-secondary mb-1">
                {t("auth.register.password")} <span class="text-error-text">*</span>
              </label>
              <PasswordInput
                id="register-password"
                class="input-field"
                name="new-password"
                autocomplete="new-password"
                data-testid="register-password"
                placeholder={t("auth.register.passwordPlaceholder")}
                value={password()}
                onInput={(e) => setPassword(e.currentTarget.value)}
                disabled={authState.isLoading}
                required
              />
              <p class="text-xs text-text-muted mt-1">{t("auth.register.passwordHint")}</p>
            </div>

            <div>
              <label for="register-confirm-password" class="block text-sm font-medium text-text-secondary mb-1">
                {t("auth.register.confirmPassword")} <span class="text-error-text">*</span>
              </label>
              <PasswordInput
                id="register-confirm-password"
                class="input-field"
                name="confirm-password"
                autocomplete="new-password"
                data-testid="register-password-confirm"
                placeholder={t("auth.register.confirmPasswordPlaceholder")}
                value={confirmPassword()}
                onInput={(e) => setConfirmPassword(e.currentTarget.value)}
                disabled={authState.isLoading}
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

            <p class="text-xs text-text-muted text-center">
              {t("auth.register.terms")}
            </p>

            <button
              type="submit"
              class="btn-primary w-full flex items-center justify-center gap-2"
              data-testid="register-submit"
              disabled={authState.isLoading}
            >
              <Show
                when={!authState.isLoading}
                fallback={
                  <>
                    <span class="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                    {t("auth.register.submitting")}
                  </>
                }
              >
                {t("auth.register.submit")}
              </Show>
            </button>
          </form>
        </Show>

        {/* Error display when no local form */}
        <Show when={!showLocalRegister() && !isClosed() && error()}>
          <div
            role="alert"
            class="p-3 rounded-md text-sm"
            style={{"background-color":"var(--color-error-bg)","border":"1px solid var(--color-error-border)","color":"var(--color-error-text)"}}
          >
            {error()}
          </div>
        </Show>

        <p class="flex justify-center gap-1 text-sm text-text-secondary mt-4">
          <span>{t("auth.register.haveAccount")}</span>
          <A href="/login" class="text-primary auth-link">
            {t("common.login")}
          </A>
        </p>
        </div>
      </div>
    </div>
  );
};

export default Register;
