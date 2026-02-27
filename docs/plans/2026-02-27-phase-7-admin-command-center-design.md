# Phase 7 Admin Command Center - Design

**Date:** 2026-02-27
**Status:** Draft
**Roadmap Scope:** Phase 7 `[Ops] Admin Command Center (Native Observability Lite)`
**Related:**
- `docs/project/roadmap.md`
- `docs/ops/observability-contract.md`
- `docs/ops/observability-runbook.md`
- `docs/plans/2026-02-15-phase-7-a11y-observability-design.md`
- `docs/plans/2026-02-15-opentelemetry-grafana-reference-design.md`

## Problem

Phase 7 observability foundations are in place (OTel + collector + Grafana stack reference), but operators currently need external tooling for most runtime visibility. This creates two gaps:

1. Self-hosted operators without full Grafana setup lack practical day-to-day insight.
2. Admin workflows are split between in-app moderation/admin actions and external telemetry tools.

We need a native admin Command Center that gives immediate operational awareness while preserving external observability as the deep-analysis path.

## Goals

- Provide a cluster-wide command center in the existing admin panel for all system admins.
- Deliver useful native observability with strict 30-day retention.
- Show high-signal health, reliability, and incident context without requiring external Grafana stack.
- Preserve compatibility with OTel/Grafana stack and deep-link into external tools when available.
- Keep runtime and storage overhead bounded for self-hosted installations.

## Non-Goals

- Replacing Grafana, Tempo, Loki, or Prometheus.
- Building an in-app trace waterfall viewer or full log query language.
- Providing retention beyond 30 days in native storage.
- Multi-tenant observability segmentation in v1.
- Per-node access restrictions in v1 (view is cluster-wide for all system admins).

## Product Decision Summary

- **Access model:** cluster-wide, visible to all system admins.
- **Retention model:** native telemetry retained for 30 days max.
- **Escalation path:** users requiring deeper history or advanced queries must use external stack.
- **UX boundary:** native panel is for fast triage and operational awareness, not full forensic analysis.

## User Experience

### Command Center Panels

1. **System Health (always visible):**
   - API request rate (1m)
   - API error rate (5m)
   - API p95/p99 latency (5m)
   - Active WebSocket connections
   - Active voice sessions
   - Telemetry pipeline status (collector/export path)

2. **Reliability Trends (30-day charts):**
   - Daily API error rate
   - Daily p95/p99 latency
   - Daily voice join success rate
   - Daily auth failure rate
   - Daily WebSocket reconnect rate

3. **Top Offenders:**
   - Top failing routes
   - Top slow routes
   - Top error categories (`auth`, `db`, `ws`, `voice`, `ratelimit`)

4. **Logs (curated native view):**
   - ERROR/WARN focused list
   - Filter by domain and time range
   - Display trace id if present

5. **Trace Index (not full traces):**
   - Recent failed trace records
   - Recent slow trace records
   - Filters by route/domain/status/time
   - `Open in Grafana/Tempo` actions when external stack is configured

## Information Architecture

### Signal Split

- **Metrics:** first-class native signal, stored in Postgres/Timescale for 30 days.
- **Logs:** native curated event stream with compact schema and 30-day TTL.
- **Traces:** native index metadata only (trace id, route/span, duration, status, timestamp), 30-day TTL.
- **Full traces/log corpus:** external stack only.

This split gives strong native value while avoiding high cardinality and storage blowups from raw spans/logs.

## Data Model (Native Retention)

### 1) `telemetry_metric_samples` (Timescale hypertable)

- `ts timestamptz not null`
- `metric_name text not null`
- `scope text not null` (cluster/domain)
- `labels jsonb not null` (allowlisted dimensions only)
- `value_double double precision null`
- `value_int bigint null`

Policies:
- Retention: 30 days hard delete.
- Downsample: rollups after day 7 to reduce storage/query cost.

### 2) `telemetry_log_events`

