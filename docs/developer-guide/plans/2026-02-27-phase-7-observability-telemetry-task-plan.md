# Phase 7 Observability and Telemetry - Task-by-Task Implementation Plan

**Date:** 2026-02-27
**Status:** Not Started
**Lifecycle:** Active
**Roadmap Reference:** `docs/project/roadmap.md` (Phase 7, `[Infra] SaaS Observability & Telemetry`)
**Design References:**
- `docs/plans/2026-02-15-phase-7-a11y-observability-design.md`
- `docs/plans/2026-02-15-opentelemetry-grafana-reference-design.md`

## Objective

Implement production-grade observability across server and client with OpenTelemetry as the primary standard and Sentry for error/crash visibility, then enforce telemetry readiness in release gates.

## Open Standards Compliance Notes

- **Telemetry:** OpenTelemetry + OTLP transport
- **Trace Context:** W3C Trace Context (`traceparent`, `tracestate`) and baggage propagation
- **Log Correlation:** Structured logs with trace and span correlation fields
- **Alerting:** SLO-based burn-rate alerting (Prometheus-compatible metrics)

## Scope

In scope:
- `server/` telemetry bootstrap and instrumentation contract
- `client/src-tauri/` Sentry integration and release metadata
- `client/src/` frontend telemetry bootstrap and key interaction timing
- `infra/` collector and monitoring stack scaffolding
- CI checks and ops/runbook documentation

Out of scope:
- Full historical dashboard migration for all legacy metrics
- Deep packet-level voice tracing
- Commercial SaaS billing integrations

## Task Breakdown (Atomic)

### Phase 0 - Baseline and Safety Guardrails

1. **Create observability branch worktree**
   - File targets: none (git operation)
   - Action: create isolated worktree and branch for phase 7 telemetry work
   - Done when: all work is performed outside default checkout

2. **Create telemetry contract doc skeleton**
   - File: `docs/ops/observability-contract.md` (new)
   - Action: define required attributes, naming, cardinality limits, and forbidden fields
   - Done when: contract includes server/client, traces/logs/metrics, and privacy rules

### Phase 1 - Server OTel Bootstrap

3. **Add OpenTelemetry dependencies**
   - File: `server/Cargo.toml`
   - Action: add `opentelemetry`, `opentelemetry_sdk`, `opentelemetry-otlp`, `tracing-opentelemetry`
   - Done when: workspace resolves and compiles with new telemetry crates

4. **Add observability module scaffold**
   - Files:
     - `server/src/observability/mod.rs` (new)
     - `server/src/observability/tracing.rs` (new)
     - `server/src/observability/metrics.rs` (new)
   - Action: centralize provider initialization, resource metadata, sampler, and shutdown flush
   - Done when: module can initialize tracer/meter and expose shutdown handle

5. **Wire observability into server startup**
   - Files:
     - `server/src/main.rs`
     - `server/src/lib.rs` (if module exports are needed)
   - Action: replace one-off subscriber init with layered subscriber + OTel integration
   - Done when: startup logs indicate OTel init success and graceful shutdown flushes providers

6. **Add telemetry env configuration**
   - File: `server/src/config.rs`
   - Action: add typed config fields for OTLP endpoint/protocol/sampler ratio and feature toggles
   - Done when: config defaults are safe and configurable via env vars

### Phase 2 - Server Instrumentation Contract

7. **Instrument HTTP critical paths with stable span names**
   - Files (minimum):
     - `server/src/auth/handlers.rs`
     - `server/src/chat/messages.rs`
     - `server/src/ws/mod.rs`
     - `server/src/voice/call_handlers.rs`
   - Action: ensure stable route-level span naming and required domain attributes
   - Done when: traces for auth/chat/ws/voice have consistent attribute schema

8. **Introduce core SLO metrics**
   - Files:
     - `server/src/observability/metrics.rs`
     - touched handlers in auth/chat/ws/voice
   - Action: emit counters/histograms for latency, error rate, reconnect behavior, and voice join success
   - Done when: test/local run shows metric emission with low-cardinality labels

