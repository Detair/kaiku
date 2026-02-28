/**
 * CommandCenterPanel - Admin observability dashboard
 *
 * Displays server health, trend charts (uPlot), top routes/errors,
 * paginated log/trace viewers, and external tool links.
 */

import {
  Component,
  Show,
  For,
  createSignal,
  createEffect,
  onMount,
  onCleanup,
} from "solid-js";
import {
  RefreshCw,
  Clock,
  Activity,
  AlertTriangle,
  Wifi,
  Mic,
  Server,
  ExternalLink,
  ChevronDown,
  Search,
  Heart,
} from "lucide-solid";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";
import {
  adminState,
  loadObsSummary,
  loadObsTrends,
  loadObsTopRoutes,
  loadObsTopErrors,
  loadObsLogs,
  loadObsTraces,
  loadObsLinks,
  setObsTimeRange,
} from "@/stores/admin";
import type { ObsTimeRange } from "@/lib/types";
import TableRowSkeleton from "./TableRowSkeleton";

// ============================================================================
// Helpers
// ============================================================================

function isSafeUrl(url: string | null | undefined): boolean {
  if (!url) return false;
  try {
    const parsed = new URL(url);
    return parsed.protocol === "https:" || parsed.protocol === "http:";
  } catch {
    return false;
  }
}

// ============================================================================
// Time formatting utilities
// ============================================================================

