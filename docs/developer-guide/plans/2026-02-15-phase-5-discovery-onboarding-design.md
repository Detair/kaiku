# Phase 5 Discovery and Onboarding - Design

**Date:** 2026-02-15
**Status:** Draft
**Roadmap Scope:** Phase 5 `[Growth] Discovery & Onboarding`

## Problem

New users and small communities need a clearer path from sign-up to first meaningful interaction, plus better guild discovery and invitation conversion.

## Goals

- Introduce discovery ranking and search entry points for suitable guilds.
- Improve first-session onboarding and activation milestones.
- Add measurable conversion funnel analytics.

## Non-Goals

- Public global guild directory without moderation safeguards.
- Growth experiments that bypass user consent and privacy controls.

## Open Standards Profile

- OpenAPI 3.1 contracts for discovery and onboarding endpoints.
- Structured analytics events with schema-version fields.
- OpenTelemetry traces for onboarding step timings.

## Approach

Add a discovery service with ranking inputs (activity, relevance, trust signals), onboarding checklist flows, and invite-context-aware suggestions.

## Success Criteria

- Increased activation completion within first session/week.
- Improved invite-to-engagement conversion.
- Discovery results remain permission and safety compliant.

## References

- `docs/project/roadmap.md`
- `server/src/guild/`
- `client/src/views/Main.tsx`
