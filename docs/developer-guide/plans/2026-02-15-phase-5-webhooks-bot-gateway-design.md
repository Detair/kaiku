# Phase 5 Webhooks and Bot Gateway Expansion - Design

**Date:** 2026-02-15
**Status:** Draft
**Roadmap Scope:** Phase 5 `[Ecosystem] Webhooks & Bot Gateway`

## Problem

Bot platform basics exist, but webhook delivery reliability, event compatibility, and operator controls need expansion for production integrations.

## Goals

- Add robust webhook delivery with retries and dead-letter handling.
- Extend bot gateway intent/event model safely.
- Add operator visibility for integration health.

## Non-Goals

- Full plugin marketplace in this phase.
- Unbounded custom event schemas.

## Open Standards Profile

- CloudEvents 1.0 envelope for webhook payload metadata.
- HMAC SHA-256 signature headers for webhook authenticity.
- OpenAPI 3.1 contracts for webhook management endpoints.
- RFC 6455 websocket behavior for bot gateway sessions.

## Approach

Introduce a webhook dispatcher pipeline (queued delivery, exponential backoff retries, dead-letter events) and expand bot gateway intents with explicit permission and rate-limit controls.

## Success Criteria

- Webhook delivery success and retry behavior are measurable.
- Bot gateway supports expanded events without security regressions.
- Admins can diagnose failing integrations quickly.

## References

- `docs/project/roadmap.md`
- `server/src/ws/bot_gateway.rs`
