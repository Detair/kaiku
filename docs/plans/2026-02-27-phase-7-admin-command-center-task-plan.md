# Phase 7 Admin Command Center - Task-by-Task Plan

**Date:** 2026-02-27
**Status:** Not Started
**Lifecycle:** Active
**Roadmap Reference:** `docs/project/roadmap.md` (Phase 7, `[Ops] Admin Command Center (Native Observability Lite)`)
**Design Reference:** `docs/plans/2026-02-27-phase-7-admin-command-center-design.md`
**Implementation Reference:** `docs/plans/2026-02-27-phase-7-admin-command-center-implementation.md`

## Objective

Deliver a cluster-wide native Command Center for system admins with strict 30-day telemetry retention, while delegating deep forensic analysis to external Grafana/Tempo/Loki/Prometheus when configured.

## Delivery Constraints

- Native retention hard cap: 30 days.
- Cluster-wide visibility for all system admins.
- Open-source, actively maintained tooling only.
- Preserve OTel standards and current observability contract.
- No full in-app trace waterfall or advanced log query DSL in v1.

## Task Breakdown (Atomic)

### Phase 0 - Project Guardrails

1. [ ] **Confirm contract and roadmap alignment** (0.5d)
   - Files:
     - `docs/ops/observability-contract.md`
     - `docs/project/roadmap.md`
   - Done when: retention/access/scope decisions are explicitly represented and consistent.

2. [ ] **Define dependency policy checklist in docs** (0.5d)
   - Files:
     - `docs/plans/2026-02-27-phase-7-admin-command-center-implementation.md`
   - Done when: maintenance/licensing/version policy is enforceable and reviewable.

### Phase 1 - Data Foundation

3. [ ] **Create native telemetry schema migration** (1.0d)
   - Files:
     - `server/migrations/<new>_command_center_telemetry.sql`
   - Includes:
     - `telemetry_metric_samples`
     - `telemetry_log_events`
     - `telemetry_trace_index`
   - Done when: schema applies cleanly and supports target query patterns.

4. [ ] **Add retention + downsampling policies** (0.75d)
   - Files:
     - `server/migrations/<new>_command_center_retention.sql`
   - Done when:
     - hard delete beyond 30 days
     - downsampling policy after day 7 for metrics

5. [ ] **Add storage/query module** (1.0d)
   - Files:
     - `server/src/observability/storage.rs` (new)
     - `server/src/observability/mod.rs`
   - Done when: bounded read/write helpers exist for summary, trends, logs, trace index.

### Phase 2 - Ingestion and Safety

6. [ ] **Wire native log ingestion with redaction** (1.0d)
   - Files:
     - `server/src/observability/tracing.rs`
   - Done when: persisted log events are allowlisted and sanitized.

7. [ ] **Wire native trace index ingestion** (0.75d)
   - Files:
     - `server/src/observability/tracing.rs`
     - `server/src/observability/storage.rs`
   - Done when: failed/slow trace metadata is persisted without payload content.

8. [ ] **Wire metric sample ingestion and label guardrails** (1.0d)
   - Files:
     - `server/src/observability/metrics.rs`
     - `server/src/observability/storage.rs`
   - Done when: only allowlisted dimensions are persisted and high-cardinality labels are dropped/rejected.

9. [ ] **Add observability access audit events** (0.5d)
   - Files:
     - `server/src/admin/handlers.rs`
     - `server/src/permissions/queries.rs`
   - Done when: command center data access emits `admin.observability.view` style audit events.

### Phase 3 - Admin API Surface

10. [ ] **Add summary/trends endpoints** (1.0d)
    - Files:
      - `server/src/admin/mod.rs`
      - `server/src/admin/handlers.rs`
      - `server/src/admin/types.rs`
    - Endpoints:
      - `GET /api/admin/observability/summary`
      - `GET /api/admin/observability/trends`

11. [ ] **Add top-offenders endpoints** (0.75d)
    - Endpoints:
      - `GET /api/admin/observability/top-routes`
      - `GET /api/admin/observability/top-errors`

12. [ ] **Add logs/trace-index list endpoints** (1.0d)
    - Endpoints:
      - `GET /api/admin/observability/logs`
      - `GET /api/admin/observability/traces`
    - Done when: pagination and filters are bounded and stable.

13. [ ] **Add external links endpoint** (0.5d)
    - Endpoint:
      - `GET /api/admin/observability/links`
    - Done when: links are returned only when configured.

### Phase 4 - Admin UI Integration

14. [ ] **Add command-center panel routing + sidebar entry** (0.5d)
    - Files:
      - `client/src/components/admin/AdminSidebar.tsx`
      - `client/src/views/AdminDashboard.tsx`

15. [ ] **Create `CommandCenterPanel` UI skeleton** (1.0d)
    - Files:
      - `client/src/components/admin/CommandCenterPanel.tsx` (new)
      - `client/src/components/admin/index.ts`

16. [ ] **Implement admin store slice + API bindings** (1.5d)
    - Files:
      - `client/src/stores/admin.ts`
      - `client/src/lib/tauri.ts`

17. [ ] **Implement health cards + trends charts** (1.0d)
    - Done when: top-level operational status is visible and refreshes predictably.

18. [ ] **Implement top-offenders/logs/trace-index tables** (1.0d)
    - Done when: filtering, pagination, and empty states are usable.

19. [ ] **Implement degraded mode + freshness indicators** (0.75d)
    - Done when: each section clearly indicates stale/unavailable data without breaking UI.

20. [ ] **Add deep-link actions to external stack** (0.5d)
    - Done when: relevant rows/cards can open Grafana/Tempo/Loki/Prometheus links when configured.

### Phase 5 - Tests and Governance

21. [ ] **Backend integration tests for auth, bounds, retention** (1.5d)
    - Files:
      - `server/tests/*observability*`

22. [ ] **Frontend tests for panel state and data rendering** (1.0d)
    - Files:
      - `client/src/components/admin/*.test.tsx`

23. [ ] **E2E smoke coverage for command center access** (0.75d)
    - Files:
      - `client/e2e/*admin*.spec.ts`

24. [ ] **CI/governance checks for retention and forbidden fields** (0.75d)
    - Files:
      - `.github/workflows/ci.yml`
      - `scripts/check_docs_governance.py`

## Estimated Effort

- Phase 0: ~1 day
- Phase 1: ~2.75 days
- Phase 2: ~3.25 days
- Phase 3: ~3.25 days
- Phase 4: ~6.25 days
- Phase 5: ~4 days

**Total estimate:** ~20.5 engineer-days (single-contributor baseline)

## Milestone Acceptance

### MVP Acceptance

- Command center tab is available to all system admins.
- Summary + trends + top-offenders + logs + trace-index panels functional.
- Native data never exceeds 30-day retention.
- External deep links render only when configured.
- Redaction and allowlist constraints verified by tests.

### Release Acceptance

- CI checks pass for schema, retention, redaction, and docs linkage.
- Observability runbook has command center operational guidance.
- No dependency policy violations (license/maintenance/deprecation checks).
