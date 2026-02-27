import { Component, createResource, Show } from "solid-js";
import { A } from "@solidjs/router";
import { ArrowLeft } from "lucide-solid";
import { fetchApi } from "../../lib/tauri";
import { ConnectionChart } from "../../components/settings/ConnectionChart";
import { SessionList } from "../../components/settings/SessionList";

interface DailyStat {
  date: string;
  avg_latency: number | null;
  avg_loss: number | null;
  avg_jitter: number | null;
  session_count: number;
}

interface ConnectionSummary {
  period_days: number;
  avg_latency: number | null;
  avg_packet_loss: number | null;
  avg_jitter: number | null;
  total_sessions: number;
  total_duration_secs: number;
  daily_stats: DailyStat[];
}

async function fetchSummary(): Promise<ConnectionSummary> {
  return fetchApi("/api/me/connection/summary");
}

export const ConnectionHistory: Component = () => {
  const [summary] = createResource(fetchSummary);

  const formatDuration = (secs: number) => {
    const hours = Math.floor(secs / 3600);
    const mins = Math.floor((secs % 3600) / 60);
    if (hours > 0) return `${hours}h ${mins}m`;
    return `${mins}m`;
  };

  return (
    <div class="min-h-screen bg-surface-base text-text-primary p-6">
      <div class="max-w-4xl mx-auto">
        <div class="flex items-center gap-4 mb-6">
          <A href="/settings" class="p-2 hover:bg-surface-layer1 rounded-lg">
            <ArrowLeft class="w-5 h-5" />
          </A>
          <h1 class="text-xl font-semibold">Connection History</h1>
        </div>

        <Show
          when={!summary.loading}
          fallback={<div class="text-text-secondary">Loading...</div>}
        >
          <Show
            when={(summary()?.total_sessions ?? 0) > 0}
            fallback={
              <div class="text-center py-16">
                <div class="text-4xl mb-4">No Data</div>
                <div class="text-lg font-medium mb-2">
                  No voice sessions yet
                </div>
                <div class="text-text-secondary">
                  Join a voice channel to start tracking your connection quality
                  over time.
                </div>
              </div>
            }
          >
            <div class="space-y-6">
              {/* Summary stats */}
              <div class="bg-surface-layer1 rounded-lg p-4">
                <h2 class="text-sm font-medium text-text-secondary mb-3">
                  Last {summary()!.period_days} Days
                </h2>
                <div class="grid grid-cols-4 gap-4 text-center">
                  <div>
                    <div class="text-2xl font-semibold">
                      {summary()!.avg_latency ?? "-"}ms
                    </div>
                    <div class="text-xs text-text-secondary">Avg Latency</div>
                  </div>
                  <div>
                    <div class="text-2xl font-semibold">
                      {summary()!.avg_packet_loss?.toFixed(1) ?? "-"}%
                    </div>
                    <div class="text-xs text-text-secondary">Avg Loss</div>
                  </div>
                  <div>
                    <div class="text-2xl font-semibold">
                      {summary()!.avg_jitter ?? "-"}ms
                    </div>
                    <div class="text-xs text-text-secondary">Avg Jitter</div>
                  </div>
                  <div>
                    <div class="text-2xl font-semibold">
                      {formatDuration(summary()!.total_duration_secs)}
                    </div>
                    <div class="text-xs text-text-secondary">Total Time</div>
                  </div>
                </div>
              </div>

              {/* Chart */}
              <div class="bg-surface-layer1 rounded-lg p-4">
                <h2 class="text-sm font-medium text-text-secondary mb-3">
                  Quality Over Time
                </h2>
                <ConnectionChart data={summary()!.daily_stats} />
              </div>

              {/* Sessions */}
              <div class="bg-surface-layer1 rounded-lg p-4">
                <h2 class="text-sm font-medium text-text-secondary mb-3">
                  Recent Sessions
                </h2>
                <SessionList />
              </div>
            </div>
          </Show>
        </Show>
      </div>
    </div>
  );
};

export default ConnectionHistory;
