import { Component, createSignal, onMount, For, Show } from "solid-js";
import { KeyRound, Unlink, Plus } from "lucide-solid";
import * as tauri from "@/lib/tauri";
import { formatRelativeTimeShort } from "@/lib/utils";
import type { IdentityInfo, OidcProvider } from "@/lib/types";
import ElevateDialog from "@/components/admin/ElevateDialog";
import { t } from "@/i18n";

/** Longest the browser link flow waits for the provider window. */
const LINK_TIMEOUT_MS = 120_000;

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
  // Provider to link once the admin has confirmed with the re-auth dialog.
  const [pendingLink, setPendingLink] = createSignal<OidcProvider | null>(null);
  // Elevated, waiting for the user to start the link themselves (a popup must
  // open from a click, not after a long re-auth).
  const [readyLink, setReadyLink] = createSignal<OidcProvider | null>(null);

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

  // Text for a failed link, from a callback code or a native error string.
  const linkErrorText = (code: string): string | null => {
    if (code.includes("LINK_SESSION_INVALID")) return t("settings.linkedAccounts.error.sessionInvalid");
    if (code.includes("ACCOUNT_BANNED")) return t("settings.linkedAccounts.error.banned");
    if (code.toUpperCase().includes("SERVICE_UNAVAILABLE"))
      return t("settings.linkedAccounts.error.serviceUnavailable");
    return null;
  };

  // `afterElevation` is true for the one retry that follows a re-auth; a second
  // "elevation required" then ends in an error instead of asking again.
  const handleLink = async (provider: OidcProvider, afterElevation = false) => {
    setError(null);
    setLinking(provider.slug);

    const needElevation = () => {
      setLinking(null);
      if (afterElevation) {
        setError(t("settings.linkedAccounts.error.elevationStillRequired"));
      } else {
        setPendingLink(provider);
      }
    };

    try {
      let result: Awaited<ReturnType<typeof tauri.linkIdentity>>;
      try {
        result = await tauri.linkIdentity(provider.slug);
      } catch (err) {
        if (tauri.isElevationRequiredError(err)) {
          needElevation();
          return;
        }
        throw err;
      }

      if (result.mode === "tauri") {
        // Native command resolved on success; refresh the list.
        await loadData();
        setLinking(null);
        return;
      }

      // Browser: open a popup and wait for the callback's postMessage.
      const expectedOrigin = new URL(tauri.getServerUrl()).origin;
      const popup = window.open(result.authUrl, "oidc-link", "width=600,height=700");
      if (!popup) {
        setError(t("settings.linkedAccounts.error.popupBlocked"));
        setLinking(null);
        return;
      }
      const timers: {
        poll?: ReturnType<typeof setInterval>;
        ceiling?: ReturnType<typeof setTimeout>;
      } = {};
      const stop = () => {
        window.removeEventListener("message", messageHandler);
        clearInterval(timers.poll);
        clearTimeout(timers.ceiling);
      };
      const messageHandler = (event: MessageEvent) => {
        if (event.origin !== expectedOrigin) return;
        if (event.data?.type !== "oidc-link-callback") return;
        stop();
        if (event.data.success) {
          loadData().catch(() => {});
          setLinking(null);
          return;
        }
        const code = String(event.data.error_code ?? "");
        if (code === "elevation_required") {
          needElevation();
          return;
        }
        setError(
          code === "IDENTITY_ALREADY_LINKED"
            ? "That account is already linked to another Kaiku account."
            : (linkErrorText(code) ?? "Linking failed. Please try again."),
        );
        setLinking(null);
      };
      window.addEventListener("message", messageHandler);
      // Stop waiting if the popup is closed without completing.
      timers.poll = setInterval(() => {
        if (popup.closed) {
          stop();
          setLinking(null);
        }
      }, 500);
      // ...or if it never reports back.
      timers.ceiling = setTimeout(() => {
        stop();
        popup.close();
        setError(t("settings.linkedAccounts.error.timeout"));
        setLinking(null);
      }, LINK_TIMEOUT_MS);
    } catch (err) {
      const text = err instanceof Error ? err.message : String(err);
      setError(linkErrorText(text) ?? text);
      setLinking(null);
    }
  };

  return (
    <div class="space-y-3">
      <h4 class="text-sm font-semibold text-text-secondary uppercase tracking-wide">
        Linked Accounts
      </h4>

      <Show when={error()}>
        <div role="alert" class="text-sm text-status-danger p-3 rounded-xl bg-status-danger/10 border border-status-danger/20">
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
      <Show when={readyLink()}>
        {(provider) => (
          <div class="flex items-center gap-3 p-3 rounded-xl bg-surface-layer2 border border-white/5">
            <p class="flex-1 text-sm text-text-primary">
              {t("settings.linkedAccounts.confirmed", { provider: provider().display_name })}
            </p>
            <button
              type="button"
              onClick={() => {
                const target = provider();
                setReadyLink(null);
                void handleLink(target, true);
              }}
              class="px-3 py-1.5 rounded-lg text-sm bg-accent-primary text-on-accent hover:bg-accent-primary/90"
            >
              {t("settings.linkedAccounts.linkNow")}
            </button>
          </div>
        )}
      </Show>
      <Show when={pendingLink()}>
        <ElevateDialog
          onElevated={() => {
            const provider = pendingLink();
            setPendingLink(null);
            setReadyLink(provider);
          }}
          onClose={() => setPendingLink(null)}
        />
      </Show>
    </div>
  );
};

export default LinkedAccountsSection;
