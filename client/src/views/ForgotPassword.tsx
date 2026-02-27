import { Component, createSignal, Show } from "solid-js";
import { A } from "@solidjs/router";

const ForgotPassword: Component = () => {
  const defaultServerUrl = import.meta.env.VITE_SERVER_URL || "";
  const storedUrl =
    typeof localStorage !== "undefined"
      ? localStorage.getItem("serverUrl") || ""
      : "";
  const [serverUrl, setServerUrl] = createSignal(storedUrl || defaultServerUrl);
  const [email, setEmail] = createSignal("");
  const [error, setError] = createSignal("");
  const [success, setSuccess] = createSignal(false);
  const [isLoading, setIsLoading] = createSignal(false);

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    setError("");
    setSuccess(false);

    if (!serverUrl().trim()) {
      setError("Server URL is required");
      return;
    }
    if (!email().trim() || !email().includes("@")) {
      setError("Please enter a valid email address");
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
        const data = await res.json().catch(() => null);
        throw new Error(data?.message || `Request failed (${res.status})`);
      }

      setSuccess(true);
    } catch (err: unknown) {
      const message = err instanceof Error ? err.message : "An error occurred";
      setError(message);
    } finally {
      setIsLoading(false);
    }
  };

  return (
    <div class="flex items-center justify-center min-h-screen bg-background-primary">
      <div class="w-full max-w-md p-8 bg-background-secondary rounded-lg shadow-lg">
        <h1 class="text-2xl font-bold mb-2 text-center text-text-primary">
          Forgot Password
        </h1>
        <p class="text-text-secondary text-center mb-6">
          Enter your email to receive a reset code
        </p>

        <Show when={!success()}>
          <form onSubmit={handleSubmit} class="space-y-4">
            <div>
              <label
                for="fp-server-url"
                class="block text-sm font-medium text-text-secondary mb-1"
              >
                Server URL
              </label>
              <input
                id="fp-server-url"
                type="url"
                class="input-field"
                placeholder="https://chat.example.com"
                value={serverUrl()}
                onInput={(e) => setServerUrl(e.currentTarget.value)}
                disabled={isLoading()}
                required
              />
            </div>

            <div>
              <label
                for="fp-email"
                class="block text-sm font-medium text-text-secondary mb-1"
              >
                Email
              </label>
              <input
                id="fp-email"
                type="email"
                class="input-field"
                placeholder="you@example.com"
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
                style="background-color: var(--color-error-bg); border: 1px solid var(--color-error-border); color: var(--color-error-text)"
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
                    Sending...
                  </>
                }
              >
                Send Reset Code
              </Show>
            </button>

            <p class="text-center text-sm text-text-secondary mt-4">
              <A href="/login" class="text-primary hover:underline">
                Back to Login
              </A>
            </p>
          </form>
        </Show>

        <Show when={success()}>
          <div
            class="p-4 rounded-md text-sm mb-4"
            style="background-color: var(--color-success-bg, rgba(34,197,94,0.1)); border: 1px solid var(--color-success-border, rgba(34,197,94,0.3)); color: var(--color-success-text, #22c55e)"
          >
            If an account with that email exists, a reset code has been sent.
            Check your inbox.
          </div>
          <div class="space-y-3">
            <A
              href={`/reset-password?serverUrl=${encodeURIComponent(serverUrl())}`}
              class="btn-primary w-full flex items-center justify-center"
            >
              Enter Reset Code
            </A>
            <p class="text-center text-sm text-text-secondary">
              <A href="/login" class="text-primary hover:underline">
                Back to Login
              </A>
            </p>
          </div>
        </Show>
      </div>
    </div>
  );
};

export default ForgotPassword;
