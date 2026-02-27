/**
 * SetupWizard Component
 *
 * First-time server setup wizard for administrators.
 * Shows a mandatory blocking modal that guides the first admin through
 * configuring the server name, registration policy, and legal URLs.
 *
 * This wizard only appears when:
 * - User is the first user (automatically granted admin)
 * - Server setup has not been completed
 * - setup_required flag is true in auth response
 */

import { Component, createSignal, createEffect, Show } from "solid-js";
import { authState, clearSetupRequired } from "@/stores/auth";
import { AlertCircle, CheckCircle, Server } from "lucide-solid";

// Setup config interface (matches server API)
interface SetupConfig {
  server_name: string;
  registration_policy: "open" | "invite_only" | "closed";
  terms_url?: string;
  privacy_url?: string;
}

// Detect if running in Tauri
const isTauri = typeof window !== "undefined" && "__TAURI__" in window;

// Get server URL from auth state or localStorage
function getServerUrl(): string {
  if (authState.serverUrl) {
    return authState.serverUrl;
  }
  if (typeof localStorage !== "undefined") {
    return localStorage.getItem("serverUrl") || "http://localhost:8080";
  }
  return "http://localhost:8080";
}

// Fetch setup config from server
async function fetchSetupConfig(): Promise<SetupConfig> {
  const serverUrl = getServerUrl();
  const response = await fetch(`${serverUrl}/api/setup/config`, {
    method: "GET",
    headers: { "Content-Type": "application/json" },
  });

  if (!response.ok) {
    let errorMessage = `Failed to fetch setup config (HTTP ${response.status})`;

    try {
      const errorBody = await response.json();
      errorMessage = errorBody.message || errorBody.error || errorMessage;
    } catch {
      errorMessage += `: ${response.statusText}`;
    }

    throw new Error(errorMessage);
  }

  try {
    return await response.json();
  } catch (parseError) {
    // Distinguish between JSON syntax errors and other unexpected errors
    if (parseError instanceof SyntaxError) {
      throw new Error(`Server returned invalid JSON: ${parseError.message}`);
    } else {
      // Unexpected error (OOM, extension interference, etc.)
      console.error("[SetupWizard] Unexpected error parsing JSON:", parseError);
      throw new Error(
        `Failed to parse server response: ${parseError instanceof Error ? parseError.message : "Unknown error"}`,
      );
    }
  }
}