- `id uuid pk`
- `ts timestamptz not null`
- `level text not null` (`ERROR`, `WARN`, selected `INFO`)
- `service text not null`
- `domain text not null`
- `event text not null`
- `message text not null`
- `trace_id text null`
- `span_id text null`
- `attrs jsonb not null default '{}'::jsonb`

Policies:
- Retention: 30 days hard delete.
- Store only sanitized/allowlisted attributes.

### 3) `telemetry_trace_index`

- `trace_id text not null`
- `span_name text not null`
- `domain text not null`
- `route text null`
- `status_code text null`
- `duration_ms integer not null`
- `ts timestamptz not null`
- `service text not null`

Policies:
- Retention: 30 days hard delete.
- No span payload/body storage.

## API Design (Admin)

All endpoints are under `/api/admin/observability/*` and require `SystemAdminUser`.

### Health and Overview

- `GET /api/admin/observability/summary`
  - Returns top-card KPIs and pipeline status.

### Trends

- `GET /api/admin/observability/trends?range=24h|7d|30d`
  - Returns pre-aggregated chart series.

### Top Offenders

- `GET /api/admin/observability/top-routes?range=...`
- `GET /api/admin/observability/top-errors?range=...`

### Logs

- `GET /api/admin/observability/logs?level=&domain=&from=&to=&limit=&offset=`

### Trace Index

- `GET /api/admin/observability/traces?status=&domain=&route=&from=&to=&limit=&offset=`

### External Deep Links Config

- `GET /api/admin/observability/links`
  - Returns enabled external URLs/actions (Grafana/Tempo/Loki/Prometheus) if configured.

## Security and Privacy

- Reuse `observability-contract.md` redaction rules.
- Enforce strict attribute allowlist for persisted logs and metric labels.
- Never persist secrets, auth tokens, private message content, key material, or raw request bodies.
- Keep admin-only access behind existing system admin middleware.
- Record access to observability endpoints in audit log (`admin.observability.view`).

## Cardinality and Cost Controls

- Ban high-cardinality labels (`user_id`, `session_id`, arbitrary ids) in native metric labels.
- Restrict route labels to parameterized templates.
- Enforce per-query limits and bounded time ranges.
- Pre-aggregate trend queries; avoid on-demand expensive scans of raw rows.

## Operational Behavior

- **Degraded mode:** if collector/external stack is unavailable, native panel remains functional using native storage.
- **Freshness indicators:** each panel shows last successful ingest/update time.
- **Backfill behavior:** no historical backfill beyond native retention boundaries.

## Performance Constraints

- Dashboard API p95 target: <= 200 ms for summary/top endpoints.
- Trend endpoint p95 target: <= 500 ms for 30-day range.
- UI refresh cadence: 10-30 seconds depending on panel.
- Ingestion jobs must be async and bounded; no hot-path blocking.

## Rollout Plan

### Phase 1 - Foundations

1. Add `command-center` panel in admin UI.
2. Define backend observability admin route group.
3. Add native telemetry tables and retention jobs.

### Phase 2 - MVP Data and Views

1. Implement summary/trends/top-routes/top-errors endpoints.
2. Implement logs and trace-index list endpoints.
3. Build native command center cards, charts, and tables.

### Phase 3 - External Hand-off

1. Add optional external deep-link configuration.
2. Add `Open in Grafana/Tempo/Loki` actions.
3. Add empty/degraded states and setup guidance.

### Phase 4 - Hardening

1. Add tests for redaction and label allowlist enforcement.
2. Add retention/downsampling verification checks.
3. Add CI checks for schema and contract conformance.

## Success Criteria

- All system admins can open cluster-wide command center and see current health signals.
- Native logs/trace-index/metrics data is queryable for last 30 days only.
- Degraded mode works without external stack.
- External links appear when stack is configured.
- No forbidden fields leak into persisted native telemetry.

## Open Questions

- Do we expose `INFO` logs in v1 or keep native logs strictly `WARN/ERROR`?
- Should trend aggregation run synchronous-on-read or precomputed materialized views?
- Do we add an export endpoint (CSV/JSON) for native command center datasets in v1?
