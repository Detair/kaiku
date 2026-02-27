# Phase 7 Admin Command Center — Implementation Plan

**Date:** 2026-02-27
**Status:** Draft (v2)
**Design Doc:** `docs/plans/2026-02-27-phase-7-admin-command-center-design.md`
**Task Plan:** `docs/plans/2026-02-27-phase-7-admin-command-center-task-plan.md`
**Roadmap Item:** Phase 7 `[Ops] Admin Command Center (Native Observability Lite)`

## Objective

Implement a cluster-wide native admin Command Center that provides 30-day operational visibility for all system admins, organized around the Four Golden Signals framework (Latency, Traffic, Errors, Saturation). The panel covers health overview, golden signals trends, voice operations, infrastructure health, curated logs, and trace index — while preserving external observability (Grafana/Tempo/Loki/Prometheus) for deep forensics.

## Open-Source and Maintenance Policy

This feature must use actively maintained open-source tooling only.

### Required policy gates

1. **Open source license only**
   - Allowed: permissive licenses compatible with project policy (MIT, Apache-2.0, BSD).
   - Disallowed: GPL/AGPL/LGPL dependencies in runtime path (already aligned with `cargo deny`).

2. **Maintenance recency**
   - Any new dependency must show active maintenance (recent releases and non-stale issue/PR activity).
   - Avoid deprecated or archived projects.

3. **No abandoned observability components**
   - Prefer OpenTelemetry ecosystem defaults and Grafana stack components with active support.
   - Do not introduce legacy/deprecated telemetry SDKs or exporters.

4. **Version pinning and upgrade discipline**
   - Pin Docker images and dependency ranges to known-good versions.
   - Track upgrade cadence in roadmap/runbook notes.

5. **Standards adherence**
   - Keep OTel semantic conventions, OTLP transport, and redaction contract alignment.
   - Preserve vendor-neutral architecture boundaries.

## Scope

### In scope (native)

- Admin UI panel `command-center` with 7-tier signal taxonomy (Design §6)
- Health overview: vital signs cards, service health matrix, live error feed (Tier 1)
- Golden signals dashboard with 30-day trends and top offenders (Tier 2)
- Voice operations tab: session quality, join success, packet loss, jitter (Tier 3)
- Infrastructure health: DB pool, Valkey, S3, OTel pipeline (Tier 4)
- Curated log list (WARN/ERROR only, redacted, filterable) (Tier 5)
- Trace index list (metadata only, no span payload storage) (Tier 6)
- Real-time WebSocket push for health transitions and vital signs (Design §9)
- Status thresholds with green/yellow/red indicators (Design §8)
- Incident correlation via shared time axis and click-through (Design §10)
- Export endpoint for logs, traces, and metrics (CSV/JSON) (Design §12.8)
- Native retention enforcement (30 days max)
- Optional deep-links to external Grafana stack when configured

### Out of scope (v1)

- In-app full trace waterfall renderer
- In-app advanced log query language
- Native retention beyond 30 days
- Alert-rule authoring UI
- Per-node or per-role observability segmentation
- Client telemetry ingestion pipeline (v1.1, Design §6 Tier 7)
- Redis INFO stats beyond connectivity (v1.1)
- Deploy markers on charts (v1.1)
- Rolling 7-day anomaly detection baselines (v1.1)

## Implementation Phases

The six phases below map directly to the design's rollout plan (Design §18), extended with chart library installation and WebSocket push as distinct phases.

---

## Phase 1 — Data Foundation

*Design references: §11 (Data Model), §11.1 (histogram strategy), §11.4 (materialized view), §11.5 (retention)*

### 1. Add telemetry storage schema

- Files:
  - `server/migrations/<timestamp>_telemetry_native_command_center.sql`
