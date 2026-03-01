# Phase 5 Discovery and Onboarding - Implementation Plan

**Date:** 2026-02-15
**Status:** Draft
**Design Reference:** `docs/plans/2026-02-15-phase-5-discovery-onboarding-design.md`

## Objective

Implement safe guild discovery and onboarding flows that measurably improve early user activation and retention.

## Open Standards Enforcement

- OpenAPI 3.1 for discovery/search/listing endpoints.
- Event payload schemas include explicit versioning.
- OpenTelemetry spans for onboarding and invite conversion milestones.

## Implementation Phases

### Phase A - Discovery backend
1. Add discovery listing/search endpoints and ranking pipeline.
2. Add policy filters for private/restricted guilds.
3. Add moderation and abuse controls for discoverability.

### Phase B - Onboarding experience
1. Add first-run onboarding checklist and progress state.
2. Add invite context integration to streamline first actions.
3. Add quick-start suggestions based on selected interests.

### Phase C - Measurement and iteration
1. Add conversion funnel metrics and dashboards.
2. Add A/B-safe configuration toggles for onboarding variants.
3. Add integration and E2E tests for activation flows.

## Verification

- onboarding and discovery APIs pass integration tests
- onboarding flows pass E2E tests on desktop and mobile layouts
- conversion telemetry and dashboards are populated correctly
