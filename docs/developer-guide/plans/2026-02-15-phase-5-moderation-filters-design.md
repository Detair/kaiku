# Phase 5 Advanced Moderation and Safety Filters - Design

**Date:** 2026-02-15
**Status:** Draft
**Roadmap Scope:** Phase 5 `[Safety] Advanced Moderation & Safety Filters`

## Problem

Baseline moderation exists, but fine-grained policy controls and false-positive handling are still missing for high-scale communities.

## Goals

- Add configurable filter categories and policy actions.
- Provide override and appeal-friendly workflows for moderators.
- Ensure moderation actions are auditable and rate-limited.

## Non-Goals

- AI-only automated moderation with no human oversight.
- Cross-guild shared moderation policies in v1.

## Open Standards Profile

- API contracts documented with OpenAPI 3.1.
- Moderation events represented in structured JSON with stable schemas.
- Telemetry and audit streams emitted via OpenTelemetry-compatible fields.

## Approach

Implement a policy matrix (category x severity x action) with configurable guild-level defaults. Add explicit bypass/allowlist controls and enforce role-based moderation permissions.

## Success Criteria

- Moderators can configure and enforce policy profiles safely.
- False-positive handling exists with explicit override controls.
- Audit logs cover all policy and enforcement transitions.

## References

- `docs/project/roadmap.md`
- `server/src/moderation/`
