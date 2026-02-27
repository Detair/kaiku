import { Component, createSignal, createResource, Show, For } from "solid-js";
import { A, useNavigate } from "@solidjs/router";
import {
  login,
  loginWithOidc,
  authState,
  clearError,
  setAuthState,
} from "@/stores/auth";
import { fetchServerSettings, oidcAuthorize } from "@/lib/tauri";
import type { OidcProvider } from "@/lib/types";
import { Github, Chrome, KeyRound, ShieldCheck } from "lucide-solid";

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
  const navigate = useNavigate();
  const defaultServerUrl = import.meta.env.VITE_SERVER_URL || "";
  const [serverUrl, setServerUrl] = createSignal(defaultServerUrl);
  const [username, setUsername] = createSignal("");
  const [password, setPassword] = createSignal("");
  const [mfaCode, setMfaCode] = createSignal("");
  const [localError, setLocalError] = createSignal("");
  const [oidcLoading, setOidcLoading] = createSignal<string | null>(null);

  // Fetch server settings when server URL is entered (debounced)
  const [settingsUrl, setSettingsUrl] = createSignal(defaultServerUrl);
  const [settings] = createResource(settingsUrl, async (url) => {
    if (!url.trim()) return null;
    try {
      return await fetchServerSettings(url);
    } catch {
      return null;
    }
  });

  // Debounce server URL changes for settings fetch
  let urlTimer: ReturnType<typeof setTimeout> | undefined;
  const handleServerUrlChange = (value: string) => {
    setServerUrl(value);
    clearTimeout(urlTimer);
    urlTimer = setTimeout(() => setSettingsUrl(value), 500);
  };

  const handleLogin = async (e: Event) => {
    e.preventDefault();
    setLocalError("");
    clearError();

    if (!serverUrl().trim()) {
      setLocalError("Server URL is required");
      return;
    }
    if (!username().trim()) {
      setLocalError("Username is required");
      return;
    }
    if (!password()) {
      setLocalError("Password is required");
      return;
    }

    // If MFA is required and no code provided
    if (authState.mfaRequired && !mfaCode().trim()) {
      setLocalError("MFA code is required");
      return;
    }

    try {
      await login(
        serverUrl(),
        username(),
        password(),
        authState.mfaRequired ? mfaCode() : undefined,
      );
      navigate("/", { replace: true });
    } catch (err) {
      // MFA_REQUIRED is handled by the store — just reset MFA code input
      const msg = err instanceof Error ? err.message : String(err);
      if (msg === "MFA_REQUIRED") {
        setMfaCode("");
        // Error is not set — mfaRequired flag drives the UI
      }
      // Other errors are already set in auth store
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
        // Tauri: tokens returned directly from the command
        await loginWithOidc(
          serverUrl(),
          result.tokens.access_token,
          result.tokens.refresh_token,
          result.tokens.expires_in || 900,
          result.tokens.setup_required ?? false,
        );
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
            .catch(() => {
              // Error is set in auth store
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
      const error = err instanceof Error ? err.message : String(err);
      setLocalError(error);
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

  const error = () => localError() || authState.error;

  return (
    <div class="flex items-center justify-center min-h-screen bg-background-primary">
      <div class="w-full max-w-md p-8 bg-background-secondary rounded-lg shadow-lg">
        <h1 class="text-2xl font-bold mb-2 text-center text-text-primary">
          Welcome back!
        </h1>
        <p class="text-text-secondary text-center mb-6">
          Login to continue to VoiceChat
        </p>

        {/* Server URL (always shown) */}
        <div class="mb-4">
          <label class="block text-sm font-medium text-text-secondary mb-1">
            Server URL
          </label>
          <input
            type="url"
            class="input-field"
            placeholder="https://chat.example.com"
            value={serverUrl()}
            onInput={(e) => handleServerUrlChange(e.currentTarget.value)}
            disabled={
              authState.isLoading || !!oidcLoading() || authState.mfaRequired
            }
            required
          />
        </div>

        {/* MFA Code Step */}
        <Show when={authState.mfaRequired}>
          <form onSubmit={handleLogin} class="space-y-4">
            <div class="flex items-center gap-3 p-3 bg-accent-primary/10 border border-accent-primary/20 rounded-lg">
              <ShieldCheck class="w-5 h-5 text-accent-primary flex-shrink-0" />
              <p class="text-sm text-text-secondary">
                Two-factor authentication is enabled. Enter a code from your
                authenticator app or a backup code.
              </p>
            </div>

            <div>
              <label class="block text-sm font-medium text-text-secondary mb-1">
                MFA Code
              </label>
              <input
                type="text"
                class="input-field font-mono text-center text-lg tracking-widest"
                placeholder="000000"
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
                Enter a 6-digit TOTP code or an 8-character backup code.
              </p>
            </div>

            <Show when={error()}>
              <div
                class="p-3 rounded-md text-sm"
                style="background-color: var(--color-error-bg); border: 1px solid var(--color-error-border); color: var(--color-error-text)"
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
                    Verifying...
                  </>
                }
              >
                Verify
              </Show>
            </button>

            <button
              type="button"
              onClick={handleBackToLogin}
              class="w-full text-sm text-text-secondary hover:text-text-primary transition-colors"
            >
              Back to login
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
                      Continue with {provider.display_name}
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
                    or
                  </span>
                </div>
              </div>
            </Show>
          </Show>

          {/* Local Login Form */}
          <Show when={showLocalLogin()}>
            <form onSubmit={handleLogin} class="space-y-4">
              <div>
                <label class="block text-sm font-medium text-text-secondary mb-1">
                  Username
                </label>
                <input
                  type="text"
                  class="input-field"
                  placeholder="Enter your username"
                  value={username()}
                  onInput={(e) => setUsername(e.currentTarget.value)}
                  disabled={authState.isLoading}
                  required
                />
              </div>

              <div>
                <label class="block text-sm font-medium text-text-secondary mb-1">
                  Password
                </label>
                <input
                  type="password"
                  class="input-field"
                  placeholder="Enter your password"
                  value={password()}
                  onInput={(e) => setPassword(e.currentTarget.value)}
                  disabled={authState.isLoading}
                  required
                />
              </div>

              <div class="text-right">
                <A
                  href="/forgot-password"
                  class="text-sm text-primary hover:underline"
                >
                  Forgot password?
                </A>
              </div>

              <Show when={error()}>
                <div
                  class="p-3 rounded-md text-sm"
                  style="background-color: var(--color-error-bg); border: 1px solid var(--color-error-border); color: var(--color-error-text)"
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
                      Logging in...
                    </>
                  }
                >
                  Login
                </Show>
              </button>
            </form>
          </Show>

          {/* Error display when no local login form */}
          <Show when={!showLocalLogin() && error()}>
            <div
              class="p-3 rounded-md text-sm"
              style="background-color: var(--color-error-bg); border: 1px solid var(--color-error-border); color: var(--color-error-text)"
            >
              {error()}
            </div>
          </Show>

          <p class="text-center text-sm text-text-secondary mt-4">
            Don't have an account?{" "}
            <A href="/register" class="text-primary hover:underline">
              Register
            </A>
          </p>
        </Show>
      </div>
    </div>
  );
};

export default Login;
