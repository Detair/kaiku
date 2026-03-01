# Tech Debt Implementation Plans

**Date:** 2026-02-18
**Branch:** `chore/tech-debt`
**Companion:** `docs/project/tech-debt.md` (inventory)

Each plan references its TD-ID from the inventory and includes files to modify, concrete steps, and verification.

---

## High Priority

### TD-01: Megolm E2EE stubs — feature-gate or implement

**Files:**
- `shared/vc-crypto/src/megolm.rs`
- `shared/vc-crypto/Cargo.toml`

**Context:** `MegolmOutboundSession` and `MegolmInboundSession` contain `todo!()` calls. Olm (1:1) is fully implemented; Megolm (group) is not. No current caller exercises Megolm, but the `todo!()` will panic if reached.

**Plan (Option A — Feature Gate, recommended for now):**
1. Add `#[cfg(feature = "megolm")]` to both structs and their impls.
2. In `Cargo.toml`, add `megolm = []` under `[features]` (disabled by default).
3. Gate the `pub mod megolm;` in `lib.rs` behind the same feature.
4. Remove the `todo!()` calls — replace with compile-time gating.

**Plan (Option B — Implement):**
1. Replace placeholder fields with real `vodozemac::megolm::GroupSession` / `InboundGroupSession`.
2. Implement `new()`, `session_key()`, `encrypt()`, `decrypt()`, `serialize()/deserialize()`.
3. Follow the same `Zeroize`/`ZeroizeOnDrop` pattern from `olm.rs`.
4. Add round-trip tests per `shared/vc-crypto/src/AGENTS.md` guidelines.

**Verify:**
```bash
cargo build -p vc-crypto              # No todo!() panic risk
cargo test -p vc-crypto               # Round-trip if Option B
```

**Recommendation:** Option A now, Option B when group E2EE is on the roadmap.

---

### TD-02: Search — channel-level permission filtering

**Files:**
- `server/src/db/queries.rs` (`search_messages_filtered`)
- `server/src/chat/search.rs` or guild search handler
- `server/src/permissions/guild.rs` (existing permission queries)

**Context:** `search_messages_filtered` accepts a `channel_ids: &[Uuid]` slice and filters by it. The guild search handler currently passes **all guild channels** without checking the user's per-channel permissions. DM search is already correctly scoped.

**Plan:**
1. In the guild search handler, after fetching all guild channel IDs, filter them through the permission system:
   ```rust
   let visible_channel_ids: Vec<Uuid> = all_channel_ids
       .into_iter()
       .filter(|cid| user_can_view_channel(pool, user_id, *cid).await)
       .collect();
   ```
2. To avoid N+1, add a batch query: `get_visible_channels_for_user(pool, guild_id, user_id) -> Vec<Uuid>` that joins `channels` with `channel_permission_overrides` and `role_permissions` in a single query.
3. Pass only `visible_channel_ids` to `search_messages_filtered`.
4. Add integration tests: create a guild with a restricted channel, search as non-permitted user, assert results exclude restricted channel.

**Verify:**
```bash
cargo test search -- --test-threads=1
```

---

### TD-03: Security advisory workaround (RUSTSEC-2026-0002)

**Files:**
- `deny.toml`
- `Cargo.lock` (after bump)

**Context:** `aws-sdk-s3` pulls in `lru` with a stacked borrows violation. We don't use `IterMut` directly. Scheduled review Q2 2026.

**Plan:**
1. Run `cargo update -p lru` and check if >= 0.16.3 is available.
2. If available: remove the `RUSTSEC-2026-0002` ignore line from `deny.toml`, run `cargo deny check advisories`.
3. If not: update the TODO date comment to next quarter.
4. Also check `RUSTSEC-2025-0008` (openh264) and `RUSTSEC-2023-0071` (rsa) for upstream progress.

**Verify:**
```bash
cargo deny check advisories
```

---

### TD-04: E2EE key store not wired into backups

**Files:**
- `client/src/components/E2EESetupPrompt.tsx` (~line 128)
- `client/src/components/settings/SettingsModal.tsx` (~line 79)
- `client/src/stores/e2ee.ts`
- `client/src/lib/tauri.ts` (E2EE wrappers)

**Context:** `CryptoManager` and `LocalKeyStore` are fully implemented in Tauri. The `init_e2ee` command returns identity keys and prekeys. The backup flow exists (`create_backup`/`restore_backup`). The only gap is that the UI passes placeholder data instead of real keys.

