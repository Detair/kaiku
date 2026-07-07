/**
 * ReactionRolesTab — manage emoji↔role bindings so members can self-assign
 * roles by reacting. Admin-only (MANAGE_ROLES). Create, list, and delete
 * bindings for a chosen message.
 */

import { Component, createSignal, For, Show, onMount } from "solid-js";
import { Plus, Trash2, Tag } from "lucide-solid";
import {
  listReactionRoles,
  createReactionRole,
  deleteReactionRole,
  type ReactionRole,
} from "@/lib/tauri";
import { loadGuildRoles, getGuildRoles } from "@/stores/permissions";
import { textChannels } from "@/stores/channels";
import type { GuildRole } from "@/lib/types";

interface ReactionRolesTabProps {
  guildId: string;
}

const ERROR_MESSAGES: Record<string, string> = {
  ROLE_NOT_SELF_ASSIGNABLE:
    "That role carries privileged permissions and cannot be self-assignable.",
  ROLE_HIERARCHY: "You can only bind roles below your highest role.",
  DUPLICATE_BINDING: "That emoji is already bound on this message.",
  DEFAULT_ROLE_NOT_BINDABLE: "The @everyone role cannot be bound.",
  MESSAGE_NOT_FOUND: "No message with that ID exists in the selected channel.",
  CHANNEL_NOT_FOUND: "That channel is not part of this guild.",
  ROLE_NOT_FOUND: "That role no longer exists.",
};