- Tasks:
  - Create `telemetry_metric_samples` as TimescaleDB hypertable (1-hour chunks), with columns: `ts`, `metric_name`, `scope`, `labels` (JSONB), `value_count`, `value_sum`, `value_p50`, `value_p95`, `value_p99`
  - Create `telemetry_log_events` with columns: `id` (UUID), `ts`, `level` (WARN/ERROR), `service`, `domain`, `event`, `message`, `trace_id`, `span_id`, `attrs` (JSONB)
  - Create `telemetry_trace_index` with columns: `trace_id`, `span_name`, `domain`, `route`, `status_code`, `duration_ms`, `ts`, `service`
  - Add all indexes from Design §11: `idx_tms_metric_ts`, `idx_tms_scope_ts`, `idx_tle_ts`, `idx_tle_level_ts`, `idx_tle_domain_ts`, `idx_tle_trace_id`, `idx_tti_ts`, `idx_tti_status_ts`, `idx_tti_domain_ts`, `idx_tti_duration`, `idx_tti_trace_id`
  - Graceful fallback: if TimescaleDB extension is unavailable, create as standard PostgreSQL table with scheduled DELETE for retention

### 2. Add materialized view for 30-day trend rollups

- Files:
  - `server/migrations/<timestamp>_telemetry_trend_rollups.sql`
- Tasks:
  - Create `telemetry_trend_rollups` materialized view (Design §11.4): daily rollup of `metric_name`, `scope`, `route` (from labels), `sample_count`, `avg_p95`, `max_p95`, `total_count`, `error_count`
  - Add unique index `idx_ttr_day_metric` on `(day, metric_name, scope, route)`
  - Document: this view is used for 7d/30d trend queries; live queries serve ranges <= 24h

### 3. Add retention and rollup jobs

- Files:
  - `server/src/observability/retention.rs` (new)
  - `server/src/observability/mod.rs`
- Tasks:
  - Implement hourly background task: `DELETE WHERE ts < now() - INTERVAL '30 days'` for all three telemetry tables
  - Implement hourly `REFRESH MATERIALIZED VIEW CONCURRENTLY telemetry_trend_rollups`
  - Log execution time and rows deleted per run (to tracing output, not to native telemetry tables)
  - Target: < 10s execution per hourly run

### 4. Add storage and query module

- Files:
  - `server/src/observability/storage.rs` (new)
  - `server/src/observability/mod.rs`
- Tasks:
  - Insert helpers: `insert_metric_sample`, `insert_log_event`, `insert_trace_index_entry`
  - Query helpers: `query_trends` (live vs. materialized view branch by range), `query_logs` (cursor pagination), `query_traces` (cursor pagination), `query_top_routes`, `query_top_errors`
  - Enforce max page size (100) and max time range (30d) at query layer
  - All public functions annotated with `#[tracing::instrument(skip(pool))]`

### 5. Implement histogram pre-computation strategy

- Files:
  - `server/src/observability/metrics.rs`
  - `server/src/observability/storage.rs`
- Tasks:
  - At ingestion time, compute p50/p95/p99 from OTel histogram data points before writing to `telemetry_metric_samples`
  - Do not store raw histogram buckets (high cardinality); store only pre-computed percentiles
  - Ingestion cadence: async task aggregates in-memory metric state every 60 seconds, inserts one row per metric per label combination

### 6. Implement label allowlist and cardinality guardrails

- Files:
  - `server/src/observability/metrics.rs`
  - `docs/ops/observability-contract.md` (reference, do not modify)
- Tasks:
  - Enforce contract Section 6 label allowlist: only allowlisted labels stored in `telemetry_metric_samples.labels` JSONB
  - Silently drop non-allowlisted labels at ingestion
  - Enforce max 100 unique label combinations per metric; log warning when exceeded
  - Route label normalization: store parameterised templates only (e.g., `/api/v1/guilds/{guild_id}`), never resolved paths

---

## Phase 2 — Admin API Surface

*Design references: §12 (API Design), §8 (Status Thresholds), §13 (Security), §14 (Cardinality Controls), §16 (Performance Constraints)*

All endpoints under `/api/admin/observability/*`, require `SystemAdminUser` middleware. Access recorded as `admin.observability.view` audit event.

### 7. Summary and health endpoint

- Files:
  - `server/src/admin/observability.rs` (new)
  - `server/src/admin/mod.rs`
  - `server/src/admin/types.rs`
