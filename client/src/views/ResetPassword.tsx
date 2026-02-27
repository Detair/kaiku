import { Component, createSignal, Show } from "solid-js";
import { A, useSearchParams } from "@solidjs/router";

const ResetPassword: Component = () => {
  const [searchParams] = useSearchParams();
  const defaultServerUrl = import.meta.env.VITE_SERVER_URL || "";
  const storedUrl =
    typeof localStorage !== "undefined"
      ? localStorage.getItem("serverUrl") || ""
      : "";
  const initialUrl = searchParams.serverUrl || storedUrl || defaultServerUrl;
  const [serverUrl, setServerUrl] = createSignal(initialUrl);
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

    if (!serverUrl().trim()) {
      setError("Server URL is required");
      return;
    }
    if (!token().trim()) {
      setError("Reset code is required");
      return;
    }
    if (newPassword().length < 8 || newPassword().length > 128) {
      setError("Password must be between 8 and 128 characters");
      return;
    }
    if (newPassword() !== confirmPassword()) {
      setError("Passwords do not match");
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
          Reset Password
        </h1>
        <p class="text-text-secondary text-center mb-6">
          Enter the code from your email and a new password
        </p>

        <Show when={!success()}>
          <form onSubmit={handleSubmit} class="space-y-4">
            <div>
              <label
                for="rp-server-url"
                class="block text-sm font-medium text-text-secondary mb-1"
              >
                Server URL
              </label>
              <input
                id="rp-server-url"
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
                for="rp-token"
                class="block text-sm font-medium text-text-secondary mb-1"
              >
                Reset Code
              </label>
              <input
                id="rp-token"
                type="text"
                class="input-field"
                placeholder="Paste reset code from email"
                value={token()}
                onInput={(e) => setToken(e.currentTarget.value)}
                disabled={isLoading()}
                required
                autocomplete="off"
              />
            </div>

            <div>
              <label
                for="rp-new-password"
                class="block text-sm font-medium text-text-secondary mb-1"
              >
                New Password
              </label>
              <input
                id="rp-new-password"
                type="password"
                class="input-field"
                placeholder="Enter new password"
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
                Confirm Password
              </label>
              <input
                id="rp-confirm-password"
                type="password"
                class="input-field"
                placeholder="Confirm new password"
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
                    Resetting...
                  </>
                }
              >
                Reset Password
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
            Password has been reset successfully. You can now log in with your
            new password.
          </div>
          <A
            href="/login"
            class="btn-primary w-full flex items-center justify-center"
          >
            Go to Login
          </A>
        </Show>
      </div>
    </div>
  );
};

export default ResetPassword;
