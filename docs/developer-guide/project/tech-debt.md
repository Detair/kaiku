# Tech Debt Inventory

**Last audited:** 2026-02-19
**Branch:** `chore/tech-debt`

This document catalogs all known tech debt across the Kaiku codebase, sourced from:
- In-code markers (TODO, FIXME, HACK)
- Roadmap unchecked items and documented known limitations
- Code quality scan (unwrap/expect in production, lint suppressions, type safety gaps)
- Open GitHub issues referenced in docs

---

## Priority: High

### TD-01: Megolm E2EE stubs (unimplemented!) ✅ RESOLVED

**Files:** `shared/vc-crypto/src/megolm.rs:9,39`
**Resolved:** 2026-02-19 — Gated behind `#[cfg(feature = "megolm")]` compile-time feature flag. Both `GroupSession` and `InboundGroupSession` structs and their impls are now excluded from compilation unless the `megolm` feature is explicitly enabled, preventing runtime panics.

---

### TD-02: Search — channel-level permission filtering missing ✅ RESOLVED

**File:** `docs/project/roadmap.md:519`
**Resolved:** 2026-02-19 — Investigation revealed channel-level VIEW_CHANNEL permission filtering was already implemented in guild search, DM search, and global search. Added 10 integration tests (TD-08) to verify the permission enforcement.

---

### TD-03: Security advisory workaround (RUSTSEC-2026-0002)

**File:** `deny.toml:20`

```toml
# TODO(2026-Q2): Re-check if aws-sdk-s3 has updated lru to >= 0.16.3
```

Transitive dependency vulnerability from `aws-sdk-s3 → lru`. Scheduled review Q2 2026.

**Risk:** Known CVE in dependency tree.
**Fix:** Check upstream and bump when available.

---

### TD-04: E2EE key store not wired up ✅ RESOLVED

**Files:** `client/src/components/E2EESetupPrompt.tsx:128`, `client/src/components/settings/SettingsModal.tsx:79`
**Resolved:** 2026-02-19 — Backup data now includes real identity keys and prekeys from `initE2EE()` instead of timestamp-only placeholders. Users can now meaningfully restore E2EE sessions from backups.

---

### TD-05: WebSocket response builders use `.expect()` ✅ RESOLVED

**File:** `server/src/ws/mod.rs:945,956,966`
**Resolved:** 2026-02-19 — Extracted `error_response(status: u16, body: &'static str) -> Response` helper function that returns proper HTTP error responses instead of panicking. All three WebSocket upgrade error paths now use this helper.

---

## Priority: Medium

### TD-06: MFA backup codes not implemented ✅ RESOLVED

**File:** `server/src/auth/AGENTS.md:83`
**Resolved:** 2026-02-19 — Full implementation: `POST /api/auth/mfa/backup-codes` generates 10 single-use alphanumeric codes hashed with Argon2id. Login flow tries backup codes on TOTP failure. Database migration adds `mfa_backup_codes` table with `used_at` soft-delete pattern.

---

### TD-07: Test infrastructure improvements (Issues #137-#140)

**Source:** `docs/project/roadmap.md:308-309`

- Issue #137: Test cleanup guards (prevent leaked state between tests)
- Issue #138: Shared DB pool (reduce test setup overhead)
- Issue #139: Stateful middleware testing
- Issue #140: HTTP-level concurrent setup completion test

---

### TD-08: Search edge case and security tests missing ✅ RESOLVED

**Source:** `docs/project/roadmap.md:521-529`
**Resolved:** 2026-02-19 — Added 10 integration tests in `search_http_test.rs`: special characters, SQL injection prevention, XSS content handling, query length validation (>1000 chars), pagination verification, and 4 channel permission filtering scenarios. Also added query length validation (max 1000 chars) to guild, DM, and global search endpoints.

---

### TD-09: Console.log flood in production client code ✅ RESOLVED

**Key files:**
- `client/src/lib/webrtc/browser.ts` — 43+ `console.log` statements
- `client/src/lib/webrtc/tauri.ts` — 26+ statements
- `client/src/lib/tauri.ts` — 16+ statements
- `client/src/lib/sound/ring.ts` — 3 statements
- `client/src/components/SetupWizard.tsx` — 1 statement

**Resolved:** 2026-02-19 — Configured Vite esbuild `pure` option to strip `console.log` and `console.debug` in production builds while preserving `console.error` and `console.warn` for diagnostics. This is a zero-code-change solution that works at the build level.

---

### TD-10: `clippy::too_many_arguments` suppressions (11 functions) ✅ RESOLVED

