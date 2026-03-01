# Sovereign and BYO Deployment Options - Design

**Date:** 2026-02-15
**Status:** Draft
**Roadmap Item:** Sovereign/BYO deployment options (customer-controlled storage and relay paths)

## Problem

Target users include self-hosted and sovereignty-sensitive communities that require stronger control over storage location, relay paths, and operational trust boundaries than a single default deployment model provides.

## Goals

- Define deployment profiles for standard, sovereign, and BYO modes.
- Allow customer-controlled storage and relay provider configuration.
- Preserve security guarantees across deployment profiles.
- Keep operational complexity manageable for administrators.

## Non-Goals

- Building full federation protocol support in this phase.
- Supporting unlimited provider permutations from day one.
- Sacrificing baseline security controls for flexibility.

## Design Options

### Option A: One canonical deployment model

**Pros:**
- Simplest to build and support.
- Lowest configuration complexity.

**Cons:**
- Poor fit for sovereignty requirements.
- Limits enterprise and regulated adoption.

### Option B: Profile-driven deployment with provider abstractions (chosen)

**Pros:**
- Supports self-hosted/SaaS dual strategy.
- Enables controlled flexibility with guardrails.
- Creates path toward future sovereign feature set.

**Cons:**
- Adds configuration validation complexity.
- Requires stronger operational documentation.

## Chosen Approach

Introduce explicit deployment profiles and provider abstraction boundaries so storage/relay components can be customer-controlled while core auth, policy, and audit controls remain consistent.

### Architecture Outline

- **Profile model:** deployment modes in config (`standard`, `sovereign`, `byo`).
- **Provider abstraction:** storage and relay interfaces with capability declarations.
- **Policy controls:** enforce region and provider constraints at config validation time.
- **Operations:** deployment guides and compatibility matrix in `docs/deployment/`.

### Implementation Planning (High Level)

1. **Profile and capability schema**
   - Define profile contract and provider capability model.
   - Add startup validation and failure-safe defaults.
2. **Provider integration baseline**
   - Implement first-party adapters for supported storage/relay backends.
   - Add conformance tests for provider behavior.
3. **Policy and governance controls**
   - Add region/policy enforcement in admin configuration workflows.
   - Add audit events for profile/provider changes.
4. **Operationalization**
   - Publish deployment runbooks and compatibility docs.
   - Add migration guidance between profiles where safe.

### Security Considerations

- Validate provider endpoints and credentials strictly.
- Keep key management controls independent of provider trust level.
- Ensure audit visibility for all profile and routing changes.

### Performance Implications

- Provider abstraction should avoid adding hot-path latency.
- Profile-specific routing may alter latency and should be measured.

## Success Criteria

- Deployment profiles are selectable with validated constraints.
- Supported provider adapters pass conformance and security tests.
- Sovereign/BYO deployment docs are complete for administrators.
- Policy/audit controls remain consistent across profiles.

## Open Questions

- Which provider combinations are officially supported in phase one?
- How should profile migration be handled when data locality constraints differ?

## References

- `docs/project/roadmap.md`
- `ARCHITECTURE.md`
