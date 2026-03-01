# Security Verification Cadence - Design

**Date:** 2026-02-15
**Status:** Draft
**Roadmap Item:** Security verification cadence (threat-model refreshes, boundary regression suites, external testing)

## Problem

Security hardening work is active, but verification is mostly reactive to discovered issues. A repeatable cadence is needed to continuously validate trust boundaries and catch regressions before release.

## Goals

- Establish recurring threat-model refresh cycles.
- Build a permission and trust-boundary regression suite.
- Integrate security gates into release readiness.
- Define schedule and scope for external security testing.

## Non-Goals

- Replacing all existing testing with security-only checks.
- Creating a one-time security audit with no follow-up cadence.
- Blocking low-risk feature work on broad security rewrites.

## Design Options

### Option A: Annual large audit only

**Pros:**
- Lower ongoing process cost.
- Easy scheduling.

**Cons:**
- Long windows for undetected regressions.
- Weak integration with everyday development.

### Option B: Continuous cadence with quarterly deep checks (chosen)

**Pros:**
- Faster detection of boundary regressions.
- Better alignment with release cycles.
- Improves security learning loop.

**Cons:**
- Requires ownership and recurring capacity.
- Adds release-gate coordination effort.

## Chosen Approach

Run a layered program: per-PR security regression checks for critical boundaries, monthly threat-model deltas, and quarterly deep reviews with external validation planning.

### Architecture Outline

- **Threat model artifacts:** maintain domain models in `docs/security/`.
- **Regression suites:** server integration tests in `server/tests/` for auth, permissions, uploads, websocket event isolation.
- **CI integration:** security checks in `.github/workflows/` with release-blocking severity thresholds.
- **External testing:** annual penetration engagement scope anchored to current threat model.

### Implementation Planning (High Level)

1. **Threat-model baseline**
   - Create canonical threat-model docs by domain.
   - Define risk rating rubric and owner assignment.
2. **Boundary regression suite**
   - Add tests for channel visibility, attachment access, event isolation, and auth token flows.
   - Add negative tests for blocked/suspended/banned actors.
3. **Release gate integration**
   - Add mandatory security checklist for release candidates.
   - Define fail/waiver process with explicit sign-off.
4. **External validation cadence**
   - Prepare penetration testing statement of work.
   - Map findings into tracked remediation backlog.

### Security Considerations

- Security artifacts may contain sensitive architecture details; limit access appropriately.
- Ensure test fixtures and logs do not expose real secrets.
- Preserve immutable audit trail for waivers and exceptions.

### Performance Implications

- Partition heavy security suites between PR and nightly pipelines.
- Keep critical boundary checks fast enough for merge gating.

## Success Criteria

- Threat model updated at defined cadence with tracked deltas.
- Boundary regression suite runs in CI and blocks high-severity failures.
- Release security checklist required for production promotion.
- External security test findings are triaged with remediation owners.

## Open Questions

- Which boundary tests are required on every PR versus nightly runs?
- What severity threshold allows temporary waiver and for how long?

## References

- `docs/project/roadmap.md`
- `server/SECURITY.md`
