import { Component, createSignal, Show } from "solid-js";
import { A, useNavigate } from "@solidjs/router";
import { login, authState, clearError } from "@/stores/auth";

const Login: Component = () => {
  const navigate = useNavigate();
  // Use environment variable for default server URL
  const defaultServerUrl = import.meta.env.VITE_SERVER_URL || "";
  const [serverUrl, setServerUrl] = createSignal(defaultServerUrl);
  const [username, setUsername] = createSignal("");
  const [password, setPassword] = createSignal("");
  const [localError, setLocalError] = createSignal("");

  const handleLogin = async (e: Event) => {
    e.preventDefault();
    setLocalError("");
    clearError();

    // Validate inputs
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

    try {
      await login(serverUrl(), username(), password());
      navigate("/", { replace: true });
    } catch (err) {
      // Error is already set in auth store
    }
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

        <form onSubmit={handleLogin} class="space-y-4">
          <div>
            <label class="block text-sm font-medium text-text-secondary mb-1">
              Server URL
            </label>
            <input
              type="url"
              class="input-field"
              placeholder="https://chat.example.com"
              value={serverUrl()}
              onInput={(e) => setServerUrl(e.currentTarget.value)}
              disabled={authState.isLoading}
              required
            />
          </div>

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
                  Logging in...
                </>
              }
            >
              Login
            </Show>
          </button>

          <p class="text-center text-sm text-text-secondary mt-4">
            Don't have an account?{" "}
            <A href="/register" class="text-primary hover:underline">
              Register
            </A>
          </p>
        </form>
      </div>
    </div>
  );
};

export default Login;