- Endpoint: `GET /api/admin/observability/summary`
- Response: vital signs (4 golden signal current values), service health matrix (7 services), server metadata (version, uptime, environment, active users, guild count), active alert count
- Implementation: in-memory cached, refreshed every 5s; health probes run async
- Performance target: p95 <= 50ms (Design §16)

### 8. Trends endpoint

- Files:
  - `server/src/admin/observability.rs`
- Endpoint: `GET /api/admin/observability/trends?range=1h|6h|24h|7d|30d&metric=<name>`
- Response: time-series data points for requested metric(s)
- Implementation: live query on `telemetry_metric_samples` for ranges <= 24h; `telemetry_trend_rollups` materialized view for 7d/30d (Design §12.2)
- Performance targets: p95 <= 200ms (24h), p95 <= 500ms (30d) (Design §16)

### 9. Top offenders endpoints

- Files:
  - `server/src/admin/observability.rs`
- Endpoints:
  - `GET /api/admin/observability/top-routes?range=...&sort=latency|errors&limit=10`
  - `GET /api/admin/observability/top-errors?range=...&limit=10`
- Response: ranked lists with route, count, p95, error rate; top error categories grouped by `error.type` label
- Performance target: p95 <= 200ms (Design §16)

### 10. Voice endpoints

- Files:
  - `server/src/admin/observability.rs`
- Endpoints:
  - `GET /api/admin/observability/voice/summary`
  - `GET /api/admin/observability/voice/quality?range=...`
- Voice summary response: active sessions, active rooms, join success rate (24h), voice health score (0-100 composite: join success 40%, packet loss p95 30%, jitter p95 20%, session crash rate 10%)
- Voice quality response: packet loss, jitter, latency time-series from `connection_metrics` TimescaleDB hypertable (p50/p95 per interval)
- Performance targets: voice summary p95 <= 100ms; quality p95 <= 200ms (Design §16)

### 11. Infrastructure health endpoint

- Files:
  - `server/src/admin/observability.rs`
- Endpoint: `GET /api/admin/observability/infrastructure`
- Response: health status for PostgreSQL (pool active/idle/max, query p95, `SELECT 1` probe), Valkey (PING probe), S3 (bucket access probe, graceful if unconfigured), OTel pipeline (export failure rate, dropped spans count, collector endpoint)
- Implementation: cached health probes, refreshed every 10s
- Performance target: p95 <= 100ms (Design §16)

### 12. Logs endpoint

- Files:
  - `server/src/admin/observability.rs`
- Endpoint: `GET /api/admin/observability/logs?level=&domain=&service=&from=&to=&search=&cursor=&limit=`
- Response: paginated log entries (cursor-based), max 100 per page
- Filters: severity (WARN/ERROR), domain, service, time range, free-text search on event/message
- Performance target: p95 <= 200ms (Design §16)

### 13. Trace index endpoint

- Files:
  - `server/src/admin/observability.rs`
- Endpoint: `GET /api/admin/observability/traces?status=&domain=&route=&duration_min=&from=&to=&cursor=&limit=`
- Response: paginated trace index entries (cursor-based), max 100 per page
- Pre-filtered views: "Recent errors" (status >= 500), "Recent slow" (duration > p95 threshold)
- Performance target: p95 <= 200ms (Design §16)

### 14. Export endpoint

- Files:
  - `server/src/admin/observability.rs`
- Endpoint: `GET /api/admin/observability/export?type=logs|traces|metrics&format=csv|json&...filters`
- Response: streamed file download of filtered data; max 10,000 rows per export
- Implementation: streaming response (not buffered in memory); same filter params as respective list endpoints
- Performance target: p95 <= 5s streaming (Design §16)

### 15. External links and config endpoints

- Files:
  - `server/src/admin/observability.rs`
  - `server/src/config.rs`
- Endpoints:
  - `GET /api/admin/observability/links` — returns configured external tool URLs (Grafana, Tempo, Loki, Prometheus) or empty object if not configured
  - `GET /api/admin/observability/config` — returns threshold values (Design §8.1), refresh cadences, feature flags for v1.1 features
- Config: threshold values read from environment variables with `COMMAND_CENTER_` prefix (Design §8.2)

### 16. Audit and security hooks

