# Phase 7 Admin Command Center — Task Plan

**Date:** 2026-02-27
**Status:** Not Started
**Lifecycle:** Active
**Roadmap Reference:** `docs/project/roadmap.md` (Phase 7, `[Ops] Admin Command Center (Native Observability Lite)`)
**Design Reference:** `docs/plans/2026-02-27-phase-7-admin-command-center-design.md`
**Implementation Reference:** `docs/plans/2026-02-27-phase-7-admin-command-center-implementation.md`

## Objective

Deliver a cluster-wide native Command Center for system admins organized around the Four Golden Signals framework, with strict 30-day telemetry retention, real-time WebSocket push, voice operations monitoring, infrastructure health, and incident correlation. Deep forensic analysis delegates to external Grafana/Tempo/Loki/Prometheus when configured.

## Delivery Constraints

- Native retention hard cap: 30 days.
- Cluster-wide visibility for all system admins.
- Open-source, actively maintained tooling only.
- Preserve OTel standards and current observability contract.
- No full in-app trace waterfall or advanced log query DSL in v1.

---

## Task Breakdown (Atomic)

### Phase 0 — Project Guardrails

- [ ] **T-01: Confirm contract and roadmap alignment** (0.5d)
  - Files:
    - `docs/ops/observability-contract.md`
    - `docs/project/roadmap.md`
    - `docs/plans/2026-02-27-phase-7-admin-command-center-design.md`
  - Done when: all metric names, label allowlists, redaction rules, and retention/access/scope decisions in the design doc are explicitly cross-referenced against the observability contract with no conflicts.

- [ ] **T-02: Define dependency policy checklist** (0.5d)
  - Files:
    - `docs/plans/2026-02-27-phase-7-admin-command-center-implementation.md`
    - `Cargo.toml` (workspace)
    - `client/package.json`
  - Done when: maintenance, licensing, and version policy for all new dependencies (chart.js, solid-chartjs, sysinfo or proc-based memory crate) is documented and enforceable via `cargo deny`.

**Phase 0 subtotal: ~1.0d**

---

### Phase 1 — Data Foundation

- [ ] **T-03: Create native telemetry schema migration** (1.0d)
  - Files:
    - `server/migrations/<timestamp>_command_center_telemetry.sql`
  - Includes:
    - `telemetry_metric_samples` hypertable (ts, metric_name, scope, labels JSONB, value_count, value_sum, value_p50, value_p95, value_p99)
    - `telemetry_log_events` table (id, ts, level, service, domain, event, message, trace_id, span_id, attrs JSONB)
    - `telemetry_trace_index` table (trace_id, span_name, domain, route, status_code, duration_ms, ts, service)
    - All indexes per design Section 11
  - Done when: migration applies cleanly on both TimescaleDB and plain PostgreSQL 16, and all indexes exist.

- [ ] **T-04: Create `telemetry_trend_rollups` materialized view** (0.5d)
  - Files:
    - `server/migrations/<timestamp>_command_center_rollups.sql`
  - Includes:
    - Materialized view aggregating daily rollups (day, metric_name, scope, route, sample_count, avg_p95, max_p95, total_count, error_count)
    - Unique index `idx_ttr_day_metric` on (day, metric_name, scope, route)
  - Done when: view creates without error, `REFRESH MATERIALIZED VIEW CONCURRENTLY` succeeds, and a 7d/30d trend query returns in < 500ms on test data.

- [ ] **T-05: Add retention and downsampling background job** (1.0d)
  - Files:
    - `server/src/observability/retention.rs` (new)
    - `server/src/observability/mod.rs`
  - Includes:
    - Hourly job: hard-delete rows older than 30 days from all three tables
    - Hourly job: `REFRESH MATERIALIZED VIEW CONCURRENTLY telemetry_trend_rollups`
    - TimescaleDB `drop_chunks` path with plain-PostgreSQL `DELETE` fallback
    - Logs execution time and rows deleted to tracing output
  - Done when: job runs on schedule, deletes correct rows, and completes in < 10s on test data.