**Files:**
- `server/src/ws/mod.rs:1159,1365`
- `server/src/voice/ws_handler.rs:534`
- `server/src/db/queries.rs:414,858,1752,1801`
- `server/src/pages/queries.rs:226,260`
- `server/src/api/mod.rs:68`

**Resolved:** 2026-02-19 — Introduced parameter structs: `AppStateConfig`, `HandlePubsubParams`, `HandleScreenShareStartParams`, `CreateChannelParams`, `CreateThreadReplyParams`, `CreateOidcProviderParams`, `UpdateOidcProviderParams`, `CreatePageParams`, `UpdatePageParams`. All `#[allow(clippy::too_many_arguments)]` suppressions removed.

---

### TD-11: `#[allow(dead_code)]` suppressions (11 instances) ✅ RESOLVED

**Files:**
- `server/src/ws/mod.rs:749,768` — Removed unused `GLOBAL_EVENTS` constant; kept `user_presence()` (actually used)
- `server/src/voice/signaling.rs:12,26` — Deleted entire file (unused types from early architecture)
- `server/src/voice/sfu.rs:220,737` — Removed unused `broadcast_all` and `room_count` methods
- `server/src/chat/messages.rs:26` — Removed unused `MessageError` variant
- `server/src/chat/channels.rs:21` — Removed unused `ChannelError` variant
- `server/src/admin/middleware.rs:13` — Removed unused struct field
- `server/src/admin/handlers.rs:237-244` — Removed unused session record fields

**Resolved:** 2026-02-19 — All truly dead code removed, all `#[allow(dead_code)]` suppressions eliminated.

---

### TD-12: TypeScript type safety gaps ✅ RESOLVED

**Production code:**
- `client/src/components/home/DMConversation.tsx:134` — Removed `@ts-ignore`, properly typed file input handler
- `client/src/stores/websocket.ts:886` — Removed `as any`, added proper `VoiceUserStats` interface
- `client/src/lib/tauri.ts:161-163` — Removed `as any`, used proper `Record<string, unknown>` type
- `client/src/lib/webrtc/browser.ts:601` — Removed `as any`, used `HTMLMediaElement` with `setSinkId` type assertion
- `client/src/lib/sound/browser.ts:173` — Removed `as any`, used `window as unknown as { webkitAudioContext: ... }` pattern
- `client/src/components/SetupWizard.tsx:319` — Removed `as any`, typed select handler
- `client/src/components/settings/NotificationSettings.tsx:51,54` — Removed `as any`, used proper `SoundOption` type

**Resolved:** 2026-02-19 — All 7 production `as any` casts and the 1 `@ts-ignore` replaced with proper types. Test code casts kept (acceptable for mocks).

---

### TD-13: Frontend/backend file size limit sync ✅ RESOLVED

**Source:** `docs/plans/2026-01-29-unified-file-size-limits.md:884`
**Resolved:** 2026-02-19 — Investigation confirmed this was already implemented: client fetches limits from `GET /api/config/upload-limits` at startup and applies them client-side via `validateFileSize()` helper.

---

### TD-14: Guild invite test stubs (blocked on features)

**File:** `server/tests/guild_invite_test.rs:412,422,432`

Three `#[ignore]` tests waiting on:
- `max_uses` field on `GuildInvite`
- Bans table implementation
- Guild suspension implementation

---

### TD-15: Tauri native webcam capture not implemented

**Source:** `docs/project/roadmap.md:418`

Multi-stream works in browser but Tauri has no native webcam commands (`start_webcam`/`stop_webcam`).

---

### TD-16: Tauri WebRTC connection metrics stub

**File:** `client/src/lib/webrtc/tauri.ts:194`

```typescript
// TODO: Add Tauri command to fetch native WebRTC connection stats
```

`getConnectionMetrics()` returns null — connectivity monitor is broken in Tauri.

---

### TD-17: Admin elevation detection stub ✅ RESOLVED

**File:** `client/src-tauri/src/commands/admin.rs:160`
**Resolved:** 2026-02-19 — Changed admin status check from `/api/admin/health` (plain text, no elevation info) to `/api/admin/status` (JSON response with `is_elevated` field). Admin dashboard now correctly shows actual elevation state.

---

## Priority: Low

### TD-18: Swagger UI not wired up

**File:** `server/src/api/mod.rs:334`

```rust
// TODO: Setup utoipa swagger-ui
```

API docs route function is a stub.

---

### TD-19: Screen share limit not configurable per channel

**File:** `server/src/voice/ws_handler.rs:583`

```rust
// TODO: Get max_screen_shares from channel settings
```

Uses `DEFAULT_MAX_SCREEN_SHARES` constant instead of per-channel config.

---