**Plan:**
1. In `E2EESetupPrompt.tsx`, after calling `initE2EE()`, capture the returned `InitE2EEResponse` (device_id, identity keys, prekeys).
2. Build the backup JSON from actual data:
   ```typescript
   const initResult = await initE2EE(encryptionKey);
   const backupData = JSON.stringify({
     version: 1,
     created_at: new Date().toISOString(),
     device_id: initResult.device_id,
     identity_key_ed25519: initResult.identity_key_ed25519,
     identity_key_curve25519: initResult.identity_key_curve25519,
     prekeys: initResult.prekeys,
   });
   ```
3. Apply the same change in `SettingsModal.tsx`.
4. For `restore_backup`, add a "Restore from Recovery Key" input flow on the login screen or settings. Parse the returned JSON and call `init_e2ee` with the restored keys.

**Verify:** Manual test — create backup, log out, restore backup, verify identity keys match.

---

### TD-05: WebSocket response builders use `.expect()`

**Files:**
- `server/src/ws/mod.rs` (lines ~945, 956, 966)

**Context:** Three `Response::builder().status(401).body(...).expect("static response builder")` calls in the WS upgrade handler. These build responses with constant status codes and string bodies — they will never fail in practice, but the `.expect()` is a code smell.

**Plan:**
1. Extract a helper function:
   ```rust
   fn error_response(status: u16, body: &str) -> Response {
       Response::builder()
           .status(status)
           .body(body.to_string().into())
           .unwrap_or_else(|_| {
               Response::builder()
                   .status(500)
                   .body("Internal Server Error".into())
                   .expect("hardcoded 500 response")
           })
   }
   ```
2. Replace the three `.expect()` call sites with `return error_response(401, "...")`.
3. This eliminates the panic risk while keeping the code readable.

**Verify:**
```bash
cargo clippy -- -D warnings
cargo test ws
```

---

## Medium Priority

### TD-06: MFA backup codes

**Files:**
- `server/src/auth/handlers.rs` (add backup code handlers)
- `server/src/auth/mfa_crypto.rs` (add backup code generation)
- `server/src/db/queries.rs` (add backup code queries)
- `server/migrations/` (new table)
- `client/src/components/settings/` (UI)

**Context:** MFA setup exists (TOTP via `totp-rs`, encrypted with AES-256-GCM). Login verifies TOTP codes. No backup codes exist.

**Plan:**
1. **Migration:** Create `mfa_backup_codes` table:
   ```sql
   CREATE TABLE mfa_backup_codes (
       id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
       user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
       code_hash TEXT NOT NULL,
       used_at TIMESTAMPTZ,
       created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
   );
   CREATE INDEX idx_mfa_backup_codes_user ON mfa_backup_codes(user_id);
   ```
2. **Generation:** In `mfa_crypto.rs`, add `generate_backup_codes() -> Vec<String>` — 10 random 8-character alphanumeric codes. Hash each with Argon2id before storing.
3. **Handlers:**
   - `POST /api/auth/mfa/backup-codes` — Generate and return codes (one time display). Store hashes.
   - Modify `mfa_verify` to also accept a backup code (check all unused hashes, mark `used_at` on match).
4. **Frontend:** Show codes during MFA setup. Prompt user to save them. Add "Use backup code" link on MFA verification screen.
5. **Tests:** Generate codes, verify one works, verify used code is rejected, verify all 10 work.

**Verify:**
```bash
cargo test mfa
```

---

### TD-07: Test infrastructure improvements (#137-#140)

**Files:**
- `server/tests/helpers/mod.rs`

**Context:** `CleanupGuard` already exists and is production-quality. Issues #137-#140 ask for more infrastructure.

**Plan:**
1. **#137 (Cleanup guards):** Already implemented via `CleanupGuard`. Mark issue as done or extend for additional resource types (Redis keys, file uploads).
2. **#138 (Shared DB pool):** Add a `lazy_static` or `once_cell::sync::Lazy<PgPool>` in test helpers that all tests share. Initialize from `DATABASE_URL` env var. Reduces connection churn.
3. **#139 (Stateful middleware testing):** Add `TestApp::with_middleware()` builder that lets tests inject custom middleware (e.g., rate limiter overrides, mock auth).
4. **#140 (Concurrent setup test):** Use `tokio::spawn` to fire 10 simultaneous `POST /api/setup/complete` requests. Assert exactly one succeeds (200) and the rest get 409 Conflict.