// Complete server setup
async function completeSetup(config: SetupConfig): Promise<void> {
  const serverUrl = getServerUrl();

  // Get access token
  let accessToken: string | null = null;
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    accessToken = await invoke<string>("get_access_token");
  } else if (typeof localStorage !== "undefined") {
    accessToken = localStorage.getItem("accessToken");
  }

  if (!accessToken) {
    throw new Error("No access token available. Please log in again.");
  }

  // Basic JWT validation: should have 3 parts (header.payload.signature)
  const parts = accessToken.split(".");
  if (parts.length !== 3) {
    throw new Error("Access token is malformed. Please log in again.");
  }

  // Optional: Check if token is expired
  try {
    const payload = JSON.parse(atob(parts[1]));
    if (payload.exp && payload.exp * 1000 < Date.now()) {
      throw new Error("Access token has expired. Please log in again.");
    }
  } catch (e) {
    // If we can't decode the token, continue - server will validate
    // Only warn if it's not the expiry check that failed
    if (e instanceof Error && !e.message.includes("expired")) {
      console.warn("[SetupWizard] Failed to decode token for expiry check:", e);
    } else {
      throw e; // Re-throw expiry errors
    }
  }

  const response = await fetch(`${serverUrl}/api/setup/complete`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${accessToken}`,
    },
    body: JSON.stringify(config),
  });

  if (!response.ok) {
    let errorMessage = `Setup failed (HTTP ${response.status})`;

    // Read body as text first to avoid double consumption
    let bodyText = "";
    try {
      bodyText = await response.text();
    } catch (textError) {
      console.error(
        "[SetupWizard] Failed to read error response body:",
        textError,
      );
    }

    // Try to parse as JSON
    try {
      if (bodyText) {
        const errorBody = JSON.parse(bodyText);
        errorMessage = errorBody.message || errorBody.error || errorMessage;

        // Handle specific error codes
        if (
          response.status === 403 &&
          errorBody.error === "SETUP_ALREADY_COMPLETE"
        ) {
          // Setup was completed by another admin - close the wizard
          console.warn(
            "[SetupWizard] Setup already completed by another admin",
          );
          clearSetupRequired();
          return;
        }

        if (response.status === 401) {
          throw new Error("Your session has expired. Please log in again.");
        }
      }
    } catch (parseError) {
      // If JSON parse fails but we have specific errors from above, re-throw them
      if (
        parseError instanceof Error &&
        parseError.message.includes("session has expired")
      ) {
        throw parseError;
      }
      // Otherwise, use the raw text as error message
      if (bodyText.length > 0 && bodyText.length < 500) {
        errorMessage += `: ${bodyText}`;
      }
    }

    throw new Error(errorMessage);
  }
}

const SetupWizard: Component = () => {
  // Form state
  const [serverName, setServerName] = createSignal("");
  const [registrationPolicy, setRegistrationPolicy] = createSignal<
    "open" | "invite_only" | "closed"
  >("open");
  const [termsUrl, setTermsUrl] = createSignal("");
  const [privacyUrl, setPrivacyUrl] = createSignal("");

  // UI state
  const [isLoading, setIsLoading] = createSignal(false);
  const [isLoadingConfig, setIsLoadingConfig] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [isConfigLoaded, setIsConfigLoaded] = createSignal(false);

  // Load config from server (async function called by effect)
  async function loadConfig() {
    if (!authState.setupRequired || isConfigLoaded()) return;

    setIsLoadingConfig(true);
    setError(null);

    try {
      const config = await fetchSetupConfig();
      setServerName(config.server_name);
      setRegistrationPolicy(config.registration_policy);
      setTermsUrl(config.terms_url || "");
      setPrivacyUrl(config.privacy_url || "");
      setIsConfigLoaded(true);
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : "Unknown error";
      console.error("[SetupWizard] Failed to load config:", {
        error: errorMessage,
        setupRequired: authState.setupRequired,
        timestamp: new Date().toISOString(),
      });
      setError(
        `Failed to load setup configuration. Please check your connection and refresh the page. Error: ${errorMessage}`,
      );
    } finally {
      setIsLoadingConfig(false);
    }
  }

  // Load current config when wizard opens
  createEffect(() => {
    if (!authState.setupRequired || isConfigLoaded()) return;
    loadConfig();
  });

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    setError(null);
    setIsLoading(true);

    try {
      const config: SetupConfig = {
        server_name: serverName().trim(),
        registration_policy: registrationPolicy(),
      };

      // Add optional URLs only if provided
      if (termsUrl().trim()) {
        config.terms_url = termsUrl().trim();
      }
      if (privacyUrl().trim()) {
        config.privacy_url = privacyUrl().trim();
      }

      await completeSetup(config);

      // Update auth state to hide the wizard
      clearSetupRequired();

      console.log("[SetupWizard] Server setup completed successfully");
    } catch (err) {
      console.error("[SetupWizard] Setup failed:", err);
      setError(err instanceof Error ? err.message : "Failed to complete setup");
      setIsLoading(false);
    }
  };

  // Only show wizard if setup is required
  return (
    <Show when={authState.setupRequired}>
      <div class="fixed inset-0 bg-black/80 flex items-center justify-center z-50">
        <div class="bg-surface-layer2 rounded-xl p-6 w-[32rem] shadow-2xl border border-white/10">
          {/* Header */}
          <div class="flex items-center gap-3 mb-6">
            <div class="p-3 bg-accent-primary/20 rounded-lg">
              <Server class="w-6 h-6 text-accent-primary" />
            </div>
            <div>
              <h2 class="text-xl font-bold text-text-primary">
                Welcome to Canis! 🎉
              </h2>
              <p class="text-sm text-text-muted">
                You're the first user. Let's set up your server.
              </p>
            </div>
          </div>

          {/* Info banner */}
          <div class="mb-6 px-4 py-3 bg-blue-500/10 border border-blue-500/30 rounded-lg">
            <p class="text-sm text-blue-200">
              As the first user, you've been granted{" "}
              <strong>system admin</strong> permissions. These settings can be
              changed later in the admin panel.
            </p>
          </div>

          {/* Loading state */}
          <Show when={isLoadingConfig()}>
            <div class="flex items-center justify-center p-8">
              <div class="animate-spin rounded-full h-12 w-12 border-b-2 border-accent-primary"></div>
            </div>
          </Show>

          {/* Error banner */}
          <Show when={error()}>
            <div class="mb-4 flex items-start gap-3 px-4 py-3 bg-red-500/20 border border-red-500/50 rounded-lg text-red-200">
              <AlertCircle class="w-5 h-5 flex-shrink-0 mt-0.5" />
              <span class="text-sm">{error()}</span>
            </div>
          </Show>

          {/* Setup form */}
          <form onSubmit={handleSubmit} class="space-y-4">
            {/* Server Name */}
            <div>
              <label class="block text-sm font-medium text-text-secondary mb-2">
                Server Name <span class="text-red-400">*</span>
              </label>
              <input
                type="text"
                value={serverName()}
                onInput={(e) => setServerName(e.currentTarget.value)}
                placeholder="My Awesome Server"
                required
                maxLength={64}
                disabled={isLoading()}
                class="w-full px-3 py-2 bg-surface-base rounded-lg text-text-primary border border-white/10 focus:border-accent-primary focus:outline-none transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              />
              <p class="mt-1 text-xs text-text-muted">
                This appears in the app and invites
              </p>
            </div>

            {/* Registration Policy */}
            <div>
              <label class="block text-sm font-medium text-text-secondary mb-2">
                Registration Policy <span class="text-red-400">*</span>
              </label>
              <select
                value={registrationPolicy()}
                onChange={(e) =>
                  setRegistrationPolicy(
                    e.currentTarget.value as "open" | "invite_only" | "closed",
                  )
                }
                disabled={isLoading()}
                class="w-full px-3 py-2 bg-surface-base rounded-lg text-text-primary border border-white/10 focus:border-accent-primary focus:outline-none transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              >
                <option value="open">Open - Anyone can register</option>
                <option value="invite_only">
                  Invite Only - Requires invite code
                </option>
                <option value="closed">Closed - Registration disabled</option>
              </select>
              <p class="mt-1 text-xs text-text-muted">
                You can change this later in settings
              </p>
            </div>

            {/* Terms of Service URL */}
            <div>
              <label class="block text-sm font-medium text-text-secondary mb-2">
                Terms of Service URL
              </label>
              <input
                type="url"
                value={termsUrl()}
                onInput={(e) => setTermsUrl(e.currentTarget.value)}
                placeholder="https://example.com/terms"
                disabled={isLoading()}
                class="w-full px-3 py-2 bg-surface-base rounded-lg text-text-primary border border-white/10 focus:border-accent-primary focus:outline-none transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              />
              <p class="mt-1 text-xs text-text-muted">Optional</p>
            </div>

            {/* Privacy Policy URL */}
            <div>
              <label class="block text-sm font-medium text-text-secondary mb-2">
                Privacy Policy URL
              </label>
              <input
                type="url"
                value={privacyUrl()}
                onInput={(e) => setPrivacyUrl(e.currentTarget.value)}
                placeholder="https://example.com/privacy"
                disabled={isLoading()}
                class="w-full px-3 py-2 bg-surface-base rounded-lg text-text-primary border border-white/10 focus:border-accent-primary focus:outline-none transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              />
              <p class="mt-1 text-xs text-text-muted">Optional</p>
            </div>

            {/* Submit button */}
            <div class="flex items-center justify-end gap-3 pt-4 border-t border-white/10">
              <button
                type="submit"
                disabled={isLoading() || !serverName().trim()}
                class="flex items-center gap-2 px-6 py-2.5 bg-accent-primary hover:bg-accent-hover text-white font-medium rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed disabled:hover:bg-accent-primary"
              >
                <Show
                  when={!isLoading()}
                  fallback={
                    <>
                      <div class="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                      <span>Completing Setup...</span>
                    </>
                  }
                >
                  <CheckCircle class="w-4 h-4" />
                  <span>Complete Setup</span>
                </Show>
              </button>
            </div>
          </form>

          {/* Footer note */}
          <div class="mt-4 pt-4 border-t border-white/10">
            <p class="text-xs text-text-muted text-center">
              <strong>Note:</strong> Setup can only be completed once and cannot
              be undone.
            </p>
          </div>
        </div>
      </div>
    </Show>
  );
};

export default SetupWizard;
