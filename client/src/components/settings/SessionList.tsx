/**
 * Session List Component
 *
 * Displays a paginated list of voice session summaries with quality indicators.
 * Shows channel name, guild, time info, and basic metrics for each session.
 */

import { Component, createResource, createSignal, Show, For } from "solid-js";
import { fetchApi } from "../../lib/tauri";

interface SessionSummary {
  id: string;
  channel_name: string;
  guild_name: string | null;
  started_at: string;
  ended_at: string;
  avg_latency: number | null;
  avg_loss: number | null;
  avg_jitter: number | null;
  worst_quality: number | null;
}

interface SessionListResponse {
  sessions: SessionSummary[];
  total: number;
  limit: number;
  offset: number;
}

const qualityColors = [
  "bg-red-500",
  "bg-orange-500",
  "bg-yellow-500",
  "bg-green-500",
];
const qualityLabels = ["Poor", "Fair", "Good", "Excellent"];

async function fetchSessions(offset: number): Promise<SessionSummary[]> {
  const response = await fetchApi<SessionListResponse>(
    `/api/me/connection/sessions?limit=10&offset=${offset}`,
  );
  return response.sessions;
}

export const SessionList: Component = () => {
  const [offset, setOffset] = createSignal(0);
  const [sessions] = createResource(offset, fetchSessions);

  const formatTime = (iso: string) => {
    const d = new Date(iso);
    return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  };

  const formatDate = (iso: string) => {
    const d = new Date(iso);
    const today = new Date();
    const yesterday = new Date(today);
    yesterday.setDate(yesterday.getDate() - 1);

    if (d.toDateString() === today.toDateString()) return "Today";
    if (d.toDateString() === yesterday.toDateString()) return "Yesterday";
    return d.toLocaleDateString([], { month: "short", day: "numeric" });
  };

  const formatDuration = (start: string, end: string) => {
    const ms = new Date(end).getTime() - new Date(start).getTime();
    const mins = Math.floor(ms / 60000);
    const hours = Math.floor(mins / 60);
    if (hours > 0) return `${hours}h ${mins % 60}m`;
    return `${mins}m`;
  };

  return (
    <div class="space-y-2">
      <Show
        when={!sessions.loading}
        fallback={<div class="text-text-secondary text-sm">Loading...</div>}
      >
        <For each={sessions()}>
          {(session) => (
            <div class="flex items-center gap-3 p-3 bg-surface-layer2 rounded-lg">
              <div
                class={`w-2 h-2 rounded-full ${qualityColors[session.worst_quality ?? 0]}`}
                title={qualityLabels[session.worst_quality ?? 0]}
              />
              <div class="flex-1 min-w-0">
                <div class="flex items-center gap-2">
                  <span class="font-medium truncate">
                    {session.channel_name}
                  </span>
                  <Show when={session.guild_name}>
                    <span class="text-text-secondary text-xs">
                      in {session.guild_name}
                    </span>
                  </Show>
                </div>
                <div class="text-xs text-text-secondary">
                  {formatDate(session.started_at)},{" "}
                  {formatTime(session.started_at)} -{" "}
                  {formatTime(session.ended_at)} (
                  {formatDuration(session.started_at, session.ended_at)})
                </div>
              </div>
              <div class="text-right text-xs text-text-secondary">
                <div>{session.avg_latency ?? "-"}ms</div>
                <div>{session.avg_loss?.toFixed(1) ?? "-"}% loss</div>
              </div>
            </div>
          )}
        </For>

        <Show when={sessions()?.length === 10}>
          <button
            class="w-full py-2 text-sm text-text-secondary hover:text-text-primary"
            onClick={() => setOffset((o) => o + 10)}
          >
            Load more...
          </button>
        </Show>
      </Show>
    </div>
  );
};

export default SessionList;