**Verify:**
```bash
cargo test --test '*' -- --test-threads=1
```

---

### TD-08: Search edge case and security tests

**Files:**
- `server/tests/search_http_test.rs` (or new file)
- `server/tests/helpers/mod.rs`

**Plan:**
1. **Special characters:** Test `@#$%^&*()`, `'; DROP TABLE`, `<script>alert(1)</script>` as search queries. Assert no SQL error, no XSS in `ts_headline` output (already uses `<mark>` tags with server-controlled delimiters).
2. **Long queries:** Test 1001-character query. Assert 400 validation error or graceful truncation.
3. **SQL injection:** `websearch_to_tsquery` is parameterized via `$1` bind — verify no raw string interpolation exists in query builder.
4. **XSS in results:** Insert a message with `<img onerror=alert(1)>`, search for it, verify `ts_headline` output is HTML-escaped except for `<mark>` tags.
5. **Large result sets:** Insert 200+ messages matching a query, verify pagination returns correct offset/limit.

**Verify:**
```bash
cargo test search -- --test-threads=1
```

---

### TD-09: Console.log stripping in production

**Files:**
- `client/vite.config.ts`

**Context:** 89+ `console.log` calls across WebRTC, Tauri, and sound modules. All use `[ModuleName]` prefixes. No production stripping configured.

**Plan:**
1. Add `esbuild.drop` option to Vite config for production builds:
   ```typescript
   build: {
     target: "esnext",
     minify: 'esbuild',
     commonjsOptions: {
       transformMixedEsModules: true,
     },
   },
   esbuild: {
     drop: process.env.NODE_ENV === 'production' ? ['console', 'debugger'] : [],
   },
   ```
2. If selective stripping is preferred (keep `console.error`/`console.warn`), use `vite-plugin-strip` or `@rollup/plugin-strip` instead with `include: ['console.log']`.
3. Verify production bundle has no `console.log` calls:
   ```bash
   cd client && bun run build && grep -r 'console.log' dist/ | wc -l
   ```

**Verify:**
```bash
cd client && bun run build
```

---

### TD-10: `clippy::too_many_arguments` — parameter structs

**Files:**
- `server/src/db/queries.rs` (4 functions)
- `server/src/ws/mod.rs` (2 functions)
- `server/src/voice/ws_handler.rs` (1 function)
- `server/src/pages/queries.rs` (2 functions)
- `server/src/api/mod.rs` (1 function — `AppState::new`)

**Plan:** For each suppressed function, introduce a request/params struct. Example pattern:

```rust
// Before:
#[allow(clippy::too_many_arguments)]
pub async fn create_channel(pool: &PgPool, name: &str, channel_type: &ChannelType, ...) -> ...

// After:
pub struct CreateChannelParams<'a> {
    pub name: &'a str,
    pub channel_type: &'a ChannelType,
    pub category_id: Option<Uuid>,
    pub guild_id: Option<Uuid>,
    pub topic: Option<&'a str>,
    pub icon_url: Option<&'a str>,
    pub user_limit: Option<i32>,
}

pub async fn create_channel(pool: &PgPool, params: CreateChannelParams<'_>) -> ...
```

**Priority order** (by impact):
1. `ws/mod.rs: handle_pubsub` — 8+ params, high-traffic path
2. `ws/mod.rs: handle_client_message` — 7 params, public test API
3. `db/queries.rs: create_oidc_provider` — 13 params (worst offender)
4. `db/queries.rs: update_oidc_provider` — 10+ params
5. `db/queries.rs: create_channel` — 8 params
6. `db/queries.rs: create_thread_reply` — 7+ params
7. `voice/ws_handler.rs: handle_screen_share_start` — 8 params
8. `pages/queries.rs: create_page/update_page` — 7 params each
9. `api/mod.rs: AppState::new` — 8 params (consider builder)

**Verify:**
```bash
cargo clippy -- -D warnings   # No more too_many_arguments suppressions
```

---

### TD-11: Dead code audit and cleanup

**Files:** See inventory for full list.

**Plan — remove or document each:**

