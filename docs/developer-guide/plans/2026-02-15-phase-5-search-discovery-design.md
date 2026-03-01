# Phase 5 Advanced Search and Bulk Read - Design

**Date:** 2026-02-15
**Status:** Draft
**Roadmap Scope:** Phase 5 `[UX] Advanced Search & Discovery`

## Problem

Search foundation is in place, but permission boundary hardening, scale edge cases, analytics, and bulk-read workflows remain unfinished.

## Goals

- Complete permission-safe search across guild and DM contexts.
- Add bulk mark-read management at category/guild/global levels.
- Strengthen security and scalability test coverage.

## Non-Goals

- Replacing PostgreSQL FTS foundation in this phase.
- Building external search-cluster architecture now.

## Open Standards Profile

- OpenAPI 3.1 contracts for search and bulk-read APIs.
- SQL query safety and escaping patterns documented and tested.
- Telemetry and analytics events emitted with OpenTelemetry fields.

## Approach

Close remaining permission and security gaps first, then complete bulk-read APIs and UX. Add scale-focused test suite and operational analytics for long-term tuning.

## Success Criteria

- Search results never leak unauthorized channels/messages.
- Bulk read operations are fast, idempotent, and auditable.
- Scale and security tests cover defined edge conditions.

## References

- `docs/project/roadmap.md`
- `server/tests/search_http_test.rs`
