import { Component, For, Show, createSignal, onMount } from "solid-js";
import { getGuildUsage } from "@/lib/tauri";
import type { GuildUsageStats } from "@/lib/types";

interface UsageTabProps {
  guildId: string;
}

const UsageTab: Component<UsageTabProps> = (props) => {
  const [usage, setUsage] = createSignal<GuildUsageStats | null>(null);
  const [loading, setLoading] = createSignal(true);
  const [error, setError] = createSignal<string | null>(null);

  onMount(async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await getGuildUsage(props.guildId);
      setUsage(data);
    } catch (err) {
      console.error("Failed to load guild usage stats:", err);
      setError("Could not load usage stats.");
    } finally {
      setLoading(false);
    }
  });

  const rows = () => {
    const data = usage();
    if (!data) return [];
    return [
      { label: "Members", value: data.members.current, limit: data.members.limit },
      { label: "Channels", value: data.channels.current, limit: data.channels.limit },
      { label: "Roles", value: data.roles.current, limit: data.roles.limit },
      { label: "Emojis", value: data.emojis.current, limit: data.emojis.limit },
      { label: "Bots", value: data.bots.current, limit: data.bots.limit },
      { label: "Pages", value: data.pages.current, limit: data.pages.limit },
    ];
  };

  return (
    <div class="p-6 space-y-4">
      <div>
        <h3 class="text-sm font-semibold text-text-primary uppercase tracking-wide">Usage</h3>
        <p class="text-xs text-text-secondary mt-1">Current guild usage against plan limits.</p>
      </div>

      <Show when={loading()}>
        <p class="text-sm text-text-secondary">Loading usage stats...</p>
      </Show>

      <Show when={error()}>{(msg) => <p class="text-sm text-red-400">{msg()}</p>}</Show>

      <Show when={!loading() && usage()}>
        <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
          <For each={rows()}>
            {(row) => {
              const percent = () => {
                if (row.limit <= 0) return 0;
                return Math.min(100, Math.round((row.value / row.limit) * 100));
              };

              return (
                <div class="p-4 rounded-xl border border-white/10 bg-surface-layer2">
                  <div class="flex items-center justify-between text-sm">
                    <span class="text-text-primary font-medium">{row.label}</span>
                    <span class="text-text-secondary">
                      {row.value} / {row.limit}
                    </span>
                  </div>
                  <div class="mt-2 h-2 rounded-full bg-white/10 overflow-hidden">
                    <div
                      class="h-full bg-accent-primary transition-all"
                      style={{ width: `${percent()}%` }}
                    />
                  </div>
                </div>
              );
            }}
          </For>
        </div>
      </Show>
    </div>
  );
};

export default UsageTab;