function formatUptime(seconds: number): string {
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const mins = Math.floor((seconds % 3600) / 60);
  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${mins}m`;
  return `${mins}m`;
}

function formatTimestamp(iso: string): string {
  const d = new Date(iso);
  return d.toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function formatDuration(ms: number): string {
  if (ms >= 1000) return `${(ms / 1000).toFixed(1)}s`;
  return `${ms}ms`;
}

function timeSince(timestamp: number | null): string {
  if (!timestamp) return "never";
  const diff = Math.floor((Date.now() - timestamp) / 1000);
  if (diff < 5) return "just now";
  if (diff < 60) return `${diff}s ago`;
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  return `${Math.floor(diff / 3600)}h ago`;
}

// ============================================================================
// Time range pills
// ============================================================================

const TIME_RANGES: { value: ObsTimeRange; label: string }[] = [
  { value: "1h", label: "1h" },
  { value: "6h", label: "6h" },
  { value: "24h", label: "24h" },
  { value: "7d", label: "7d" },
  { value: "30d", label: "30d" },
];

const TimeRangePills: Component<{
  selected: ObsTimeRange;
  onChange: (range: ObsTimeRange) => void;
}> = (props) => (
  <div class="flex gap-1">
    <For each={TIME_RANGES}>
      {(range) => (
        <button
          onClick={() => props.onChange(range.value)}
          class="px-2.5 py-1 text-xs font-medium rounded-md transition-colors"
          classList={{
            "bg-accent-primary/20 text-accent-primary":
              props.selected === range.value,
            "text-text-secondary hover:text-text-primary hover:bg-white/5":
              props.selected !== range.value,
          }}
        >
          {range.label}
        </button>
      )}
    </For>
  </div>
);

// ============================================================================
// Vital sign card
// ============================================================================

const VitalCard: Component<{
  label: string;
  value: string;
  icon: typeof Activity;
  color: string;
  loading: boolean;
}> = (props) => (
  <div class="p-4 rounded-xl bg-white/5 border border-white/10">
    <div class="flex items-center gap-3 mb-2">
      <div
        class="w-10 h-10 rounded-lg flex items-center justify-center"
        style={{ background: `${props.color}20` }}
      >
        <props.icon class="w-5 h-5" style={{ color: props.color }} />
      </div>
      <div class="text-sm text-text-secondary">{props.label}</div>
    </div>
    <div class="text-2xl font-bold text-text-primary">
      <Show when={!props.loading} fallback="...">
        {props.value}
      </Show>
    </div>
  </div>
);

// ============================================================================
// Trend chart component
// ============================================================================

const TrendChart: Component<{
  title: string;
  metricName: string;
  dataKey: "value_p95" | "value_count" | "value_sum";
  unit: string;
  color: string;
}> = (props) => {
  let container: HTMLDivElement | undefined;
  let chart: uPlot | null = null;

  const getData = (): uPlot.AlignedData => {
    const trends = adminState.obsTrends;
    if (!trends) return [[], []];

    const metric = trends.metrics.find(
      (m) => m.metric_name === props.metricName,
    );
    if (!metric || metric.datapoints.length === 0) return [[], []];

    const timestamps = metric.datapoints.map(
      (dp) => new Date(dp.ts).getTime() / 1000,
    );
    const values = metric.datapoints.map((dp) => {
      const val = dp[props.dataKey];
      return val ?? null;
    });

    return [timestamps, values as (number | null)[]];
  };

  const createChart = () => {
    if (!container) return;

    const width = container.clientWidth;
    const opts: uPlot.Options = {
      width,
      height: 160,
      cursor: { show: true },
      select: { show: false, left: 0, top: 0, width: 0, height: 0 },
      legend: { show: false },
      axes: [
        {
          stroke: "rgba(255,255,255,0.3)",
          grid: { stroke: "rgba(255,255,255,0.05)" },
          ticks: { stroke: "rgba(255,255,255,0.1)" },
          font: "11px sans-serif",
        },
        {
          stroke: "rgba(255,255,255,0.3)",
          grid: { stroke: "rgba(255,255,255,0.05)" },
          ticks: { stroke: "rgba(255,255,255,0.1)" },
          font: "11px sans-serif",
          size: 50,
        },
      ],
      series: [
        {},
        {
          stroke: props.color,
          width: 2,
          fill: `${props.color}15`,
        },
      ],
    };

    chart = new uPlot(opts, getData(), container);
  };

  let ro: ResizeObserver | null = null;

  // Create or update chart when data changes and container is available
  createEffect(() => {
    void adminState.obsTrends;
    if (!container) return;
    if (chart) {
      chart.setData(getData());
    } else {
      createChart();
      ro = new ResizeObserver(() => {
        if (chart && container) {
          chart.setSize({ width: container.clientWidth, height: 160 });
        }
      });
      ro.observe(container);
    }
  });

  onCleanup(() => {
    ro?.disconnect();
    chart?.destroy();
  });

  return (
    <div class="rounded-xl bg-white/5 border border-white/10 p-4">
      <div class="flex items-center justify-between mb-3">
        <h4 class="text-sm font-medium text-text-primary">{props.title}</h4>
        <span class="text-xs text-text-secondary">{props.unit}</span>
      </div>
      <div
        ref={container}
        classList={{ hidden: adminState.isObsTrendsLoading }}
      />
      <Show when={adminState.isObsTrendsLoading}>
        <div class="h-40 flex items-center justify-center text-text-secondary text-sm">
          Loading...
        </div>
      </Show>
    </div>
  );
};

// ============================================================================
// Level badge
// ============================================================================

const LevelBadge: Component<{ level: string }> = (props) => {
  const colors = () => {
    switch (props.level) {
      case "ERROR":
        return "bg-red-500/20 text-red-400";
      case "WARN":
        return "bg-yellow-500/20 text-yellow-400";
      case "INFO":
        return "bg-blue-500/20 text-blue-400";
      case "DEBUG":
        return "bg-gray-500/20 text-gray-400";
      default:
        return "bg-gray-500/20 text-gray-400";
    }
  };

  return (
    <span class={`px-1.5 py-0.5 rounded text-xs font-medium ${colors()}`}>
      {props.level}
    </span>
  );
};

// ============================================================================
// Status badge for traces
// ============================================================================

const StatusBadge: Component<{ statusCode: string | null; isError: boolean }> =
  (props) => {
    const color = () => {
      if (props.isError) return "bg-red-500/20 text-red-400";
      const code = parseInt(props.statusCode ?? "", 10);
      if (isNaN(code)) return "bg-gray-500/20 text-gray-400";
      if (code >= 500) return "bg-red-500/20 text-red-400";
      if (code >= 400) return "bg-yellow-500/20 text-yellow-400";
      return "bg-green-500/20 text-green-400";
    };

    return (
      <span class={`px-1.5 py-0.5 rounded text-xs font-medium ${color()}`}>
        {props.statusCode ?? "OK"}
      </span>
    );
  };

// ============================================================================
// Duration badge with color coding
// ============================================================================

const DurationBadge: Component<{ ms: number }> = (props) => {
  const color = () => {
    if (props.ms >= 5000) return "text-red-400";
    if (props.ms >= 1000) return "text-yellow-400";
    return "text-green-400";
  };

  return (
    <span class={`text-xs font-mono ${color()}`}>
      {formatDuration(props.ms)}
    </span>
  );
};

// ============================================================================
// Main Component
// ============================================================================

const CommandCenterPanel: Component = () => {
  const [routeSort, setRouteSort] = createSignal<"latency" | "errors">(
    "latency",
  );
  const [logsLevel, setLogsLevel] = createSignal<string>("ERROR");
  const [logsDomain, setLogsDomain] = createSignal<string>("");
  const [logsSearch, setLogsSearch] = createSignal<string>("");
  const [tracesStatus, setTracesStatus] = createSignal<string>("error");
  const [tracesDomain, setTracesDomain] = createSignal<string>("");
  let searchTimeout: ReturnType<typeof setTimeout> | null = null;

  // Initial data load + 30-second polling for summary
  onMount(() => {
    loadObsSummary();
    loadObsTrends();
    loadObsTopRoutes();
    loadObsTopErrors();
    loadObsLogs(true, "ERROR");
    loadObsTraces(true, "error");
    loadObsLinks();

    const pollInterval = setInterval(() => {
      loadObsSummary();
    }, 30000);

    onCleanup(() => {
      clearInterval(pollInterval);
      if (searchTimeout) clearTimeout(searchTimeout);
    });
  });

  // Stale data detection (> 2 min)
  const isStale = () => {
    const last = adminState.obsLastRefresh;
    if (!last) return false;
    return Date.now() - last > 120000;
  };

  // Time range change handler
  const handleTimeRangeChange = (range: ObsTimeRange) => {
    setObsTimeRange(range);
    loadObsTrends(range);
    loadObsTopRoutes(range);
    loadObsTopErrors(range);
  };

  // Manual refresh
  const handleRefresh = () => {
    loadObsSummary();
    loadObsTrends();
    loadObsTopRoutes(undefined, routeSort());
    loadObsTopErrors();
    loadObsLogs(true, logsLevel() || undefined, logsDomain() || undefined, logsSearch() || undefined);
    loadObsTraces(true, tracesStatus() || undefined, tracesDomain() || undefined);
  };

  // Route sort change
  const handleRouteSortChange = (sort: "latency" | "errors") => {
    setRouteSort(sort);
    loadObsTopRoutes(undefined, sort);
  };

  // Logs filter handlers
  const handleLogsLevelChange = (level: string) => {
    setLogsLevel(level);
    loadObsLogs(true, level || undefined, logsDomain() || undefined, logsSearch() || undefined);
  };

  const handleLogsDomainChange = (domain: string) => {
    setLogsDomain(domain);
    loadObsLogs(true, logsLevel() || undefined, domain || undefined, logsSearch() || undefined);
  };

  const handleLogsSearchChange = (value: string) => {
    setLogsSearch(value);
    if (searchTimeout) clearTimeout(searchTimeout);
    searchTimeout = setTimeout(() => {
      loadObsLogs(true, logsLevel() || undefined, logsDomain() || undefined, value || undefined);
    }, 300);
  };

  const handleLoadMoreLogs = () => {
    loadObsLogs(false, logsLevel() || undefined, logsDomain() || undefined, logsSearch() || undefined);
  };

  // Traces filter handlers
  const handleTracesStatusChange = (status: string) => {
    setTracesStatus(status);
    loadObsTraces(true, status || undefined, tracesDomain() || undefined);
  };

  const handleTracesDomainChange = (domain: string) => {
    setTracesDomain(domain);
    loadObsTraces(true, tracesStatus() || undefined, domain || undefined);
  };

  const handleLoadMoreTraces = () => {
    loadObsTraces(false, tracesStatus() || undefined, tracesDomain() || undefined);
  };

  const summary = () => adminState.obsSummary;
  const vitals = () => summary()?.vital_signs;
  const meta = () => summary()?.server_metadata;

  return (
    <div class="flex-1 p-6 overflow-auto">
      <div class="max-w-6xl mx-auto space-y-6">
        {/* Header */}
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-3">
            <Activity class="w-5 h-5 text-accent-primary" />
            <h2 class="text-lg font-bold text-text-primary">Command Center</h2>
          </div>
          <div class="flex items-center gap-3">
            <Show when={isStale()}>
              <div class="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-status-warning/15 text-status-warning text-xs font-medium">
                <AlertTriangle class="w-3 h-3" />
                Stale data
              </div>
            </Show>
            <span class="text-xs text-text-secondary">
              Updated {timeSince(adminState.obsLastRefresh)}
            </span>
            <button
              onClick={handleRefresh}
              class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm text-text-secondary hover:text-text-primary hover:bg-white/5 transition-colors"
              disabled={adminState.isObsSummaryLoading}
            >
              <RefreshCw
                class="w-4 h-4"
                classList={{ "animate-spin": adminState.isObsSummaryLoading }}
              />
            </button>
          </div>
        </div>

        {/* Vital Signs */}
        <section>
          <div class="grid grid-cols-4 gap-4">
            <VitalCard
              label="Latency P95"
              value={
                vitals()?.latency_p95_ms != null
                  ? `${vitals()!.latency_p95_ms!.toFixed(0)}ms`
                  : "N/A"
              }
              icon={Clock}
              color="#f59e0b"
              loading={adminState.isObsSummaryLoading && !summary()}
            />
            <VitalCard
              label="Error Rate"
              value={
                vitals()?.error_rate_percent != null
                  ? `${vitals()!.error_rate_percent!.toFixed(2)}%`
                  : "N/A"
              }
              icon={AlertTriangle}
              color="#ef4444"
              loading={adminState.isObsSummaryLoading && !summary()}
            />
            <VitalCard
              label="WS Connections"
              value={
                vitals()?.active_ws_connections != null
                  ? String(vitals()!.active_ws_connections)
                  : "N/A"
              }
              icon={Wifi}
              color="#3b82f6"
              loading={adminState.isObsSummaryLoading && !summary()}
            />
            <VitalCard
              label="Voice Sessions"
              value={
                vitals()?.active_voice_sessions != null
                  ? String(vitals()!.active_voice_sessions)
                  : "N/A"
              }
              icon={Mic}
              color="#8b5cf6"
              loading={adminState.isObsSummaryLoading && !summary()}
            />
          </div>
        </section>

        {/* Server Metadata + Voice Health */}
        <Show when={summary()}>
          <section class="flex items-center gap-4 px-4 py-3 rounded-xl bg-white/5 border border-white/10 text-sm">
            <div class="flex items-center gap-2 text-text-secondary">
              <Server class="w-4 h-4" />
              <span class="text-text-primary font-medium">
                v{meta()?.version}
              </span>
            </div>
            <div class="w-px h-4 bg-white/10" />
            <span class="text-text-secondary">
              Up {formatUptime(meta()?.uptime_seconds ?? 0)}
            </span>
            <div class="w-px h-4 bg-white/10" />
            <span class="text-text-secondary">{meta()?.environment}</span>
            <div class="w-px h-4 bg-white/10" />
            <span class="text-text-secondary">
              {meta()?.active_user_count} users
            </span>
            <div class="w-px h-4 bg-white/10" />
            <span class="text-text-secondary">
              {meta()?.guild_count} guilds
            </span>
            <Show when={summary()!.voice_health_score != null}>
              <div class="w-px h-4 bg-white/10" />
              <div class="flex items-center gap-1.5">
                <Heart class="w-3.5 h-3.5 text-green-400" />
                <span class="text-text-primary font-medium">
                  {(summary()!.voice_health_score! * 100).toFixed(0)}%
                </span>
              </div>
            </Show>
            <Show when={summary()!.active_alert_count > 0}>
              <div class="w-px h-4 bg-white/10" />
              <div class="flex items-center gap-1.5 px-2 py-0.5 rounded-full bg-red-500/20 text-red-400 text-xs font-medium">
                <AlertTriangle class="w-3 h-3" />
                {summary()!.active_alert_count} alerts
              </div>
            </Show>
          </section>
        </Show>

        {/* Trends Section */}
        <section>
          <div class="flex items-center justify-between mb-4">
            <h3 class="text-base font-semibold text-text-primary">Trends</h3>
            <TimeRangePills
              selected={adminState.obsTimeRange}
              onChange={handleTimeRangeChange}
            />
          </div>
          <div class="grid grid-cols-2 gap-4">
            <TrendChart
              title="Latency P95"
              metricName="kaiku_http_request_duration_ms"
              dataKey="value_p95"
              unit="ms"
              color="#f59e0b"
            />
            <TrendChart
              title="Error Rate"
              metricName="kaiku_http_errors_total"
              dataKey="value_count"
              unit="count"
              color="#ef4444"
            />
            <TrendChart
              title="WebSocket Connections"
              metricName="kaiku_ws_connections_active"
              dataKey="value_count"
              unit="connections"
              color="#3b82f6"
            />
            <TrendChart
              title="Voice Sessions"
              metricName="kaiku_voice_sessions_active"
              dataKey="value_count"
              unit="sessions"
              color="#8b5cf6"
            />
          </div>
        </section>

        {/* Top Routes + Top Errors */}
        <div class="grid grid-cols-2 gap-4">
          {/* Top Routes */}
          <section class="rounded-xl bg-white/5 border border-white/10 overflow-hidden">
            <div class="flex items-center justify-between p-4 border-b border-white/10">
              <h3 class="text-sm font-semibold text-text-primary">
                Top Routes
              </h3>
              <div class="flex gap-1">
                <button
                  onClick={() => handleRouteSortChange("latency")}
                  class="px-2 py-0.5 text-xs rounded transition-colors"
                  classList={{
                    "bg-accent-primary/20 text-accent-primary":
                      routeSort() === "latency",
                    "text-text-secondary hover:text-text-primary":
                      routeSort() !== "latency",
                  }}
                >
                  Latency
                </button>
                <button
                  onClick={() => handleRouteSortChange("errors")}
                  class="px-2 py-0.5 text-xs rounded transition-colors"
                  classList={{
                    "bg-accent-primary/20 text-accent-primary":
                      routeSort() === "errors",
                    "text-text-secondary hover:text-text-primary":
                      routeSort() !== "errors",
                  }}
                >
                  Errors
                </button>
              </div>
            </div>
            <Show
              when={!adminState.isObsTopRoutesLoading}
              fallback={<TableRowSkeleton columns={5} rows={5} />}
            >
              <Show
                when={
                  adminState.obsTopRoutes &&
                  adminState.obsTopRoutes.routes.length > 0
                }
                fallback={
                  <div class="p-8 text-center text-text-secondary text-sm">
                    No route data available
                  </div>
                }
              >
                {/* Header */}
                <div class="grid grid-cols-[1fr_80px_80px_60px_70px] gap-2 px-4 py-2 text-xs font-medium text-text-secondary border-b border-white/5">
                  <span>Route</span>
                  <span class="text-right">Requests</span>
                  <span class="text-right">P95</span>
                  <span class="text-right">Errors</span>
                  <span class="text-right">Rate</span>
                </div>
                <For each={adminState.obsTopRoutes!.routes}>
                  {(route) => (
                    <div class="grid grid-cols-[1fr_80px_80px_60px_70px] gap-2 px-4 py-2 text-xs border-b border-white/5 hover:bg-white/3">
                      <span class="text-text-primary font-mono truncate">
                        {route.route ?? "unknown"}
                      </span>
                      <span class="text-right text-text-secondary">
                        {route.request_count}
                      </span>
                      <span class="text-right text-text-secondary">
                        {route.latency_p95_ms != null
                          ? `${route.latency_p95_ms.toFixed(0)}ms`
                          : "-"}
                      </span>
                      <span class="text-right text-text-secondary">
                        {route.error_count}
                      </span>
                      <span
                        class="text-right"
                        classList={{
                          "text-red-400": route.error_rate_percent > 5,
                          "text-yellow-400":
                            route.error_rate_percent > 1 &&
                            route.error_rate_percent <= 5,
                          "text-text-secondary":
                            route.error_rate_percent <= 1,
                        }}
                      >
                        {route.error_rate_percent.toFixed(1)}%
                      </span>
                    </div>
                  )}
                </For>
              </Show>
            </Show>
          </section>

          {/* Top Errors */}
          <section class="rounded-xl bg-white/5 border border-white/10 overflow-hidden">
            <div class="p-4 border-b border-white/10">
              <h3 class="text-sm font-semibold text-text-primary">
                Top Errors
              </h3>
            </div>
            <Show
              when={!adminState.isObsTopErrorsLoading}
              fallback={<TableRowSkeleton columns={3} rows={5} />}
            >
              <Show
                when={
                  adminState.obsTopErrors &&
                  adminState.obsTopErrors.error_categories.length > 0
                }
                fallback={
                  <div class="p-8 text-center text-text-secondary text-sm">
                    No error data available
                  </div>
                }
              >
                {/* Header */}
                <div class="grid grid-cols-[1fr_80px_80px] gap-2 px-4 py-2 text-xs font-medium text-text-secondary border-b border-white/5">
                  <span>Error Type</span>
                  <span class="text-right">Count</span>
                  <span class="text-right">Avg P95</span>
                </div>
                <For each={adminState.obsTopErrors!.error_categories}>
                  {(err) => (
                    <div class="grid grid-cols-[1fr_80px_80px] gap-2 px-4 py-2 text-xs border-b border-white/5 hover:bg-white/3">
                      <span class="text-text-primary font-mono truncate">
                        {err.error_type ?? "unknown"}
                      </span>
                      <span class="text-right text-red-400">{err.count}</span>
                      <span class="text-right text-text-secondary">
                        {err.avg_p95_ms != null
                          ? `${err.avg_p95_ms.toFixed(0)}ms`
                          : "-"}
                      </span>
                    </div>
                  )}
                </For>
              </Show>
            </Show>
          </section>
        </div>

        {/* Logs Section */}
        <section class="rounded-xl bg-white/5 border border-white/10 overflow-hidden">
          <div class="flex items-center justify-between p-4 border-b border-white/10">
            <h3 class="text-sm font-semibold text-text-primary">Logs</h3>
            <div class="flex items-center gap-2">
              <select
                value={logsLevel()}
                onChange={(e) => handleLogsLevelChange(e.currentTarget.value)}
                class="px-2 py-1 text-xs rounded-md bg-white/5 border border-white/10 text-text-primary"
              >
                <option value="">All Levels</option>
                <option value="ERROR">ERROR</option>
                <option value="WARN">WARN</option>
                <option value="INFO">INFO</option>
                <option value="DEBUG">DEBUG</option>
              </select>
              <input
                type="text"
                placeholder="Domain..."
                value={logsDomain()}
                onInput={(e) => handleLogsDomainChange(e.currentTarget.value)}
                class="w-24 px-2 py-1 text-xs rounded-md bg-white/5 border border-white/10 text-text-primary placeholder:text-text-secondary/50"
              />
              <div class="relative">
                <Search class="absolute left-2 top-1/2 -translate-y-1/2 w-3 h-3 text-text-secondary" />
                <input
                  type="text"
                  placeholder="Search..."
                  value={logsSearch()}
                  onInput={(e) =>
                    handleLogsSearchChange(e.currentTarget.value)
                  }
                  class="w-40 pl-6 pr-2 py-1 text-xs rounded-md bg-white/5 border border-white/10 text-text-primary placeholder:text-text-secondary/50"
                />
              </div>
            </div>
          </div>
          <Show
            when={!adminState.isObsLogsLoading || adminState.obsLogs.length > 0}
            fallback={<TableRowSkeleton columns={5} rows={8} />}
          >
            <Show
              when={adminState.obsLogs.length > 0}
              fallback={
                <div class="p-8 text-center text-text-secondary text-sm">
                  No log events found
                </div>
              }
            >
              {/* Header */}
              <div class="grid grid-cols-[100px_60px_80px_120px_1fr] gap-2 px-4 py-2 text-xs font-medium text-text-secondary border-b border-white/5">
                <span>Time</span>
                <span>Level</span>
                <span>Domain</span>
                <span>Event</span>
                <span>Message</span>
              </div>
              <div class="max-h-80 overflow-auto">
                <For each={adminState.obsLogs}>
                  {(log) => (
                    <div class="grid grid-cols-[100px_60px_80px_120px_1fr] gap-2 px-4 py-1.5 text-xs border-b border-white/5 hover:bg-white/3">
                      <span class="text-text-secondary font-mono">
                        {formatTimestamp(log.ts)}
                      </span>
                      <LevelBadge level={log.level} />
                      <span class="text-text-secondary truncate">
                        {log.domain}
                      </span>
                      <span class="text-text-primary truncate">
                        {log.event}
                      </span>
                      <span class="text-text-secondary truncate">
                        {log.message}
                      </span>
                    </div>
                  )}
                </For>
              </div>
              <Show when={adminState.obsLogsHasMore}>
                <div class="p-3 border-t border-white/5">
                  <button
                    onClick={handleLoadMoreLogs}
                    disabled={adminState.isObsLogsLoading}
                    class="flex items-center gap-1.5 mx-auto px-4 py-1.5 text-xs font-medium text-text-secondary hover:text-text-primary hover:bg-white/5 rounded-lg transition-colors"
                  >
                    <ChevronDown class="w-3 h-3" />
                    {adminState.isObsLogsLoading
                      ? "Loading..."
                      : "Load more"}
                  </button>
                </div>
              </Show>
            </Show>
          </Show>
        </section>

        {/* Traces Section */}
        <section class="rounded-xl bg-white/5 border border-white/10 overflow-hidden">
          <div class="flex items-center justify-between p-4 border-b border-white/10">
            <h3 class="text-sm font-semibold text-text-primary">Traces</h3>
            <div class="flex items-center gap-2">
              <select
                value={tracesStatus()}
                onChange={(e) =>
                  handleTracesStatusChange(e.currentTarget.value)
                }
                class="px-2 py-1 text-xs rounded-md bg-white/5 border border-white/10 text-text-primary"
              >
                <option value="">All</option>
                <option value="error">Errors</option>
                <option value="slow">Slow (&gt;1s)</option>
              </select>
              <input
                type="text"
                placeholder="Domain..."
                value={tracesDomain()}
                onInput={(e) =>
                  handleTracesDomainChange(e.currentTarget.value)
                }
                class="w-24 px-2 py-1 text-xs rounded-md bg-white/5 border border-white/10 text-text-primary placeholder:text-text-secondary/50"
              />
            </div>
          </div>
          <Show
            when={
              !adminState.isObsTracesLoading ||
              adminState.obsTraces.length > 0
            }
            fallback={<TableRowSkeleton columns={5} rows={8} />}
          >
            <Show
              when={adminState.obsTraces.length > 0}
              fallback={
                <div class="p-8 text-center text-text-secondary text-sm">
                  No trace data found
                </div>
              }
            >
              {/* Header */}
              <div class="grid grid-cols-[100px_60px_1fr_80px_180px] gap-2 px-4 py-2 text-xs font-medium text-text-secondary border-b border-white/5">
                <span>Time</span>
                <span>Status</span>
                <span>Route</span>
                <span class="text-right">Duration</span>
                <span>Trace ID</span>
              </div>
              <div class="max-h-80 overflow-auto">
                <For each={adminState.obsTraces}>
                  {(trace) => (
                    <div class="grid grid-cols-[100px_60px_1fr_80px_180px] gap-2 px-4 py-1.5 text-xs border-b border-white/5 hover:bg-white/3">
                      <span class="text-text-secondary font-mono">
                        {formatTimestamp(trace.ts)}
                      </span>
                      <StatusBadge
                        statusCode={trace.status_code}
                        isError={
                          trace.status_code != null &&
                          parseInt(trace.status_code, 10) >= 500
                        }
                      />
                      <span class="text-text-primary font-mono truncate">
                        {trace.route ?? trace.span_name}
                      </span>
                      <div class="text-right">
                        <DurationBadge ms={trace.duration_ms} />
                      </div>
                      <Show
                        when={isSafeUrl(adminState.obsLinks?.tempo_url)}
                        fallback={
                          <span class="text-text-secondary font-mono truncate">
                            {trace.trace_id}
                          </span>
                        }
                      >
                        <a
                          href={`${adminState.obsLinks!.tempo_url}/trace/${encodeURIComponent(trace.trace_id)}`}
                          target="_blank"
                          rel="noopener noreferrer"
                          class="text-accent-primary font-mono truncate hover:underline"
                        >
                          {trace.trace_id}
                        </a>
                      </Show>
                    </div>
                  )}
                </For>
              </div>
              <Show when={adminState.obsTracesHasMore}>
                <div class="p-3 border-t border-white/5">
                  <button
                    onClick={handleLoadMoreTraces}
                    disabled={adminState.isObsTracesLoading}
                    class="flex items-center gap-1.5 mx-auto px-4 py-1.5 text-xs font-medium text-text-secondary hover:text-text-primary hover:bg-white/5 rounded-lg transition-colors"
                  >
                    <ChevronDown class="w-3 h-3" />
                    {adminState.isObsTracesLoading
                      ? "Loading..."
                      : "Load more"}
                  </button>
                </div>
              </Show>
            </Show>
          </Show>
        </section>

        {/* External Links */}
        <Show
          when={
            adminState.obsLinks &&
            (adminState.obsLinks.grafana_url ||
              adminState.obsLinks.tempo_url ||
              adminState.obsLinks.loki_url ||
              adminState.obsLinks.prometheus_url)
          }
        >
          <section class="rounded-xl bg-white/5 border border-white/10 p-4">
            <h3 class="text-sm font-semibold text-text-primary mb-3">
              External Tools
            </h3>
            <div class="flex flex-wrap gap-3">
              <Show when={isSafeUrl(adminState.obsLinks!.grafana_url)}>
                <a
                  href={adminState.obsLinks!.grafana_url!}
                  target="_blank"
                  rel="noopener noreferrer"
                  class="flex items-center gap-2 px-3 py-2 rounded-lg bg-white/5 text-sm text-text-primary hover:bg-white/10 transition-colors"
                >
                  <ExternalLink class="w-4 h-4 text-orange-400" />
                  Grafana
                </a>
              </Show>
              <Show when={isSafeUrl(adminState.obsLinks!.tempo_url)}>
                <a
                  href={adminState.obsLinks!.tempo_url!}
                  target="_blank"
                  rel="noopener noreferrer"
                  class="flex items-center gap-2 px-3 py-2 rounded-lg bg-white/5 text-sm text-text-primary hover:bg-white/10 transition-colors"
                >
                  <ExternalLink class="w-4 h-4 text-blue-400" />
                  Tempo
                </a>
              </Show>
              <Show when={isSafeUrl(adminState.obsLinks!.loki_url)}>
                <a
                  href={adminState.obsLinks!.loki_url!}
                  target="_blank"
                  rel="noopener noreferrer"
                  class="flex items-center gap-2 px-3 py-2 rounded-lg bg-white/5 text-sm text-text-primary hover:bg-white/10 transition-colors"
                >
                  <ExternalLink class="w-4 h-4 text-green-400" />
                  Loki
                </a>
              </Show>
              <Show when={isSafeUrl(adminState.obsLinks!.prometheus_url)}>
                <a
                  href={adminState.obsLinks!.prometheus_url!}
                  target="_blank"
                  rel="noopener noreferrer"
                  class="flex items-center gap-2 px-3 py-2 rounded-lg bg-white/5 text-sm text-text-primary hover:bg-white/10 transition-colors"
                >
                  <ExternalLink class="w-4 h-4 text-red-400" />
                  Prometheus
                </a>
              </Show>
            </div>
          </section>
        </Show>
      </div>
    </div>
  );
};

export default CommandCenterPanel;