- [ ] **T-06: Add storage/query module** (1.0d)
  - Files:
    - `server/src/observability/storage.rs` (new)
    - `server/src/observability/mod.rs`
  - Includes:
    - Write helpers: `insert_metric_sample`, `insert_log_event`, `insert_trace_span`
    - Read helpers: `query_summary`, `query_trends`, `query_top_routes`, `query_top_errors`, `query_logs`, `query_traces`
    - Live query path (ranges <= 24h) vs. materialized view path (7d/30d)
    - All queries bounded by max page size and required time filters
  - Done when: all helpers compile, have unit tests with in-memory or test-DB fixtures, and meet response time targets from design Section 16.

- [ ] **T-07: Add `kaiku_process_memory_bytes` metric collection** (0.5d)
  - Files:
    - `server/src/observability/metrics.rs`
    - `Cargo.toml` (add `sysinfo` or read `/proc/self/statm` directly)
  - Includes:
    - RSS memory gauge sampled every 60s
    - Metric name `kaiku_process_memory_bytes` per observability contract
    - Threshold color logic: < 70% of configured limit = green, 70-90% = yellow, > 90% = red
  - Done when: metric appears in metric samples table and threshold status is computed correctly.

**Phase 1 subtotal: ~4.0d**

---

### Phase 2 — Ingestion and Safety

- [ ] **T-08: Wire native log ingestion with redaction** (1.0d)
  - Files:
    - `server/src/observability/tracing.rs`
    - `server/src/observability/storage.rs`
  - Includes:
    - Tracing subscriber layer that captures WARN/ERROR events
    - Applies all redaction rules from observability contract Section 9 before storage
    - Persists only allowlisted `attrs` fields
    - INFO logs are explicitly dropped (not stored)
  - Done when: WARN/ERROR events appear in `telemetry_log_events`, INFO events do not, and no forbidden fields appear in `attrs`.

- [ ] **T-09: Wire native trace index ingestion** (0.75d)
  - Files:
    - `server/src/observability/tracing.rs`
    - `server/src/observability/storage.rs`
  - Includes:
    - Span completion hook that writes to `telemetry_trace_index`
    - Stores only metadata (trace_id, span_name, domain, route, status_code, duration_ms, ts, service)
    - No span payload, attributes, or events stored
  - Done when: failed and slow spans appear in trace index without any payload content.

- [ ] **T-10: Wire metric sample ingestion with label guardrails** (1.0d)
  - Files:
    - `server/src/observability/metrics.rs`
    - `server/src/observability/storage.rs`
  - Includes:
    - Async task that aggregates in-memory metric state every 60s and inserts rows
    - Label allowlist enforcement: only contract Section 6 labels stored in `labels` JSONB
    - Cardinality budget: max 100 unique label combinations per metric; excess logged as warnings
    - Route label normalization: parameterised templates only (e.g., `/api/v1/guilds/{guild_id}`)
    - Pre-computes p50/p95/p99 from OTel histogram data points at ingestion
  - Done when: only allowlisted labels persist, high-cardinality combinations are dropped with a warning log, and percentiles are correct.

- [ ] **T-11: Add voice health score computation** (0.75d)
  - Files:
    - `server/src/observability/voice.rs` (new)
    - `server/src/observability/mod.rs`
  - Includes:
    - Composite score (0-100): join success rate 40%, packet loss p95 30%, jitter p95 20%, session crash rate 10%
    - Reads from `connection_metrics` and `connection_sessions` TimescaleDB hypertables
    - Cached in memory, refreshed every 10s
  - Done when: score is computed correctly from test data and updates on schedule.

- [ ] **T-12: Add observability access audit events** (0.5d)
  - Files:
    - `server/src/admin/handlers.rs`
    - `server/src/observability/mod.rs`
  - Includes:
    - `admin.observability.view` audit event emitted on each command center endpoint access
    - Records admin user ID, endpoint, and timestamp
  - Done when: audit events appear in the audit log for every observability endpoint call.

**Phase 2 subtotal: ~4.0d**

---

### Phase 3 — Admin API Surface

- [ ] **T-13: Add summary endpoint** (0.75d)
  - Files:
    - `server/src/admin/observability.rs` (new)
    - `server/src/admin/mod.rs`
    - `server/src/admin/types.rs`
  - Endpoint: `GET /api/admin/observability/summary`
  - Returns: vital signs (4 golden signal values), service health matrix (7 services), server metadata (version, uptime, environment, active users, guild count), active alert count
  - Done when: response time < 50ms, all 7 services represented, and `SystemAdminUser` middleware enforced.

