import { Component, createSignal, createResource, Show, For, onCleanup, onMount } from "solid-js";
import { A, useNavigate, useSearchParams } from "@solidjs/router";
import {
  login,
  loginWithOidc,
  completeNativeOidcLogin,
  authState,
  clearError,
  setAuthState,
} from "@/stores/auth";
import { fetchServerSettings, getDefaultServerUrl, oidcAuthorize } from "@/lib/tauri";
import type { OidcProvider } from "@/lib/types";
import { KeyRound, ShieldCheck } from "lucide-solid";
import { Github, Chrome } from "@/components/icons/BrandIcons";
import PasswordInput from "@/components/ui/PasswordInput";
import { getThemeImage } from "@/lib/themeImage";
import { t } from "@/i18n";
import { safeReturnUrl } from "@/lib/returnUrl";
import { errorText, SSO_ERRORS } from "@/i18n/errors";
import { LOGIN_ERRORS } from "@/lib/authErrors";

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

const Login: Component = () => {
  document.title = `${t("common.login")} | Kaiku`;
  onCleanup(() => { document.title = "Kaiku"; });
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const defaultServerUrl = import.meta.env.VITE_SERVER_URL || window.location.origin;
  const [serverUrl, setServerUrl] = createSignal(defaultServerUrl);
  // Native mode: a tester override (KAIKU_SERVER_URL) beats the build-time default.
  onMount(async () => {
    try {
      const override = await getDefaultServerUrl();
      if (override) setServerUrl(override);
    } catch (err) {
      console.warn("[Login] Could not read default server URL:", err instanceof Error ? err.message : err);
    }
  });
  const [username, setUsername] = createSignal("");
  const [password, setPassword] = createSignal("");
  const [mfaCode, setMfaCode] = createSignal("");
  const [localError, setLocalError] = createSignal("");
  const [oidcLoading, setOidcLoading] = createSignal<string | null>(null);

  // Fetch server settings from hardcoded beta URL
  const [settings] = createResource(serverUrl, async (url) => {
    if (!url.trim()) return null;
    try {
      return await fetchServerSettings(url);
    } catch (err) {
      console.warn("[Login] Failed to fetch server settings:", err instanceof Error ? err.message : err);
      return null;
    }
  });

  const handleLogin = async (e: Event) => {
    e.preventDefault();
    setLocalError("");
    clearError();

    if (!username().trim()) {
      setLocalError(t("auth.login.usernameRequired"));
      return;
    }
    if (!password()) {
      setLocalError(t("auth.login.passwordRequired"));
      return;
    }

    // If MFA is required and no code provided
    if (authState.mfaRequired && !mfaCode().trim()) {
      setLocalError(t("auth.login.mfaRequired"));
      return;
    }

    try {
      await login(
        serverUrl(),
        username(),
        password(),
        authState.mfaRequired ? mfaCode() : undefined,
      );
      // Navigate to returnUrl if valid (relative path only), otherwise home
      navigate(safeReturnUrl(searchParams.returnUrl) ?? "/", { replace: true });
    } catch (err) {
      // MFA_REQUIRED is handled by the store — just reset MFA code input
      const msg = err instanceof Error ? err.message : String(err);
      if (msg === "MFA_REQUIRED") {
        setMfaCode("");
      } else if (!authState.error) {
        setLocalError(errorText(err, "auth.login.failed", LOGIN_ERRORS));
      }
    }
  };

  const handleBackToLogin = () => {
    setAuthState({ mfaRequired: false, error: null });
    setMfaCode("");
    setPassword("");
  };

  const handleOidcLogin = async (provider: OidcProvider) => {
    setLocalError("");
    clearError();
    setOidcLoading(provider.slug);

    try {
      const result = await oidcAuthorize(serverUrl(), provider.slug);

      if (result.mode === "tauri") {
        // Tauri: Rust kept the tokens and returned the user
        await completeNativeOidcLogin(serverUrl(), result.result);
        navigate("/", { replace: true });
        setOidcLoading(null);
        return;
      }

      // Browser: open popup and listen for postMessage callback
      const expectedOrigin = new URL(serverUrl()).origin;
      const messageHandler = (event: MessageEvent) => {
        // Validate origin to prevent cross-origin token theft
        if (event.origin !== expectedOrigin) return;
        if (event.data?.type === "oidc-callback" && event.data.access_token) {
          window.removeEventListener("message", messageHandler);
          loginWithOidc(
            serverUrl(),
            event.data.access_token,
            event.data.refresh_token,
            event.data.expires_in || 900,
            event.data.setup_required ?? false,
          )
            .then(() => {
              navigate("/", { replace: true });
            })
            .catch((err) => {
              if (!authState.error) {
                setLocalError(errorText(err, "auth.login.ssoFailed"));
              }
            })
            .finally(() => {
              setOidcLoading(null);
            });
        }
      };
      window.addEventListener("message", messageHandler);

      const popup = window.open(
        result.authUrl,
        "oidc-login",
        "width=600,height=700",
      );

      // Cleanup if popup is closed without completing
      const checkClosed = setInterval(() => {
        if (popup?.closed) {
          clearInterval(checkClosed);
          window.removeEventListener("message", messageHandler);
          setOidcLoading(null);
        }
      }, 500);
    } catch (err) {
      setLocalError(errorText(err, "auth.login.ssoFailed", SSO_ERRORS.byCode));
      setOidcLoading(null);
    }
  };

  const showLocalLogin = () => {
    const s = settings();
    // Show local login if settings haven't loaded yet, or if local auth is enabled
    return !s || s.auth_methods.local;
  };

  const showOidc = () => {
    const s = settings();
    return s?.oidc_enabled && s.oidc_providers.length > 0;
  };

  // Offer registration only when the server says it is open.
  // Invite-only servers need an invite, so offer it only when we came from one.
  const inviteReturnUrl = () => {
    const url = safeReturnUrl(searchParams.returnUrl);
    return url && /^\/invite\/[A-Za-z0-9]+/.test(url) ? url : null;
  };
  const cameWithInvite = () => inviteReturnUrl() !== null;
  const showRegisterLink = () => {
    const policy = settings()?.registration_policy;
    return policy === "open" || (policy === "invite_only" && cameWithInvite());
  };

  const error = () => localError() || authState.error;

  return (
    <div class="flex items-center justify-center min-h-dvh overflow-y-auto bg-background-primary">
      <div class="flex w-full max-w-4xl mx-4 bg-background-secondary rounded-lg shadow-lg overflow-hidden">
        {/* Left: Floki illustration */}
        <div class="hidden lg:flex w-1/2 items-center justify-center p-8 bg-surface-base">
          <img src={getThemeImage("floki_auth_welcome.png")} alt={t("auth.login.illustrationAlt")} class="w-full max-w-xs object-contain" loading="eager" decoding="async" fetchpriority="high" />
        </div>
        {/* Right: Form */}
        <div class="w-full lg:w-1/2 p-8">
        <h1 class="text-2xl font-bold mb-2 text-center text-text-primary">
          {t("auth.login.heading")}
        </h1>
        <p class="text-text-secondary text-center mb-6">
          {t("auth.login.subtitle")}
        </p>

        {/* Server URL input hidden during beta — hardcoded to kaiku.pmind.de */}

        {/* MFA Code Step */}
        <Show when={authState.mfaRequired}>
          <form onSubmit={handleLogin} method="post" noValidate class="space-y-4">
            <div class="flex items-center gap-3 p-3 bg-accent-primary/10 border border-accent-primary/20 rounded-lg">
              <ShieldCheck class="w-5 h-5 text-accent-primary flex-shrink-0" />
              <p class="text-sm text-text-secondary">
                {t("auth.login.mfaIntro")}
              </p>
            </div>

            <div>
              <label for="login-mfa-code" class="block text-sm font-medium text-text-secondary mb-1">
                {t("auth.login.mfaCode")}
              </label>
              <input
                id="login-mfa-code"
                type="text"
                class="input-field font-mono text-center text-lg tracking-widest"
                name="one-time-code"
                autocomplete="one-time-code"
                placeholder={"000000"}
                value={mfaCode()}
                onInput={(e) =>
                  setMfaCode(e.currentTarget.value.replace(/\s/g, ""))
                }
                disabled={authState.isLoading}
                maxLength={20}
                autofocus
                required
              />
              <p class="text-xs text-text-muted mt-1">
                {t("auth.login.mfaHint")}
              </p>
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
              disabled={authState.isLoading}
            >
              <Show
                when={!authState.isLoading}
                fallback={
                  <>
                    <span class="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                    {t("auth.login.verifying")}
                  </>
                }
              >
                {t("auth.login.verify")}
              </Show>
            </button>

            <button
              type="button"
              onClick={handleBackToLogin}
              class="w-full text-sm text-text-secondary hover:text-text-primary transition-colors"
            >
              {t("auth.login.backToLogin")}
            </button>
          </form>
        </Show>

        {/* Normal Login Flow (hidden during MFA) */}
        <Show when={!authState.mfaRequired}>
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
                      onClick={() => handleOidcLogin(provider)}
                    >
                      <Show
                        when={oidcLoading() !== provider.slug}
                        fallback={
                          <span class="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                        }
                      >
                        <Icon class="w-4 h-4" />
                      </Show>
                      {t("auth.login.continueWith", { provider: provider.display_name })}
                    </button>
                  );
                }}
              </For>
            </div>

            <Show when={showLocalLogin()}>
              <div class="relative my-5">
                <div class="absolute inset-0 flex items-center">
                  <div class="w-full border-t border-white/10" />
                </div>
                <div class="relative flex justify-center text-xs">
                  <span class="bg-background-secondary px-3 text-text-muted">
                    {t("auth.login.or")}
                  </span>
                </div>
              </div>
            </Show>
          </Show>

          {/* Local Login Form */}
          <Show when={showLocalLogin()}>
            <form onSubmit={handleLogin} method="post" noValidate class="space-y-4">
              <div>
                <label for="login-username" class="block text-sm font-medium text-text-secondary mb-1">
                  {t("common.username")}
                </label>
                <input
                  id="login-username"
                  type="text"
                  class="input-field"
                  name="username"
                  autocomplete="username"
                  placeholder={t("auth.login.usernamePlaceholder")}
                  data-testid="login-username"
                  value={username()}
                  onInput={(e) => setUsername(e.currentTarget.value)}
                  disabled={authState.isLoading}
                  required
                />
              </div>

              <div>
                <label for="login-password" class="block text-sm font-medium text-text-secondary mb-1">
                  {t("auth.login.password")}
                </label>
                <PasswordInput
                  id="login-password"
                  class="input-field"
                  name="password"
                  autocomplete="current-password"
                  placeholder={t("auth.login.passwordPlaceholder")}
                  data-testid="login-password"
                  value={password()}
                  onInput={(e) => setPassword(e.currentTarget.value)}
                  disabled={authState.isLoading}
                  required
                />
              </div>

              <div class="text-right">
                <A
                  href="/forgot-password"
                  class="text-sm text-primary auth-link"
                >
                  {t("auth.login.forgotPassword")}
                </A>
              </div>

              <Show when={error()}>
                <div
                  role="alert"
                  class="p-3 rounded-md text-sm"
                  data-testid="login-error"
                  style={{"background-color":"var(--color-error-bg)","border":"1px solid var(--color-error-border)","color":"var(--color-error-text)"}}
                >
                  {error()}
                </div>
              </Show>

              <button
                type="submit"
                class="btn-primary w-full flex items-center justify-center gap-2"
                data-testid="login-submit"
                disabled={authState.isLoading}
              >
                <Show
                  when={!authState.isLoading}
                  fallback={
                    <>
                      <span class="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                      {t("auth.login.submitting")}
                    </>
                  }
                >
                  {t("common.login")}
                </Show>
              </button>
            </form>
          </Show>

          {/* Error display when no local login form */}
          <Show when={!showLocalLogin() && error()}>
            <div
              role="alert"
              class="p-3 rounded-md text-sm"
              style={{"background-color":"var(--color-error-bg)","border":"1px solid var(--color-error-border)","color":"var(--color-error-text)"}}
            >
              {error()}
            </div>
          </Show>

          <Show when={showRegisterLink()}>
            <p class="flex justify-center gap-1 text-sm text-text-secondary mt-4">
              <span>{t("auth.login.noAccount")}</span>
              <A
                href={
                  inviteReturnUrl()
                    ? `/register?returnUrl=${encodeURIComponent(inviteReturnUrl()!)}`
                    : "/register"
                }
                class="text-primary auth-link"
              >
                {t("auth.login.registerLink")}
              </A>
            </p>
          </Show>
        </Show>
        </div>
      </div>
    </div>
  );
};

export default Login;
