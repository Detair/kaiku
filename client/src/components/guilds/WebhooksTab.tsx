/**
 * WebhooksTab — manage incoming (Discord-compatible) webhooks. Admin-only
 * (MANAGE_WEBHOOKS). External services POST to a webhook's secret URL to
 * create messages in its channel; the URL works anywhere a Discord webhook
 * URL is expected.
 */

import { Component, createSignal, For, Show, onMount } from "solid-js";
import { Plus, Trash2, Check, Copy, Pencil, X } from "lucide-solid";
import {
  listGuildWebhooks,
  createWebhook,
  updateWebhook,
  deleteWebhook,
  type IncomingWebhook,
} from "@/lib/tauri";
import { channelsState } from "@/stores/channels";

interface WebhooksTabProps {
  guildId: string;
}

const WebhooksTab: Component<WebhooksTabProps> = (props) => {
  const [webhooks, setWebhooks] = createSignal<IncomingWebhook[]>([]);
  const [loading, setLoading] = createSignal(true);
  const [error, setError] = createSignal<string | null>(null);

  // Create-form state.
  const [channelId, setChannelId] = createSignal("");
  const [name, setName] = createSignal("");
  const [submitting, setSubmitting] = createSignal(false);

  // Inline rename state.
  const [editingId, setEditingId] = createSignal<string | null>(null);
  const [editName, setEditName] = createSignal("");

  const [copiedId, setCopiedId] = createSignal<string | null>(null);

  // Text, announcement, and forum channels can host webhooks.
  const webhookChannels = () =>
    channelsState.channels
      .filter((c) =>
        ["text", "announcement", "forum"].includes(c.channel_type),
      )
      .sort((a, b) => a.position - b.position);

  const channelName = (id: string): string =>
    channelsState.channels.find((c) => c.id === id)?.name ?? "unknown channel";

  const refresh = async () => {
    setLoading(true);
    try {
      setWebhooks(await listGuildWebhooks(props.guildId));
    } catch (err) {
      console.error("Failed to load webhooks:", err);
      setError("Failed to load webhooks.");
    } finally {
      setLoading(false);
    }
  };

  onMount(() => {
    void refresh();
  });

  const canSubmit = () =>
    channelId() !== "" && name().trim() !== "" && !submitting();

  const handleCreate = async (e: Event) => {
    e.preventDefault();
    if (!canSubmit()) return;
    setSubmitting(true);
    setError(null);
    try {
      await createWebhook(channelId(), { name: name().trim() });
      setName("");
      await refresh();
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setError(msg || "Failed to create webhook.");
    } finally {
      setSubmitting(false);
    }
  };

  const handleDelete = async (webhookId: string) => {
    if (!confirm("Delete this webhook? Integrations using its URL will stop working.")) {
      return;
    }
    try {
      await deleteWebhook(webhookId);
      await refresh();
    } catch (err) {
      console.error("Failed to delete webhook:", err);
      setError("Failed to delete webhook.");
    }
  };

  const handleRename = async (webhookId: string) => {
    const newName = editName().trim();
    setEditingId(null);
    if (newName === "") return;
    try {
      await updateWebhook(webhookId, { name: newName });
      await refresh();
    } catch (err) {
      console.error("Failed to rename webhook:", err);
      setError("Failed to rename webhook.");
    }
  };

  /** Copy the execute URL, preferring the client's own server origin. */
  const copyUrl = async (webhook: IncomingWebhook) => {
    const url = webhook.url;
    try {
      await navigator.clipboard.writeText(url);
      setCopiedId(webhook.id);
      setTimeout(() => setCopiedId(null), 2000);
    } catch (err) {
      console.error("Failed to copy webhook URL:", err);
      setError("Failed to copy URL to clipboard.");
    }
  };

  const inputClass =
    "w-full px-3 py-2 rounded-lg text-sm text-text-primary bg-[var(--color-surface-layer2)] border border-white/10 focus:border-accent-primary outline-none";

  return (
    <div class="p-6">
      <div class="mb-4">
        <h3 class="text-lg font-semibold text-text-primary">Webhooks</h3>
        <p class="text-sm text-text-secondary mt-1">
          Webhooks let external services — game servers, CI, monitoring — post
          messages into a channel. The URL is Discord-compatible: paste it
          anywhere a Discord webhook URL is expected. Treat it as a secret.
        </p>
      </div>

      <Show when={error()}>
        <div class="mb-4 px-3 py-2 rounded-lg text-sm text-text-primary bg-accent-danger/20 border border-accent-danger/30">
          {error()}
        </div>
      </Show>

      {/* Create form */}
      <form
        onSubmit={handleCreate}
        class="mb-6 p-4 rounded-lg border border-white/10 space-y-3"
        style={{ "background-color": "var(--color-surface-layer1)" }}
      >
        <div class="grid grid-cols-2 gap-3">
          <label class="block">
            <span class="text-xs text-text-secondary">Channel</span>
            <select
              class={inputClass}
              value={channelId()}
              onChange={(e) => setChannelId(e.currentTarget.value)}
              data-testid="webhook-channel-select"
            >
              <option value="">Select a channel…</option>
              <For each={webhookChannels()}>
                {(c) => <option value={c.id}>#{c.name}</option>}
              </For>
            </select>
          </label>
          <label class="block">
            <span class="text-xs text-text-secondary">Name</span>
            <input
              class={inputClass}
              type="text"
              maxLength={80}
              placeholder="e.g. Game Server"
              value={name()}
              onInput={(e) => setName(e.currentTarget.value)}
              data-testid="webhook-name-input"
            />
          </label>
        </div>

        <button
          type="submit"
          disabled={!canSubmit()}
          data-testid="webhook-create-button"
          class="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-accent-primary text-on-accent text-sm font-medium hover:bg-accent-primary/90 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <Plus class="w-4 h-4" />
          Create webhook
        </button>
      </form>

      {/* Existing webhooks */}
      <Show
        when={!loading()}
        fallback={
          <div class="text-center py-8 text-text-secondary">Loading…</div>
        }
      >
        <Show
          when={webhooks().length > 0}
          fallback={
            <div class="text-center py-8 text-text-secondary text-sm">
              No webhooks yet.
            </div>
          }
        >
          <div class="space-y-2">
            <For each={webhooks()}>
              {(w) => (
                <div
                  class="flex items-center gap-3 p-3 rounded-lg border border-white/10"
                  style={{ "background-color": "var(--color-surface-layer1)" }}
                  data-testid="webhook-row"
                >
                  <div class="flex-1 min-w-0">
                    <Show
                      when={editingId() === w.id}
                      fallback={
                        <div class="font-medium text-text-primary truncate">
                          {w.name}
                        </div>
                      }
                    >
                      <input
                        class={inputClass}
                        type="text"
                        maxLength={80}
                        value={editName()}
                        onInput={(e) => setEditName(e.currentTarget.value)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") void handleRename(w.id);
                          if (e.key === "Escape") setEditingId(null);
                        }}
                      />
                    </Show>
                    <div class="text-xs text-text-secondary flex items-center gap-2 mt-0.5">
                      <span>#{channelName(w.channel_id)}</span>
                      <Show when={w.user}>
                        <span>created by {w.user!.display_name}</span>
                      </Show>
                    </div>
                  </div>
                  <Show
                    when={editingId() === w.id}
                    fallback={
                      <button
                        onClick={() => {
                          setEditName(w.name);
                          setEditingId(w.id);
                        }}
                        class="p-2 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors"
                        title="Rename webhook"
                      >
                        <Pencil class="w-4 h-4" />
                      </button>
                    }
                  >
                    <button
                      onClick={() => void handleRename(w.id)}
                      class="p-2 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors"
                      title="Save name"
                    >
                      <Check class="w-4 h-4" />
                    </button>
                    <button
                      onClick={() => setEditingId(null)}
                      class="p-2 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors"
                      title="Cancel"
                    >
                      <X class="w-4 h-4" />
                    </button>
                  </Show>
                  <button
                    onClick={() => void copyUrl(w)}
                    data-testid="webhook-copy-url"
                    class="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-sm text-text-secondary hover:text-text-primary hover:bg-white/10 transition-colors"
                    title="Copy webhook URL"
                  >
                    <Show
                      when={copiedId() === w.id}
                      fallback={<Copy class="w-4 h-4" />}
                    >
                      <Check class="w-4 h-4 text-accent-success" />
                    </Show>
                    {copiedId() === w.id ? "Copied" : "Copy URL"}
                  </button>
                  <button
                    onClick={() => void handleDelete(w.id)}
                    data-testid="webhook-delete"
                    class="p-2 rounded-lg text-text-secondary hover:text-accent-danger hover:bg-white/10 transition-colors"
                    title="Delete webhook"
                  >
                    <Trash2 class="w-4 h-4" />
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

export default WebhooksTab;
