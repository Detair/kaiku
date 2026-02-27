# Phase 7 Admin Command Center — Design

**Date:** 2026-02-27
**Status:** Draft (v2)
**Roadmap Scope:** Phase 7 `[Ops] Admin Command Center (Native Observability Lite)`
**Related:**
- `docs/project/roadmap.md`
- `docs/ops/observability-contract.md` — canonical metric/label/redaction spec
- `docs/ops/observability-runbook.md`
- `docs/plans/2026-02-15-phase-7-a11y-observability-design.md`
- `docs/plans/2026-02-15-opentelemetry-grafana-reference-design.md`

---

## 1. Problem

Phase 7 observability foundations are in place (OTel instrumentation, collector pipeline, Grafana stack reference design), but operators currently need external tooling for runtime visibility. This creates three gaps:

1. **Self-hosted operators without Grafana** lack practical day-to-day insight into system health.
2. **Admin workflows are split** between in-app moderation actions and external telemetry tools.
3. **Incident triage is slow** because signals (metrics, logs, traces) are scattered across separate tools with no shared context.

Research into mature self-hosted platforms (GitLab admin area, Mattermost System Console, Zulip analytics, Sentry self-hosted monitoring) shows a clear pattern: every platform converges on a **built-in triage layer** that answers "is something wrong right now?" in under 10 seconds, then routes deep analysis to external tools. Our current design covers the bottom of this hierarchy — this rewrite raises it to industry standard.

---

## 2. Goals

- Provide a cluster-wide command center in the existing admin panel for all system admins.
- Deliver useful native observability with strict 30-day retention.
- Organize signals around the **Four Golden Signals** framework (Latency, Traffic, Errors, Saturation) from the Google SRE book.
- Provide a dedicated **voice operations** view — voice is the product's core differentiator and requires domain-specific monitoring.
- Show **infrastructure health** (database, cache, storage, OTel pipeline) as first-class signals.
- Enable **incident correlation** via shared time axis across metrics, logs, and traces.
- Support **real-time push** via WebSocket for live counters and health transitions.
- Preserve compatibility with OTel/Grafana stack and deep-link into external tools when available.
- Keep runtime and storage overhead bounded for self-hosted installations.

## 3. Non-Goals

- Replacing Grafana, Tempo, Loki, or Prometheus.
- Building an in-app trace waterfall viewer or full log query language.
- Providing retention beyond 30 days in native storage.
- Multi-tenant observability segmentation in v1.
- Per-node access restrictions in v1 (view is cluster-wide for all system admins).
- Alert-rule authoring UI in v1 (static thresholds only, configurable via server config).
- Full anomaly detection with ML-based baselines in v1 (rolling 7-day deviation planned for v1.1).
- Native `INFO`-level log ingestion in v1 (volume would exceed storage budget; INFO stays in external Loki).

---

## 4. Design Decisions (Resolved)

| Decision | Resolution | Rationale |
|----------|-----------|-----------|
| Access model | Cluster-wide, all system admins | Simplest v1; per-role segmentation is v2 |
| Retention | 30 days hard cap, all native tables | User constraint; external stack for longer |
| INFO logs in native store | No — WARN/ERROR only in v1 | INFO volume would blow storage; operators need errors first |
| Trend aggregation strategy | Hybrid — materialized views (hourly refresh) for 30d trends, live queries for real-time cards | Zulip proves pre-aggregation works in PostgreSQL at this scale |
| Data export | Yes in v1 — CSV/JSON for current filtered view | Operators need this for incident reports and audits |
| Escalation path | External stack for deep forensics | Explicit boundary; native panel is triage, not forensics |
| UX boundary | Fast triage + operational awareness | Not full observability platform |
| Real-time delivery | WebSocket push for health transitions + live counters; polling (10-30s) for trends | Platform already has WS infrastructure; leverage it |

---

## 5. Design Framework: Four Golden Signals

