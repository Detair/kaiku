# Phase 5 Slash Command Reliability and /ping Reference Command - Implementation Plan

**Date:** 2026-02-15
**Status:** Draft
**Priority:** High
**Design Reference:** `docs/plans/2026-02-15-phase-5-slash-command-reliability-design.md`

## Objective

Close identified `/ command` reliability gaps and ship a stable, standards-aligned reference command path (`/ping`) with end-to-end coverage.

## Open Standards Enforcement

- OpenAPI 3.1 updates for command APIs and response contracts.
- RFC 6455-compliant websocket event behavior for bot and user-facing interaction updates.
- JSON Schema-compatible validation for command options and request payloads.
- OpenTelemetry instrumentation for command lifecycle and failure modes.

## Implementation Phases

### Phase A - Backend correctness hardening (P0)

1. Add migration for global command uniqueness:
   - unique index for `(application_id, name)` where `guild_id IS NULL`.
2. Add duplicate-name preflight validation in `register_commands`.
3. Map uniqueness/validation collisions to typed `409` responses.
4. Add tests for duplicate global and guild-scoped registration cases.

### Phase B - Ambiguity and listing consistency (P0)

1. Update guild command listing to expose duplicate providers instead of hiding with `DISTINCT ON`.
2. Extend response model with scope/provider context for UI disambiguation.
3. Keep invocation ambiguity rejection and align error text with UI behavior.
4. Add tests for list-vs-invoke consistency with multi-bot same-name scenarios.

### Phase C - Response delivery completion (P0)

1. Implement user-facing interaction response channel consumer path.
2. Deliver non-ephemeral command responses into channel-visible message/event flow.
3. Deliver ephemeral responses only to invoking user.
4. Add timeout/failure handling when response is missing after owner-key TTL.

### Phase D - Gateway and frontend reliability fixes (P1)

1. Emit explicit bot `error` events for parse/validation/rate-limit failures.
2. Reuse existing pooled Redis client in command invocation path.
3. Frontend fixes:
   - allow hyphen in slash autocomplete trigger,
   - retry-friendly command fetch state,
   - safe keyboard handling for empty popup lists,
   - prevent double-submit races in slash settings actions.
4. Add frontend unit/E2E tests for slash autocomplete and keyboard behavior.

### Phase E - `/ping` reference command and regression harness (P1)

1. Define canonical `/ping` command payload and expected response schema.
2. Add integration tests for invoke -> bot response -> user-visible delivery.
3. Add docs example and smoke-test playbook for operators.
4. Add canary metric/alert for command-response timeout spikes.

## File Targets

- `server/migrations/`
- `server/src/api/commands.rs`
- `server/src/chat/messages.rs`
- `server/src/guild/handlers.rs`
- `server/src/ws/bot_gateway.rs`
- `server/tests/bot_ecosystem_test.rs`
- `client/src/components/messages/MessageInput.tsx`
- `client/src/components/ui/PopupList.tsx`
- `client/src/pages/settings/BotSlashCommands.tsx`
- `docs/development/bot-system.md`

## Verification

```bash
cargo test --test bot_ecosystem_test
cargo test --test websocket_integration_test
cd client && bun run test:run
python3 scripts/check_docs_governance.py
```

## Done Criteria

- Blocking issues from slash-command review are resolved and test-covered.
- `/ping` reference flow is stable in automated tests and manual smoke checks.
- Observability and error contracts are in place for command operations.
