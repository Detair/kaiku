# Slash Command Reliability & /ping Reference Command - Design

**Date:** 2026-02-16
**Status:** Approved
**Priority:** High
**Roadmap Scope:** Phase 5 slash command hardening
**Supersedes:** `2026-02-15-phase-5-slash-command-reliability-design.md` (draft)

## Problem

Slash commands are functional but have correctness and reliability gaps:

1. **Response delivery is broken end-to-end.** Bots store responses in Redis, but nothing relays them to the invoking user. Users never see command output.
2. **Listing hides ambiguity that invocation rejects.** `DISTINCT ON` in guild command listing deduplicates by name with a different sort order than invocation, so the autocomplete shows one bot but invocation may pick another or fail.
3. **Global command uniqueness is not enforced** at the DB level for nullable `guild_id` rows.
4. **Frontend UX issues:** hyphen not allowed in autocomplete trigger, fetch retry lockout, empty popup keyboard trap.

## Goals

- Users reliably receive command responses (or deterministic timeout errors).
- List and invoke behavior are consistent in multi-bot scenarios.
- Registration rejects duplicate names with typed errors.
- Ship built-in `/ping` as a smoke-test command and an example bot script for developers.

## Non-Goals

- Full command framework redesign.
- Cross-guild command federation.
- Rich command-option type validation beyond basic parsing.

## Open Standards Profile

| Domain | Standard |
|--------|----------|
| Realtime transport | WebSocket RFC 6455 for bot gateway and user interaction updates |
| API contracts | OpenAPI 3.1 for command registration/listing/invocation endpoints |
| Payload schema | JSON Schema-compatible option structures |
| Observability | OpenTelemetry events, W3C trace-context propagation |
| Security | Deterministic authz checks, replay-safe interaction ownership |

## Design Decisions

### 1) Registration & Uniqueness

- **DB migration:** Add partial unique index `(application_id, name) WHERE guild_id IS NULL` for global commands.
- **Batch validation:** Reject registration requests containing duplicate names within the same batch (checked via `HashSet` before DB insert).
- **Error mapping:** DB uniqueness violations return typed `409 Conflict` responses.

### 2) Listing & Ambiguity Consistency

- **Remove `DISTINCT ON`** from `list_guild_commands` query.
- **Return all commands** from all installed bots, annotated with `bot_name`, `application_id`.
- **Add `is_ambiguous` flag** to each entry (true when multiple bots provide the same command name at the same priority level).
- **Frontend autocomplete** shows separate entries per bot for ambiguous commands: `"/ping - PingBot"`, `"/ping - UtilBot"`.
- **Ambiguity error message** includes conflicting bot names: `"Command '/ping' is ambiguous: provided by PingBot, UtilBot"`.

Response shape:
```json
[
  {"name": "ping", "description": "...", "bot_name": "PingBot", "application_id": "...", "is_ambiguous": false}
]
```

### 3) Response Delivery (Critical Path)

**Mechanism:** WebSocket relay via existing user WS connection.

**Flow:**
1. User sends `/command` in guild channel.
2. Server invocation handler publishes `command_invoked` to bot, then spawns a short-lived tokio task subscribed to `interaction:{id}` in Redis.
3. Bot processes and sends `command_response` via gateway, which stores response in Redis and publishes to `interaction:{id}`.
4. The relay task receives the response and delivers it through the user's existing WebSocket.

**Non-ephemeral responses:**
- Server creates a real message record authored by the bot in the channel.
- Broadcast via normal `guild:{guild_id}` channel event.
- Becomes a permanent, visible chat message.

**Ephemeral responses:**
- Delivered only to the invoking user via `user:{user_id}` WebSocket channel.
- Shown inline with "Only you can see this" label.
- Not persisted to DB.

**New `ServerEvent` variants:**
- `CommandResponse { interaction_id, content, command_name, bot_name, ephemeral }` - delivered to invoking user (ephemeral) or channel (non-ephemeral).
- `CommandResponseTimeout { interaction_id, command_name }` - delivered after 30s with no bot response.

**Frontend:**
- "Thinking..." indicator in chat when command is processing.
- `CommandResponseMessage` component for inline bot responses (bot badge, distinct styling).
- Ephemeral responses show "Only you can see this" disclaimer.

**Timeout:** 30 seconds. Relay task sends `CommandResponseTimeout` event to user, then cleans up.

### 4) Bot Gateway Error Contract

Emit structured `error` events for all failure modes:

| Code | Trigger |
|------|---------|
| `invalid_json` | Unparseable client message |
| `unknown_event` | Unrecognized event type |
| `interaction_not_found` | Response for expired/unknown interaction |
| `unauthorized_channel` | Message to channel bot isn't member of |
| `rate_limited` | Rate limit exceeded (existing) |

Shape: `{"type": "error", "code": "...", "message": "..."}`.

### 5) Frontend Reliability Fixes

- **Autocomplete trigger:** Allow hyphens in regex (currently `[a-z0-9_]`, add `-`).
- **Fetch retry:** If command fetch fails, allow retry on next `/` keystroke instead of permanent lockout.
- **Empty popup:** Handle zero-match state gracefully (no keyboard trap).
- **Settings debounce:** Debounce slash command settings actions to prevent double-submit.

### 6) Reference Command: /ping

**Built-in `/ping`:**
- Server handles `/ping` natively, before bot routing.
- Returns `"Pong!"` with `latency_ms` field as a non-ephemeral message.
- Available in any guild without bot installation.
- Implemented as special-case check in invocation path.

**Example bot script:**
- `docs/examples/ping-bot.py` - standalone Python script using `websocket-client`.
- Demonstrates full lifecycle: create app, register command, connect gateway, handle events.
- Referenced from `docs/development/bot-system.md`.

## Security & Abuse Controls

- Guild membership and bot installation checks before invocation routing (existing).
- Per-category rate limits on invoke and response paths (existing).
- Command responses cannot be spoofed across bot identities (Redis ownership check, existing).
- 30-second relay timeout prevents resource leaks from unresponsive bots.

## Observability

- Tracing events: `command_invoked`, `command_ambiguous`, `command_response_delivered`, `command_response_timeout`.
- Command latency histograms and failure counters by guild/bot/command name (cardinality-budgeted).

## Success Criteria

1. No duplicate global command rows for same app+name (DB enforced).
2. List and invoke behavior are consistent; autocomplete shows all providers for ambiguous commands.
3. Users always receive command responses or deterministic timeout errors within 30s.
4. `/ping` works in any guild, covered by integration tests.
5. Example bot script in docs demonstrates full bot lifecycle.

## References

- `server/src/api/commands.rs` - registration handlers
- `server/src/chat/messages.rs:434-581` - invocation routing
- `server/src/ws/bot_gateway.rs` - gateway and response handling
- `server/src/guild/handlers.rs:787-823` - guild command listing
- `server/migrations/20260202204100_bot_ecosystem.sql` - schema
- `server/tests/bot_ecosystem_test.rs` - existing test suite
- `client/src/components/messages/MessageInput.tsx` - autocomplete
- `client/src/components/messages/AutocompletePopup.tsx` - popup rendering
- `docs/development/bot-system.md` - developer docs
