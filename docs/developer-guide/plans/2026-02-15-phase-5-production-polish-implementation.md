# Phase 5 Production-Scale Polish - Implementation Plan

**Date:** 2026-02-15
**Status:** Draft
**Design Reference:** `docs/plans/2026-02-15-phase-5-production-polish-design.md`

## Objective

Finalize large-history message rendering reliability and unified UI feedback behavior for production-scale usage.

## Open Standards Enforcement

- ARIA list semantics and keyboard behavior maintained in virtualized list.
- OpenTelemetry metrics for render latency, pagination timing, and memory behavior.
- Pagination contract schema documented and versioned.

## Implementation Phases

### Phase A - Virtualization wiring
1. Integrate virtualized rendering in message list.
2. Connect upward pagination to existing cursor API.
3. Add robust scroll restoration on prepend.

### Phase B - Operational polish
1. Add configurable memory eviction policy.
2. Validate grouping, reactions, and attachment heights under virtualization.
3. Add load/perf tests with high-message channels.

### Phase C - Toast and UX consistency
1. Audit global toast usage across major flows.
2. Enforce deduplication and action semantics consistency.
3. Add E2E checks for warning/error/success patterns.

## Verification

- virtualized list behavior passes UI and E2E tests
- large-channel performance remains stable
- toast behavior remains consistent across routes