- [ ] **T-14: Add trends endpoint** (1.0d)
  - Files:
    - `server/src/admin/observability.rs`
    - `server/src/admin/types.rs`
  - Endpoint: `GET /api/admin/observability/trends?range=1h|6h|24h|7d|30d&metric=<name>`
  - Returns: time-series data points for requested metric(s)
  - Routes to live query (ranges <= 24h) or materialized view (7d/30d)
  - Done when: response time < 200ms for 24h, < 500ms for 30d, and invalid metric names return 400.

- [ ] **T-15: Add top-offenders endpoints** (0.75d)
  - Files:
    - `server/src/admin/observability.rs`
  - Endpoints:
    - `GET /api/admin/observability/top-routes?range=...&sort=latency|errors&limit=10`
    - `GET /api/admin/observability/top-errors?range=...&limit=10`
  - Returns: ranked lists with route, count, p95, error rate
  - Done when: both endpoints return correct rankings and respect the `limit` bound.

- [ ] **T-16: Add voice summary and quality endpoints** (1.0d)
  - Files:
    - `server/src/admin/observability.rs`
    - `server/src/observability/voice.rs`
  - Endpoints:
    - `GET /api/admin/observability/voice/summary` — active sessions, rooms, join success rate, voice health score
    - `GET /api/admin/observability/voice/quality?range=...` — packet loss, jitter, latency time-series from TimescaleDB `connection_metrics`
  - Done when: summary response time < 100ms, quality time-series returns p50/p95 for all three signals, and empty state handled when no voice data exists.

- [ ] **T-17: Add infrastructure health endpoint** (0.75d)
  - Files:
    - `server/src/admin/observability.rs`
  - Endpoint: `GET /api/admin/observability/infrastructure`
  - Returns: health status for DB (pool active/idle/max, query p95, connectivity), Valkey (PING), S3 (bucket access), OTel pipeline (export failure rate, dropped spans)
  - Done when: response time < 100ms, all four services represented, and graceful degradation when S3 is not configured.

- [ ] **T-18: Add logs and trace-index list endpoints** (1.0d)
  - Files:
    - `server/src/admin/observability.rs`
  - Endpoints:
    - `GET /api/admin/observability/logs?level=&domain=&service=&from=&to=&search=&cursor=&limit=`
    - `GET /api/admin/observability/traces?status=&domain=&route=&duration_min=&from=&to=&cursor=&limit=`
  - Cursor-based pagination, max 100 per page, required time filters
  - Done when: filters work correctly, cursor pagination is stable, and response time < 200ms.

- [ ] **T-19: Add export endpoint** (1.0d)
  - Files:
    - `server/src/admin/observability.rs`
  - Endpoint: `GET /api/admin/observability/export?type=logs|traces|metrics&format=csv|json&...filters`
  - Streamed response, max 10,000 rows per export
  - Done when: CSV and JSON formats both work, streaming starts within 1s, and row limit is enforced.

- [ ] **T-20: Add config and external links endpoints** (0.5d)
  - Files:
    - `server/src/admin/observability.rs`
  - Endpoints:
    - `GET /api/admin/observability/config` — threshold values, refresh cadences, feature flags
    - `GET /api/admin/observability/links` — configured external tool URLs (Grafana, Tempo, Loki, Prometheus)
  - Done when: config returns all `COMMAND_CENTER_*` threshold env vars with defaults, and links returns empty object when not configured.

**Phase 3 subtotal: ~6.75d**

---

### Phase 4 — WebSocket Push Events

- [ ] **T-21: Add `admin.observability.*` WebSocket event types** (1.0d)
  - Files:
    - `shared/vc-common/src/protocol/server_events.rs`
    - `shared/vc-common/src/lib.rs`
  - Includes four new event variants:
    - `admin.observability.health_change` — `{ service, old_status, new_status }`
    - `admin.observability.error_event` — `{ ts, service, domain, event, message, trace_id }`
    - `admin.observability.vital_signs` — `{ latency_p95, traffic_rate, error_rate, saturation_pct }`
    - `admin.observability.voice_pulse` — `{ active_sessions, active_rooms, join_success_rate_1h }`
  - Done when: all four variants compile, are serializable, and are documented as breaking protocol additions.

