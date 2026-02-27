# Phase 7 Admin Command Center - Implementation Plan

**Date:** 2026-02-27
**Status:** Draft
**Design Doc:** `docs/plans/2026-02-27-phase-7-admin-command-center-design.md`
**Task Plan:** `docs/plans/2026-02-27-phase-7-admin-command-center-task-plan.md`
**Roadmap Item:** Phase 7 `[Ops] Admin Command Center (Native Observability Lite)`

## Objective

Implement a cluster-wide native admin Command Center that provides 30-day operational visibility for all system admins while preserving external observability (Grafana/Tempo/Loki/Prometheus) for deep analysis.

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

- Admin UI panel `command-center`
- Summary health cards and 30-day trends
- Top offenders (slow/failing routes, error categories)
- Curated log list (WARN/ERROR, redacted)
- Trace index list (metadata only, no span payload storage)
- Native retention enforcement (30 days max)
- Optional deep-links to external Grafana stack when configured

### Out of scope (v1)

- In-app full trace waterfall renderer
- In-app advanced log query language
- Native retention beyond 30 days
- Alert-rule authoring UI
- Per-node or per-role observability segmentation

## Implementation Phases

## Phase 1 - Data Foundation

1. **Add telemetry storage schema (30-day native model)**
   - Files:
     - `server/migrations/<new>_telemetry_native_command_center.sql`
   - Tasks:
     - Create `telemetry_metric_samples` (Timescale hypertable)
     - Create `telemetry_log_events`
     - Create `telemetry_trace_index`
     - Add indexes for `ts`, domain, severity/status, route/span

2. **Add retention/downsampling jobs**
   - Files:
     - `server/migrations/<same or follow-up>_telemetry_retention_policies.sql`
   - Tasks:
     - Enforce strict 30-day deletion policies
     - Add metric downsampling after day 7

3. **Add storage/query modules**
   - Files:
     - `server/src/observability/storage.rs` (new)
     - `server/src/observability/mod.rs`
   - Tasks:
     - Insert/query helpers for metrics/logs/trace index
     - Add bounded query defaults and max limits

## Phase 2 - Ingestion and Contracts

4. **Wire curated native telemetry ingestion**
   - Files:
     - `server/src/observability/tracing.rs`
     - `server/src/observability/metrics.rs`
     - `server/src/main.rs`
   - Tasks:
     - Persist only allowlisted fields for logs and trace index
     - Persist selected metric aggregates for command center views
     - Preserve existing OTLP export path

5. **Implement cardinality guardrails**
   - Files:
     - `server/src/observability/metrics.rs`
     - `docs/ops/observability-contract.md`
   - Tasks:
     - Explicit label allowlist for native metric storage
     - Reject/drop forbidden high-cardinality labels

6. **Audit and security hooks**
   - Files:
     - `server/src/admin/handlers.rs`
     - `server/src/permissions/queries.rs`
   - Tasks:
     - Add `admin.observability.view` audit event on access
     - Ensure redaction and no sensitive field persistence

## Phase 3 - Admin API Surface

7. **Add command center admin endpoints**
   - Files:
     - `server/src/admin/mod.rs`
     - `server/src/admin/handlers.rs`
     - `server/src/admin/types.rs`
   - Endpoints:
     - `GET /api/admin/observability/summary`
     - `GET /api/admin/observability/trends`
     - `GET /api/admin/observability/top-routes`
     - `GET /api/admin/observability/top-errors`
     - `GET /api/admin/observability/logs`
     - `GET /api/admin/observability/traces`
     - `GET /api/admin/observability/links`

8. **Bounded query protections**
   - Files:
     - `server/src/admin/handlers.rs`
   - Tasks:
     - Max page size and strict range validation
     - Default sort order and stable pagination keys

## Phase 4 - Client Admin Panel

9. **Add sidebar and panel wiring**
   - Files:
     - `client/src/components/admin/AdminSidebar.tsx`
     - `client/src/views/AdminDashboard.tsx`
   - Tasks:
     - Add `command-center` panel id and nav entry
     - Route panel rendering in admin dashboard

10. **Create Command Center UI components**
    - Files:
      - `client/src/components/admin/CommandCenterPanel.tsx` (new)
      - `client/src/components/admin/index.ts`
    - Tasks:
      - Health cards, trend charts, offender tables, logs table, trace index table
      - Degraded-state and empty-state handling

11. **Add store and API client methods**
    - Files:
      - `client/src/stores/admin.ts`
      - `client/src/lib/tauri.ts`
    - Tasks:
      - Add state/actions for summary/trends/logs/traces/top offenders
      - Add polling cadence and freshness timestamps
      - Add optional external deep-link actions

## Phase 5 - External Stack Hand-off

12. **Config-gated deep links**
    - Files:
      - `server/src/config.rs`
      - `server/src/admin/handlers.rs`
      - `docs/ops/observability-runbook.md`
    - Tasks:
      - Add optional config for Grafana/Tempo/Loki/Prometheus URLs
      - Return links via `/links` endpoint only when configured

13. **UI integration for deep analysis**
    - Files:
      - `client/src/components/admin/CommandCenterPanel.tsx`
    - Tasks:
      - `Open in Grafana/Tempo/Loki` actions from relevant rows/cards

## Phase 6 - Quality Gates

14. **Tests: backend**
    - Files:
      - `server/tests/*` (new coverage files)
    - Tasks:
      - Endpoint auth/access tests (system admin required)
      - Retention and query-bound checks
      - Redaction/allowlist persistence tests

15. **Tests: frontend**
    - Files:
      - `client/src/components/admin/*.test.tsx`
      - `client/e2e/*admin*.spec.ts`
    - Tasks:
      - Panel rendering and degraded-state tests
      - Data table/filter interactions

16. **Governance + CI checks**
    - Files:
      - `.github/workflows/ci.yml`
      - `scripts/check_docs_governance.py`
    - Tasks:
      - Check retention policy constants (30-day max)
      - Check forbidden telemetry fields in native schemas
      - Ensure roadmap links to active design + implementation docs

## Verification Strategy

### Functional

- Summary/trends/top/logs/traces endpoints return expected bounded results.
- Admin UI shows cluster-wide telemetry for all system admins.
- Degraded mode works when external stack is unavailable.

### Privacy/Security

- No forbidden fields stored in `telemetry_log_events` or `telemetry_trace_index`.
- Access is restricted to system admins only.
- Observability view actions are audit-logged.

### Performance

- Summary endpoints p95 <= 200ms under expected load.
- Trend endpoint p95 <= 500ms for 30-day range.
- Polling does not regress server CPU/memory targets.

### Operability

- Retention purge/downsample jobs run successfully.
- Data freshness indicators remain accurate.

## Done Criteria

- Command center is available in admin panel and usable without external stack.
- Native telemetry retention is strictly capped at 30 days.
- External deep links work when configured.
- CI/governance checks enforce retention and redaction constraints.
- Documentation updated (roadmap, runbook, contract references).

## Risk Register

1. **Cardinality growth risk**
   - Mitigation: strict label allowlist, bounded dimensions, query limits.

2. **Storage growth risk**
   - Mitigation: 30-day hard TTL, downsampling, daily size checks.

3. **Scope creep into full observability platform**
   - Mitigation: enforce non-goals; route deep analysis to external stack.

4. **Dependency drift to stale tooling**
   - Mitigation: maintenance policy gate and pinned versions.