- Files:
  - `server/src/admin/observability.rs`
  - `server/src/permissions/queries.rs`
- Tasks:
  - Record `admin.observability.view` audit event on every observability endpoint access
  - Verify `SystemAdminUser` middleware is applied to all routes (elevated session NOT required for read-only access)
  - Confirm no PII, user IDs, IPs, message content, or credentials reach any native telemetry table

---

## Phase 3 — Chart Library Installation

*Design references: §17 (Chart Library Decision)*

The client currently has no chart or visualization library. This phase installs the chosen library before building any chart components.

### 17. Install chart.js and solid-chartjs

- Files:
  - `client/package.json`
  - `client/src/lib/charts.ts` (new — thin wrapper/config)
- Tasks:
  - Run `bun add chart.js solid-chartjs` in `client/`
  - Verify MIT license compliance with `cargo deny check licenses` equivalent for frontend (check `bun pm ls`)
  - Create `client/src/lib/charts.ts`: export pre-configured Chart.js defaults (font, color palette matching CSS variables, no animation on initial load for performance)
  - Confirm bundle size impact is acceptable (~65KB gzip per Design §17)
  - Add chart.js to `client/src/lib/tauri.ts` dual-mode awareness if any chart data fetching differs between Tauri and browser dev mode

---

## Phase 4 — Client Admin Panel

*Design references: §6 (Signal Taxonomy), §7 (Layout and Information Hierarchy), §8 (Status Thresholds), §9.2 (Polling Cadence)*

### 18. Add sidebar entry and panel routing

- Files:
  - `client/src/components/admin/AdminSidebar.tsx`
  - `client/src/views/AdminDashboard.tsx`
- Tasks:
  - Add `command-center` panel ID and nav entry to admin sidebar
  - Route panel rendering in admin dashboard to `CommandCenterPanel`

### 19. Add store and API client methods

- Files:
  - `client/src/stores/admin.ts`
  - `client/src/lib/tauri.ts`
- Tasks:
  - Add state and actions for: summary, trends, top-routes, top-errors, voice summary, voice quality, infrastructure, logs, traces, config, links
  - Add polling cadence per Design §9.2: trends/traces 30s, logs 15s, infrastructure 10s
  - Add freshness timestamps per section; mark stale if > 2x expected interval
  - Add time range state (1h/6h/24h/7d/30d, default 24h) shared across all trend panels
  - Add optional external deep-link actions (open in Grafana/Tempo/Loki)

### 20. Build Health Overview (Tier 1 — always visible)

- Files:
  - `client/src/components/admin/CommandCenterPanel.tsx` (new)
  - `client/src/components/admin/VitalSignsCards.tsx` (new)
  - `client/src/components/admin/ServiceHealthMatrix.tsx` (new)
  - `client/src/components/admin/LiveErrorFeed.tsx` (new)
  - `client/src/components/admin/index.ts`
- Tasks:
  - Four vital-sign cards (one per golden signal): current value, sparkline (1h), threshold color (green/yellow/red per Design §8.1)
  - Service health matrix: 7 services (HTTP API, WebSocket Hub, Voice SFU, PostgreSQL, Valkey, S3, OTel Pipeline) with status dots
  - Live error feed: last 25 errors, cross-service, scrolling; fields: timestamp, service, domain, event, message (truncated), trace_id link; "View all logs" link
  - Server metadata strip: version, uptime, environment, active users, guild count
  - Layout: health overview always above the fold per Design §7.2; 2-column (health matrix + error feed) on desktop, single column on tablet

### 21. Build Golden Signals tab (Tier 2)

- Files:
  - `client/src/components/admin/GoldenSignalsTab.tsx` (new)
