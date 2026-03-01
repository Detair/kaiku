# Release Governance - Design

**Date:** 2026-02-15
**Status:** Draft
**Roadmap Item:** Release governance (feature flags, staged rollouts, rollback playbooks)

## Problem

Delivery velocity is high and spans server, web client, and Tauri client. Without structured rollout controls, failures can impact all users simultaneously and rollback steps may be inconsistent.

## Goals

- Introduce feature-flag controls for high-risk changes.
- Define staged rollout process with objective promotion gates.
- Standardize rollback procedures for server and clients.
- Improve release note structure per milestone.

## Non-Goals

- Building a fully custom deployment platform.
- Gating every low-risk patch behind flags.
- Replacing existing CI pipelines wholesale.

## Design Options

### Option A: Version-based releases only

**Pros:**
- Minimal process overhead.
- Easy to reason about version boundaries.

**Cons:**
- Coarse rollback granularity.
- Limited mitigation when one feature misbehaves.

### Option B: Trunk-based delivery with governance controls (chosen)

**Pros:**
- Safer incremental rollouts.
- Faster mitigation using kill switches.
- Better separation of deploy vs release.

**Cons:**
- Requires flag lifecycle discipline.
- More release metadata to maintain.

## Chosen Approach

Adopt staged rollout governance with feature flags, canary environments, and domain-specific rollback playbooks.

### Architecture Outline

- **Feature flags:** central config model for server/client toggles.
- **Rollout stages:** local -> staging -> canary -> general availability.
- **Promotion gates:** test suite, SLO health checks, and key business flow validation.
- **Rollback assets:** per-domain playbooks in `docs/operations/rollbacks/`.

### Implementation Planning (High Level)

1. **Governance baseline**
   - Define release taxonomy (major/minor/patch/hotfix).
   - Define mandatory release checklist and approvals.
2. **Feature flag framework**
   - Introduce typed flags with owner, expiry, and fallback semantics.
   - Add kill-switch path for high-risk features.
3. **Staged rollout workflow**
   - Add canary deployment slice and promotion criteria.
   - Record release evidence for each stage gate.
4. **Rollback readiness**
   - Write and test rollback playbooks by domain.
   - Add rollback drill to release rehearsal cadence.

### Security Considerations

- Restrict who can toggle production flags.
- Require audit logs for flag and release state changes.
- Protect rollback tooling from unauthorized use.

### Performance Implications

- Keep feature-flag evaluation low overhead (cached, typed lookups).
- Minimize startup latency impact in client/server flag hydration.

## Success Criteria

- All high-risk features ship behind controlled flags.
- Each release follows documented staged promotion gates.
- Rollback drill completes inside agreed recovery window.
- Release notes are segmented by milestone with user impact clarity.

## Open Questions

- Should flags be environment-only or support cohort targeting?
- Which deployment unit should define canary scope first (guild, region, or tenant)?

## References

- `docs/project/roadmap.md`
- `docs/plans/2026-02-15-sre-foundations-design.md`
