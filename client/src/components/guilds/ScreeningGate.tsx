/**
 * ScreeningGate — a full-screen rules gate shown when the current user is a
 * `pending` member of a screening-enabled guild. Until they accept, the server
 * blocks all channel access anyway; this gives them the acceptance screen
 * instead of an empty guild. Renders nothing for active members.
 */

import { Component, Show, createResource, createSignal } from "solid-js";
import { ShieldCheck } from "lucide-solid";
import { getScreening, acceptScreening } from "@/lib/tauri";
import { renderEmbedRich } from "@/lib/embedSanitize";

const ScreeningGate: Component<{ guildId: string }> = (props) => {
  const [accepted, setAccepted] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [config, { refetch }] = createResource(
    () => (accepted() ? null : props.guildId),
    (g) => getScreening(g),
  );

  const pending = () =>
    !accepted() && config()?.screening_enabled && config()?.my_state === "pending";

  const accept = async () => {
    setBusy(true);
    setError(null);
    try {
      await acceptScreening(props.guildId);
      setAccepted(true);
      // Reload so channels/permissions re-resolve now that we're active.
      window.location.reload();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to accept");
      void refetch();
    } finally {
      setBusy(false);
    }
  };

  return (
    <Show when={pending()}>
      <div class="fixed inset-0 z-40 flex items-center justify-center p-6 bg-[var(--color-surface-base)]">
        <div class="w-full max-w-md text-center">
          <div class="flex justify-center mb-3">
            <div class="w-14 h-14 rounded-2xl bg-accent-primary/15 flex items-center justify-center">
              <ShieldCheck class="w-7 h-7 text-accent-primary" />
            </div>
          </div>
          <h2 class="text-xl font-bold text-text-primary mb-1">Before you join in</h2>
          <p class="text-sm text-text-secondary mb-4">
            Please read and accept this server's rules to participate.
          </p>
          <Show
            when={config()?.rules_md}
            fallback={
              <div class="text-sm text-text-secondary mb-4">
                This server asks members to agree to its guidelines before posting.
              </div>
            }
          >
            <div
              class="text-left text-sm text-text-primary mb-4 p-4 rounded-lg max-h-[40vh] overflow-y-auto"
              style={{ "background-color": "var(--color-surface-layer1)" }}
              // eslint-disable-next-line solid/no-innerhtml -- rules_md sanitized via isolated DOMPurify (lib/embedSanitize.ts)
              innerHTML={renderEmbedRich(config()!.rules_md!)}
            />
          </Show>
          <Show when={error()}>
            <div class="text-xs text-accent-danger mb-2">{error()}</div>
          </Show>
          <button
            class="w-full px-4 py-2.5 rounded-lg bg-accent-primary text-on-accent font-medium hover:bg-accent-primary/90 disabled:opacity-50"
            disabled={busy()}
            onClick={accept}
          >
            {busy() ? "Accepting…" : "I agree"}
          </button>
        </div>
      </div>
    </Show>
  );
};

export default ScreeningGate;