- Tasks:
  - Time-series line charts for Latency panel: API p50/p95/p99, DB query p50/p95, voice session duration (Design §6.2.1)
  - Time-series area/line charts for Traffic panel: API req/s, active WS connections, active voice sessions, WS messages, RTP packets (Design §6.2.2)
  - Time-series stacked area for Errors panel: HTTP 4xx/5xx, auth failures, token refresh failures, WS reconnects, voice join failures, OTel export failures (Design §6.2.3)
  - Gauge + time-series for Saturation panel: DB pool utilization, idle connections, OTel dropped spans, process memory RSS (Design §6.2.4)
  - Top slow routes table (top 10 by p95 in selected range)
  - Top failing routes table (top 10 by error count)
  - Top error categories table (grouped by `error.type`)
  - Incident correlation toggle: overlay error rate on metric chart, log event markers as vertical lines (Design §10.1)
  - Note: never show averages for latency — always p50/p95/p99 per Design §6.2.1

### 22. Build Voice tab (Tier 3)

- Files:
  - `client/src/components/admin/VoiceTab.tsx` (new)
- Tasks:
  - Voice health score gauge (0-100 composite, prominent display)
  - Active sessions and active rooms as large number cards with sparklines
  - Join success rate percentage + trend chart
  - Packet loss p50/p95 time-series line chart
  - Network jitter p50/p95 time-series line chart
  - Session latency p50/p95 time-series line chart
  - Quality score distribution stacked bar chart (poor/fair/good/excellent)
  - RTP forwarding rate time-series
  - Session duration distribution histogram
  - Participant distribution histogram (participants per room)

### 23. Build Infrastructure tab (Tier 4)

- Files:
  - `client/src/components/admin/InfrastructureTab.tsx` (new)
- Tasks:
  - Database section: pool active/idle/max gauge bars, query p95 value + sparkline, query rate time-series, top slow queries table (span_name, duration, count), connectivity status badge
  - Valkey section: connectivity status badge, usage info text
  - S3 section: connectivity status badge, configured/not-configured/degraded status
  - OTel pipeline section: export success/failure rate time-series, dropped spans counter (alert if > 0), collector endpoint info, pipeline status badge
  - Poll every 10s per Design §9.2

### 24. Build Logs tab (Tier 5)

- Files:
  - `client/src/components/admin/LogsTab.tsx` (new)
- Tasks:
  - Filterable log table: severity, domain, service, time range, free-text search
  - Columns: severity, timestamp, service, domain, event, message (truncated to 200 chars), trace_id (clickable)
  - Click to expand full error context
  - Cursor-based pagination, max 100 per page
  - Export button (CSV/JSON of current filtered view)
  - Click on trace_id navigates to Traces tab filtered by that trace_id (Design §10.2)

### 25. Build Traces tab (Tier 6)

- Files:
  - `client/src/components/admin/TracesTab.tsx` (new)
- Tasks:
  - Filterable trace index table: status (error/slow/all), domain, route, duration threshold, time range
  - Columns: trace_id (clickable), span_name, route, domain, status, duration (color-coded by threshold), timestamp
  - Pre-filtered quick views: "Recent errors" and "Recent slow"
  - Cursor-based pagination, max 100 per page
  - Export button (CSV/JSON of current filtered view)
  - "Open in Tempo" button per row when external Tempo URL is configured (Design §10.2)

### 26. Time range picker and auto-refresh

- Files:
  - `client/src/components/admin/TimeRangePicker.tsx` (new)
  - `client/src/components/admin/CommandCenterPanel.tsx`
- Tasks:
  - Global time range selector: presets 1h/6h/24h/7d/30d, custom range with calendar picker
  - Auto-refresh toggle with cadence indicator ("Refreshing every 10s")
  - Applies to all trend charts and tables in active tab
  - Tab state persists across panel switches

### 27. Degraded and empty state handling

- Files:
  - `client/src/components/admin/CommandCenterPanel.tsx`
  - All tab components
- Tasks:
  - "Collecting data..." placeholder for first 60s after server start (Design §15)
  - Degraded badges when external stack is unavailable
  - Stale data indicator: "Last updated: Xs ago" turns yellow with "(stale)" suffix if > 2x expected interval (Design §9.3)
  - Empty state for each panel when no data exists yet

---

## Phase 5 — Real-Time and Correlation

*Design references: §9 (Real-Time Update Strategy), §9.1 (WebSocket Push Events), §10 (Incident Correlation)*

### 28. Add WebSocket push event types

- Files:
  - `shared/vc-common/src/protocol/` (add new `admin.observability.*` event variants)
  - `server/src/ws/` (event dispatch logic)
