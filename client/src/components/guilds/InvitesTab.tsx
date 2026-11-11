/**
 * InvitesTab - Invite management for guild owners
 *
 * A new code is shown large and copied right away, with one sentence on how
 * to pass it on (ADMIN-16). Failures show a toast (ADMIN-6); removing a code
 * asks in a dialog that names it (ADMIN-13).
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
import { plural, t, type MessageKey } from "@/i18n";
import type { InviteExpiry } from "@/lib/types";
import ConfirmActionDialog from "@/components/admin/ConfirmActionDialog";
import { reportFailure } from "./settingsAction";

interface InvitesTabProps {
  guildId: string;
}

const EXPIRY_OPTIONS: { value: InviteExpiry; label: MessageKey }[] = [
  { value: "30m", label: "guildSettings.invites.expiry.halfHour" },
  { value: "1h", label: "guildSettings.invites.expiry.oneHour" },
  { value: "1d", label: "guildSettings.invites.expiry.oneDay" },
  { value: "7d", label: "guildSettings.invites.expiry.sevenDays" },
  { value: "never", label: "guildSettings.invites.expiry.never" },
];

const InvitesTab: Component<InvitesTabProps> = (props) => {
  const [expiresIn, setExpiresIn] = createSignal<InviteExpiry>("7d");
  const [isCreating, setIsCreating] = createSignal(false);
  const [copiedCode, setCopiedCode] = createSignal<string | null>(null);
  const [newCode, setNewCode] = createSignal<string | null>(null);
  const [deleting, setDeleting] = createSignal<string | null>(null);

  onMount(() => {
    loadGuildInvites(props.guildId);
  });

  const invites = () => getGuildInvites(props.guildId);

  const copy = async (code: string) => {
    // The code, not a link: the family server has no browser client to open one.
    await secureCopy(code, "invite_link");
    setCopiedCode(code);
    setTimeout(() => setCopiedCode((c) => (c === code ? null : c)), 2000);
  };

  const handleCreate = async () => {
    setIsCreating(true);
    try {
      const invite = await createInvite(props.guildId, expiresIn());
      setNewCode(invite.code);
      await copy(invite.code);
    } catch (err) {
      reportFailure(t("guildSettings.invites.createFailed"), err);
    } finally {
      setIsCreating(false);
    }
  };

  const removeCode = async (code: string) => {
    await deleteInvite(props.guildId, code);
    if (newCode() === code) setNewCode(null);
  };

  const formatExpiry = (expiresAt: string | null): string => {
    if (!expiresAt) return t("guildSettings.invites.neverExpires");
    const diff = new Date(expiresAt).getTime() - Date.now();
    if (diff <= 0) return t("guildSettings.invites.expired");

    const minutes = Math.floor(diff / 60000);
    const hours = Math.floor(diff / 3600000);
    const days = Math.floor(diff / 86400000);

    if (days > 0)
      return plural(days, { one: "guildSettings.invites.expiresDays.one", other: "guildSettings.invites.expiresDays.other" });
    if (hours > 0)
      return plural(hours, { one: "guildSettings.invites.expiresHours.one", other: "guildSettings.invites.expiresHours.other" });
    return plural(minutes, { one: "guildSettings.invites.expiresMinutes.one", other: "guildSettings.invites.expiresMinutes.other" });
  };

  return (
    <div class="p-6">
      {/* Create Invite */}
      <div
        class="p-4 rounded-xl border border-white/10"
        style={{"background-color":"var(--color-surface-layer1)"}}
      >
        <h3 class="text-sm font-semibold text-text-primary mb-3">
          {t("guildSettings.invites.createTitle")}
        </h3>
        <div class="flex items-center gap-3">
          <div class="flex-1">
            <label for="invite-expiry" class="text-xs text-text-secondary mb-1 block">
              {t("guildSettings.invites.expiresAfter")}
            </label>
            <select
              id="invite-expiry"
              value={expiresIn()}
              onChange={(e) =>
                setExpiresIn(e.currentTarget.value as InviteExpiry)
              }
              class="w-full px-3 py-2 rounded-lg border border-white/10 text-text-primary"
              style={{"background-color":"var(--color-surface-layer2)"}}
            >
              <For each={EXPIRY_OPTIONS}>
                {(opt) => <option value={opt.value}>{t(opt.label)}</option>}
              </For>
            </select>
          </div>
          <button
            data-testid="create-invite-button"
            onClick={handleCreate}
            disabled={isCreating()}
            class="flex items-center gap-2 px-4 py-2 bg-accent-primary text-on-accent rounded-lg font-medium hover:opacity-90 mt-5 state-disabled"
          >
            <Plus class="w-4 h-4" />
            {isCreating() ? t("guildSettings.working") : t("guildSettings.invites.create")}
          </button>
        </div>

        {/* The code just created: large, copied, with how to pass it on. */}
        <Show when={newCode()}>
          {(code) => (
            <div
              data-testid="new-invite"
              role="status"
              class="mt-4 p-4 rounded-lg bg-accent-primary/20 border border-accent-primary/40 space-y-2"
            >
              <div class="text-sm text-text-primary">
                {copiedCode() === code()
                  ? t("guildSettings.invites.newCopied")
                  : t("guildSettings.invites.newCode")}
              </div>
              <div class="flex items-center gap-3">
                <code class="text-2xl font-mono font-bold tracking-wider text-text-primary select-all">
                  {code()}
                </code>
                <button
                  type="button"
                  onClick={() => copy(code())}
                  class="px-3 py-1.5 rounded-lg bg-white/10 text-sm text-text-primary hover:bg-white/20"
                >
                  {t("guildSettings.invites.copy")}
                </button>
              </div>
              <p class="text-sm text-text-secondary">{t("guildSettings.invites.howToShare")}</p>
            </div>
          )}
        </Show>
      </div>

      {/* Active Invites */}
      <div class="mt-6">
        <h3 class="text-sm font-semibold text-text-primary mb-3">
          {t("guildSettings.invites.active", { count: invites().length })}
        </h3>

        <Show
          when={invites().length > 0}
          fallback={
            <div class="text-center py-8 text-text-secondary">
              {t("guildSettings.invites.empty")}
            </div>
          }
        >
          <div class="space-y-2">
            <For each={invites()}>
              {(invite) => (
                <div
                  class="flex items-center justify-between p-3 rounded-lg border border-white/5"
                  style={{"background-color":"var(--color-surface-layer1)"}}
                >
                  <div class="flex-1 min-w-0">
                    <code data-testid="invite-code" class="text-sm text-text-primary font-mono truncate block">
                      {invite.code}
                    </code>
                    <div class="text-xs text-text-secondary mt-1">
                      {formatExpiry(invite.expires_at)} &bull;{" "}
                      {plural(invite.use_count, { one: "guildSettings.invites.uses.one", other: "guildSettings.invites.uses.other" })}
                    </div>
                  </div>
                  <div class="flex items-center gap-2 ml-3">
                    <button
                      onClick={() => copy(invite.code)}
                      class="p-2 text-text-secondary hover:text-text-primary hover:bg-white/10 rounded-lg transition-colors"
                      title={t("settings.invites.copyCode")}
                      aria-label={t("settings.invites.copyCode")}
                    >
                      <Show
                        when={copiedCode() === invite.code}
                        fallback={<Copy class="w-4 h-4" />}
                      >
                        <span class="text-xs text-text-primary">{t("guildSettings.invites.copied")}</span>
                      </Show>
                    </button>
                    <button
                      onClick={() => setDeleting(invite.code)}
                      class="p-2 rounded-lg transition-colors text-text-secondary hover:text-text-primary hover:bg-white/10"
                      title={t("guildSettings.invites.deleteNamed", { code: invite.code })}
                      aria-label={t("guildSettings.invites.deleteNamed", { code: invite.code })}
                    >
                      <Trash2 class="w-4 h-4" />
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
          {t("guildSettings.loading")}
        </div>
      </Show>

      <Show when={deleting()}>
        {(code) => (
          <ConfirmActionDialog
            title={t("guildSettings.invites.deleteTitle")}
            message={t("guildSettings.invites.deleteBody", { code: code() })}
            confirmLabel={t("guildSettings.invites.deleteConfirm")}
            danger
            onConfirm={() => removeCode(code())}
            onDone={() => setDeleting(null)}
            onCancel={() => setDeleting(null)}
          />
        )}
      </Show>
    </div>
  );
};

export default InvitesTab;
