# Phase 5 Slash Command Reliability and /ping Reference Command - Design

**Date:** 2026-02-15
**Status:** Draft
**Priority:** High
**Roadmap Scope:** Phase 5 `/ command` feature hardening

## Problem

Slash commands are functional, but review findings show correctness and reliability gaps in command uniqueness, ambiguity behavior, response delivery, and frontend UX consistency.

## Goals

- Make slash command registration and invocation deterministic and safe.
- Ensure users reliably receive command responses.
- Align list behavior with invoke behavior in duplicate-command scenarios.
- Improve autocomplete and keyboard UX reliability.
- Ship a reference `/ping` command path for smoke testing and regression checks.

## Non-Goals

- Full command framework redesign.
- Cross-guild command federation in this phase.
- Rich command-option type system overhaul beyond required validation baseline.

## Open Standards Profile

- **Realtime transport:** WebSocket RFC 6455 semantics for bot gateway and interaction updates.
- **API contracts:** OpenAPI 3.1 for command registration/listing/invocation-related endpoints.
- **Payload schema:** JSON Schema-compatible option structures and validation rules.
- **Observability:** OpenTelemetry events and W3C trace-context propagation for command lifecycle.
- **Security:** deterministic authz checks + replay-safe interaction ownership semantics.

## Core Risks Identified

1. Global command-name uniqueness is not guaranteed for nullable `guild_id` rows.
2. Command list deduplication can hide ambiguity that invoke path rejects.
3. Command-response path stores/publishes responses, but end-user delivery path is incomplete.
4. Frontend slash UX has reliability issues (hyphen autocomplete mismatch, fetch retry lockout, empty-popup keyboard edge case).

## Design Decisions

### 1) Registration and uniqueness

- Add explicit DB uniqueness for global commands: `(application_id, name) WHERE guild_id IS NULL`.
- Add request-level duplicate-name validation in registration API.
- Map uniqueness conflicts to typed `409 Conflict` responses.

### 2) Ambiguity model

- Keep invocation rule: if multiple same-priority matches exist, fail with explicit ambiguity error.
- Change list endpoint and autocomplete to **surface** duplicates with bot context instead of hiding them.
- Add clear UX for ambiguous names (show provider bot and scope).

### 3) Interaction response delivery

- Define and implement end-user response channel for command invocations:
  - non-ephemeral responses become visible message/system events in channel,
  - ephemeral responses are delivered only to invoking user via user-scoped realtime event.
- Keep Redis ownership check and single-response NX behavior.

### 4) Bot gateway error contract

- Emit explicit `error` events to bot socket on parse/validation/rate-limit failures.
- Keep server logs and tracing aligned with sent error events for debuggability.

### 5) Reference command (`/ping`)

- Add `/ping` as a canonical reference command profile used for tests/docs/smoke checks.
- Output includes deterministic payload (`pong`, timestamp/latency field) for stable validation.

## Security and Abuse Controls

- Enforce guild membership and installation checks before invocation routing.
- Keep per-category rate limits on invoke and bot gateway response paths.
- Ensure command responses cannot be spoofed across bot identities (existing owner checks retained).

## Observability Requirements

- Add command lifecycle events: `command_invoked`, `command_ambiguous`, `command_response_delivered`, `command_response_timeout`.
- Emit command latency histograms and failure counters by guild/bot/command name (cardinality budgeted).

## Success Criteria

- No duplicate global command rows can be inserted for same app+name.
- List and invoke behavior are consistent and understandable in duplicate scenarios.
- Invokers always receive command responses or deterministic timeout errors.
- `/ping` path is fully covered by integration and E2E tests.

## References

- `docs/project/roadmap.md`
- `server/src/api/commands.rs`
- `server/src/chat/messages.rs`
- `server/src/guild/handlers.rs`
- `server/src/ws/bot_gateway.rs`
- `client/src/components/messages/MessageInput.tsx`
- `client/src/components/ui/PopupList.tsx`