| Location | Decision | Rationale |
|---|---|---|
| `ws/mod.rs:749` `user_presence()` | Remove | Presence uses `user:{id}` channel, not `presence:{id}` |
| `ws/mod.rs:768` `GLOBAL_EVENTS` | Remove | No global broadcast mechanism planned |
| `signaling.rs:12,26` | Remove file | Unused types from early voice architecture. Current signaling is in `ws/mod.rs` |
| `sfu.rs:220` `broadcast_all` | Keep, remove allow | Grep for callers — if used by screen share, remove the `#[allow]` |
| `sfu.rs:737` `room_count` | Keep, expose in metrics | Useful for admin dashboard `/api/admin/stats` |
| `messages.rs:26` `MessageError` | Audit variants | Remove unused variants, keep used ones, remove blanket `#[allow]` |
| `channels.rs:21` `ChannelError` | Audit variants | Same approach |
| `admin/middleware.rs:13` `id` field | Remove field | Query only needed columns: `SELECT user_id, elevated_at, expires_at, reason` |
| `admin/handlers.rs:237-244` | Consolidate | Merge duplicate `ElevatedSessionRecord` structs into shared module, remove unused fields |

**Verify:**
```bash
cargo clippy -- -D warnings   # No dead_code allows remaining
```

---

### TD-12: TypeScript type safety fixes

**Plan per location:**

| File | Fix |
|---|---|
| `DMConversation.tsx:134` | Replace `@ts-ignore` + `as any` with typed ref: `ref={(el: HTMLInputElement) => { el.onchange = (e: Event) => { const file = (e.target as HTMLInputElement).files?.[0]; ...}}` |
| `websocket.ts:886` | Define `VoiceUserStatsEvent` interface extending `ServerEvent`. Cast to it: `event as VoiceUserStatsEvent` |
| `tauri.ts:161-163` | Define `UploadLimitsResponse` interface. Use type guard: `function isUploadLimits(data: unknown): data is UploadLimitsResponse` |
| `browser.ts:601` | Define `type AudioElementWithSinkId = HTMLAudioElement & { setSinkId(id: string): Promise<void> }`. Cast once. |
| `sound/browser.ts:173` | Use `'webkitAudioContext' in window` check instead of `typeof (window as any).webkitAudioContext` |
| `SetupWizard.tsx:319` | Type the value: `e.currentTarget.value as "open" \| "invite_only" \| "closed"` |
| `NotificationSettings.tsx:51,54` | Change parameter type: `handleSoundSelect(soundId: SoundOption)` or assert at call site |

**Verify:**
```bash
cd client && bun run typecheck   # or tsc --noEmit
```

---

### TD-13: Frontend/backend file size limit sync

**Files:**
- `server/src/api/mod.rs` (add config endpoint or extend existing)
- `client/src/lib/tauri.ts` (fetch on startup)
- `client/src/stores/` (store limits)

**Plan:**
1. Add `GET /api/config/limits` endpoint returning `{max_upload_size, max_avatar_size, max_emoji_size}` from server config. No auth required.
2. Client fetches on app startup, stores in a signal.
3. Replace hardcoded `5 * 1024 * 1024` etc. with stored values.
4. Fallback to hardcoded defaults if fetch fails.

**Verify:**
```bash
cargo test config
cd client && bun run test:run
```

---

### TD-14: Guild invite test stubs

**Files:**
- `server/tests/guild_invite_test.rs`

**Context:** Three `#[ignore]` tests waiting on features (max_uses, bans, suspension). These are design documentation, not bugs.

**Plan:** No code change needed now. When each feature is implemented:
1. Add `max_uses` column to `guild_invites` table → un-ignore `test_invite_rejected_after_max_uses`.
2. Add `guild_bans` table → un-ignore `test_banned_user_cannot_join_via_invite`.
3. Add `guilds.suspended_at` column → un-ignore `test_invite_to_suspended_guild_rejected`.

**Action now:** Add a comment linking each test to its blocking feature/issue number.

---

### TD-15: Tauri native webcam capture

**Files:**
- `client/src-tauri/src/commands/voice.rs` (new commands)
- `client/src-tauri/Cargo.toml` (add `nokhwa` or similar crate)
- `client/src/lib/webrtc/tauri.ts` (wire up)

**Context:** Browser webcam works via `getUserMedia`. Tauri needs native capture.

**Plan:**
1. Add `nokhwa` crate (MIT-licensed) for cross-platform camera access.
2. Implement `start_webcam(device_id) -> MediaStream` and `stop_webcam()` Tauri commands.
3. Pipe frames to the SFU via the existing track system.
4. Add device enumeration: `list_webcams() -> Vec<WebcamDevice>`.

**Blocked on:** Voice architecture decision (webrtc-rs vs str0m). Defer until that's resolved.

---

### TD-16: Tauri WebRTC connection metrics

