import { Component, createSignal, onMount, For, Show } from "solid-js";
import { KeyRound, Unlink, Plus } from "lucide-solid";
import * as tauri from "@/lib/tauri";
import { formatRelativeTimeShort } from "@/lib/utils";
import type { IdentityInfo, OidcProvider } from "@/lib/types";

/**
 * Lists the external (OIDC) identities linked to the account, and lets the user
 * link an additional provider or unlink an existing one.
 *
 * Linking uses the native command on desktop (Tauri) and a popup + postMessage
 * handshake in the browser — see `tauri.linkIdentity`.
 */
const LinkedAccountsSection: Component = () => {
  const [identities, setIdentities] = createSignal<IdentityInfo[]>([]);
  const [providers, setProviders] = createSignal<OidcProvider[]>([]);
  const [loading, setLoading] = createSignal(true);
  const [error, setError] = createSignal<string | null>(null);
  // Slug currently being linked (disables its button / shows progress).
  const [linking, setLinking] = createSignal<string | null>(null);

  const loadData = async () => {
    try {
      setError(null);
      const resp = await tauri.listIdentities();
      setIdentities(resp.identities);
      // Available providers come from the public server settings; ignore
      // failures here (linking just won't be offered).
      try {
        const settings = await tauri.fetchServerSettings(tauri.getServerUrl());
        setProviders(settings.oidc_enabled ? settings.oidc_providers : []);
      } catch {
        setProviders([]);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  };

  onMount(loadData);

  // Providers not already linked to this account.
  const availableProviders = (): OidcProvider[] => {
    const linkedSlugs = new Set(identities().map((i) => i.provider_slug));
    return providers().filter((p) => !linkedSlugs.has(p.slug));
  };

  const handleUnlink = async (identity: IdentityInfo) => {
    const label = identity.provider_name || identity.provider_slug;
    if (
      !window.confirm(
        `Unlink ${label}? You will no longer be able to sign in with it.`,
      )
    ) {
      return;
    }
    try {
      setError(null);
      await tauri.unlinkIdentity(identity.id);
      setIdentities((prev) => prev.filter((i) => i.id !== identity.id));
    } catch (err) {
      // Surfaces the server's message, e.g. the last-login-method guard (409).
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const handleLink = async (provider: OidcProvider) => {
    setError(null);
    setLinking(provider.slug);
    try {
      const result = await tauri.linkIdentity(provider.slug);

      if (result.mode === "tauri") {
        // Native command resolved on success; refresh the list.
        await loadData();
        setLinking(null);
        return;
      }

      // Browser: open a popup and wait for the callback's postMessage.
      const expectedOrigin = new URL(tauri.getServerUrl()).origin;
      const messageHandler = (event: MessageEvent) => {
        if (event.origin !== expectedOrigin) return;
        if (event.data?.type !== "oidc-link-callback") return;
        window.removeEventListener("message", messageHandler);
        if (event.data.success) {
          loadData().catch(() => {});
        } else {
          setError(
            event.data.error_code === "IDENTITY_ALREADY_LINKED"
              ? "That account is already linked to another Kaiku account."
              : "Linking failed. Please try again.",
          );
        }
        setLinking(null);
      };
      window.addEventListener("message", messageHandler);

      const popup = window.open(result.authUrl, "oidc-link", "width=600,height=700");
      // Stop waiting if the popup is closed without completing.
      const checkClosed = setInterval(() => {
        if (popup?.closed) {
          clearInterval(checkClosed);
          window.removeEventListener("message", messageHandler);
          setLinking(null);
        }
      }, 500);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setLinking(null);
    }
  };

  return (
    <div class="space-y-3">
      <h4 class="text-sm font-semibold text-text-secondary uppercase tracking-wide">
        Linked Accounts
      </h4>

      <Show when={error()}>
        <div class="text-sm text-status-danger p-3 rounded-xl bg-status-danger/10 border border-status-danger/20">
          {error()}
        </div>
      </Show>

      <Show when={loading()}>
        <div class="text-sm text-text-secondary py-4 text-center">
          Loading linked accounts...
        </div>
      </Show>

      <Show when={!loading()}>
        <Show
          when={identities().length > 0}
          fallback={
            <div class="text-sm text-text-secondary py-2">
              No external accounts are linked to your profile.
            </div>
          }
        >
          <div class="space-y-2">
            <For each={identities()}>
              {(identity) => (
                <div class="flex items-center gap-3 p-3 rounded-xl bg-surface-layer2 border border-white/5">
                  <div class="shrink-0 text-text-secondary">
                    <KeyRound size={20} />
                  </div>
                  <div class="flex-1 min-w-0">
                    <span class="text-sm font-medium text-text-primary truncate block">
                      {identity.provider_name || identity.provider_slug}
                    </span>
                    <div class="flex items-center gap-2 text-xs text-text-secondary mt-0.5">
                      <Show when={identity.email}>
                        <span class="truncate">{identity.email}</span>
                        <span class="opacity-40">&middot;</span>
                      </Show>
                      <span>
                        {identity.last_used_at
                          ? `last used ${formatRelativeTimeShort(identity.last_used_at)}`
                          : `linked ${formatRelativeTimeShort(identity.created_at)}`}
                      </span>
                    </div>
                  </div>
                  <button
                    onClick={() => handleUnlink(identity)}
                    class="shrink-0 p-1.5 rounded-lg text-text-secondary hover:text-status-danger hover:bg-status-danger/10 transition-colors"
                    title={`Unlink ${identity.provider_name || identity.provider_slug}`}
                  >
                    <Unlink size={16} />
                  </button>
                </div>
              )}
            </For>
          </div>
        </Show>

        <Show when={availableProviders().length > 0}>
          <div class="flex flex-wrap gap-2 pt-1">
            <For each={availableProviders()}>
              {(provider) => (
                <button
                  onClick={() => handleLink(provider)}
                  disabled={linking() !== null}
                  class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm bg-surface-layer2 hover:bg-surface-highlight border border-white/5 text-text-primary transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  <Plus size={14} />
                  {linking() === provider.slug
                    ? "Linking..."
                    : `Link ${provider.display_name}`}
                </button>
              )}
            </For>
          </div>
        </Show>
      </Show>
    </div>
  );
};

export default LinkedAccountsSection;
