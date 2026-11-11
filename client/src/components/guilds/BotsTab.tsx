/**
 * BotsTab - Installed bot management for guild settings
 */

import { Component, createSignal, For, Show, onMount } from "solid-js";
import { Trash2, Bot } from "lucide-solid";
import { listInstalledBots, removeInstalledBot } from "@/lib/api/bots";
import type { InstalledBot } from "@/lib/api/bots";
import { t } from "@/i18n";
import ConfirmActionDialog from "@/components/admin/ConfirmActionDialog";
import { formatDay } from "@/components/admin/formatDate";
import { reportFailure } from "./settingsAction";

interface BotsTabProps {
  guildId: string;
}

const BotsTab: Component<BotsTabProps> = (props) => {
  const [bots, setBots] = createSignal<InstalledBot[]>([]);
  const [loading, setLoading] = createSignal(true);
  const [removing, setRemoving] = createSignal<InstalledBot | null>(null);

  onMount(async () => {
    try {
      const data = await listInstalledBots(props.guildId);
      setBots(data);
    } catch (err) {
      reportFailure(t("guildSettings.bots.loadFailed"), err);
    } finally {
      setLoading(false);
    }
  });

  const remove = async (botUserId: string) => {
    await removeInstalledBot(props.guildId, botUserId);
    setBots((prev) => prev.filter((b) => b.bot_user_id !== botUserId));
  };

  return (
    <div class="p-6">
      {/* Header */}
      <div class="flex items-center justify-between mb-6">
        <div>
          <h3 class="text-lg font-semibold text-text-primary">{t("guildSettings.bots.title")}</h3>
          <p class="text-sm text-text-secondary">
            {t("guildSettings.bots.hint")}
          </p>
        </div>
      </div>

      <Show
        when={!loading()}
        fallback={
          <div class="py-8 text-center text-text-secondary">{t("guildSettings.loading")}</div>
        }
      >
        <div class="space-y-2">
          <For
            each={bots()}
            fallback={
              <div class="py-8 text-center text-text-secondary border border-dashed border-white/10 rounded-xl">
                {t("guildSettings.bots.empty")}
              </div>
            }
          >
            {(bot) => (
              <div class="group relative flex items-center gap-3 p-3 rounded-xl bg-surface-layer1 border border-white/5 hover:border-white/10 transition-colors">
                <div class="w-10 h-10 flex-shrink-0 rounded-full bg-accent-primary/20 flex items-center justify-center">
                  <Bot class="w-5 h-5 text-accent-primary" />
                </div>
                <div class="flex-1 min-w-0">
                  <div class="font-medium text-text-primary truncate">
                    {bot.name}
                  </div>
                  <div class="text-xs text-text-secondary">
                    {t("guildSettings.bots.line", {
                      description: bot.description || t("guildSettings.bots.noDescription"),
                      date: formatDay(bot.installed_at),
                    })}
                  </div>
                </div>

                <button
                  onClick={() => setRemoving(bot)}
                  class="p-1.5 rounded-lg opacity-0 group-hover:opacity-100 focus-visible:opacity-100 touch:opacity-100 transition-opacity bg-black/50 text-white hover:bg-accent-danger-fill"
                  title={t("guildSettings.bots.removeNamed", { name: bot.name })}
                  aria-label={t("guildSettings.bots.removeNamed", { name: bot.name })}
                >
                  <Trash2 class="w-3.5 h-3.5" />
                </button>
              </div>
            )}
          </For>
        </div>
      </Show>

      <Show when={removing()}>
        {(bot) => (
          <ConfirmActionDialog
            title={t("guildSettings.bots.removeTitle", { name: bot().name })}
            message={t("guildSettings.bots.removeBody", { name: bot().name })}
            confirmLabel={t("guildSettings.bots.remove")}
            danger
            onConfirm={() => remove(bot().bot_user_id)}
            onDone={() => setRemoving(null)}
            onCancel={() => setRemoving(null)}
          />
        )}
      </Show>
    </div>
  );
};

export default BotsTab;
