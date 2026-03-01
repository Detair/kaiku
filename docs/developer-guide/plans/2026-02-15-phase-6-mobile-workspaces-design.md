# Phase 6 Mobile and Workspaces - Design

**Date:** 2026-02-15
**Status:** Draft
**Roadmap Scope:** Phase 6 `[Client] Mobile Support`, `[UX] Personal Workspaces (Favorites v2)`

## Problem

Core collaboration is desktop-first today. Users still need a first-class mobile experience and a better way to organize channels across many guilds without context switching overhead.

## Goals

- Define a practical mobile architecture path that reuses existing API contracts.
- Introduce personal workspaces as cross-guild channel collections.
- Preserve permission boundaries and realtime consistency.
- Keep client performance and startup behavior within existing targets.

## Non-Goals

- Rebuilding every desktop interaction for mobile in one release.
- Adding server-side shared workspaces in phase one (user-local ownership first).
- Breaking existing navigation flows.

## Open Standards Profile

- **API Contracts:** OpenAPI 3.1 for mobile-facing endpoint definitions.
- **Auth:** OAuth 2.1 profile + OpenID Connect; JWT with short-lived access tokens.
- **Realtime:** WebSocket (RFC 6455) with existing event model and reconnect semantics.
- **Telemetry:** OpenTelemetry + OTLP for mobile/network performance signals.
- **Push (future):** Web Push / platform-native push adapters behind a common abstraction.

## Architecture (High Level)

- Continue using existing Rust backend APIs and websocket events.
- Introduce a workspace domain model:
  - `workspace` (id, owner_user_id, name, icon, sort_order)
  - `workspace_entry` (workspace_id, guild_id, channel_id, position)
- Add server endpoints for CRUD + reorder operations.
- Add client-side workspace manager integrated into current side rail/navigation stack.

## Security and Privacy

- Workspace entries must be validated against current membership + channel access.
- No cross-guild data leakage in aggregated views.
- Audit workspace mutations for abuse diagnostics.

## Performance Constraints

- Minimize initial workspace hydration payload.
- Batch workspace channel metadata lookups to avoid N+1 queries.
- Preserve smooth channel switching with optimistic UI updates.

## Success Criteria

- Mobile adaptation strategy selected and documented with migration path.
- Users can create/edit/reorder personal workspaces and entries.
- Access control checks enforce guild/channel permissions at all times.
- Aggregated workspace view remains responsive at high channel counts.

## References

- `docs/project/roadmap.md`
- `docs/plans/2026-02-15-roadmap-gap-closure-implementation.md`