**Files:**
- `client/src-tauri/src/commands/voice.rs`
- `client/src/lib/webrtc/tauri.ts:194`

**Context:** `getConnectionMetrics()` returns null. Browser uses `RTCPeerConnection.getStats()`.

**Plan:**
1. Add a Tauri command `get_webrtc_stats()` that calls the native WebRTC peer connection stats API.
2. Map native stats to the same `ConnectionMetrics` interface used by browser adapter.
3. Wire up in `tauri.ts:getConnectionMetrics()`.

**Blocked on:** Same as TD-15 — depends on voice architecture.

---

### TD-17: Admin elevation detection

**Files:**
- `client/src-tauri/src/commands/admin.rs:160`
- Server: check if `/api/admin/status` already returns elevation info

**Plan:**
1. Verify server response from `GET /api/admin/status` — does it include `is_elevated` and `elevation_expires_at`?
2. If yes: parse those fields from the JSON response in the Tauri command.
3. If no: add them to the server response, then parse client-side.
4. Update `AdminStatus` struct to reflect real values instead of hardcoded `false`.

**Verify:**
```bash
cargo test admin
```

---

## Low Priority

### TD-18: Swagger UI setup

**Files:**
- `server/src/api/mod.rs:334`
- `server/Cargo.toml` (add `utoipa-swagger-ui`)

**Plan:**
1. Add `utoipa = { version = "5", features = ["axum_extras"] }` and `utoipa-swagger-ui = { version = "8", features = ["axum"] }` to Cargo.toml.
2. Annotate key routes with `#[utoipa::path(...)]` macros (start with auth and channel endpoints).
3. Wire up `SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", openapi)` in `api_docs()`.
4. This is a large effort — can be done incrementally per module.

---

### TD-19: Per-channel screen share limit

**Files:**
- `server/src/voice/ws_handler.rs:583`
- `server/src/db/queries.rs` (channel query)
- Migration: add `max_screen_shares` column to `channels`

**Plan:**
1. Add `max_screen_shares INTEGER DEFAULT 2` to channels table.
2. In `handle_screen_share_start`, fetch channel config and use `channel.max_screen_shares` instead of `DEFAULT_MAX_SCREEN_SHARES`.
3. Add to guild settings UI for channel configuration.

---

### TD-20: Window focus check for notifications

**Files:**
- `client/src/stores/websocket.ts:93`

**Plan:**
1. Replace:
   ```typescript
   if (channelsState.selectedChannelId === message.channel_id) {
     // TODO: Also check if window is focused
     return;
   }
   ```
   With:
   ```typescript
   if (channelsState.selectedChannelId === message.channel_id && !document.hidden) {
     return;
   }
   ```
2. `document.hidden` is the standard Page Visibility API — returns `true` when tab is hidden or window is minimized.

**Verify:**
```bash
cd client && bun run test:run
```

---

### TD-21: Toast component rendering tests

**Files:**
- `client/src/components/ui/__tests__/Toast.test.ts`

**Plan:**
1. Add `@solidjs/testing-library` as dev dependency (if not present).
2. Add rendering tests:
   - Render `ToastContainer`, add 6 toasts, verify only 5 are visible in DOM.
   - Render toast with action button, click it, verify callback fires.
   - Render toast, verify dismiss button removes it from DOM.
3. For auto-dismiss timing tests, use `vi.useFakeTimers()`.

---

### TD-22: Spoiler reveal state persistence

**Files:**
- `client/src/stores/messages.ts` (or new `spoilers.ts` store)
- `client/src/components/messages/SpoilerText.tsx`

**Plan:**
1. Add a `createSignal<Set<string>>` for `revealedSpoilers` keyed by `messageId:spoilerIndex`.
2. On spoiler click, add to set. On unmount/scroll-away, state persists.
3. Pass set to `SpoilerText` component to check initial reveal state.

---

### TD-23: Home unread aggregator pagination

**Files:**
- `server/src/api/` (unread endpoint)
- `client/src/components/home/UnreadModule.tsx`

**Plan:**
1. Add `offset` query parameter to `GET /api/me/unread`.
2. Add "Show More" button in `UnreadModule` that fetches next page.
3. Append results instead of replacing.

---

### TD-24: HTML emails for password reset

**Files:**
- `server/src/auth/handlers.rs` (forgot password handler)

**Plan:**
1. Use `lettre::message::MultiPart::alternative()` to send both plain text and HTML.
2. Create a simple HTML template with the reset link styled consistently.
3. Keep plain text as fallback for email clients that don't support HTML.