- Tasks:
  - Add four new event types (Design §9.1):
    - `admin.observability.health_change` — payload: `{ service, old_status, new_status }`
    - `admin.observability.error_event` — payload: `{ ts, service, domain, event, message, trace_id }`
    - `admin.observability.vital_signs` — payload: `{ latency_p95, traffic_rate, error_rate, saturation_pct }`
    - `admin.observability.voice_pulse` — payload: `{ active_sessions, active_rooms, join_success_rate_1h }`
  - Note: protocol changes are BREAKING per project conventions; coordinate with any in-flight WS work

### 29. Implement server-side push dispatch

- Files:
  - `server/src/ws/` (dispatch logic)
  - `server/src/observability/` (health monitor)
- Tasks:
  - Health monitor: detect service state transitions (healthy/degraded/down) and emit `health_change` events
  - Error ingestion: emit `error_event` on each new ERROR-level log written to `telemetry_log_events`
  - Vital signs broadcaster: emit `vital_signs` every 5s to admin connections with command center active
  - Voice pulse broadcaster: emit `voice_pulse` every 5s to admin connections with voice tab active
  - Events sent ONLY to authenticated system admin WebSocket connections (Design §13.3)
  - Frontend sends subscription message when command center panel becomes active; server tracks subscription state

### 30. Wire real-time updates in client

- Files:
  - `client/src/stores/admin.ts`
  - `client/src/components/admin/LiveErrorFeed.tsx`
  - `client/src/components/admin/ServiceHealthMatrix.tsx`
  - `client/src/components/admin/VitalSignsCards.tsx`
- Tasks:
  - Subscribe to `admin.observability.*` events on panel mount; unsubscribe on unmount
  - Live error feed: auto-scroll on new `error_event` push
  - Service health matrix: update status dots on `health_change` push without full re-fetch
  - Vital signs cards: update values on `vital_signs` push (5s cadence)
  - Voice tab active sessions/rooms: update on `voice_pulse` push

### 31. Implement incident correlation

- Files:
  - `client/src/components/admin/GoldenSignalsTab.tsx`
  - `client/src/components/admin/LogsTab.tsx`
  - `client/src/components/admin/TracesTab.tsx`
- Tasks:
  - Incident View toggle on Golden Signals tab: overlay error rate on selected metric chart (secondary y-axis), add vertical line markers at ERROR log timestamps (Design §10.1)
  - Click-through from live error feed to Logs tab filtered by trace_id (Design §10.2)
  - Click-through from log entry with trace_id to Traces tab filtered by trace_id
  - Click on chart data point opens Logs tab with time filter set to that point's window
  - "Open in Tempo" from trace index when external Tempo URL configured

---

## Phase 6 — Quality Gates

*Design references: §18 Phase 5 (Quality Gates), §19 (Success Criteria)*

### 32. Backend integration tests

- Files:
  - `server/tests/observability_auth.rs` (new)
  - `server/tests/observability_bounds.rs` (new)
  - `server/tests/observability_redaction.rs` (new)
- Tasks:
  - Auth tests: all `/api/admin/observability/*` endpoints return 403 for non-admin users
  - Bounds tests: pagination max (100), time range max (30d), export max (10,000 rows) enforced
  - Retention tests: rows older than 30 days are deleted by retention job
  - Redaction tests: forbidden fields from observability contract never appear in `telemetry_log_events.attrs` or `telemetry_metric_samples.labels`
  - Label allowlist tests: non-allowlisted labels are silently dropped at ingestion

### 33. Frontend component tests

- Files:
  - `client/src/components/admin/CommandCenterPanel.test.tsx` (new)
  - `client/src/components/admin/VoiceTab.test.tsx` (new)
  - `client/src/components/admin/InfrastructureTab.test.tsx` (new)
  - `client/src/components/admin/LogsTab.test.tsx` (new)
- Tasks:
  - Panel rendering with mock data for each tab
  - Degraded state rendering (empty data, stale indicators)
  - Filter interaction tests for logs and traces tabs
  - Threshold color coding tests (green/yellow/red per Design §8.1)

