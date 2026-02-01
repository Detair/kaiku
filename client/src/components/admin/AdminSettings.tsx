/**
 * AdminSettings - Auth methods, registration policy, and OIDC provider management.
 *
 * Requires elevated admin session. All mutations reload the provider list.
 */

import { Component, createSignal, For, Show, onMount } from "solid-js";
import { createStore } from "solid-js/store";
import {
  Plus,
  Trash2,
  Github,
  Chrome,
  KeyRound,
  AlertTriangle,
  Check,
  X,
  Loader2,
} from "lucide-solid";
import {
  adminGetAuthSettings,
  adminUpdateAuthSettings,
  adminListOidcProviders,
  adminCreateOidcProvider,
  adminUpdateOidcProvider,
  adminDeleteOidcProvider,
} from "@/lib/tauri";
import type { AuthMethodsConfig, AdminOidcProvider } from "@/lib/types";
import { adminState } from "@/stores/admin";

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

interface SettingsState {
  authMethods: AuthMethodsConfig;
  registrationPolicy: string;
  providers: AdminOidcProvider[];
  isLoading: boolean;
  isSaving: boolean;
  error: string | null;
  success: string | null;
}

const AdminSettings: Component = () => {
  const [state, setState] = createStore<SettingsState>({
    authMethods: { local: true, oidc: false },
    registrationPolicy: "open",
    providers: [],
    isLoading: true,
    isSaving: false,
    error: null,
    success: null,
  });

  const [showAddProvider, setShowAddProvider] = createSignal(false);
  const [deletingProvider, setDeletingProvider] = createSignal<string | null>(null);

  // Add provider form state
  const [addForm, setAddForm] = createStore({
    slug: "",
    display_name: "",
    icon_hint: "",
    client_id: "",
    client_secret: "",
    issuer_url: "",
    scopes: "openid profile email",
    preset: "" as "" | "github" | "google" | "custom",
  });

  const loadSettings = async () => {
    setState({ isLoading: true, error: null });
    try {
      const [settings, providers] = await Promise.all([
        adminGetAuthSettings(),
        adminListOidcProviders(),
      ]);
      setState({
        authMethods: settings.auth_methods,
        registrationPolicy: settings.registration_policy,
        providers,
        isLoading: false,
      });
    } catch (err) {
      setState({
        isLoading: false,
        error: err instanceof Error ? err.message : String(err),
      });
    }
  };

  onMount(loadSettings);

  const saveAuthSettings = async (
    methods?: AuthMethodsConfig,
    policy?: string
  ) => {
    setState({ isSaving: true, error: null, success: null });
    try {
      const result = await adminUpdateAuthSettings({
        auth_methods: methods,
        registration_policy: policy,
      });
      setState({
        authMethods: result.auth_methods,
        registrationPolicy: result.registration_policy,
        isSaving: false,
        success: "Settings saved",
      });
      setTimeout(() => setState({ success: null }), 3000);
    } catch (err) {
      setState({
        isSaving: false,
        error: err instanceof Error ? err.message : String(err),
      });
    }
  };

  const handleToggleLocal = () => {
    const newMethods = { ...state.authMethods, local: !state.authMethods.local };
    // Prevent disabling all methods
    if (!newMethods.local && !newMethods.oidc) {
      setState({ error: "At least one auth method must be enabled" });
      return;
    }
    saveAuthSettings(newMethods);
  };

  const handleToggleOidc = () => {
    const newMethods = { ...state.authMethods, oidc: !state.authMethods.oidc };
    if (!newMethods.local && !newMethods.oidc) {
      setState({ error: "At least one auth method must be enabled" });
      return;
    }
    saveAuthSettings(newMethods);
  };

  const handlePolicyChange = (policy: string) => {
    saveAuthSettings(undefined, policy);
  };

  const applyPreset = (preset: string) => {
    switch (preset) {
      case "github":
        setAddForm({
          preset: "github",
          slug: "github",
          display_name: "GitHub",
          icon_hint: "github",
          issuer_url: "",
          scopes: "read:user user:email",
        });
        break;
      case "google":
        setAddForm({
          preset: "google",
          slug: "google",
          display_name: "Google",
          icon_hint: "chrome",
          issuer_url: "https://accounts.google.com",
          scopes: "openid profile email",
        });
        break;
      default:
        setAddForm({
          preset: "custom",
          slug: "",
          display_name: "",
          icon_hint: "",
          issuer_url: "",
          scopes: "openid profile email",
        });
    }
  };

  const handleCreateProvider = async () => {
    setState({ isSaving: true, error: null });
    try {
      await adminCreateOidcProvider({
        slug: addForm.slug,
        display_name: addForm.display_name,
        icon_hint: addForm.icon_hint || undefined,
        client_id: addForm.client_id,
        client_secret: addForm.client_secret,
        issuer_url: addForm.issuer_url || undefined,
        scopes: addForm.scopes || undefined,
      });
      setShowAddProvider(false);
      setAddForm({
        slug: "",
        display_name: "",
        icon_hint: "",
        client_id: "",
        client_secret: "",
        issuer_url: "",
        scopes: "openid profile email",
        preset: "",
      });
      await loadSettings();
      setState({ success: "Provider created" });
      setTimeout(() => setState({ success: null }), 3000);
    } catch (err) {
      setState({
        isSaving: false,
        error: err instanceof Error ? err.message : String(err),
      });
    }
  };

  const handleDeleteProvider = async (id: string) => {
    setState({ error: null });
    try {
      await adminDeleteOidcProvider(id);
      setDeletingProvider(null);
      await loadSettings();
      setState({ success: "Provider deleted" });
      setTimeout(() => setState({ success: null }), 3000);
    } catch (err) {
      setState({
        error: err instanceof Error ? err.message : String(err),
      });
    }
  };

  const handleToggleProviderEnabled = async (provider: AdminOidcProvider) => {
    try {
      await adminUpdateOidcProvider(provider.id, {
        display_name: provider.display_name,
        icon_hint: provider.icon_hint || undefined,
        issuer_url: provider.issuer_url || undefined,
        authorization_url: provider.authorization_url || undefined,
        token_url: provider.token_url || undefined,
        userinfo_url: provider.userinfo_url || undefined,
        client_id: provider.client_id,
        scopes: provider.scopes,
        enabled: !provider.enabled,
      });
      await loadSettings();
    } catch (err) {
      setState({
        error: err instanceof Error ? err.message : String(err),
      });
    }
  };

  return (
    <div class="flex-1 p-6 overflow-auto">
      <div class="max-w-3xl mx-auto space-y-8">
        {/* Header */}
        <div>
          <h2 class="text-lg font-bold text-text-primary">Server Settings</h2>
          <p class="text-sm text-text-secondary mt-1">
            Configure authentication methods, registration policy, and identity providers.
          </p>
        </div>

        {/* Elevation required notice */}
        <Show when={!adminState.isElevated}>
          <div class="p-4 rounded-xl bg-status-warning/10 border border-status-warning/30">
            <div class="flex items-center gap-3">
              <AlertTriangle class="w-5 h-5 text-status-warning" />
              <p class="text-sm text-status-warning">
                Settings require an elevated session. Elevate your session to make changes.
              </p>
            </div>
          </div>
        </Show>

        {/* Loading */}
        <Show when={state.isLoading}>
          <div class="flex items-center justify-center py-12">
            <Loader2 class="w-6 h-6 text-text-muted animate-spin" />
          </div>
        </Show>

        {/* Error / Success messages */}
        <Show when={state.error}>
          <div class="p-3 rounded-lg text-sm" style="background-color: var(--color-error-bg); border: 1px solid var(--color-error-border); color: var(--color-error-text)">
            {state.error}
          </div>
        </Show>
        <Show when={state.success}>
          <div class="p-3 rounded-lg text-sm bg-status-success/10 border border-status-success/30 text-status-success">
            {state.success}
          </div>
        </Show>

        <Show when={!state.isLoading}>
          {/* Auth Methods */}
          <section class="space-y-4">
            <h3 class="text-sm font-semibold text-text-primary uppercase tracking-wider">
              Authentication Methods
            </h3>

            <div class="space-y-3">
              {/* Local Auth Toggle */}
              <div class="flex items-center justify-between p-4 rounded-xl bg-white/5 border border-white/10">
                <div>
                  <div class="text-sm font-medium text-text-primary">Local Authentication</div>
                  <div class="text-xs text-text-muted mt-0.5">
                    Username/password registration and login
                  </div>
                </div>
                <button
                  onClick={handleToggleLocal}
                  class="relative w-11 h-6 rounded-full transition-colors"
                  classList={{
                    "bg-accent-primary": state.authMethods.local,
                    "bg-white/20": !state.authMethods.local,
                  }}
                  disabled={!adminState.isElevated || state.isSaving}
                >
                  <span
                    class="absolute top-0.5 left-0.5 w-5 h-5 rounded-full bg-white transition-transform"
                    classList={{
                      "translate-x-5": state.authMethods.local,
                      "translate-x-0": !state.authMethods.local,
                    }}
                  />
                </button>
              </div>

              {/* OIDC Toggle */}
              <div class="flex items-center justify-between p-4 rounded-xl bg-white/5 border border-white/10">
                <div>
                  <div class="text-sm font-medium text-text-primary">SSO / OIDC</div>
                  <div class="text-xs text-text-muted mt-0.5">
                    Login with external identity providers (GitHub, Google, etc.)
                  </div>
                </div>
                <button
                  onClick={handleToggleOidc}
                  class="relative w-11 h-6 rounded-full transition-colors"
                  classList={{
                    "bg-accent-primary": state.authMethods.oidc,
                    "bg-white/20": !state.authMethods.oidc,
                  }}
                  disabled={!adminState.isElevated || state.isSaving}
                >
                  <span
                    class="absolute top-0.5 left-0.5 w-5 h-5 rounded-full bg-white transition-transform"
                    classList={{
                      "translate-x-5": state.authMethods.oidc,
                      "translate-x-0": !state.authMethods.oidc,
                    }}
                  />
                </button>
              </div>
            </div>
          </section>

          {/* Registration Policy */}
          <section class="space-y-4">
            <h3 class="text-sm font-semibold text-text-primary uppercase tracking-wider">
              Registration Policy
            </h3>
            <div class="flex gap-3">
              <For each={[
                { value: "open", label: "Open", desc: "Anyone can register" },
                { value: "invite_only", label: "Invite Only", desc: "Requires invitation" },
                { value: "closed", label: "Closed", desc: "No new registrations" },
              ]}>
                {(option) => (
                  <button
                    onClick={() => handlePolicyChange(option.value)}
                    class="flex-1 p-3 rounded-xl border text-left transition-colors"
                    classList={{
                      "border-accent-primary bg-accent-primary/10": state.registrationPolicy === option.value,
                      "border-white/10 bg-white/5 hover:bg-white/10": state.registrationPolicy !== option.value,
                    }}
                    disabled={!adminState.isElevated || state.isSaving}
                  >
                    <div class="text-sm font-medium text-text-primary">{option.label}</div>
                    <div class="text-xs text-text-muted mt-0.5">{option.desc}</div>
                  </button>
                )}
              </For>
            </div>
          </section>

          {/* OIDC Providers */}
          <section class="space-y-4">
            <div class="flex items-center justify-between">
              <h3 class="text-sm font-semibold text-text-primary uppercase tracking-wider">
                Identity Providers
              </h3>
              <Show when={adminState.isElevated}>
                <button
                  onClick={() => {
                    setShowAddProvider(true);
                    applyPreset("");
                  }}
                  class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-accent-primary text-white text-xs font-medium hover:bg-accent-primary/90 transition-colors"
                >
                  <Plus class="w-3.5 h-3.5" />
                  Add Provider
                </button>
              </Show>
            </div>

            {/* Provider List */}
            <Show
              when={state.providers.length > 0}
              fallback={
                <div class="p-6 text-center text-text-muted text-sm rounded-xl bg-white/5 border border-white/10">
                  No identity providers configured.
                </div>
              }
            >
              <div class="space-y-2">
                <For each={state.providers}>
                  {(provider) => {
                    const Icon = providerIcon(provider.icon_hint);
                    return (
                      <div class="flex items-center justify-between p-4 rounded-xl bg-white/5 border border-white/10">
                        <div class="flex items-center gap-3">
                          <Icon class="w-5 h-5 text-text-secondary" />
                          <div>
                            <div class="text-sm font-medium text-text-primary">
                              {provider.display_name}
                            </div>
                            <div class="text-xs text-text-muted">
                              {provider.slug} &middot; {provider.provider_type}
                            </div>
                          </div>
                        </div>
                        <div class="flex items-center gap-2">
                          <span
                            class="text-xs px-2 py-0.5 rounded-full"
                            classList={{
                              "bg-status-success/20 text-status-success": provider.enabled,
                              "bg-white/10 text-text-muted": !provider.enabled,
                            }}
                          >
                            {provider.enabled ? "Enabled" : "Disabled"}
                          </span>
                          <Show when={adminState.isElevated}>
                            <button
                              onClick={() => handleToggleProviderEnabled(provider)}
                              class="p-1.5 rounded-lg text-text-muted hover:text-text-primary hover:bg-white/10 transition-colors"
                              title={provider.enabled ? "Disable" : "Enable"}
                            >
                              <Show when={provider.enabled} fallback={<Check class="w-4 h-4" />}>
                                <X class="w-4 h-4" />
                              </Show>
                            </button>
                            <button
                              onClick={() => {
                                setDeletingProvider(provider.id);
                              }}
                              class="p-1.5 rounded-lg text-text-muted hover:text-status-error hover:bg-status-error/10 transition-colors"
                              title="Delete"
                            >
                              <Trash2 class="w-4 h-4" />
                            </button>
                          </Show>
                        </div>
                      </div>
                    );
                  }}
                </For>
              </div>
            </Show>

            {/* Delete Confirmation */}
            <Show when={deletingProvider()}>
              <div class="p-4 rounded-xl bg-status-error/10 border border-status-error/30">
                <p class="text-sm text-text-primary mb-3">
                  Delete this provider? Users who registered via this provider will need to use a different login method.
                </p>
                <div class="flex gap-2">
                  <button
                    onClick={() => handleDeleteProvider(deletingProvider()!)}
                    class="px-3 py-1.5 rounded-lg bg-status-error text-white text-xs font-medium"
                  >
                    Delete
                  </button>
                  <button
                    onClick={() => setDeletingProvider(null)}
                    class="px-3 py-1.5 rounded-lg bg-white/10 text-text-primary text-xs font-medium"
                  >
                    Cancel
                  </button>
                </div>
              </div>
            </Show>

            {/* Add Provider Form */}
            <Show when={showAddProvider()}>
              <div class="p-5 rounded-xl bg-white/5 border border-white/10 space-y-4">
                <h4 class="text-sm font-medium text-text-primary">Add Identity Provider</h4>

                {/* Preset selector */}
                <div class="flex gap-2">
                  <button
                    onClick={() => applyPreset("github")}
                    class="flex items-center gap-2 px-3 py-2 rounded-lg border text-xs font-medium transition-colors"
                    classList={{
                      "border-accent-primary bg-accent-primary/10 text-text-primary": addForm.preset === "github",
                      "border-white/10 bg-white/5 text-text-secondary hover:bg-white/10": addForm.preset !== "github",
                    }}
                  >
                    <Github class="w-4 h-4" /> GitHub
                  </button>
                  <button
                    onClick={() => applyPreset("google")}
                    class="flex items-center gap-2 px-3 py-2 rounded-lg border text-xs font-medium transition-colors"
                    classList={{
                      "border-accent-primary bg-accent-primary/10 text-text-primary": addForm.preset === "google",
                      "border-white/10 bg-white/5 text-text-secondary hover:bg-white/10": addForm.preset !== "google",
                    }}
                  >
                    <Chrome class="w-4 h-4" /> Google
                  </button>
                  <button
                    onClick={() => applyPreset("custom")}
                    class="flex items-center gap-2 px-3 py-2 rounded-lg border text-xs font-medium transition-colors"
                    classList={{
                      "border-accent-primary bg-accent-primary/10 text-text-primary": addForm.preset === "custom",
                      "border-white/10 bg-white/5 text-text-secondary hover:bg-white/10": addForm.preset !== "custom",
                    }}
                  >
                    <KeyRound class="w-4 h-4" /> Custom
                  </button>
                </div>

                {/* Form fields */}
                <div class="grid grid-cols-2 gap-3">
                  <div>
                    <label class="block text-xs text-text-muted mb-1">Slug</label>
                    <input
                      type="text"
                      class="input-field text-sm"
                      placeholder="my-provider"
                      value={addForm.slug}
                      onInput={(e) => setAddForm("slug", e.currentTarget.value)}
                      disabled={addForm.preset === "github" || addForm.preset === "google"}
                    />
                  </div>
                  <div>
                    <label class="block text-xs text-text-muted mb-1">Display Name</label>
                    <input
                      type="text"
                      class="input-field text-sm"
                      placeholder="My Provider"
                      value={addForm.display_name}
                      onInput={(e) => setAddForm("display_name", e.currentTarget.value)}
                    />
                  </div>
                </div>

                <div>
                  <label class="block text-xs text-text-muted mb-1">Client ID</label>
                  <input
                    type="text"
                    class="input-field text-sm"
                    placeholder="OAuth client ID"
                    value={addForm.client_id}
                    onInput={(e) => setAddForm("client_id", e.currentTarget.value)}
                  />
                </div>

                <div>
                  <label class="block text-xs text-text-muted mb-1">Client Secret</label>
                  <input
                    type="password"
                    class="input-field text-sm"
                    placeholder="OAuth client secret"
                    value={addForm.client_secret}
                    onInput={(e) => setAddForm("client_secret", e.currentTarget.value)}
                  />
                </div>

                <Show when={addForm.preset === "custom" || addForm.preset === "google"}>
                  <div>
                    <label class="block text-xs text-text-muted mb-1">
                      Issuer URL (OIDC Discovery)
                    </label>
                    <input
                      type="url"
                      class="input-field text-sm"
                      placeholder="https://accounts.google.com"
                      value={addForm.issuer_url}
                      onInput={(e) => setAddForm("issuer_url", e.currentTarget.value)}
                    />
                  </div>
                </Show>

                <div>
                  <label class="block text-xs text-text-muted mb-1">Scopes</label>
                  <input
                    type="text"
                    class="input-field text-sm"
                    placeholder="openid profile email"
                    value={addForm.scopes}
                    onInput={(e) => setAddForm("scopes", e.currentTarget.value)}
                  />
                </div>

                <div class="flex gap-2 pt-2">
                  <button
                    onClick={handleCreateProvider}
                    class="flex items-center gap-1.5 px-4 py-2 rounded-lg bg-accent-primary text-white text-sm font-medium hover:bg-accent-primary/90 transition-colors"
                    disabled={state.isSaving || !addForm.slug || !addForm.client_id || !addForm.client_secret}
                  >
                    <Show when={state.isSaving} fallback={<Plus class="w-4 h-4" />}>
                      <Loader2 class="w-4 h-4 animate-spin" />
                    </Show>
                    Create
                  </button>
                  <button
                    onClick={() => setShowAddProvider(false)}
                    class="px-4 py-2 rounded-lg bg-white/10 text-text-primary text-sm font-medium hover:bg-white/20 transition-colors"
                  >
                    Cancel
                  </button>
                </div>
              </div>
            </Show>
          </section>
        </Show>
      </div>
    </div>
  );
};

export default AdminSettings;
