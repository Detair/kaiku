# Phase 6 Focus Engine and Digital Library - Design

**Date:** 2026-02-15
**Status:** Draft
**Roadmap Scope:** Phase 6 `[UX] Context-Aware Focus Engine`, `[SaaS] The Digital Library (Wiki Mastery)`

## Problem

Users with high message volume need stronger focus controls, and guilds need long-lived knowledge management beyond current information pages.

## Goals

- Provide context-aware notification routing with explicit override policies.
- Evolve information pages into a searchable, versioned digital library.
- Keep privacy boundaries explicit for foreground-context signals.

## Non-Goals

- Collecting intrusive desktop telemetry beyond explicit consent.
- Full enterprise document suite replacement.
- AI-generated knowledge workflows in v1.

## Open Standards Profile

- **Notifications:** Web Notifications model and platform notification best practices.
- **Content Format:** CommonMark-compatible markdown for library documents.
- **Deep Links:** URL fragment and route-based anchors for stable section links.
- **Search Contract:** OpenAPI 3.1 documented query and filter schema.
- **Telemetry:** OpenTelemetry spans/metrics for routing and library interactions.

## Architecture (High Level)

- Focus engine service:
  - Input signals: user-set focus modes + optional foreground app category.
  - Policy matrix: default suppression rules + VIP/Emergency bypass list.
  - Output: ranked delivery policy for notification channels.
- Digital library service:
  - Document revisions and rollback pointers.
  - Section anchor index and shareable deep links.
  - Library catalog view grouped by guild collections.

## Security and Privacy

- Foreground context is opt-in and locally processed where possible.
- VIP override configuration is explicit and auditable.
- Library permission checks enforce guild role boundaries on read/write.

## Performance Constraints

- Focus-policy decisions must be O(1) or cached per session.
- Library indexing must not degrade message-path latency.
- Deep-link navigation should remain instant with lazy section hydration.

## Success Criteria

- Focus routing reduces notification noise without missed critical alerts.
- Library supports version recovery and section-deep-link navigation.
- Permissions and privacy settings are enforceable and test-covered.

## References

- `docs/project/roadmap.md`
- `docs/plans/2026-02-15-roadmap-gap-closure-implementation.md`
