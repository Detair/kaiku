/**
 * Admin observability / command center types.
 */

// ============================================================================
// Observability Types (Command Center)
// ============================================================================

export interface VitalSigns {
  latency_p95_ms: number | null;
  error_rate_percent: number | null;
  active_ws_connections: number | null;
  active_voice_sessions: number | null;
}

export interface ServerMetadata {
  version: string;
  uptime_seconds: number;
  environment: string;
  active_user_count: number;
  guild_count: number;
}

export interface ObservabilitySummary {
  vital_signs: VitalSigns;
  server_metadata: ServerMetadata;
  voice_health_score: number | null;
  active_alert_count: number;
}

export interface TrendDataPoint {
  ts: string;
  metric_name: string;
  value_count: number | null;
  value_sum: number | null;
  value_p50: number | null;
  value_p95: number | null;
  value_p99: number | null;
}

export interface MetricTrend {
  metric_name: string;
  datapoints: TrendDataPoint[];
}

export interface TrendsResponse {
  metrics: MetricTrend[];
}

export interface RouteEntry {
  route: string | null;
  request_count: number;
  error_count: number;
  error_rate_percent: number;
  latency_p95_ms: number | null;
}

export interface TopRoutesResponse {
  routes: RouteEntry[];
}

export interface ErrorCategoryEntry {
  error_type: string | null;
  count: number;
  avg_p95_ms: number | null;
}

export interface TopErrorsResponse {
  error_categories: ErrorCategoryEntry[];
}

export interface ObsLogEvent {
  id: string;
  ts: string;
  level: string;
  service: string;
  domain: string;
  event: string;
  message: string;
  trace_id: string | null;
  span_id: string | null;
  attrs: Record<string, unknown>;
}

export interface LogsResponse {
  logs: ObsLogEvent[];
  next_cursor: string | null;
}

export interface ObsTraceEntry {
  id: string;
  trace_id: string;
  span_name: string;
  domain: string;
  route: string | null;
  status_code: string | null;
  duration_ms: number;
  ts: string;
  service: string;
}

export interface TracesResponse {
  traces: ObsTraceEntry[];
  next_cursor: string | null;
}

export interface ObsLinksResponse {
  grafana_url: string | null;
  tempo_url: string | null;
  loki_url: string | null;
  prometheus_url: string | null;
}

export type ObsTimeRange = "1h" | "6h" | "24h" | "7d" | "30d";