- [ ] **T-22: Add server-side WS push dispatcher** (1.0d)
  - Files:
    - `server/src/ws/admin_push.rs` (new)
    - `server/src/ws/mod.rs`
    - `server/src/observability/mod.rs`
  - Includes:
    - Subscription tracking: events sent only to authenticated system admin connections with command center active
    - `health_change` triggered on service state transitions
    - `error_event` triggered on new ERROR log ingestion
    - `vital_signs` pushed every 5s while any admin has command center open
    - `voice_pulse` pushed every 5s while any admin has voice tab open
  - Done when: events reach only admin connections, non-admin connections receive nothing, and push cadence matches design Section 9.1.

**Phase 4 subtotal: ~2.0d**

---

### Phase 5 — Client Admin Panel

- [ ] **T-23: Install chart library** (0.5d)
  - Files:
    - `client/package.json`
    - `client/src/lib/charts.ts` (new — thin wrapper/re-export)
  - Installs: `chart.js` + `solid-chartjs` (MIT licensed)
  - Done when: `cargo deny check licenses` passes, `bun run build` succeeds, and a minimal line chart renders in a test component.

- [ ] **T-24: Add command-center panel routing and sidebar entry** (0.5d)
  - Files:
    - `client/src/components/admin/AdminSidebar.tsx`
    - `client/src/views/AdminDashboard.tsx`
  - Done when: "Command Center" entry appears in admin sidebar, clicking it renders the panel, and tab state persists across panel switches.

- [ ] **T-25: Create `CommandCenterPanel` skeleton with layout** (1.0d)
  - Files:
    - `client/src/components/admin/CommandCenterPanel.tsx` (new)
    - `client/src/components/admin/index.ts`
  - Implements the wireframe from design Section 7:
    - Top row: four vital-sign cards (always visible)
    - Middle row: service health matrix + live error feed (2-column)
    - Tab bar: Golden Signals / Voice / Infrastructure / Logs / Traces
    - "Collecting data..." placeholder for first 60s after server start
  - Done when: layout matches wireframe, tab switching works, and responsive behavior handles tablet width (768-1200px).

- [ ] **T-26: Implement time range picker component** (0.75d)
  - Files:
    - `client/src/components/admin/TimeRangePicker.tsx` (new)
  - Includes:
    - Presets: 1h / 6h / 24h / 7d / 30d (default: 24h)
    - Auto-refresh toggle with cadence indicator ("Refreshing every 10s")
    - Applies to all trend charts and tables in the active tab
  - Done when: selecting a preset updates all charts, auto-refresh fires on schedule, and selected range persists when switching tabs.

- [ ] **T-27: Implement admin store slice and API bindings** (1.5d)
  - Files:
    - `client/src/stores/admin.ts`
    - `client/src/lib/tauri.ts`
  - Includes:
    - `createStore` slice for command center state (summary, trends, voice, infra, logs, traces)
    - Fetch functions for all 10 observability endpoints
    - WS event handlers for all four `admin.observability.*` event types
    - Polling timers per design Section 9.2 cadences (10s/15s/30s by data type)
    - Freshness tracking: last-updated timestamp per section
  - Done when: store updates correctly from both polling and WS push, and stale detection triggers at 2x expected interval.

- [ ] **T-28: Implement vital-sign cards with sparklines** (1.0d)
  - Files:
    - `client/src/components/admin/VitalSignCard.tsx` (new)
  - Includes:
    - Four cards: Latency (p95), Traffic (req/s), Errors (rate), Saturation (max %)
    - Sparkline trend (1h) using chart.js
    - Threshold color coding (green/yellow/red) from config endpoint
    - Updates via `admin.observability.vital_signs` WS push
  - Done when: all four cards render with correct values, sparklines animate on update, and threshold colors match config.

- [ ] **T-29: Implement service health matrix** (0.75d)
  - Files:
    - `client/src/components/admin/ServiceHealthMatrix.tsx` (new)
  - Includes:
    - All 7 services: HTTP API, WebSocket Hub, Voice SFU, PostgreSQL, Valkey, S3, OTel Pipeline
    - Status dot: healthy (green) / degraded (yellow) / down (red) / unavailable (gray)
    - Updates via `admin.observability.health_change` WS push
  - Done when: all 7 services shown, status transitions animate, and S3 shows "unavailable" (not "down") when not configured.

