# Documentation Governance - Design

**Date:** 2026-02-15
**Status:** Draft
**Roadmap Item:** Documentation governance (canonical feature matrix and active/superseded plan lifecycle)

## Problem

Roadmap, changelog, and plan files can drift over time, especially when v2 documents supersede earlier plans. This creates ambiguity about current status, canonical references, and ownership.

## Goals

- Define a single source-of-truth feature matrix.
- Add lifecycle metadata for plan documents (active, superseded, archived).
- Standardize status language across roadmap, plans, and changelog.
- Add lightweight validation checks in CI.

## Non-Goals

- Rewriting all historical documents immediately.
- Introducing heavy process bureaucracy for small updates.
- Blocking emergency fixes on non-critical docs cleanup.

## Design Options

### Option A: Manual doc hygiene only

**Pros:**
- No tooling effort.
- Flexible for contributors.

**Cons:**
- Drift is likely to reappear.
- Hard to audit and enforce consistently.

### Option B: Metadata + matrix + automation checks (chosen)

**Pros:**
- Improves consistency and discoverability.
- Creates clear canonical references.
- Scales with project growth.

**Cons:**
- Requires initial migration effort.
- Adds small CI maintenance burden.

## Chosen Approach

Create a documentation governance baseline with required front-matter metadata, a generated feature matrix, and consistency checks between roadmap/changelog/plans.

### Architecture Outline

- **Metadata schema:** document status, owner, supersedes/superseded-by, last validated date.
- **Feature matrix:** `docs/project/feature-matrix.md` with feature -> status -> canonical docs.
- **Validation tooling:** script-based checks in CI for stale references and broken links.

### Implementation Planning (High Level)

1. **Schema definition**
   - Define front-matter keys and accepted status values.
   - Publish contribution guidance in docs.
2. **Matrix bootstrap**
   - Seed canonical matrix from roadmap and active plans.
   - Mark superseded plans with explicit cross-links.
3. **Automation checks**
   - Add link checker and metadata linter.
   - Add drift check for roadmap item references.
4. **Operational cadence**
   - Add monthly doc reconciliation routine.
   - Track exceptions and remediation owners.

### Security Considerations

- Ensure operational docs do not leak sensitive internal details.
- Add reviewer policy for security-relevant document changes.

### Performance Implications

- Keep documentation CI checks incremental and fast.
- Prefer deterministic scripts to avoid flaky validation jobs.

## Success Criteria

- Every active roadmap item links to at least one canonical plan/design doc.
- Superseded plans are explicitly marked and discoverable.
- CI rejects broken references and invalid status metadata.
- Monthly doc reconciliation performed and recorded.

## Open Questions

- Should feature-matrix generation be fully automated or semi-curated?
- What is the minimal required metadata for legacy documents?

## References

- `docs/project/roadmap.md`
- `CHANGELOG.md`
