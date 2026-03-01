# Phase 6 Sovereign Guild and Live Session Toolkit - Implementation Plan

**Date:** 2026-02-15
**Status:** Draft
**Design Reference:** `docs/plans/2026-02-15-phase-6-sovereign-livekit-design.md`

## Objective

Ship a secure BYO infrastructure profile for sovereign guilds and a permissions-aware live toolkit suite for voice sessions.

## Open Standards Enforcement

- BYO object storage integrations validated against S3-compatible behavior.
- Relay configuration and signaling remain WebRTC standards-compatible.
- Profile and provider config validated with JSON Schema-based contracts.
- Toolkit and profile events exported with OpenTelemetry context fields.

## Implementation Phases

### Phase A - Sovereign profile enablement

1. Add profile capability schema and runtime validation.
2. Add storage and relay provider adapters with conformance tests.
3. Add startup checks and safe fallback/error handling.
4. Add audit logging for profile changes.

### Phase B - Live toolkit primitives

1. Implement timer and structured notes models.
2. Add role-scoped action-item management and summary publishing.
3. Add websocket event channel for toolkit updates.
4. Add abuse/rate-limit protections for toolkit operations.

### Phase C - Client UX and operations

1. Add toolkit controls in voice UI surfaces.
2. Add sovereign profile settings workflow and validation feedback.
3. Add operator runbook for provider onboarding and incident handling.
4. Add load and reconnect validation for toolkit-enabled sessions.

## File Targets

- `server/src/config.rs`
- `server/src/main.rs`
- `server/src/voice/`
- `server/src/ws/`
- `server/tests/websocket_integration_test.rs`
- `server/tests/voice_sfu_test.rs`
- `client/src/components/voice/`
- `docs/deployment/`

## Verification

- provider conformance tests pass
- voice websocket integration tests pass
- sovereign profile startup validation tested with invalid and valid configs

## Done Criteria

- Sovereign profile is deployable with documented constraints.
- Live toolkit scenarios are functional and role-restricted.
- Operational docs cover onboarding, rollback, and incident handling.