---

### TD-25: Windows Tauri build

**Files:**
- `.github/workflows/ci.yml`
- `client/src-tauri/Cargo.toml`

**Context:** `libvpx` (used by webrtc-rs) is not available via Chocolatey on Windows CI.

**Plan:**
1. Option A: Use `vcpkg` to install `libvpx` on Windows CI.
2. Option B: Feature-gate VP8/VP9 codec behind a non-default feature, use only Opus audio on Windows.
3. Option C: Wait for str0m migration (TD-29 dependency) which may not need libvpx.

**Recommendation:** Keep `continue-on-error: true` until voice architecture is decided.

---

### TD-26: Moderation filter patterns

**Context:** Not actionable until the moderation filter feature (Phase 5 roadmap item) is implemented. The TODOs are in the implementation plan doc, not in code.

**Action:** No code change. When the feature ships, ensure real patterns replace placeholders and `fail-closed` mode is considered.

---

### TD-27: Connectivity monitor gaps

**Files:**
- `server/tests/` (new integration tests)
- `client/src/components/` (pagination UX)

**Plan:**
1. Add integration tests for `POST /api/connectivity/stats` and `GET /api/connectivity/history`.
2. Fix pagination: append results instead of replacing in the frontend component.
3. Add retry logic for session finalization (`POST /api/connectivity/sessions/{id}/end`).
4. Session ID validation is acceptable as-is for telemetry data.

---

### TD-28: Guild-level thread toggle

**Files:**
- `server/src/db/queries.rs` (add `threads_enabled` to guild settings)
- `server/migrations/` (alter guilds table)
- `server/src/chat/messages.rs` (check toggle before creating thread reply)
- `client/src/components/guild/settings/` (toggle UI)

**Plan:**
1. Add `threads_enabled BOOLEAN NOT NULL DEFAULT true` to `guilds` table.
2. In `create_thread_reply`, check `guild.threads_enabled`. Return 403 if disabled.
3. In guild settings UI, add toggle under "Features" section.
4. Hide thread indicator in message list when disabled.

---

### TD-29: Simulcast

**Context:** Quality tier switching for video streams. Tightly coupled to the WebRTC stack choice (webrtc-rs vs str0m).

**Plan:** Defer until voice architecture is decided. If str0m is adopted, simulcast is a natural fit (Sans-IO gives full control over SVC layers). If staying on webrtc-rs, use its `RTCRtpEncodingParameters` API.

---

### TD-30: Search query analytics logging

**Files:**
- `server/src/chat/search.rs` (or guild search handler)

**Plan:**
1. Add a `tracing::info!` event after each search query:
   ```rust
   tracing::info!(
       user_id = %auth.id,
       query_length = query.len(),
       result_count = results.len(),
       duration_ms = elapsed.as_millis(),
       filters = ?filters,
       "search_query"
   );
   ```
2. This integrates with existing tracing infrastructure. No new dependencies needed.
3. Dashboards can be built later from structured log output.

---

## Execution Grouping

For efficient PRs, these items group naturally:

### PR 1: Quick Wins (TD-05, TD-11, TD-20, TD-30)
- WS `.expect()` → helper function
- Dead code audit and cleanup
- Window focus check (1-line fix)
- Search analytics logging (1 `tracing::info!`)

### PR 2: Type Safety (TD-12, TD-09)
- All TypeScript `as any` / `@ts-ignore` fixes
- Vite production console.log stripping

### PR 3: Parameter Structs (TD-10)
- All `too_many_arguments` refactors (start with `db/queries.rs`)

### PR 4: Search Hardening (TD-02, TD-08)
- Channel permission filtering
- Edge case and security tests

### PR 5: MFA Backup Codes (TD-06)
- Migration, generation, verification, UI

### PR 6: E2EE Wiring (TD-01, TD-04)
- Megolm feature gate
- Wire real keys into backup

### PR 7: Config & Limits (TD-13, TD-19, TD-28)
- Server config endpoint for limits
- Per-channel screen share config
- Guild thread toggle

### Deferred
- TD-03: Check quarterly (Q2 2026)
- TD-07: Incremental per test file
- TD-14: Blocked on features
- TD-15, TD-16: Blocked on voice architecture
- TD-17: Small, can go in any PR
- TD-18: Large, incremental effort
- TD-22, TD-23, TD-24, TD-25, TD-26, TD-27, TD-29: Low priority, do when adjacent work touches those areas
