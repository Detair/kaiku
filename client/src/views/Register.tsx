import { Component, createSignal, Show } from "solid-js";
import { A, useNavigate } from "@solidjs/router";
import { register, authState, clearError } from "@/stores/auth";

const Register: Component = () => {
  const navigate = useNavigate();
  const [serverUrl, setServerUrl] = createSignal("");
  const [username, setUsername] = createSignal("");
  const [email, setEmail] = createSignal("");
  const [displayName, setDisplayName] = createSignal("");
  const [password, setPassword] = createSignal("");
  const [confirmPassword, setConfirmPassword] = createSignal("");
  const [localError, setLocalError] = createSignal("");

  const handleRegister = async (e: Event) => {
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

        <form onSubmit={handleRegister} class="space-y-4">
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
            <div class="p-3 bg-danger/10 border border-danger/20 rounded-md text-danger text-sm">
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

          <p class="text-center text-sm text-text-secondary mt-4">
            Already have an account?{" "}
            <A href="/login" class="text-primary hover:underline">
              Login
            </A>
          </p>
        </form>
      </div>
    </div>
  );
};

export default Register;
