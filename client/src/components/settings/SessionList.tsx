/**
 * Session List Component
 *
 * Displays a paginated list of voice session summaries with quality indicators.
 * Shows channel name, guild, time info, and basic metrics for each session.
 */

import {
  Component,
  createResource,
  createSignal,
  createEffect,
  Show,
  For,
} from "solid-js";
import { fetchApi } from "../../lib/tauri";
import { formatShortDate, formatTime } from "@/i18n/format";
import { t, type MessageKey } from "@/i18n";

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
const qualityLabels: MessageKey[] = [
  "settings.sessionList.qualityPoor",
  "settings.sessionList.qualityFair",
  "settings.sessionList.qualityGood",
  "settings.sessionList.qualityExcellent",
];

async function fetchSessions(offset: number): Promise<SessionSummary[]> {
  const response = await fetchApi<SessionListResponse>(
    `/api/me/connection/sessions?limit=10&offset=${offset}`,
  );
  return response.sessions;
}

export const SessionList: Component = () => {
  const [offset, setOffset] = createSignal(0);
  // Accumulate pages instead of replacing: each fetched page is appended so
  // "Load more" grows the list rather than swapping to just the newest page.
  const [allSessions, setAllSessions] = createSignal<SessionSummary[]>([]);
  const [pageSize, setPageSize] = createSignal(0);
  const [page] = createResource(offset, fetchSessions);

  createEffect(() => {
    const fetched = page();
    if (!fetched) return;
    setPageSize(fetched.length);
    setAllSessions((prev) =>
      offset() === 0 ? fetched : [...prev, ...fetched],
    );
  });

  const formatDate = (iso: string) => {
    const d = new Date(iso);
    const today = new Date();
    const yesterday = new Date(today);
    yesterday.setDate(yesterday.getDate() - 1);

    if (d.toDateString() === today.toDateString()) return t("settings.sessionList.today");
    if (d.toDateString() === yesterday.toDateString()) return t("settings.sessionList.yesterday");
    return formatShortDate(d, today);
  };

  const formatDuration = (start: string, end: string) => {
    const ms = new Date(end).getTime() - new Date(start).getTime();
    const mins = Math.floor(ms / 60000);
    const hours = Math.floor(mins / 60);
    if (hours > 0) {
      return t("settings.sessionList.durationHoursMinutes", { hours, minutes: mins % 60 });
    }
    return t("settings.sessionList.durationMinutes", { minutes: mins });
  };

  return (
    <div class="space-y-2">
      <Show
        when={!page.loading || allSessions().length > 0}
        fallback={<div class="text-text-secondary text-sm">{t("common.loading")}</div>}
      >
        <For each={allSessions()}>
          {(session) => (
            <div class="flex items-center gap-3 p-3 bg-surface-layer2 rounded-lg">
              <div
                class={`w-2 h-2 rounded-full ${qualityColors[session.worst_quality ?? 0]}`}
                title={t(qualityLabels[session.worst_quality ?? 0])}
              />
              <div class="flex-1 min-w-0">
                <div class="flex items-center gap-2">
                  <span class="font-medium truncate">
                    {session.channel_name}
                  </span>
                  <Show when={session.guild_name}>
                    <span class="text-text-secondary text-xs">
                      {t("settings.sessionList.inGuild", { server: session.guild_name ?? "" })}
                    </span>
                  </Show>
                </div>
                <div class="text-xs text-text-secondary">
                  {t("settings.sessionList.when", {
                    date: formatDate(session.started_at),
                    start: formatTime(session.started_at),
                    end: formatTime(session.ended_at),
                    duration: formatDuration(session.started_at, session.ended_at),
                  })}
                </div>
              </div>
              <div class="text-right text-xs text-text-secondary">
                <div>{t("common.unit.ms", { value: session.avg_latency ?? "-" })}</div>
                <div>{t("settings.sessionList.loss", { percent: session.avg_loss?.toFixed(1) ?? "-" })}</div>
              </div>
            </div>
          )}
        </For>

        <Show when={pageSize() === 10}>
          <button
            class="w-full py-2 text-sm text-text-secondary hover:text-text-primary"
            disabled={page.loading}
            onClick={() => setOffset((o) => o + 10)}
          >
            {page.loading ? t("common.loading") : t("settings.sessionList.loadMore")}
          </button>
        </Show>
      </Show>
    </div>
  );
};

export default SessionList;