- [ ] **T-30: Implement live error feed component** (1.0d)
  - Files:
    - `client/src/components/admin/LiveErrorFeed.tsx` (new)
  - Includes:
    - Scrolling list of last 25 errors (cross-service)
    - Fields: timestamp, service, domain, event name, message (truncated), trace_id link
    - Auto-scrolls on new entries via `admin.observability.error_event` WS push
    - Click to expand full error context
    - "View all logs" link navigates to Logs tab
  - Done when: feed auto-scrolls on new WS events, click-through to Logs tab works with trace_id filter pre-applied, and list caps at 25 entries.

- [ ] **T-31: Implement Golden Signals tab (charts and tables)** (1.5d)
  - Files:
    - `client/src/components/admin/GoldenSignalsTab.tsx` (new)
    - `client/src/components/admin/TrendChart.tsx` (new)
  - Includes:
    - Latency panel: p50/p95/p99 line chart, DB query latency chart, top slow routes table
    - Traffic panel: API req rate area chart, active WS connections, active voice sessions, WS messages stacked area
    - Errors panel: HTTP 4xx/5xx stacked area, auth failure rate, WS reconnect rate, voice join failure rate, OTel export failures
    - Saturation panel: DB pool gauge + time-series, OTel dropped spans counter, process memory RSS time-series
    - All charts respect time range picker selection
    - Top offenders tables (top 10 routes by p95, top 10 by error count)
  - Done when: all four signal panels render with correct chart types, tables sort correctly, and charts update when time range changes.

- [ ] **T-32: Implement Voice tab** (1.5d)
  - Files:
    - `client/src/components/admin/VoiceTab.tsx` (new)
  - Includes:
    - Voice health score gauge (0-100, prominent)
    - Active sessions + active rooms large-number cards (updated via `admin.observability.voice_pulse`)
    - Join success rate percentage + trend chart
    - Packet loss p50/p95 time-series line chart
    - Network jitter p50/p95 time-series line chart
    - Session latency p50/p95 time-series line chart
    - Quality score distribution stacked bar chart (poor/fair/good/excellent)
    - RTP forwarding rate area chart
    - Session duration histogram
  - Done when: all charts render from voice quality endpoint data, health score gauge updates every 10s, and empty state shows when no voice sessions exist.

- [ ] **T-33: Implement Infrastructure tab** (1.0d)
  - Files:
    - `client/src/components/admin/InfrastructureTab.tsx` (new)
  - Includes:
    - Database section: pool active/idle/max gauge bars, query p95 sparkline, query rate time-series, top slow queries table, connectivity badge
    - Valkey section: connectivity badge, usage info text
    - S3 section: connectivity badge, configured/not-configured/degraded status
    - OTel pipeline section: export success/failure rate time-series, dropped spans counter (alert if > 0), collector endpoint info, pipeline status badge
  - Done when: all four service sections render, S3 section handles unconfigured state gracefully, and OTel dropped spans shows alert styling when count > 0.

- [ ] **T-34: Implement Logs tab** (1.0d)
  - Files:
    - `client/src/components/admin/LogsTab.tsx` (new)
  - Includes:
    - Filterable log list: severity, domain, service, time range, free-text search
    - Cursor-based pagination (max 100 per page)
    - Trace ID column links to Traces tab with filter pre-applied
    - Export button (CSV/JSON) for current filtered view
    - New ERROR entries auto-appended via WS push
  - Done when: all filters work, pagination navigates correctly, trace_id click-through pre-filters Traces tab, and export downloads a valid file.

- [ ] **T-35: Implement Traces tab** (1.0d)
  - Files:
    - `client/src/components/admin/TracesTab.tsx` (new)
  - Includes:
    - Filterable trace index: status (error/slow/all), domain, route, duration threshold, time range
    - Pre-filtered views: "Recent errors" (status >= 500), "Recent slow" (duration > p95 threshold)
    - Cursor-based pagination (max 100 per page)
    - "Open in Tempo" button per row (visible only when Tempo URL configured)
    - Export button (CSV/JSON) for current filtered view
    - Duration column color-coded by threshold
  - Done when: pre-filtered views work, "Open in Tempo" appears only when configured, and export downloads a valid file.

