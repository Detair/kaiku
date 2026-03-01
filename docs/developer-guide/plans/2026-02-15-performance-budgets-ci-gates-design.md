# Performance Budgets as CI Gates - Design

**Date:** 2026-02-15
**Status:** Placeholder (Planned)
**Roadmap Item:** Phase 8 - Performance Budgets as CI Gates

## Problem

Performance regressions can land incrementally and only become visible after release. Current quality gates focus on correctness but do not enforce explicit latency, memory, and CPU budgets.

## Goals

- Define baseline performance budgets for critical paths.
- Enforce budget checks in CI for merge and release candidate flows.
- Provide trend reporting and regression diffs for reviewers.

## Initial Scope

- Voice connect/reconnect latency budget checks.
- API p95 latency and error-budget signal checks.
- Client startup and idle resource budget checks.

## Non-Goals

- Full microbenchmark coverage for every endpoint in phase one.
- Replacing functional tests with synthetic perf tests.

## Open Questions

- Which budgets block PR merges vs release promotions only?
- What tolerance window is acceptable for flaky CI performance variance?

## References

- `docs/project/roadmap.md`
- `docs/plans/2026-02-15-sre-foundations-design.md`
