# Phase 5 Advanced Moderation and Safety Filters - Implementation Plan

**Date:** 2026-02-15
**Status:** Draft
**Design Reference:** `docs/plans/2026-02-15-phase-5-moderation-filters-design.md`

## Objective

Implement guild-configurable moderation filter policies with strong permission checks, actionable override workflows, and complete audit trails.

## Open Standards Enforcement

- OpenAPI 3.1 schemas for policy configuration and enforcement actions.
- Structured moderation events with stable JSON contracts.
- OpenTelemetry events for policy evaluation and moderation actions.

## Implementation Phases

### Phase A - Policy model
1. Add filter taxonomy and action matrix types.
2. Add guild-level policy storage and versioning.
3. Add validation for invalid/unsafe policy combinations.

### Phase B - Enforcement path
1. Integrate policy checks into message and upload moderation path.
2. Add moderator override and allowlist controls.
3. Add abuse rate limits for repeated violations.

### Phase C - UX and auditability
1. Add guild settings controls for filter policy tuning.
2. Add moderation queue visibility and action history.
3. Add integration tests for false positives and bypass attempts.

## Verification

- moderation policy CRUD and enforcement tests pass
- audit logs capture all policy/action transitions
- permission checks block unauthorized policy changes