### TD-20: Window focus check missing for notifications ✅ RESOLVED

**File:** `client/src/stores/websocket.ts:93`
**Resolved:** 2026-02-19 — Added `&& !document.hidden` check to the notification sound trigger condition. Sounds now only play when both the channel is not selected AND the window is not focused.

---

### TD-21: Toast component rendering tests missing

**File:** `client/src/components/ui/__tests__/Toast.test.ts:5`

API tests exist (16 passing), but no `@solidjs/testing-library` component rendering tests.

---

### TD-22: Spoiler reveal state not persistent ✅ RESOLVED

**Source:** `docs/plans/2026-01-29-spoilers-mentions-implementation.md:798`
**Resolved:** 2026-02-19 — Added module-level `revealedSpoilers` signal with `Set<string>` in `MessageItem.tsx`. Revealed spoilers persist across virtual scroll remounts (the Set lives outside the component lifecycle).

---

### TD-23: Home unread aggregator capped at 100 channels

**Source:** `docs/plans/2026-01-29-home-unread-aggregator-implementation.md:1024`

Users with >100 unread channels only see the first 100. No pagination or "Show More".

---

### TD-24: Plain text emails only (forgot password)

**Source:** `docs/plans/2026-01-29-forgot-password-implementation.md:1137`

Password reset emails are plain text. HTML alternative with `lettre::message::MultiPart` planned.

---

### TD-25: Windows Tauri build broken

**Source:** `docs/project/roadmap.md:397`

`libvpx` not available via choco. CI job marked `continue-on-error: true`.

---

### TD-26: Moderation filter patterns are placeholders

**Source:** `docs/plans/2026-01-29-moderation-safety-implementation-v2.md`

Multiple TODOs in moderation implementation doc:
- "Replace with real patterns before production" (3 occurrences)
- "Add more harassment patterns"
- "Consider fail-closed mode for critical filters"

Note: Content filter feature itself is not yet implemented (Phase 5 open item). These are documented requirements for when it ships.

---

### TD-27: Connectivity monitor known gaps

**Source:** `docs/plans/2026-01-19-user-connectivity-monitor-PR.md:139-144`

- No integration tests for REST API endpoints
- Session ID is client-generated, not validated server-side
- Pagination replaces list instead of appending
- No retry for session finalization on DB failure

---

### TD-28: Guild-level thread toggle missing

**Source:** `docs/project/roadmap.md:562`

Threads feature is complete but there's no guild-level setting to enable/disable it.

---

### TD-29: Simulcast not implemented

**Source:** `docs/project/roadmap.md:419`

No quality tier switching for bandwidth management in voice/video streams.

---

### TD-30: Search query analytics logging ✅ RESOLVED

**Source:** `docs/project/roadmap.md:528`
**Resolved:** 2026-02-19 — Added structured `tracing::info!` logging with `search_query` event name to guild search, DM search, and global search handlers. Logs include user_id, query_length, result_count, and duration_ms fields for performance monitoring and UX analytics.

---

## Resolution Summary

| Status | Count |
|--------|-------|
| ✅ Resolved | 16 |
| Open | 14 |
| **Total** | 30 |

**Resolved items:** TD-01, TD-02, TD-04, TD-05, TD-06, TD-08, TD-09, TD-10, TD-11, TD-12, TD-13, TD-17, TD-20, TD-22, TD-30

## Code Quality Summary

| Metric | Before | After | Notes |
|--------|--------|-------|-------|
| `#[allow(dead_code)]` | 11 | 0 | All dead code removed (TD-11) |
| `#[allow(clippy::too_many_arguments)]` | 11 | 0 | Parameter structs introduced (TD-10) |
| `@ts-ignore` in client | 1 | 0 | Properly typed (TD-12) |
| `as any` in production client | 7 | 0 | All replaced with proper types (TD-12) |
| `expect()` in WS upgrade | 3 | 0 | Proper error responses (TD-05) |
| `unsafe` blocks | 0 | 0 | Excellent |
| `println!`/`eprintln!` in server | 0 | 0 | All logging uses tracing |
| `console.log` in client src/ | 89+ | 89+ | Stripped in production builds via esbuild `pure` (TD-09) |
| `eslint-disable` | 1 | 1 | Test setup only |

---

## Open GitHub Issues Referenced

| Issue | Description | Source |
|-------|-------------|--------|
| #132 | Tauri WebSocket Event Parity (11/22 done) | roadmap.md |
| #137 | Test cleanup guards | roadmap.md |
| #138 | Shared DB pool for tests | roadmap.md |
| #139 | Stateful middleware testing | roadmap.md |
| #140 | Concurrent setup completion test | roadmap.md |
