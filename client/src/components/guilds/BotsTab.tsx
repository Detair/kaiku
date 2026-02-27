/**
 * BotsTab - Installed bot management for guild settings
 */

import { Component, createSignal, For, Show, onMount } from "solid-js";
import { Trash2, Bot } from "lucide-solid";
import { listInstalledBots, removeInstalledBot } from "@/lib/api/bots";
import type { InstalledBot } from "@/lib/api/bots";

interface BotsTabProps {
  guildId: string;
}

const BotsTab: Component<BotsTabProps> = (props) => {
  const [bots, setBots] = createSignal<InstalledBot[]>([]);
  const [loading, setLoading] = createSignal(true);
  const [deleteConfirm, setDeleteConfirm] = createSignal<string | null>(null);

  onMount(async () => {
    try {
      const data = await listInstalledBots(props.guildId);
      setBots(data);
    } catch (err) {
      console.error("Failed to load installed bots:", err);
    } finally {
      setLoading(false);
    }
  });

  const handleRemove = async (botUserId: string) => {
    if (deleteConfirm() === botUserId) {
      try {
        await removeInstalledBot(props.guildId, botUserId);
        setBots((prev) => prev.filter((b) => b.bot_user_id !== botUserId));
      } catch (err) {
        console.error("Failed to remove bot:", err);
      }
      setDeleteConfirm(null);
    } else {
      setDeleteConfirm(botUserId);
      setTimeout(() => setDeleteConfirm(null), 3000);
    }
  };

  const formatDate = (iso: string) => {
    const d = new Date(iso);
    return d.toLocaleDateString(undefined, {
      year: "numeric",
      month: "short",
      day: "numeric",
    });
  };

  return (
    <div class="p-6">
      {/* Header */}
      <div class="flex items-center justify-between mb-6">
        <div>
          <h3 class="text-lg font-semibold text-text-primary">Bots</h3>
          <p class="text-sm text-text-secondary">
            Manage bots installed in this server.
          </p>
        </div>
      </div>

      <Show
        when={!loading()}
        fallback={
          <div class="py-8 text-center text-text-secondary">Loading...</div>
        }
      >
        <div class="space-y-2">
          <For
            each={bots()}
            fallback={
              <div class="py-8 text-center text-text-secondary border border-dashed border-white/10 rounded-xl">
                No bots installed
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
                    {bot.description || "No description"}
                    {" \u00b7 "}
                    Installed {formatDate(bot.installed_at)}
                  </div>
                </div>

                <button
                  onClick={() => handleRemove(bot.bot_user_id)}
                  class="p-1.5 rounded-lg opacity-0 group-hover:opacity-100 transition-opacity"
                  classList={{
                    "bg-accent-danger text-white opacity-100":
                      deleteConfirm() === bot.bot_user_id,
                    "bg-black/50 text-white hover:bg-accent-danger":
                      deleteConfirm() !== bot.bot_user_id,
                  }}
                  title="Remove Bot"
                >
                  <Trash2 class="w-3.5 h-3.5" />
                </button>
              </div>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
};

export default BotsTab;