const ReactionRolesTab: Component<ReactionRolesTabProps> = (props) => {
  const [bindings, setBindings] = createSignal<ReactionRole[]>([]);
  const [loading, setLoading] = createSignal(true);
  const [error, setError] = createSignal<string | null>(null);

  // Create-form state.
  const [channelId, setChannelId] = createSignal("");
  const [messageId, setMessageId] = createSignal("");
  const [emoji, setEmoji] = createSignal("");
  const [roleId, setRoleId] = createSignal("");
  const [mode, setMode] = createSignal<"toggle" | "unique">("toggle");
  const [groupKey, setGroupKey] = createSignal("");
  const [submitting, setSubmitting] = createSignal(false);

  const refresh = async () => {
    setLoading(true);
    try {
      setBindings(await listReactionRoles(props.guildId));
    } catch (err) {
      console.error("Failed to load reaction roles:", err);
      setError("Failed to load reaction roles.");
    } finally {
      setLoading(false);
    }
  };

  onMount(() => {
    loadGuildRoles(props.guildId);
    void refresh();
  });

  // Only non-default roles are bindable; the server enforces hierarchy + safety.
  const assignableRoles = (): GuildRole[] =>
    getGuildRoles(props.guildId).filter((r) => !r.is_default);

  const roleName = (id: string): string =>
    getGuildRoles(props.guildId).find((r) => r.id === id)?.name ?? "unknown role";

  const channelName = (id: string): string =>
    textChannels().find((c) => c.id === id)?.name ?? "unknown channel";

  const parseError = (err: unknown): string => {
    const msg = err instanceof Error ? err.message : String(err);
    for (const [code, friendly] of Object.entries(ERROR_MESSAGES)) {
      if (msg.includes(code)) return friendly;
    }
    return msg || "Failed to create binding.";
  };

  const canSubmit = () =>
    channelId() !== "" &&
    messageId().trim() !== "" &&
    emoji().trim() !== "" &&
    roleId() !== "" &&
    !submitting();

  const handleCreate = async (e: Event) => {
    e.preventDefault();
    if (!canSubmit()) return;
    setSubmitting(true);
    setError(null);
    try {
      await createReactionRole(props.guildId, {
        channel_id: channelId(),
        message_id: messageId().trim(),
        emoji: emoji().trim(),
        role_id: roleId(),
        mode: mode(),
        group_key:
          mode() === "unique" && groupKey().trim() !== ""
            ? groupKey().trim()
            : undefined,
      });
      // Reset the volatile fields; keep channel/message for adding siblings.
      setEmoji("");
      setRoleId("");
      await refresh();
    } catch (err) {
      setError(parseError(err));
    } finally {
      setSubmitting(false);
    }
  };

  const handleDelete = async (bindingId: string) => {
    try {
      await deleteReactionRole(props.guildId, bindingId);
      await refresh();
    } catch (err) {
      console.error("Failed to delete binding:", err);
      setError("Failed to delete binding.");
    }
  };

  const inputClass =
    "w-full px-3 py-2 rounded-lg text-sm text-text-primary bg-[var(--color-surface-layer2)] border border-white/10 focus:border-accent-primary outline-none";

  return (
    <div class="p-6">
      <div class="mb-4">
        <h3 class="text-lg font-semibold text-text-primary">Reaction Roles</h3>
        <p class="text-sm text-text-secondary mt-1">
          Bind an emoji on a message to a role. Members grant or revoke that
          role themselves by reacting. Use <b>unique</b> with a shared group to
          make a pick-one set (e.g. a color).
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
            >
              <option value="">Select a channel…</option>
              <For each={textChannels()}>
                {(c) => <option value={c.id}>#{c.name}</option>}
              </For>
            </select>
          </label>
          <label class="block">
            <span class="text-xs text-text-secondary">Message ID</span>
            <input
              class={inputClass}
              type="text"
              placeholder="Paste a message ID"
              value={messageId()}
              onInput={(e) => setMessageId(e.currentTarget.value)}
            />
          </label>
        </div>

        <div class="grid grid-cols-2 gap-3">
          <label class="block">
            <span class="text-xs text-text-secondary">Emoji</span>
            <input
              class={inputClass}
              type="text"
              placeholder="🎨 or <:name:id>"
              value={emoji()}
              onInput={(e) => setEmoji(e.currentTarget.value)}
            />
          </label>
          <label class="block">
            <span class="text-xs text-text-secondary">Role</span>
            <select
              class={inputClass}
              value={roleId()}
              onChange={(e) => setRoleId(e.currentTarget.value)}
            >
              <option value="">Select a role…</option>
              <For each={assignableRoles()}>
                {(r) => <option value={r.id}>{r.name}</option>}
              </For>
            </select>
          </label>
        </div>

        <div class="grid grid-cols-2 gap-3">
          <label class="block">
            <span class="text-xs text-text-secondary">Mode</span>
            <select
              class={inputClass}
              value={mode()}
              onChange={(e) =>
                setMode(e.currentTarget.value as "toggle" | "unique")
              }
            >
              <option value="toggle">Toggle (grant / revoke)</option>
              <option value="unique">Unique (pick one in group)</option>
            </select>
          </label>
          <Show when={mode() === "unique"}>
            <label class="block">
              <span class="text-xs text-text-secondary">Group key</span>
              <input
                class={inputClass}
                type="text"
                placeholder="e.g. color"
                value={groupKey()}
                onInput={(e) => setGroupKey(e.currentTarget.value)}
              />
            </label>
          </Show>
        </div>

        <button
          type="submit"
          disabled={!canSubmit()}
          class="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-accent-primary text-on-accent text-sm font-medium hover:bg-accent-primary/90 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <Plus class="w-4 h-4" />
          Add binding
        </button>
      </form>

      {/* Existing bindings */}
      <Show
        when={!loading()}
        fallback={
          <div class="text-center py-8 text-text-secondary">Loading…</div>
        }
      >
        <Show
          when={bindings().length > 0}
          fallback={
            <div class="text-center py-8 text-text-secondary text-sm">
              No reaction roles yet.
            </div>
          }
        >
          <div class="space-y-2">
            <For each={bindings()}>
              {(b) => (
                <div
                  class="flex items-center gap-3 p-3 rounded-lg border border-white/10"
                  style={{ "background-color": "var(--color-surface-layer1)" }}
                >
                  <span class="text-xl flex-shrink-0">{b.emoji}</span>
                  <div class="flex-1 min-w-0">
                    <div class="font-medium text-text-primary truncate">
                      {roleName(b.role_id)}
                    </div>
                    <div class="text-xs text-text-secondary flex items-center gap-2">
                      <span>#{channelName(b.channel_id)}</span>
                      <span class="px-1.5 py-0.5 rounded bg-accent-primary/20 text-text-primary">
                        {b.mode}
                      </span>
                      <Show when={b.group_key}>
                        <span class="flex items-center gap-1">
                          <Tag class="w-3 h-3" />
                          {b.group_key}
                        </span>
                      </Show>
                    </div>
                  </div>
                  <button
                    onClick={() => handleDelete(b.id)}
                    class="p-2 rounded-lg text-text-secondary hover:text-accent-danger hover:bg-white/10 transition-colors"
                    title="Delete binding"
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

export default ReactionRolesTab;
