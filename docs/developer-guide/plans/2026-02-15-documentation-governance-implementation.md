# Documentation Governance - Implementation Plan

**Date:** 2026-02-15
**Status:** Draft
**Design Reference:** `docs/plans/2026-02-15-documentation-governance-design.md`

## Objective

Operationalize roadmap/changelog consistency, superseded-plan lifecycle hygiene, and standardized milestone release-note generation as enforced workflows.

## Delivered Baseline

- Added automated checker: `scripts/check_docs_governance.py`.
- Added release-note generator: `scripts/generate_release_notes.py`.
- Added lifecycle registry: `docs/plans/PLAN_LIFECYCLE.md`.
- Added release template: `docs/project/RELEASE_NOTES_TEMPLATE.md`.
- Added CI and release workflow integration for governance checks.

## Open Standards Enforcement

- Changelog and release notes follow Keep a Changelog categories.
- Structured markdown templates use stable, machine-checkable headings.
- Governance checks run in CI as deterministic scripts.

## Implementation Phases

### Phase A - Consistency automation

1. Validate roadmap metadata against changelog alignment block.
2. Validate roadmap relative plan links resolve.
3. Fail CI on mismatch or missing references.

### Phase B - Lifecycle governance

1. Maintain `PLAN_LIFECYCLE.md` as canonical supersession map.
2. Require `Lifecycle` and `Supersedes/Superseded By` headers in replaced plans.
3. Validate lifecycle integrity in checker.

### Phase C - Release-note standardization

1. Generate milestone notes from `CHANGELOG.md` `[Unreleased]`.
2. Enforce template section completeness.
3. Publish generated release notes in release workflow output.

## Verification

- `python3 scripts/check_docs_governance.py`
- `python3 scripts/generate_release_notes.py --version v0.0.0-test --output /tmp/release-notes.md`

## Done Criteria

- Governance checks are required in CI/release workflows.
- Superseded plan relationships are explicit and validated.
- Release notes are generated in one consistent milestone format.
