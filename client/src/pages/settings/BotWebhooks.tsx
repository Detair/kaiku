/**
 * Bot Webhooks Management Page
 */

import { Component, createSignal, For, Show, onMount } from "solid-js";
import { A, useParams } from "@solidjs/router";
import {
  ArrowLeft,
  Plus,
  Trash2,
  Webhook,
  Play,
  Eye,
  Copy,
  Check,
} from "lucide-solid";
import { getBotApplication, type BotApplication } from "../../lib/api/bots";
import {
  createWebhook,
  listWebhooks,
  updateWebhook,
  deleteWebhook,
  testWebhook,
  listDeliveries,
  type Webhook as WebhookType,
  type WebhookCreated,
  type WebhookEventType,
  type DeliveryLogEntry,
  type TestDeliveryResult,
} from "../../lib/api/webhooks";
import { showToast } from "../../components/ui/Toast";

const ALL_EVENT_TYPES: WebhookEventType[] = [
  "message.created",
  "member.joined",
  "member.left",
  "command.invoked",
];

const BotWebhooks: Component = () => {
  const params = useParams<{ id: string }>();
  const [app, setApp] = createSignal<BotApplication | null>(null);
  const [webhooks, setWebhooks] = createSignal<WebhookType[]>([]);
  const [loading, setLoading] = createSignal(true);
  const [showCreateModal, setShowCreateModal] = createSignal(false);
  const [showSecretModal, setShowSecretModal] = createSignal(false);
  const [createdWebhook, setCreatedWebhook] =
    createSignal<WebhookCreated | null>(null);
  const [secretCopied, setSecretCopied] = createSignal(false);

  // Create form state
  const [newUrl, setNewUrl] = createSignal("");
  const [newDescription, setNewDescription] = createSignal("");
  const [newEvents, setNewEvents] = createSignal<WebhookEventType[]>([]);

  // Delivery log state
  const [expandedWebhook, setExpandedWebhook] = createSignal<string | null>(
    null,
  );
  const [deliveries, setDeliveries] = createSignal<DeliveryLogEntry[]>([]);
  const [deliveriesLoading, setDeliveriesLoading] = createSignal(false);

  // Test result
  const [testResults, setTestResults] = createSignal<
    Record<string, TestDeliveryResult>
  >({});
  const [testingId, setTestingId] = createSignal<string | null>(null);

  onMount(() => {
    loadData();
  });

  async function loadData() {
    try {
      setLoading(true);
      const [appData, webhookData] = await Promise.all([
        getBotApplication(params.id),
        listWebhooks(params.id),
      ]);
      setApp(appData);
      setWebhooks(webhookData);
    } catch (error) {
      showToast({
        type: "error",
        title: "Failed to load webhooks",
        message: error instanceof Error ? error.message : "Unknown error",
        duration: 8000,
      });
    } finally {
      setLoading(false);
    }
  }

  async function handleCreate() {
    const url = newUrl().trim();
    if (!url || url.length < 10) {
      showToast({
        type: "error",
        title: "Invalid URL",
        message: "URL must be at least 10 characters",
        duration: 5000,
      });
      return;
    }
    if (newEvents().length === 0) {
      showToast({
        type: "error",
        title: "No events selected",
        message: "Select at least one event type",
        duration: 5000,
      });
      return;
    }

    try {
      const result = await createWebhook(params.id, {
        url,
        subscribed_events: newEvents(),
        description: newDescription().trim() || undefined,
      });
      setCreatedWebhook(result);
      setShowCreateModal(false);
      setShowSecretModal(true);
      setNewUrl("");
      setNewDescription("");
      setNewEvents([]);
      await loadData();
      showToast({ type: "success", title: "Webhook created", duration: 3000 });
    } catch (error) {
      showToast({
        type: "error",
        title: "Failed to create webhook",
        message: error instanceof Error ? error.message : "Unknown error",
        duration: 8000,
      });
    }
  }

  async function handleDelete(webhookId: string) {
    if (!confirm("Are you sure you want to delete this webhook?")) return;
    try {
      await deleteWebhook(params.id, webhookId);
      await loadData();
      showToast({ type: "success", title: "Webhook deleted", duration: 3000 });
    } catch (error) {
      showToast({
        type: "error",
        title: "Failed to delete webhook",
        message: error instanceof Error ? error.message : "Unknown error",
        duration: 8000,
      });
    }
  }

  async function handleToggleActive(wh: WebhookType) {
    try {
      await updateWebhook(params.id, wh.id, { active: !wh.active });
      await loadData();
    } catch (error) {
      showToast({
        type: "error",
        title: "Failed to update webhook",
        message: error instanceof Error ? error.message : "Unknown error",
        duration: 8000,
      });
    }
  }

  async function handleTest(webhookId: string) {
    setTestingId(webhookId);
    try {
      const result = await testWebhook(params.id, webhookId);
      setTestResults((prev) => ({ ...prev, [webhookId]: result }));
      showToast({
        type: result.success ? "success" : "error",
        title: result.success ? "Test ping succeeded" : "Test ping failed",
        message: result.error_message || `${result.latency_ms}ms`,
        duration: 5000,
      });
    } catch (error) {
      showToast({
        type: "error",
        title: "Failed to test webhook",
        message: error instanceof Error ? error.message : "Unknown error",
        duration: 8000,
      });
    } finally {
      setTestingId(null);
    }
  }

  async function handleToggleDeliveries(webhookId: string) {
    if (expandedWebhook() === webhookId) {
      setExpandedWebhook(null);
      return;
    }
    setExpandedWebhook(webhookId);
    setDeliveriesLoading(true);
    try {
      const entries = await listDeliveries(params.id, webhookId);
      setDeliveries(entries);
    } catch (error) {
      showToast({
        type: "error",
        title: "Failed to load deliveries",
        message: error instanceof Error ? error.message : "Unknown error",
        duration: 8000,
      });
    } finally {
      setDeliveriesLoading(false);
    }
  }

  function toggleEvent(event: WebhookEventType) {
    setNewEvents((prev) =>
      prev.includes(event) ? prev.filter((e) => e !== event) : [...prev, event],
    );
  }

  async function copySecret() {
    const secret = createdWebhook()?.signing_secret;
    if (!secret) return;
    await navigator.clipboard.writeText(secret);
    setSecretCopied(true);
    setTimeout(() => setSecretCopied(false), 2000);
  }

  return (
    <div class="p-6 max-w-4xl mx-auto">
      {/* Header */}
      <div class="flex items-center gap-3 mb-6">
        <A
          href="/settings"
          class="p-2 rounded hover:bg-[var(--bg-secondary)] transition-colors"
        >
          <ArrowLeft size={20} />
        </A>
        <Webhook size={24} class="text-[var(--text-secondary)]" />
        <h1 class="text-xl font-semibold">{app()?.name ?? "Bot"} — Webhooks</h1>
      </div>

      <Show
        when={!loading()}
        fallback={<div class="text-[var(--text-secondary)]">Loading...</div>}
      >
        {/* Webhook List */}
        <div class="flex items-center justify-between mb-4">
          <p class="text-sm text-[var(--text-secondary)]">
            {webhooks().length}/5 webhooks configured
          </p>
          <Show when={webhooks().length < 5}>
            <button
              class="flex items-center gap-2 px-3 py-2 bg-[var(--accent)] text-white rounded hover:opacity-90 text-sm"
              onClick={() => setShowCreateModal(true)}
            >
              <Plus size={16} /> Add Webhook
            </button>
          </Show>
        </div>

        <Show when={webhooks().length === 0}>
          <div class="text-center py-12 text-[var(--text-secondary)]">
            <Webhook size={48} class="mx-auto mb-4 opacity-50" />
            <p>No webhooks configured yet.</p>
            <p class="text-sm mt-1">
              Webhooks deliver platform events to your bot's HTTP endpoint.
            </p>
          </div>
        </Show>

        <div class="space-y-3">
          <For each={webhooks()}>
            {(wh) => (
              <div class="bg-[var(--bg-secondary)] rounded-lg p-4 border border-[var(--border)]">
                <div class="flex items-start justify-between gap-3">
                  <div class="flex-1 min-w-0">
                    <div class="flex items-center gap-2 mb-1">
                      <span class="font-mono text-sm truncate">{wh.url}</span>
                      <span
                        class={`text-xs px-2 py-0.5 rounded-full ${wh.active ? "bg-green-500/20 text-green-400" : "bg-gray-500/20 text-gray-400"}`}
                      >
                        {wh.active ? "Active" : "Inactive"}
                      </span>
                    </div>
                    <Show when={wh.description}>
                      <p class="text-sm text-[var(--text-secondary)] mb-2">
                        {wh.description}
                      </p>
                    </Show>
                    <div class="flex flex-wrap gap-1.5">
                      <For each={wh.subscribed_events}>
                        {(evt) => (
                          <span class="text-xs px-2 py-0.5 bg-[var(--bg-tertiary)] rounded text-[var(--text-secondary)]">
                            {evt}
                          </span>
                        )}
                      </For>
                    </div>
                  </div>
                  <div class="flex items-center gap-2 shrink-0">
                    <button
                      class="p-2 rounded hover:bg-[var(--bg-tertiary)] transition-colors text-[var(--text-secondary)]"
                      title={wh.active ? "Disable" : "Enable"}
                      onClick={() => handleToggleActive(wh)}
                    >
                      <Eye size={16} />
                    </button>
                    <button
                      class="p-2 rounded hover:bg-[var(--bg-tertiary)] transition-colors text-[var(--text-secondary)]"
                      title="Test Ping"
                      disabled={testingId() === wh.id}
                      onClick={() => handleTest(wh.id)}
                    >
                      <Play size={16} />
                    </button>
                    <button
                      class="p-2 rounded hover:bg-[var(--bg-tertiary)] transition-colors text-[var(--text-secondary)]"
                      title="Delivery Log"
                      onClick={() => handleToggleDeliveries(wh.id)}
                    >
                      <Eye size={16} />
                    </button>
                    <button
                      class="p-2 rounded hover:bg-red-500/20 transition-colors text-red-400"
                      title="Delete"
                      onClick={() => handleDelete(wh.id)}
                    >
                      <Trash2 size={16} />
                    </button>
                  </div>
                </div>

                {/* Test Result */}
                <Show when={testResults()[wh.id]}>
                  {(result) => (
                    <div
                      class={`mt-3 p-2 rounded text-xs ${result().success ? "bg-green-500/10 text-green-400" : "bg-red-500/10 text-red-400"}`}
                    >
                      {result().success ? "OK" : "Failed"} —{" "}
                      {result().response_status
                        ? `HTTP ${result().response_status}`
                        : "Connection error"}{" "}
                      ({result().latency_ms}ms)
                      <Show when={result().error_message}>
                        <span class="ml-2">{result().error_message}</span>
                      </Show>
                    </div>
                  )}
                </Show>

                {/* Delivery Log (expanded) */}
                <Show when={expandedWebhook() === wh.id}>
                  <div class="mt-3 border-t border-[var(--border)] pt-3">
                    <p class="text-sm font-medium mb-2">Recent Deliveries</p>
                    <Show when={deliveriesLoading()}>
                      <p class="text-xs text-[var(--text-secondary)]">
                        Loading...
                      </p>
                    </Show>
                    <Show
                      when={!deliveriesLoading() && deliveries().length === 0}
                    >
                      <p class="text-xs text-[var(--text-secondary)]">
                        No deliveries yet.
                      </p>
                    </Show>
                    <Show
                      when={!deliveriesLoading() && deliveries().length > 0}
                    >
                      <div class="space-y-1 max-h-60 overflow-y-auto">
                        <For each={deliveries()}>
                          {(entry) => (
                            <div class="flex items-center gap-2 text-xs py-1">
                              <span
                                class={`w-2 h-2 rounded-full shrink-0 ${entry.success ? "bg-green-400" : "bg-red-400"}`}
                              />
                              <span class="text-[var(--text-secondary)]">
                                {entry.event_type}
                              </span>
                              <span class="text-[var(--text-secondary)]">
                                #{entry.attempt}
                              </span>
                              <Show when={entry.response_status}>
                                <span>HTTP {entry.response_status}</span>
                              </Show>
                              <Show when={entry.latency_ms}>
                                <span class="text-[var(--text-secondary)]">
                                  {entry.latency_ms}ms
                                </span>
                              </Show>
                              <Show when={entry.error_message}>
                                <span class="text-red-400 truncate">
                                  {entry.error_message}
                                </span>
                              </Show>
                              <span class="ml-auto text-[var(--text-secondary)]">
                                {new Date(entry.created_at).toLocaleString()}
                              </span>
                            </div>
                          )}
                        </For>
                      </div>
                    </Show>
                  </div>
                </Show>
              </div>
            )}
          </For>
        </div>
      </Show>

      {/* Create Webhook Modal */}
      <Show when={showCreateModal()}>
        <div
          class="fixed inset-0 bg-black/50 flex items-center justify-center z-50"
          onClick={() => setShowCreateModal(false)}
        >
          <div
            class="bg-[var(--bg-primary)] rounded-lg p-6 w-full max-w-md border border-[var(--border)]"
            onClick={(e) => e.stopPropagation()}
          >
            <h2 class="text-lg font-semibold mb-4">Create Webhook</h2>

            <div class="space-y-4">
              <div>
                <label class="block text-sm font-medium mb-1">URL</label>
                <input
                  type="url"
                  class="w-full px-3 py-2 bg-[var(--bg-secondary)] border border-[var(--border)] rounded text-sm"
                  placeholder="https://your-bot.example.com/webhook"
                  value={newUrl()}
                  onInput={(e) => setNewUrl(e.currentTarget.value)}
                />
              </div>

              <div>
                <label class="block text-sm font-medium mb-1">
                  Description (optional)
                </label>
                <input
                  type="text"
                  class="w-full px-3 py-2 bg-[var(--bg-secondary)] border border-[var(--border)] rounded text-sm"
                  placeholder="Production webhook"
                  value={newDescription()}
                  onInput={(e) => setNewDescription(e.currentTarget.value)}
                  maxLength={500}
                />
              </div>

              <div>
                <label class="block text-sm font-medium mb-2">Events</label>
                <div class="space-y-2">
                  <For each={ALL_EVENT_TYPES}>
                    {(evt) => (
                      <label class="flex items-center gap-2 cursor-pointer">
                        <input
                          type="checkbox"
                          checked={newEvents().includes(evt)}
                          onChange={() => toggleEvent(evt)}
                          class="rounded"
                        />
                        <span class="text-sm">{evt}</span>
                      </label>
                    )}
                  </For>
                </div>
              </div>
            </div>

            <div class="flex justify-end gap-2 mt-6">
              <button
                class="px-4 py-2 text-sm rounded hover:bg-[var(--bg-secondary)] transition-colors"
                onClick={() => setShowCreateModal(false)}
              >
                Cancel
              </button>
              <button
                class="px-4 py-2 text-sm bg-[var(--accent)] text-white rounded hover:opacity-90"
                onClick={handleCreate}
              >
                Create
              </button>
            </div>
          </div>
        </div>
      </Show>

      {/* Signing Secret Modal (shown once after creation) */}
      <Show when={showSecretModal() && createdWebhook()}>
        <div class="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div class="bg-[var(--bg-primary)] rounded-lg p-6 w-full max-w-md border border-[var(--border)]">
            <h2 class="text-lg font-semibold mb-2">Signing Secret</h2>
            <p class="text-sm text-[var(--text-secondary)] mb-4">
              Copy this secret now. It will not be shown again. Use it to verify
              webhook signatures (X-Webhook-Signature header).
            </p>

            <div class="flex items-center gap-2 p-3 bg-[var(--bg-secondary)] rounded font-mono text-sm break-all">
              <span class="flex-1">{createdWebhook()!.signing_secret}</span>
              <button
                class="p-1.5 rounded hover:bg-[var(--bg-tertiary)] transition-colors shrink-0"
                onClick={copySecret}
                title="Copy"
              >
                <Show when={secretCopied()} fallback={<Copy size={16} />}>
                  <Check size={16} class="text-green-400" />
                </Show>
              </button>
            </div>

            <div class="flex justify-end mt-6">
              <button
                class="px-4 py-2 text-sm bg-[var(--accent)] text-white rounded hover:opacity-90"
                onClick={() => {
                  setShowSecretModal(false);
                  setCreatedWebhook(null);
                  setSecretCopied(false);
                }}
              >
                Done
              </button>
            </div>
          </div>
        </div>
      </Show>
    </div>
  );
};

export default BotWebhooks;
