/**
 * Connection Chart Component
 *
 * Simple bar chart displaying daily connection quality stats.
 * Bar height represents session count, color represents quality.
 */

import { Component, For } from "solid-js";

interface DailyStat {
  date: string;
  avg_latency: number | null;
  avg_loss: number | null;
  session_count: number;
}

interface ConnectionChartProps {
  data: DailyStat[];
}

/**
 * Calculate quality level (0-4) from latency and loss metrics.
 * 0 = no data (gray), 1 = poor (red), 2 = fair (orange), 3 = good (yellow), 4 = excellent (green)
 */
function getQualityFromStats(
  latency: number | null,
  loss: number | null,
): number {
  if (latency === null || loss === null) return 0;
  if (latency > 350 || loss > 5) return 1;
  if (latency > 200 || loss > 3) return 2;
  if (latency > 100 || loss > 1) return 3;
  return 4;
}

const qualityColors = [
  "bg-gray-600", // 0: no data
  "bg-red-500", // 1: poor
  "bg-orange-500", // 2: fair
  "bg-yellow-500", // 3: good
  "bg-green-500", // 4: excellent
];

export const ConnectionChart: Component<ConnectionChartProps> = (props) => {
  const maxSessions = () =>
    Math.max(...props.data.map((d) => d.session_count), 1);

  return (
    <div class="h-32 flex items-end gap-1">
      <For each={props.data}>
        {(day) => {
          const quality = getQualityFromStats(day.avg_latency, day.avg_loss);
          const height = (day.session_count / maxSessions()) * 100;

          return (
            <div class="flex-1 flex flex-col items-center gap-1">
              <div
                class={`w-full rounded-t ${qualityColors[quality]}`}
                style={{ height: `${Math.max(height, 4)}%` }}
                title={`${day.date}: ${day.session_count} sessions, ${day.avg_latency ?? "-"}ms latency`}
              />
              <span class="text-[10px] text-text-secondary">
                {new Date(day.date).getDate()}
              </span>
            </div>
          );
        }}
      </For>
    </div>
  );
};

export default ConnectionChart;
