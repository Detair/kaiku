# Operator Supportability Pack - Design

**Date:** 2026-02-15
**Status:** Placeholder (Planned)
**Roadmap Item:** Phase 8 - Operator Supportability Pack

## Problem

When incidents happen, operators need consistent diagnostics and support workflows. Missing or fragmented tooling increases mean time to detect and recover.

## Goals

- Define a standard diagnostics bundle format and collection flow.
- Provide health endpoints and troubleshooting runbook index.
- Improve operator self-service for common failure modes.

## Initial Scope

- Health and readiness endpoint specification.
- Diagnostics export bundle with redaction-safe defaults.
- Runbook index mapped to alerts and common operator tasks.

## Non-Goals

- Full managed support product in phase one.
- Replacing existing logging/monitoring stack components.

## Open Questions

- Which diagnostics artifacts are always safe to export by default?
- Should supportability bundle generation be CLI-only or API-accessible?

## References

- `docs/project/roadmap.md`
- `docs/plans/2026-02-15-sre-foundations-design.md`
