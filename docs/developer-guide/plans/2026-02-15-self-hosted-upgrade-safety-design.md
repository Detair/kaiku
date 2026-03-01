# Self-Hosted Upgrade Safety Framework - Design

**Date:** 2026-02-15
**Status:** Placeholder (Planned)
**Roadmap Item:** Phase 8 - Self-Hosted Upgrade Safety Framework

## Problem

Self-hosted operators need predictable, low-risk upgrade paths. Without preflight validation and rollback guidance, upgrades can cause avoidable outages or data inconsistency.

## Goals

- Provide a pre-upgrade validation checklist and automated preflight checks.
- Define compatibility matrix for server, client, schema, and infra dependencies.
- Standardize rollback procedures and backup prerequisites.

## Initial Scope

- Version compatibility matrix by release.
- Preflight checks for DB migration readiness and config validity.
- Rollback flow with post-rollback integrity checks.

## Non-Goals

- Supporting every historical version as an upgrade source.
- Fully automated zero-touch upgrades in phase one.

## Open Questions

- How many previous major/minor versions will be officially supported for direct upgrade?
- Which preflight checks are blocking vs warning-only?

## References

- `docs/project/roadmap.md`
- `docs/plans/2026-02-15-backup-restore-drills-design.md`
