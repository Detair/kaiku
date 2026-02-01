import { Component, createSignal, createResource, Show, For } from "solid-js";
import { A, useNavigate } from "@solidjs/router";
import { register, loginWithOidc, authState, clearError } from "@/stores/auth";
import { fetchServerSettings, oidcAuthorize } from "@/lib/tauri";
import type { OidcProvider } from "@/lib/types";
import { Github, Chrome, KeyRound, ShieldAlert } from "lucide-solid";

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
  const navigate = useNavigate();
  const defaultServerUrl = import.meta.env.VITE_SERVER_URL || "";
  const [serverUrl, setServerUrl] = createSignal(defaultServerUrl);
  const [username, setUsername] = createSignal("");
  const [email, setEmail] = createSignal("");
  const [displayName, setDisplayName] = createSignal("");
  const [password, setPassword] = createSignal("");
  const [confirmPassword, setConfirmPassword] = createSignal("");
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

  let urlTimer: ReturnType<typeof setTimeout> | undefined;
  const handleServerUrlChange = (value: string) => {
    setServerUrl(value);
    clearTimeout(urlTimer);
    urlTimer = setTimeout(() => setSettingsUrl(value), 500);
  };

  const handleRegister = async (e: Event) => {
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
    if (username().length < 3) {
      setLocalError("Username must be at least 3 characters");
      return;
    }
    if (!password()) {
      setLocalError("Password is required");
      return;
    }
    if (password().length < 8) {
      setLocalError("Password must be at least 8 characters");
      return;
    }
    if (password() !== confirmPassword()) {
      setLocalError("Passwords do not match");
      return;
    }

    try {
      await register(
        serverUrl(),
        username(),
        password(),
        email() || undefined,
        displayName() || undefined
      );
      navigate("/", { replace: true });
    } catch (err) {
      // Error is already set in auth store
    }
  };

  const handleOidcRegister = async (provider: OidcProvider) => {
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
          result.tokens.expires_in || 900
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
            event.data.expires_in || 900
          ).then(() => {
            navigate("/", { replace: true });
          }).catch(() => {
            // Error is set in auth store
          }).finally(() => {
            setOidcLoading(null);
          });
        }
      };
      window.addEventListener("message", messageHandler);

      const popup = window.open(result.authUrl, "oidc-register", "width=600,height=700");

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
    return s?.oidc_enabled && s.oidc_providers.length > 0 && s.registration_policy !== "closed";
  };

  const error = () => localError() || authState.error;

  return (
    <div class="flex items-center justify-center min-h-screen bg-background-primary py-8">
      <div class="w-full max-w-md p-8 bg-background-secondary rounded-lg shadow-lg">
        <h1 class="text-2xl font-bold mb-2 text-center text-text-primary">
          Create an account
        </h1>
        <p class="text-text-secondary text-center mb-6">
          Join VoiceChat to start chatting
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
            disabled={authState.isLoading || !!oidcLoading()}
            required
          />
        </div>

        {/* Registration closed message */}
        <Show when={isClosed()}>
          <div class="flex items-center gap-3 p-4 rounded-lg border border-white/10 bg-white/5 text-text-secondary">
            <ShieldAlert class="w-5 h-5 flex-shrink-0" />
            <div>
              <p class="font-medium text-text-primary">Registration is closed</p>
              <p class="text-sm mt-1">
                This server is not accepting new registrations. Contact the server admin for an invite.
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
                    Sign up with {provider.display_name}
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
                <span class="bg-background-secondary px-3 text-text-muted">or</span>
              </div>
            </div>
          </Show>
        </Show>

        {/* Local Registration Form */}
        <Show when={showLocalRegister()}>
          <form onSubmit={handleRegister} class="space-y-4">
            <div>
              <label class="block text-sm font-medium text-text-secondary mb-1">
                Username <span class="text-danger">*</span>
              </label>
              <input
                type="text"
                class="input-field"
                placeholder="Choose a username"
                value={username()}
                onInput={(e) => setUsername(e.currentTarget.value)}
                disabled={authState.isLoading}
                required
              />
              <p class="text-xs text-text-muted mt-1">
                3-32 characters, lowercase letters, numbers, and underscores only
              </p>
            </div>

            <div>
              <label class="block text-sm font-medium text-text-secondary mb-1">
                Email <span class="text-text-muted">(optional)</span>
              </label>
              <input
                type="email"
                class="input-field"
                placeholder="your@email.com"
                value={email()}
                onInput={(e) => setEmail(e.currentTarget.value)}
                disabled={authState.isLoading}
              />
            </div>

            <div>
              <label class="block text-sm font-medium text-text-secondary mb-1">
                Display Name <span class="text-text-muted">(optional)</span>
              </label>
              <input
                type="text"
                class="input-field"
                placeholder="How others will see you"
                value={displayName()}
                onInput={(e) => setDisplayName(e.currentTarget.value)}
                disabled={authState.isLoading}
              />
            </div>

            <div>
              <label class="block text-sm font-medium text-text-secondary mb-1">
                Password <span class="text-danger">*</span>
              </label>
              <input
                type="password"
                class="input-field"
                placeholder="Create a password"
                value={password()}
                onInput={(e) => setPassword(e.currentTarget.value)}
                disabled={authState.isLoading}
                required
              />
              <p class="text-xs text-text-muted mt-1">
                At least 8 characters
              </p>
            </div>

            <div>
              <label class="block text-sm font-medium text-text-secondary mb-1">
                Confirm Password <span class="text-danger">*</span>
              </label>
              <input
                type="password"
                class="input-field"
                placeholder="Confirm your password"
                value={confirmPassword()}
                onInput={(e) => setConfirmPassword(e.currentTarget.value)}
                disabled={authState.isLoading}
                required
              />
            </div>

            <Show when={error()}>
              <div class="p-3 rounded-md text-sm" style="background-color: var(--color-error-bg); border: 1px solid var(--color-error-border); color: var(--color-error-text)">
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
                    Creating account...
                  </>
                }
              >
                Create Account
              </Show>
            </button>
          </form>
        </Show>

        {/* Error display when no local form */}
        <Show when={!showLocalRegister() && !isClosed() && error()}>
          <div class="p-3 rounded-md text-sm" style="background-color: var(--color-error-bg); border: 1px solid var(--color-error-border); color: var(--color-error-text)">
            {error()}
          </div>
        </Show>

        <p class="text-center text-sm text-text-secondary mt-4">
          Already have an account?{" "}
          <A href="/login" class="text-primary hover:underline">
            Login
          </A>
        </p>
      </div>
    </div>
  );
};

export default Register;