9. **Apply privacy and cardinality enforcement**
   - Files:
     - `server/src/observability/tracing.rs`
     - `docs/ops/observability-contract.md`
   - Action: codify skip/redaction rules and prohibit user-content labels in metrics
   - Done when: contract and code align on forbidden fields and allowed dimensions

### Phase 3 - Client and Crash Visibility

10. **Integrate Sentry in Tauri backend**
    - Files:
      - `client/src-tauri/Cargo.toml`
      - `client/src-tauri/src/lib.rs`
    - Action: initialize Sentry with release/environment metadata and privacy defaults
    - Done when: native errors reach Sentry in dev/staging test project

11. **Bootstrap frontend telemetry/Sentry wrapper**
    - Files:
      - `client/src/lib/sentry.ts` (new)
      - `client/src/index.tsx` (or app bootstrap entry)
    - Action: initialize browser-side error/performance capture with filtering
    - Done when: frontend exceptions are tagged by release and environment

12. **Add client timing events for critical UX flows**
    - Files:
      - `client/src/stores/websocket.ts`
      - `client/src/lib/webrtc/browser.ts`
      - `client/src/lib/webrtc/tauri.ts`
    - Action: track connection/reconnection/join timing and error classes (without sensitive payloads)
    - Done when: telemetry events are visible and mapped to known flow names

### Phase 4 - Infra, Dashboards, and Alerts

13. **Add collector and monitoring compose profile**
    - Files (new/updated under `infra/`):
      - `infra/monitoring/otel-collector.yaml`
      - `infra/monitoring/prometheus.yaml`
      - `infra/monitoring/alerts/phase7-observability-rules.yaml`
      - compose wiring file in `infra/compose/`
    - Action: define OTLP receivers/processors/exporters and SLO alerts
    - Done when: local stack receives telemetry from server and exposes alert rules

14. **Create starter dashboard definitions and runbook**
    - Files:
      - `docs/ops/observability-runbook.md` (new)
      - `docs/ops/incident-triage-observability.md` (new)
    - Action: document dashboards, thresholds, triage workflow, and fallback behavior
    - Done when: operators can trace incident path from alert to root-cause signals

### Phase 5 - CI and Release Gates

15. **Add telemetry readiness checks to CI**
    - File: `.github/workflows/ci.yml`
    - Action: add job/steps for observability contract validation and telemetry smoke checks
    - Done when: CI fails if required telemetry metadata/contract checks fail

16. **Add docs governance checks for observability plan linkage**
    - File: `scripts/check_docs_governance.py`
    - Action: ensure roadmap/plan references remain valid for phase 7 observability items
    - Done when: link drift is caught automatically in CI

## Verification Plan

Run after each phase slice:

```bash
python3 scripts/check_docs_governance.py
cargo fmt --check
SQLX_OFFLINE=true cargo clippy --workspace --all-features --exclude vc-client -- -D warnings
cargo test -p vc-server
cd client && bun run test:run && bun run build
```

Telemetry-specific checks:

- OTel startup smoke: service starts with OTEL env configured and exports without panic
- Trace correlation smoke: one request yields trace + correlated logs with matching trace id
- Metrics smoke: latency/error counters visible in collector output
- Sentry smoke: synthetic server/client error appears with correct release + environment tags

## Rollout Strategy

1. Ship server OTel bootstrap behind `OBSERVABILITY_ENABLED` (default on in staging, off in local unless configured).
2. Validate perf overhead and cardinality in staging.
3. Enable in production with conservative trace sampling.
4. Enable client Sentry after release tagging is confirmed.
5. Turn CI telemetry checks from advisory to required.

## Risks and Mitigations

- **Risk:** high-cardinality explosion in metrics labels
  - **Mitigation:** strict attribute allowlist + contract tests + collector processors
- **Risk:** accidental PII leakage
  - **Mitigation:** `skip(...)` + redaction hooks + documented forbidden field list
- **Risk:** voice performance degradation
  - **Mitigation:** no per-packet spans, benchmark before/after in staging

## Definition of Done

- Phase 7 observability telemetry item has concrete implementation path and linked docs.
- Server emits OTel traces/logs/metrics with correlation fields.
- Client errors/crashes are captured with release/environment tags.
- Collector/alerts/runbooks are in repo and tested.
- CI enforces telemetry readiness and doc linkage.
