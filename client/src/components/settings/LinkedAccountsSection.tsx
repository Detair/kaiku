import { Component, createSignal, onMount, For, Show } from "solid-js";
import { KeyRound, Unlink } from "lucide-solid";
import * as tauri from "@/lib/tauri";
import type { IdentityInfo } from "@/lib/types";

/**
 * Lists the external (OIDC) identities linked to the account and lets the user
 * unlink them. Adding new identities is handled separately (the link flow needs
 * the desktop app's native callback handling).
 */
const LinkedAccountsSection: Component = () => {
  const [identities, setIdentities] = createSignal<IdentityInfo[]>([]);
  const [loading, setLoading] = createSignal(true);
  const [error, setError] = createSignal<string | null>(null);

  const loadIdentities = async () => {
    try {
      setLoading(true);
      setError(null);
      const resp = await tauri.listIdentities();
      setIdentities(resp.identities);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  };

  onMount(loadIdentities);

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

  const formatRelativeTime = (dateStr: string): string => {
    const date = new Date(dateStr);
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffMins = Math.floor(diffMs / 60000);
    if (diffMins < 1) return "just now";
    if (diffMins < 60) return `${diffMins}m ago`;
    const diffHours = Math.floor(diffMins / 60);
    if (diffHours < 24) return `${diffHours}h ago`;
    const diffDays = Math.floor(diffHours / 24);
    return `${diffDays}d ago`;
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
            <div class="text-sm text-text-secondary py-4 text-center">
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
                          ? `last used ${formatRelativeTime(identity.last_used_at)}`
                          : `linked ${formatRelativeTime(identity.created_at)}`}
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
      </Show>
    </div>
  );
};

export default LinkedAccountsSection;