The command center is organized around the [Four Golden Signals](https://sre.google/sre-book/monitoring-distributed-systems/) from Google's SRE handbook. This provides operators with an instant mental model for system health:

| Signal | What It Measures | Kaiku Domain Mapping |
|--------|-----------------|---------------------|
| **Latency** | Time to service requests | API handler duration, DB query time, WebRTC connect time |
| **Traffic** | Demand on the system | API req/sec, active WS connections, concurrent voice sessions |
| **Errors** | Rate of failures | HTTP 5xx, auth failures, WS disconnects, voice join failures |
| **Saturation** | How "full" resources are | DB pool %, OTel queue overflow, process memory |

Every metric displayed in the command center maps to one of these four signals. This prevents the common "dashboard of random metrics" anti-pattern.

---

## 6. Signal Taxonomy

Signals are organized in tiers by operator priority. Tier 1 is always visible; lower tiers are tabbed views.

### Tier 1: Health Overview (Triage Entry Point)

**Purpose:** Answer "is something wrong right now?" in under 5 seconds.
**Update model:** WebSocket push for state transitions; poll every 10s for freshness.

**Always visible at the top of the command center.**

#### 6.1.1 Service Health Matrix

A named list of services with current state, inspired by GitLab's admin area and Zulip's supervisorctl model:

| Service | Health Probe | Status Values |
|---------|-------------|---------------|
| HTTP API | `GET /health` response + `kaiku_http_requests_total` rate > 0 | `healthy` / `degraded` / `down` |
| WebSocket Hub | `kaiku_ws_connections_active` gauge queryable | `healthy` / `degraded` / `down` |
| Voice SFU | SFU `room_count()` callable, RTP forwarding active | `healthy` / `degraded` / `down` |
| PostgreSQL | Pool `SELECT 1` success + `kaiku_db_pool_connections_idle` > 0 | `healthy` / `degraded` / `down` |
| Valkey (Redis) | `PING` success | `healthy` / `down` |
| S3 Storage | `health_check()` bucket access | `healthy` / `unavailable` (graceful) |
| OTel Pipeline | `kaiku_otel_export_failures_total` rate < threshold | `healthy` / `degraded` / `down` |

**Degraded** means: service responds but with elevated error rate or latency above threshold.

#### 6.1.2 Key Vital Signs (Top Cards)

Four summary cards — one per golden signal — with sparkline trend (1h):

| Card | Source Metric | Display |
|------|--------------|---------|
| **Latency** | `kaiku_http_request_duration_seconds` p95 (5m window) | Value + sparkline + threshold color |
| **Traffic** | `kaiku_http_requests_total` rate (1m window) | Value + sparkline |
| **Errors** | `kaiku_http_errors_total` rate (5m window) | Value + sparkline + threshold color |
| **Saturation** | Max of (DB pool %, OTel queue %) | Highest-pressure resource + sparkline |

#### 6.1.3 Live Error Feed

A scrolling unified error stream (last 25 errors, cross-service), inspired by Zulip's `errors.log` triage pattern:

- Source: `telemetry_log_events` where level = `ERROR`
- Display: timestamp, service, domain, event name, message (truncated), trace_id link
- Auto-scrolls on new entries via WebSocket push
- Click to expand full error context
- "View all logs" link to Tier 4

#### 6.1.4 Server Metadata

- Server version (`CARGO_PKG_VERSION`)
- Uptime (process start time)
- Environment (`deployment.environment`)
- Last deploy timestamp (from config or git tag, if available)
- Active user count, guild count (from existing `AdminStatsResponse`)

---

### Tier 2: Golden Signals Dashboard (30-Day Trends)

**Purpose:** Show trends over time. Detect gradual degradation. Provide evidence for capacity planning.
**Update model:** Poll every 30s for live view; pre-aggregated materialized views for historical ranges.
**Time range picker:** 1h / 6h / 24h / 7d / 30d (default: 24h)

#### 6.2.1 Latency Panel

All latency metrics sourced from histograms with percentile computation. Per SRE guidance: **never show averages for latency — always p50/p95/p99.**

| Metric | Source (Observability Contract) | Visualization |
|--------|-------------------------------|---------------|
| API request latency p50/p95/p99 | `kaiku_http_request_duration_seconds` histogram | Time-series line chart, percentile overlays |
| DB query latency p50/p95 | `kaiku_db_query_duration_seconds` histogram | Time-series line chart |
| Voice session duration distribution | `kaiku_voice_session_duration_seconds` histogram | Time-series line chart |
| Client WebRTC connect time p95 | `kaiku_client_voice_webrtc_connect_duration_seconds` histogram | Time-series line chart (v1.1, requires client metric ingestion) |

**Top slow routes table:** Top 10 routes by p95 latency in selected range, from `telemetry_metric_samples` grouped by `http.route` label.

#### 6.2.2 Traffic Panel

| Metric | Source (Observability Contract) | Visualization |
|--------|-------------------------------|---------------|
| API request rate | `kaiku_http_requests_total` | Time-series area chart |
| Active WebSocket connections | `kaiku_ws_connections_active` | Time-series line chart |
| Active voice sessions | `kaiku_voice_sessions_active` | Time-series line chart |
| WebSocket messages dispatched | `kaiku_ws_messages_total` | Time-series area chart, stacked by event type |
| Voice RTP packets forwarded | `kaiku_voice_rtp_packets_forwarded_total` | Time-series area chart |

#### 6.2.3 Errors Panel

| Metric | Source (Observability Contract) | Visualization |
|--------|-------------------------------|---------------|
| HTTP error rate (4xx + 5xx) | `kaiku_http_errors_total` by status class | Time-series stacked area (4xx vs 5xx) |
| Auth failure rate | `kaiku_auth_login_attempts_total{outcome=failure}` | Time-series line chart |
| Auth token refresh failures | `kaiku_auth_token_refresh_total{outcome=failure}` | Time-series line chart |
| WebSocket reconnect rate | `kaiku_ws_reconnects_total` | Time-series line chart |
| Voice join failure rate | `kaiku_voice_joins_total{outcome=error}` | Time-series line chart |
| OTel export failures | `kaiku_otel_export_failures_total` | Time-series line chart |
| Client WebRTC failures | `kaiku_client_voice_webrtc_failures_total` | Time-series line chart (v1.1) |

**Top failing routes table:** Top 10 routes by error count in selected range.
**Top error categories table:** Grouped by `error.type` label (auth, db, ws, voice, ratelimit).

#### 6.2.4 Saturation Panel

| Metric | Source (Observability Contract) | Visualization |
|--------|-------------------------------|---------------|
| DB pool utilization | `kaiku_db_pool_connections_active` / pool max | Gauge + time-series |
| DB idle connections | `kaiku_db_pool_connections_idle` | Time-series |
| OTel dropped spans | `kaiku_otel_dropped_spans_total` | Counter time-series |
| Process memory (RSS) | New: `kaiku_process_memory_bytes` (from `/proc/self/statm` or `sysinfo`) | Time-series line chart |

**Note:** Process CPU usage is intentionally omitted from v1 native metrics. CPU profiling belongs in external tooling. Memory (RSS) is included because it's cheap to collect and directly actionable (leak detection).

---

### Tier 3: Voice Operations

**Purpose:** Domain-specific monitoring for the platform's core differentiator.
**Update model:** WebSocket push for active session count; poll every 10s for quality metrics.

Voice telemetry already exists in TimescaleDB (`connection_metrics`, `connection_sessions` hypertables). This tier surfaces it in the admin panel.

| Signal | Source | Visualization |
|--------|--------|---------------|
| Active voice sessions | `kaiku_voice_sessions_active` gauge | Large number + sparkline |
| Active voice rooms | SFU `room_count()` | Large number |
| Participant distribution | SFU room data | Histogram (participants per room) |
| Join success rate (24h) | `kaiku_voice_joins_total` success / total | Percentage + trend chart |
| Packet loss rate (rolling) | `connection_metrics.packet_loss` | Time-series line chart (p50/p95) |
| Network jitter (rolling) | `connection_metrics.jitter_ms` | Time-series line chart (p50/p95) |
| Session latency (rolling) | `connection_metrics.latency_ms` | Time-series line chart (p50/p95) |
| Quality score distribution | `connection_metrics.quality` (0-3 scale) | Stacked bar chart (poor/fair/good/excellent) |
| RTP forwarding rate | `kaiku_voice_rtp_packets_forwarded_total` | Time-series rate chart |
| Session duration distribution | `kaiku_voice_session_duration_seconds` | Histogram |

**Voice Health Score:** A single composite indicator (0-100) derived from: join success rate (40% weight), packet loss p95 (30% weight), jitter p95 (20% weight), session crash rate (10% weight). Displayed as a prominent gauge on the voice tab.

---

### Tier 4: Infrastructure Health

**Purpose:** Show the health of backing services. Critical for self-hosted operators.
**Update model:** Poll every 10s.

#### 6.4.1 Database

| Signal | Source | Display |
|--------|--------|---------|
| Connection pool active/idle/max | `kaiku_db_pool_connections_active`, `kaiku_db_pool_connections_idle`, pool config | Gauge bars |
| Query latency p95 | `kaiku_db_query_duration_seconds` | Value + trend sparkline |
| Query rate | `kaiku_db_query_duration_seconds` count | Time-series |
| Top slow queries | From `telemetry_trace_index` where domain = `db`, sorted by duration | Table (span_name, duration, count) |
| Connectivity | Health check `SELECT 1` | Status badge |

#### 6.4.2 Valkey (Redis)

| Signal | Source | Display |
|--------|--------|---------|
| Connectivity | Health check `PING` | Status badge |
| Used for | Static: rate limiting, sessions, blocks, elevated admin cache | Info text |

**Note:** Redis `INFO` stats (memory, hit rate, connected clients) require a new `REDIS INFO` query — planned for v1.1. v1 shows connectivity only.

#### 6.4.3 S3 Storage

| Signal | Source | Display |
|--------|--------|---------|
| Connectivity | `health_check()` bucket access | Status badge |
| Status | Configured / Not configured / Degraded | Status badge |

**Note:** Upload/download throughput metrics require new instrumentation — planned for v1.1.

#### 6.4.4 OTel Pipeline

| Signal | Source | Display |
|--------|--------|---------|
| Export success/failure rate | `kaiku_otel_export_failures_total` | Time-series + status badge |
| Dropped spans | `kaiku_otel_dropped_spans_total` | Counter + alert if > 0 |
| Collector endpoint | Config value | Info text |
| Pipeline status | Export failure rate < 1% = healthy | Status badge |

---

### Tier 5: Curated Logs

**Purpose:** Triage errors without leaving the admin panel.
**Update model:** WebSocket push for new ERROR entries; poll every 15s for WARN.

| Field | Source | Notes |
|-------|--------|-------|
| Severity | `level` | WARN and ERROR only in v1 |
| Timestamp | `ts` | |
| Service | `service` | `vc-server` or `vc-client` |
| Domain | `domain` | `auth`, `chat`, `voice`, `ws`, `db`, `admin`, `crypto` |
| Event | `event` | Short event name |
| Message | `message` | Truncated to 200 chars in list view |
| Trace ID | `trace_id` | Clickable — links to trace index or external Tempo |
| Span ID | `span_id` | |

**Filters:** severity, domain, service, time range, free-text search on event/message.
**Pagination:** Cursor-based, max 100 per page.
**Export:** CSV/JSON of current filtered view.

---

### Tier 6: Trace Index

**Purpose:** Find problematic requests by metadata, then hand off to external tools for full trace analysis.
**Update model:** Poll every 30s.

| Field | Source | Notes |
|-------|--------|-------|
| Trace ID | `trace_id` | Clickable — "Open in Tempo" when configured |
| Span name | `span_name` | e.g. `http.server.request`, `voice.session_join` |
| Route | `route` | Parameterised template |
| Domain | `domain` | |
| Status | `status_code` | HTTP status or outcome |
| Duration | `duration_ms` | Color-coded by threshold |
| Timestamp | `ts` | |

**Filters:** status (error/slow/all), domain, route, duration threshold, time range.
**Pre-filtered views:** "Recent errors" (status >= 500), "Recent slow" (duration > p95 threshold).
**Pagination:** Cursor-based, max 100 per page.
**Export:** CSV/JSON of current filtered view.

---

### Tier 7: Client Telemetry (v1.1)

**Purpose:** Visibility into client-side performance. Deferred to v1.1 because it requires client-to-server metric ingestion pipeline.

Planned signals from the observability contract:

| Metric | Source | Notes |
|--------|--------|-------|
| Tauri command latency p95 | `kaiku_client_tauri_command_duration_seconds` | Per-command breakdown |
| Tauri command errors | `kaiku_client_tauri_command_errors_total` | By command name |
| WebRTC connect time p95 | `kaiku_client_voice_webrtc_connect_duration_seconds` | ICE negotiation time |
| WebRTC connection failures | `kaiku_client_voice_webrtc_failures_total` | By failure reason |

**Ingestion path:** Client → Tauri command → Server endpoint → native storage. Requires new `/api/telemetry/client-metrics` endpoint with rate limiting and size bounds.

---

## 7. Layout and Information Hierarchy

### 7.1 Spatial Layout

```
┌─────────────────────────────────────────────────────────────┐
│  COMMAND CENTER                    [Time Range ▾] [⟳ Auto]  │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐          │
│  │ Latency │ │ Traffic │ │ Errors  │ │ Saturat.│  ← Vital  │
│  │  12ms   │ │ 847/s   │ │  0.3%   │ │  42%    │    Signs  │
│  │ ~~~^^^~ │ │ ~~~^^^~ │ │ ~~~___~ │ │ ~~~^^^~ │           │
│  └─────────┘ └─────────┘ └─────────┘ └─────────┘          │
│                                                             │
│  ┌───────────────────────────┐ ┌───────────────────────────┐│
│  │ Service Health Matrix     │ │ Live Error Feed           ││
│  │ ● API        healthy     │ │ 17:04:12 auth login_fail  ││
│  │ ● WebSocket  healthy     │ │ 17:03:58 db   query_slow  ││
│  │ ● Voice SFU  healthy     │ │ 17:03:41 voice join_fail  ││
│  │ ● PostgreSQL healthy     │ │ 17:03:22 ws   disconnect  ││
│  │ ● Valkey     healthy     │ │ ...                       ││
│  │ ● S3         available   │ │ [View all logs →]         ││
│  │ ● OTel       healthy     │ │                           ││
│  └───────────────────────────┘ └───────────────────────────┘│
│                                                             │
│  ┌─ [Golden Signals] [Voice] [Infrastructure] [Logs] [Traces]│
│  │                                                          │
│  │  (Selected tab content fills remaining vertical space)   │
│  │                                                          │
│  └──────────────────────────────────────────────────────────┘
└─────────────────────────────────────────────────────────────┘
```

### 7.2 Above the Fold

The **Health Overview** (Tier 1) is always visible at the top — never scrolled away. It consists of:
1. Four vital-sign cards (golden signals summary, 1-row)
2. Service health matrix + live error feed (2-column, equal width)

### 7.3 Tabbed Detail Area

Below the fold, a tab bar provides access to:
- **Golden Signals** (Tier 2) — default active tab
- **Voice** (Tier 3)
- **Infrastructure** (Tier 4)
- **Logs** (Tier 5)
- **Traces** (Tier 6)

Each tab fills the remaining vertical space. Tab state persists across panel switches.

### 7.4 Time Range Picker

Global time range selector in the top-right corner:
- Presets: `1h`, `6h`, `24h`, `7d`, `30d`
- Custom range with calendar picker
- Auto-refresh toggle with cadence indicator (e.g., "Refreshing every 10s")
- Applies to all trend charts and tables in the active tab

### 7.5 Responsive Behavior

- Desktop (>1200px): Full 2-column health overview, tab content fills width
- Tablet (768-1200px): Health overview stacks to single column; tab content full-width
- Below 768px: Not targeted (admin panel is desktop-first)

---

## 8. Status Thresholds

Every numeric signal has a three-tier status (green/yellow/red) with configurable thresholds via server config. Defaults are chosen to be safe for most deployments:

### 8.1 Default Thresholds

| Signal | Green | Yellow | Red |
|--------|-------|--------|-----|
| API p95 latency | < 200ms | 200-1000ms | > 1000ms |
| API error rate (5xx) | < 1% | 1-5% | > 5% |
| DB pool utilization | < 70% | 70-90% | > 90% |
| DB query p95 | < 100ms | 100-500ms | > 500ms |
| Voice join success rate | > 95% | 80-95% | < 80% |
| Voice packet loss p95 | < 2% | 2-5% | > 5% |
| OTel export failure rate | < 1% | 1-5% | > 5% |
| OTel dropped spans (last 5m) | 0 | 1-100 | > 100 |
| Process memory (RSS) | < 70% of limit | 70-90% | > 90% |

### 8.2 Configuration

Thresholds are stored in server config (environment variables with `COMMAND_CENTER_` prefix):

```
COMMAND_CENTER_API_LATENCY_P95_WARN_MS=200
COMMAND_CENTER_API_LATENCY_P95_CRIT_MS=1000
COMMAND_CENTER_API_ERROR_RATE_WARN_PCT=1
COMMAND_CENTER_API_ERROR_RATE_CRIT_PCT=5
# ... etc
```

Thresholds are returned via `GET /api/admin/observability/config` and displayed as colored indicators (status dot + background tint) on cards and chart annotations.

---

## 9. Real-Time Update Strategy

The platform has mature WebSocket infrastructure. The command center leverages it.

### 9.1 WebSocket Push Events (new `admin.observability.*` event types)

| Event | Payload | Trigger |
|-------|---------|---------|
| `admin.observability.health_change` | `{ service, old_status, new_status }` | Service transitions between healthy/degraded/down |
| `admin.observability.error_event` | `{ ts, service, domain, event, message, trace_id }` | New ERROR-level log ingested |
| `admin.observability.vital_signs` | `{ latency_p95, traffic_rate, error_rate, saturation_pct }` | Every 5s while admin panel is open |
| `admin.observability.voice_pulse` | `{ active_sessions, active_rooms, join_success_rate_1h }` | Every 5s while voice tab is open |

Events are only sent to WebSocket connections authenticated as system admins with the command center panel active (not broadcast to all connections).

### 9.2 Polling Cadence

| Data Type | Interval | Rationale |
|-----------|----------|-----------|
| Trend charts (time-series) | 30s | Pre-aggregated, not latency-sensitive |
| Top offenders tables | 30s | Aggregate queries, moderate cost |
| Log list | 15s (+ WS push for errors) | Balance freshness vs. DB load |
| Trace index | 30s | Not latency-sensitive |
| Infrastructure health | 10s | Backing service changes need prompt detection |

### 9.3 Freshness Indicators

Every panel section shows a "Last updated: Xs ago" indicator. If data is stale (> 2x expected interval), the indicator turns yellow with "(stale)" suffix.

---

## 10. Incident Correlation

**Purpose:** When a metric anomaly is detected, operators need to see related signals on a shared time axis to understand cause and effect.

### 10.1 Shared Time Axis

The Golden Signals tab supports an "Incident View" toggle that overlays:
- Metric chart (selected signal) as primary
- Error rate overlay (secondary y-axis)
- Log event markers (vertical lines at ERROR timestamps)
- Deploy markers (vertical dashed lines, sourced from config or audit log)

This answers the SRE question: *"Our latency just shot up — what else happened around the same time?"*

### 10.2 Click-Through Correlation

| From | To | Mechanism |
|------|----|-----------|
| Error in live feed | Full log entry in Logs tab | Click navigates to Logs tab, filters by trace_id |
| Log entry with trace_id | Trace index entry | Click navigates to Traces tab, filters by trace_id |
| Trace index entry | External Tempo | "Open in Tempo" button (when configured) |
| Metric anomaly on chart | Logs tab for that time window | Click on chart point opens Logs tab with time filter |

### 10.3 Deploy Markers (v1.1)

Deploy events are recorded as audit log entries (`admin.deploy.detected`) and displayed as vertical markers on all time-series charts. Source: server start time with new version, or explicit deploy webhook.

---

## 11. Data Model (Native Retention)

All native telemetry tables live in the same PostgreSQL database. TimescaleDB extension is used where available (graceful fallback to standard PostgreSQL with periodic `DELETE` for retention).

### 11.1 `telemetry_metric_samples` (Timescale hypertable)

Stores pre-aggregated metric samples aligned with the observability contract.

```sql
CREATE TABLE telemetry_metric_samples (
    ts          TIMESTAMPTZ NOT NULL,
    metric_name TEXT        NOT NULL,  -- from contract: kaiku_* names
    scope       TEXT        NOT NULL,  -- 'cluster' (future: per-node)
    labels      JSONB       NOT NULL DEFAULT '{}'::jsonb,  -- contract-allowlisted only
    value_count BIGINT      NULL,      -- for counters: cumulative count
    value_sum   DOUBLE PRECISION NULL, -- for histograms: sum of observations
    value_p50   DOUBLE PRECISION NULL, -- pre-computed percentile
    value_p95   DOUBLE PRECISION NULL, -- pre-computed percentile
    value_p99   DOUBLE PRECISION NULL  -- pre-computed percentile
);

-- TimescaleDB: convert to hypertable with 1-hour chunks
SELECT create_hypertable('telemetry_metric_samples', 'ts',
    chunk_time_interval => INTERVAL '1 hour');

-- Indexes for command center query patterns
CREATE INDEX idx_tms_metric_ts ON telemetry_metric_samples (metric_name, ts DESC);
CREATE INDEX idx_tms_scope_ts ON telemetry_metric_samples (scope, ts DESC);
```

**Histogram strategy:** Rather than storing raw histogram buckets (high cardinality), we pre-compute p50/p95/p99 at ingestion time using the OTel histogram data points. This keeps storage bounded while providing the percentiles operators actually need.

**Ingestion cadence:** Every 60 seconds, an async task aggregates in-memory metric state and inserts one row per metric per label combination.

### 11.2 `telemetry_log_events`

Stores curated log events (WARN/ERROR only).

```sql
CREATE TABLE telemetry_log_events (
    id       UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    ts       TIMESTAMPTZ NOT NULL,
    level    TEXT        NOT NULL CHECK (level IN ('ERROR', 'WARN')),
    service  TEXT        NOT NULL,
    domain   TEXT        NOT NULL,
    event    TEXT        NOT NULL,
    message  TEXT        NOT NULL,
    trace_id TEXT        NULL,
    span_id  TEXT        NULL,
    attrs    JSONB       NOT NULL DEFAULT '{}'::jsonb  -- allowlisted attrs only
);

CREATE INDEX idx_tle_ts ON telemetry_log_events (ts DESC);
CREATE INDEX idx_tle_level_ts ON telemetry_log_events (level, ts DESC);
CREATE INDEX idx_tle_domain_ts ON telemetry_log_events (domain, ts DESC);
CREATE INDEX idx_tle_trace_id ON telemetry_log_events (trace_id) WHERE trace_id IS NOT NULL;
```

### 11.3 `telemetry_trace_index`

Stores trace metadata only — no span payloads or attributes beyond what's needed for filtering.

```sql
CREATE TABLE telemetry_trace_index (
    trace_id    TEXT        NOT NULL,
    span_name   TEXT        NOT NULL,
    domain      TEXT        NOT NULL,
    route       TEXT        NULL,
    status_code TEXT        NULL,
    duration_ms INTEGER     NOT NULL,
    ts          TIMESTAMPTZ NOT NULL,
    service     TEXT        NOT NULL
);

CREATE INDEX idx_tti_ts ON telemetry_trace_index (ts DESC);
CREATE INDEX idx_tti_status_ts ON telemetry_trace_index (status_code, ts DESC);
CREATE INDEX idx_tti_domain_ts ON telemetry_trace_index (domain, ts DESC);
CREATE INDEX idx_tti_duration ON telemetry_trace_index (duration_ms DESC, ts DESC);
CREATE INDEX idx_tti_trace_id ON telemetry_trace_index (trace_id);
```

### 11.4 `telemetry_trend_rollups` (materialized view)

Pre-aggregated daily rollups for 30-day trend queries. Refreshed hourly by background job.

```sql
CREATE MATERIALIZED VIEW telemetry_trend_rollups AS
SELECT
    date_trunc('day', ts) AS day,
    metric_name,
    scope,
    labels->>'http.route' AS route,
    COUNT(*) AS sample_count,
    AVG(value_p95) AS avg_p95,
    MAX(value_p95) AS max_p95,
    SUM(value_count) AS total_count,
    SUM(CASE WHEN (labels->>'http.response.status_code')::int >= 500
         THEN value_count ELSE 0 END) AS error_count
FROM telemetry_metric_samples
GROUP BY 1, 2, 3, 4;

CREATE UNIQUE INDEX idx_ttr_day_metric ON telemetry_trend_rollups (day, metric_name, scope, route);
```

### 11.5 Retention Policies

| Table | Policy | Mechanism |
|-------|--------|-----------|
| `telemetry_metric_samples` | 30 days hard delete | TimescaleDB `drop_chunks` / scheduled `DELETE WHERE ts < now() - '30d'` |
| `telemetry_log_events` | 30 days hard delete | Scheduled `DELETE WHERE ts < now() - '30d'` |
| `telemetry_trace_index` | 30 days hard delete | Scheduled `DELETE WHERE ts < now() - '30d'` |
| `telemetry_trend_rollups` | Refreshed hourly, covers available data | `REFRESH MATERIALIZED VIEW CONCURRENTLY` |

Retention job runs as a background task every hour. Logs execution time and rows deleted to its own observability output.

---

## 12. API Design (Admin)

All endpoints under `/api/admin/observability/*`, require `SystemAdminUser` middleware.

### 12.1 Health and Overview

- `GET /api/admin/observability/summary`
  - Returns: vital signs (4 golden signal values), service health matrix, server metadata, active alert count
  - Response time target: < 50ms

### 12.2 Trends

- `GET /api/admin/observability/trends?range=1h|6h|24h|7d|30d&metric=<name>`
  - Returns: time-series data points for the requested metric(s)
  - Uses live query for ranges <= 24h, materialized view for 7d/30d
  - Response time target: < 200ms (24h), < 500ms (30d)

### 12.3 Top Offenders

- `GET /api/admin/observability/top-routes?range=...&sort=latency|errors&limit=10`
- `GET /api/admin/observability/top-errors?range=...&limit=10`
  - Returns: ranked lists with route, count, p95, error rate

### 12.4 Voice

- `GET /api/admin/observability/voice/summary`
  - Returns: active sessions, rooms, join success rate, voice health score
- `GET /api/admin/observability/voice/quality?range=...`
  - Returns: packet loss, jitter, latency time-series from TimescaleDB

### 12.5 Infrastructure

- `GET /api/admin/observability/infrastructure`
  - Returns: health status for DB, Valkey, S3, OTel pipeline with available metrics

### 12.6 Logs

- `GET /api/admin/observability/logs?level=&domain=&service=&from=&to=&search=&cursor=&limit=`
  - Returns: paginated log entries, cursor for next page
  - Max limit: 100

### 12.7 Trace Index

- `GET /api/admin/observability/traces?status=&domain=&route=&duration_min=&from=&to=&cursor=&limit=`
  - Returns: paginated trace index entries, cursor for next page
  - Max limit: 100

### 12.8 Export

- `GET /api/admin/observability/export?type=logs|traces|metrics&format=csv|json&...filters`
  - Returns: streamed file download of filtered data
  - Max: 10,000 rows per export

### 12.9 External Links

- `GET /api/admin/observability/links`
  - Returns: configured external tool URLs (Grafana, Tempo, Loki, Prometheus) or empty if not configured

### 12.10 Configuration

- `GET /api/admin/observability/config`
  - Returns: threshold values, refresh cadences, feature flags (v1.1 features enabled/disabled)

---

## 13. Security and Privacy

### 13.1 Access Control

- All endpoints require `SystemAdminUser` middleware (existing admin auth flow).
- Elevated session (`ElevatedAdmin`) NOT required for read-only observability — reduces friction for monitoring.
- Access to observability endpoints is recorded in audit log as `admin.observability.view` event.

### 13.2 Data Redaction

Native telemetry storage inherits ALL redaction rules from `observability-contract.md`:

- **Forbidden fields** (Section 7): Never persisted in `attrs` JSONB, `labels` JSONB, or any text field.
- **Label allowlist** (Section 6): Only contract-allowlisted labels stored in `telemetry_metric_samples.labels`.
- **Log scrubbing** (Section 9): Applied before native storage, not just before OTLP export.
- **No PII**: No user IDs, IPs, message content, or credentials in any native telemetry table.

### 13.3 WebSocket Security

- `admin.observability.*` events are only sent to authenticated system admin connections.
- Event subscription is implicit when the command center panel is active (frontend sends a subscription message).
- No sensitive data in push events — only aggregate values and sanitized error summaries.

---

## 14. Cardinality and Cost Controls

- **Label allowlist enforcement:** Native metric storage ONLY accepts labels from the contract's Section 6 allowlist. All others are silently dropped at ingestion.
- **Route label normalization:** `http.route` labels use parameterised templates only (e.g., `/api/v1/guilds/{guild_id}`), never resolved paths.
- **Cardinality budget:** Max 100 unique label combinations per metric (contract rule). Enforced at ingestion; excess combinations logged as warnings.
- **Query bounds:** All list endpoints enforce max page size (100), max time range (30d), and required time filters.
- **Export limits:** Max 10,000 rows per export request.
- **Storage budget (estimated):** At moderate load (1000 DAU), ~500MB/month for all native telemetry tables combined. At heavy load (10,000 DAU), ~2GB/month. Retention purge keeps total under 6x monthly volume.

---

## 15. Operational Behavior

- **Degraded mode:** If OTel collector or external stack is unavailable, native panel remains fully functional using native storage. Degraded badges appear for affected services.
- **Freshness indicators:** Every panel section shows last successful ingest/update timestamp. Stale data (> 2x expected interval) shows yellow warning.
- **Startup behavior:** Command center shows "Collecting data..." placeholder for the first 60 seconds after server start while initial metrics accumulate.
- **Backfill:** No historical backfill beyond native retention boundaries. After 30 days of downtime, the panel starts fresh.
- **Feature flags:** v1.1 features (client telemetry, deploy markers, Redis stats) gated behind config flags. Disabled by default until implemented.

---

## 16. Performance Constraints

| Endpoint | p95 Target | Notes |
|----------|-----------|-------|
| Summary | <= 50ms | In-memory cached, refreshed every 5s |
| Trends (<= 24h) | <= 200ms | Live query on hypertable |
| Trends (7d/30d) | <= 500ms | Materialized view |
| Top offenders | <= 200ms | Aggregate query with limit |
| Voice summary | <= 100ms | Mix of in-memory gauge + TimescaleDB |
| Logs/Traces list | <= 200ms | Indexed cursor pagination |
| Infrastructure | <= 100ms | Cached health probes |
| Export | <= 5s (streaming) | Streamed response, not buffered |

UI refresh cadence: configurable 5-30 seconds depending on panel (see Section 9.2).
Ingestion jobs: async, bounded queue, no hot-path blocking.
Background retention job: hourly, < 10s execution target.

---

## 17. Chart Library Decision

The client currently has **no chart/visualization library** installed. The command center requires time-series charts, gauges, and sparklines. Evaluation:

| Library | Solid.js Support | Bundle Size | Features | Decision |
|---------|-----------------|-------------|----------|----------|
| Chart.js | Via solid-chartjs | ~65KB gzip | Line, bar, area, doughnut | **Recommended** |
| Recharts | React-only (no Solid adapter) | ~130KB gzip | Full featured | Rejected — React dependency |
| D3 | Framework-agnostic | ~90KB gzip | Maximum flexibility | Rejected — over-engineered for our needs |
| Lightweight custom (Canvas) | Native | ~5KB | Sparklines only | Rejected — insufficient for trend charts |

**Recommendation:** `chart.js` + `solid-chartjs` (or thin wrapper). Provides line charts, area charts, bar charts, and doughnut/gauge — all needed by the command center. MIT licensed, actively maintained, 60K+ GitHub stars.

---

## 18. Rollout Plan

### Phase 1 — Data Foundation
1. Native telemetry schema migration (3 tables + materialized view).
2. Retention and downsampling jobs.
3. Storage/query module in `server/src/observability/storage.rs`.
4. Ingestion pipeline wiring (curated logs, trace index, metric samples).
5. Label allowlist enforcement and cardinality guardrails.

### Phase 2 — Admin API Surface
6. Summary, trends, top-offenders endpoints.
7. Voice summary and quality endpoints.
8. Infrastructure health endpoint.
9. Logs and trace-index list endpoints with pagination.
10. Export endpoint (CSV/JSON).
11. External links and config endpoints.
12. Audit logging for observability access.

### Phase 3 — Client Admin Panel
13. Sidebar entry and panel routing (`command-center`).
14. Health overview (vital signs, service matrix, live error feed).
15. Golden Signals tab (charts, tables).
16. Voice tab.
17. Infrastructure tab.
18. Logs and Traces tabs.
19. Time range picker and auto-refresh.
20. Degraded/empty state handling.

### Phase 4 — Real-Time and Correlation
21. WebSocket push events for health transitions and vital signs.
22. Live error feed auto-scroll.
23. Incident correlation view (shared time axis, click-through).
24. External deep-link integration (Grafana/Tempo/Loki actions).

### Phase 5 — Quality Gates
25. Backend integration tests (auth, bounds, retention, redaction).
26. Frontend component tests (panel states, data rendering).
27. E2E smoke test (admin can open command center, see health data).
28. CI governance checks (retention constants, forbidden fields, schema conformance).

---

## 19. Success Criteria

- [ ] All system admins can open cluster-wide command center and see current health signals within 5 seconds.
- [ ] Health overview answers "is something wrong?" with service matrix + vital signs + live error feed.
- [ ] Golden signals are organized as Latency/Traffic/Errors/Saturation with time-series charts.
- [ ] Voice operations tab shows session quality, join success, packet loss, jitter trends.
- [ ] Infrastructure tab shows DB pool, Valkey, S3, OTel pipeline health.
- [ ] Logs are WARN/ERROR only, filterable, with trace ID correlation.
- [ ] Trace index is searchable with "Open in Tempo" when external stack configured.
- [ ] Native data never exceeds 30-day retention.
- [ ] Status thresholds are color-coded (green/yellow/red) with configurable values.
- [ ] Real-time updates via WebSocket for health transitions and vital signs.
- [ ] Export to CSV/JSON works for logs, traces, and metrics views.
- [ ] Degraded mode works without external stack.
- [ ] No forbidden fields leak into persisted native telemetry.
- [ ] All metrics map to observability contract definitions.