### 34. E2E smoke tests

- Files:
  - `client/e2e/admin-command-center.spec.ts` (new)
- Tasks:
  - System admin can open command center and see health data within 5 seconds
  - Health overview shows service matrix and vital signs
  - Tab navigation works (Golden Signals, Voice, Infrastructure, Logs, Traces)
  - Export button triggers file download for logs view

### 35. CI governance checks

- Files:
  - `.github/workflows/ci.yml`
  - `scripts/check_docs_governance.py`
- Tasks:
  - Check retention policy constants (30-day max) in migration files
  - Check forbidden telemetry fields are not referenced in native schema columns
  - Verify `cargo deny check licenses` passes with new chart.js dependency (frontend)
  - Ensure roadmap links to active design and implementation docs

---

## Verification Strategy

### Functional

- Summary/trends/top/logs/traces/voice/infrastructure endpoints return expected bounded results.
- Admin UI shows cluster-wide telemetry for all system admins.
- All 7 services appear in the health matrix with correct status values.
- Golden signals are organized as Latency/Traffic/Errors/Saturation with time-series charts.
- Voice operations tab shows session quality, join success, packet loss, jitter trends.
- Infrastructure tab shows DB pool, Valkey, S3, OTel pipeline health.
- Logs are WARN/ERROR only, filterable, with trace ID correlation.
- Trace index is searchable with "Open in Tempo" when external stack configured.
- Export to CSV/JSON works for logs, traces, and metrics views.
- Degraded mode works without external stack.

### Privacy/Security

- No forbidden fields stored in `telemetry_log_events`, `telemetry_trace_index`, or `telemetry_metric_samples`.
- No PII, user IDs, IPs, message content, or credentials in any native telemetry table.
- Access is restricted to system admins only (403 for all other roles).
- Observability view actions are audit-logged as `admin.observability.view`.
- WebSocket push events sent only to authenticated system admin connections.

### Performance

- Summary endpoint p95 <= 50ms under expected load.
- Trends endpoint p95 <= 200ms (24h range), <= 500ms (30d range).
- Top offenders endpoint p95 <= 200ms.
- Voice summary endpoint p95 <= 100ms.
- Infrastructure endpoint p95 <= 100ms.
- Logs/traces list endpoints p95 <= 200ms.
- Export endpoint p95 <= 5s (streaming).
- Polling does not regress server CPU/memory targets.
- Ingestion jobs are async and do not block hot path.

### Operability

- Retention purge and rollup refresh jobs run successfully every hour.
- Data freshness indicators remain accurate; stale detection triggers at 2x expected interval.
- "Collecting data..." placeholder appears for first 60s after server start.
- Degraded badges appear correctly when backing services are unavailable.
- Background retention job logs execution time and rows deleted.

---

## Done Criteria

- [ ] All system admins can open cluster-wide command center and see current health signals within 5 seconds.
- [ ] Health overview answers "is something wrong?" with service matrix, vital signs, and live error feed.
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
- [ ] chart.js + solid-chartjs installed and MIT license verified.
- [ ] CI/governance checks enforce retention and redaction constraints.
- [ ] Documentation updated (roadmap, runbook, contract references).

---

## Risk Register

1. **Cardinality growth risk**
   - Mitigation: strict label allowlist, max 100 unique label combinations per metric, bounded query limits.

2. **Storage growth risk**
   - Mitigation: 30-day hard TTL, hourly retention job, estimated ~500MB/month at moderate load (1000 DAU) per Design §14.

3. **Scope creep into full observability platform**
   - Mitigation: enforce non-goals; route deep forensics to external stack; v1.1 features gated behind config flags.

4. **Dependency drift to stale tooling**
   - Mitigation: maintenance policy gate, pinned versions, chart.js chosen for active maintenance (60K+ stars, MIT).

5. **WebSocket protocol change coordination**
   - Mitigation: new `admin.observability.*` event types are additive; coordinate with any in-flight WS work; protocol changes are BREAKING per project conventions.

6. **TimescaleDB availability**
   - Mitigation: graceful fallback to standard PostgreSQL with scheduled DELETE; hypertable creation wrapped in conditional migration.
