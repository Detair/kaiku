/**
 * Server name (ADMIN-5): the owner renames the server here. Only the owner
 * may (`PATCH /api/guilds/{id}` checks it). The server symbol needs an
 * upload endpoint the server does not have yet, so it is not offered.
 */
import { Component, Show, createSignal } from "solid-js";
import { guildsState, isGuildOwner } from "@/stores/guilds";
import { authState } from "@/stores/auth";
import { renameServer } from "@/stores/guildAdmin";
import { t } from "@/i18n";
import { reportFailure, reportSuccess } from "./settingsAction";

const ServerNameSection: Component<{ guildId: string }> = (props) => {
  const guild = () => guildsState.guilds.find((g) => g.id === props.guildId);
  const isOwner = () => isGuildOwner(props.guildId, authState.user?.id || "");
  const [draft, setDraft] = createSignal<string | null>(null);
  const [saving, setSaving] = createSignal(false);

  const name = () => draft() ?? guild()?.name ?? "";
  const valid = () => {
    const n = name().trim();
    return n.length >= 2 && n.length <= 100;
  };
  const changed = () => draft() !== null && name().trim() !== guild()?.name && valid();

  const save = async (e: Event) => {
    e.preventDefault();
    if (!changed() || saving()) return;
    setSaving(true);
    try {
      await renameServer(props.guildId, name());
      reportSuccess(t("guildSettings.server.renamed", { name: name().trim() }));
      setDraft(null);
    } catch (err) {
      reportFailure(t("guildSettings.server.renameFailed"), err);
    } finally {
      setSaving(false);
    }
  };

  return (
    <Show when={isOwner()}>
      <form onSubmit={save} class="space-y-2" data-testid="server-name-section">
        <label for="server-name-input" class="block text-sm font-medium text-text-primary">
          {t("guildSettings.server.name")}
        </label>
        <div class="flex gap-2">
          <input
            id="server-name-input"
            type="text"
            maxLength={100}
            value={name()}
            onInput={(e) => setDraft(e.currentTarget.value)}
            class="flex-1 px-3 py-2 rounded-lg border border-white/10 text-text-primary focus:outline-none focus:border-accent-primary"
            style={{ "background-color": "var(--color-surface-layer1)" }}
          />
          <button
            type="submit"
            disabled={!changed() || saving()}
            class="px-4 py-2 rounded-lg bg-accent-primary text-on-accent font-medium hover:bg-accent-primary/90 state-disabled"
          >
            {saving() ? t("guildSettings.working") : t("common.save")}
          </button>
        </div>
        <Show when={draft() !== null && !valid()}>
          <p class="text-xs text-text-secondary">{t("guildSettings.server.nameRule")}</p>
        </Show>
      </form>
    </Show>
  );
};

export default ServerNameSection;
