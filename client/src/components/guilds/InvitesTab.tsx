/**
 * InvitesTab - Invite management for guild owners
 */

import { Component, createSignal, For, Show, onMount } from "solid-js";
import { Copy, Trash2, Plus } from "lucide-solid";
import {
  guildsState,
  loadGuildInvites,
  createInvite,
  deleteInvite,
  getGuildInvites,
} from "@/stores/guilds";
import { secureCopy } from "@/lib/clipboard";
import type { InviteExpiry } from "@/lib/types";

interface InvitesTabProps {
  guildId: string;
}

const EXPIRY_OPTIONS: { value: InviteExpiry; label: string }[] = [
  { value: "30m", label: "30 minutes" },
  { value: "1h", label: "1 hour" },
  { value: "1d", label: "1 day" },
  { value: "7d", label: "7 days" },
  { value: "never", label: "Never" },
];

const InvitesTab: Component<InvitesTabProps> = (props) => {
  const [expiresIn, setExpiresIn] = createSignal<InviteExpiry>("7d");
  const [isCreating, setIsCreating] = createSignal(false);
  const [copiedCode, setCopiedCode] = createSignal<string | null>(null);
  const [deletingCode, setDeletingCode] = createSignal<string | null>(null);

  onMount(() => {
    loadGuildInvites(props.guildId);
  });

  const invites = () => getGuildInvites(props.guildId);

  const handleCreate = async () => {
    setIsCreating(true);
    try {
      await createInvite(props.guildId, expiresIn());
    } catch (err) {
      console.error("Failed to create invite:", err);
    } finally {
      setIsCreating(false);
    }
  };

  const handleCopy = async (code: string) => {
    const url = `${window.location.origin}/invite/${code}`;
    await secureCopy(url, "invite_link");
    setCopiedCode(code);
    setTimeout(() => setCopiedCode(null), 2000);
  };

  const handleDelete = async (code: string) => {
    if (deletingCode() === code) {
      // Confirmed, delete it
      try {
        await deleteInvite(props.guildId, code);
      } catch (err) {
        console.error("Failed to delete invite:", err);
      }
      setDeletingCode(null);
    } else {
      // First click, show confirmation
      setDeletingCode(code);
      setTimeout(() => setDeletingCode(null), 3000);
    }
  };

  const formatExpiry = (expiresAt: string | null): string => {
    if (!expiresAt) return "Never expires";
    const expires = new Date(expiresAt);
    const now = new Date();
    const diff = expires.getTime() - now.getTime();

    if (diff <= 0) return "Expired";

    const minutes = Math.floor(diff / 60000);
    const hours = Math.floor(diff / 3600000);
    const days = Math.floor(diff / 86400000);

    if (days > 0) return `Expires in ${days} day${days > 1 ? "s" : ""}`;
    if (hours > 0) return `Expires in ${hours} hour${hours > 1 ? "s" : ""}`;
    return `Expires in ${minutes} minute${minutes > 1 ? "s" : ""}`;
  };

  return (
    <div class="p-6">
      {/* Create Invite */}
      <div
        class="p-4 rounded-xl border border-white/10"
        style="background-color: var(--color-surface-layer1)"
      >
        <h3 class="text-sm font-semibold text-text-primary mb-3">
          Create New Invite
        </h3>
        <div class="flex items-center gap-3">
          <div class="flex-1">
            <label class="text-xs text-text-secondary mb-1 block">
              Expires after
            </label>
            <select
              value={expiresIn()}
              onChange={(e) =>
                setExpiresIn(e.currentTarget.value as InviteExpiry)
              }
              class="w-full px-3 py-2 rounded-lg border border-white/10 text-text-primary"
              style="background-color: var(--color-surface-layer2)"
            >
              <For each={EXPIRY_OPTIONS}>
                {(opt) => <option value={opt.value}>{opt.label}</option>}
              </For>
            </select>
          </div>
          <button
            onClick={handleCreate}
            disabled={isCreating()}
            class="flex items-center gap-2 px-4 py-2 bg-accent-primary text-white rounded-lg font-medium hover:opacity-90 disabled:opacity-50 mt-5"
          >
            <Plus class="w-4 h-4" />
            {isCreating() ? "Creating..." : "Create"}
          </button>
        </div>
      </div>

      {/* Active Invites */}
      <div class="mt-6">
        <h3 class="text-sm font-semibold text-text-primary mb-3">
          Active Invites ({invites().length})
        </h3>

        <Show
          when={invites().length > 0}
          fallback={
            <div class="text-center py-8 text-text-secondary">
              No active invites. Create one to let people join!
            </div>
          }
        >
          <div class="space-y-2">
            <For each={invites()}>
              {(invite) => (
                <div
                  class="flex items-center justify-between p-3 rounded-lg border border-white/5"
                  style="background-color: var(--color-surface-layer1)"
                >
                  <div class="flex-1 min-w-0">
                    <code class="text-sm text-accent-primary font-mono truncate block">
                      {window.location.origin}/invite/{invite.code}
                    </code>
                    <div class="text-xs text-text-secondary mt-1">
                      {formatExpiry(invite.expires_at)} &bull;{" "}
                      {invite.use_count} use{invite.use_count !== 1 ? "s" : ""}
                    </div>
                  </div>
                  <div class="flex items-center gap-2 ml-3">
                    <button
                      onClick={() => handleCopy(invite.code)}
                      class="p-2 text-text-secondary hover:text-accent-primary hover:bg-white/10 rounded-lg transition-colors"
                      title="Copy invite link"
                    >
                      <Show
                        when={copiedCode() === invite.code}
                        fallback={<Copy class="w-4 h-4" />}
                      >
                        <span class="text-xs text-accent-primary">Copied!</span>
                      </Show>
                    </button>
                    <button
                      onClick={() => handleDelete(invite.code)}
                      class="p-2 rounded-lg transition-colors"
                      classList={{
                        "bg-accent-danger text-white":
                          deletingCode() === invite.code,
                        "text-text-secondary hover:text-accent-danger hover:bg-white/10":
                          deletingCode() !== invite.code,
                      }}
                      title={
                        deletingCode() === invite.code
                          ? "Click again to confirm"
                          : "Delete invite"
                      }
                    >
                      <Show
                        when={deletingCode() === invite.code}
                        fallback={<Trash2 class="w-4 h-4" />}
                      >
                        <span class="text-xs">Confirm?</span>
                      </Show>
                    </button>
                  </div>
                </div>
              )}
            </For>
          </div>
        </Show>
      </div>

      {/* Loading state */}
      <Show when={guildsState.isInvitesLoading}>
        <div class="text-center py-4 text-text-secondary">
          Loading invites...
        </div>
      </Show>
    </div>
  );
};

export default InvitesTab;