- [ ] **T-36: Implement incident correlation view** (1.5d)
  - Files:
    - `client/src/components/admin/IncidentCorrelationView.tsx` (new)
    - `client/src/components/admin/GoldenSignalsTab.tsx`
  - Includes:
    - "Incident View" toggle on Golden Signals tab
    - Shared time axis overlaying: selected metric (primary), error rate (secondary y-axis), log event markers (vertical lines at ERROR timestamps), deploy markers (dashed vertical lines, v1.1 placeholder)
    - Click on chart point opens Logs tab with time filter for that window
    - Click on error in live feed navigates to Logs tab filtered by trace_id
    - Click on log entry with trace_id navigates to Traces tab filtered by trace_id
  - Done when: incident view toggle works, all three overlays render on shared time axis, and all three click-through navigations land on the correct tab with correct filters pre-applied.

- [ ] **T-37: Implement degraded mode and freshness indicators** (0.75d)
  - Files:
    - `client/src/components/admin/CommandCenterPanel.tsx`
    - `client/src/components/admin/FreshnessIndicator.tsx` (new)
  - Includes:
    - "Last updated: Xs ago" indicator on every panel section
    - Stale indicator turns yellow with "(stale)" suffix at 2x expected interval
    - "Collecting data..." placeholder for first 60s after server start
    - Degraded badges on service health matrix when external stack unavailable
  - Done when: freshness indicators update correctly, stale state triggers at correct threshold, and degraded mode shows without breaking any panel layout.

- [ ] **T-38: Add external deep-link actions** (0.5d)
  - Files:
    - `client/src/components/admin/TracesTab.tsx`
    - `client/src/components/admin/LogsTab.tsx`
    - `client/src/components/admin/CommandCenterPanel.tsx`
  - Includes:
    - "Open in Grafana", "Open in Tempo", "Open in Loki", "Open in Prometheus" actions
    - Visible only when respective URL is returned by config/links endpoints
    - Opens in system browser via Tauri shell API
  - Done when: links appear only when configured, and clicking opens the correct external URL.

**Phase 5 subtotal: ~15.25d**

---

### Phase 6 — Tests and Governance

- [ ] **T-39: Backend integration tests (auth, bounds, retention, redaction)** (1.5d)
  - Files:
    - `server/tests/observability_auth.rs` (new)
    - `server/tests/observability_bounds.rs` (new)
    - `server/tests/observability_retention.rs` (new)
    - `server/tests/observability_redaction.rs` (new)
  - Covers:
    - Non-admin requests to all observability endpoints return 403
    - Page size bounds enforced (max 100)
    - Time range bounds enforced (max 30d)
    - Retention job deletes rows older than 30 days and nothing newer
    - No forbidden fields appear in persisted `attrs` or `labels` JSONB
    - Export row limit enforced (max 10,000)
  - Done when: all tests pass with `SQLX_OFFLINE=true cargo test -p vc-server`.

- [ ] **T-40: Backend integration tests (voice and infra endpoints)** (1.0d)
  - Files:
    - `server/tests/observability_voice.rs` (new)
    - `server/tests/observability_infra.rs` (new)
  - Covers:
    - Voice summary returns correct health score from fixture data
    - Voice quality endpoint returns p50/p95 for packet loss, jitter, latency
    - Infrastructure endpoint handles S3 unconfigured state
    - Infrastructure endpoint handles Valkey unavailable state
  - Done when: all tests pass and edge cases (no voice data, no S3) return correct empty/degraded responses.

- [ ] **T-41: Frontend component tests (panel states and data rendering)** (1.0d)
  - Files:
    - `client/src/components/admin/CommandCenterPanel.test.tsx` (new)
    - `client/src/components/admin/VoiceTab.test.tsx` (new)
    - `client/src/components/admin/LiveErrorFeed.test.tsx` (new)
  - Covers:
    - Panel renders "Collecting data..." on first load
    - Vital-sign cards show correct threshold colors
    - Service health matrix transitions on WS health_change event
    - Live error feed appends and caps at 25 entries
    - Voice health score gauge renders correct value
    - Freshness indicator turns stale at 2x interval
  - Done when: `bun run test:run` passes with no failures.

- [ ] **T-42: E2E smoke test for command center access** (0.75d)
  - Files:
    - `client/e2e/admin_command_center.spec.ts` (new)
  - Covers:
    - System admin can open command center and see health data within 5s
    - Non-admin user cannot access command center
    - Time range picker changes chart data
    - Tab switching works for all 5 tabs
    - Export button downloads a file
  - Done when: `npx playwright test` passes for all command center scenarios.

- [ ] **T-43: CI governance checks** (0.75d)
  - Files:
    - `.github/workflows/ci.yml`
    - `scripts/check_docs_governance.py`
  - Includes:
    - `cargo deny check licenses` covers new chart.js dependency (MIT confirmed)
    - Schema conformance check: retention constants match 30-day spec
    - Forbidden field check: no PII field names in migration SQL
    - Design/implementation doc linkage check
  - Done when: CI pipeline passes on a clean branch with all new code, and governance script catches a deliberate violation in a test run.

**Phase 6 subtotal: ~4.0d**

---

## Estimated Effort

| Phase | Description | Days |
|-------|-------------|------|
| Phase 0 | Project Guardrails | 1.0d |
| Phase 1 | Data Foundation | 4.0d |
| Phase 2 | Ingestion and Safety | 4.0d |
| Phase 3 | Admin API Surface | 6.75d |
| Phase 4 | WebSocket Push Events | 2.0d |
| Phase 5 | Client Admin Panel | 15.25d |
| Phase 6 | Tests and Governance | 4.0d |

**Grand total: ~37.0 engineer-days (single-contributor baseline)**

---

## Milestone Acceptance

### MVP Acceptance

- [ ] Command center tab is available to all system admins.
- [ ] Health overview (vital signs, service matrix, live error feed) answers "is something wrong?" within 5 seconds.
- [ ] Golden Signals tab shows Latency/Traffic/Errors/Saturation with time-series charts.
- [ ] Voice tab shows session quality, join success rate, packet loss, jitter, and health score.
- [ ] Infrastructure tab shows DB pool, Valkey, S3, and OTel pipeline health.
- [ ] Logs tab is WARN/ERROR only, filterable, with trace ID correlation.
- [ ] Traces tab is searchable with "Open in Tempo" when external stack configured.
- [ ] Native data never exceeds 30-day retention.
- [ ] Status thresholds are color-coded (green/yellow/red) with configurable values.
- [ ] External deep links render only when configured.
- [ ] Redaction and allowlist constraints verified by tests.

### Release Acceptance

- [ ] Real-time updates via WebSocket for health transitions, vital signs, error feed, and voice pulse.
- [ ] Export to CSV/JSON works for logs, traces, and metrics views.
- [ ] Incident correlation view (shared time axis, click-through) is functional.
- [ ] Time range picker applies to all charts and tables.
- [ ] Degraded mode works without external stack.
- [ ] Freshness indicators show stale state correctly.
- [ ] CI checks pass for schema, retention, redaction, and docs linkage.
- [ ] Observability runbook has command center operational guidance.
- [ ] No dependency policy violations (license/maintenance/deprecation checks).
- [ ] No forbidden fields leak into persisted native telemetry.
- [ ] All metrics map to observability contract definitions.

---

## Future Tasks (v1.1)

The following are explicitly out of scope for v1 and should not be started until the release milestone is accepted:

- **Client telemetry ingestion** (Tier 7): Tauri command latency, WebRTC connect time, client-side errors. Requires new `/api/telemetry/client-metrics` endpoint with rate limiting.
- **Deploy markers on charts**: Audit log `admin.deploy.detected` events as vertical markers on time-series charts.
- **Redis INFO stats**: Memory, hit rate, connected clients from `REDIS INFO` command.
- **Rolling anomaly detection**: 7-day deviation baselines for automatic degraded state detection.
- **S3 throughput metrics**: Upload/download throughput requires new instrumentation.
- **Client WebRTC metrics on charts**: `kaiku_client_voice_webrtc_connect_duration_seconds` and `kaiku_client_voice_webrtc_failures_total` (depend on client telemetry pipeline).
- **Per-role observability segmentation**: v1 is cluster-wide for all system admins.
- **Alert-rule authoring UI**: v1 uses static configurable thresholds only.
