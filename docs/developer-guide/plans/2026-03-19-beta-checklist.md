# Closed Beta Readiness Checklist

> Generated: 2026-03-19
> Goal: Track all improvements needed before and during the closed beta launch.

---

## Critical — Fix Before Beta Launch

### Reconnection & Reliability

- [x] **Channel subscriptions not restored after WebSocket reconnect** (#410)
  Fixed: `handleReconnect()` snapshots and re-subscribes all channels on reconnect.

- [x] **WebSocket reconnection invisible to users** (#410)
  Fixed: persistent "Reconnecting..." toast shown during reconnect, dismissed after recovery.

- [x] **No server-side WebSocket heartbeat / idle timeout** (#410)
  Fixed: 30s ping/pong heartbeat with `OutboundMsg` enum and `tokio::select!` read loop.

- [x] **Messages during disconnection are never backfilled** (#410)
  Fixed: `loadInitialMessages()` called for all channels with loaded content on reconnect.

### Security

- [x] **JWT token exposed in WebSocket URL query parameter** (#411)
  Fixed: Tauri client now uses `Sec-WebSocket-Protocol` header matching server and browser-mode client.

### Deployment

- [x] **JWT key mismatch — server won't start with compose config** (#412)
  Fixed: setup-beta.sh now generates Ed25519 key pair, compose passes JWT_PRIVATE_KEY/JWT_PUBLIC_KEY.

### Performance

- [x] **Token refresh holds DB transaction during GeoIP HTTP call** (#411)
  Fixed: GeoIP calls moved before `state.db.begin()` in register and refresh handlers.

---

## Important — Fix Early in Beta

### Backend Reliability

- [x] **Permission check swallows DB errors as "Unauthorized"** (#413)
  Fixed: now matches on variant, logs `DatabaseError`, only maps permission errors to `Unauthorized`.

- [x] **Permission resolution fires 3-5 queries per typing event** (#497)
  Every `Typing`/`StopTyping` WS event triggers the full permission chain.
  - `server/src/permissions/helpers.rs:82-158`
  - `server/src/ws/mod.rs:1472-1493`
  - Fix: cache per `(user_id, channel_id)` with 30s TTL, or skip for already-subscribed channels

- [x] **Regex compiled on every message (`detect_mention_type`)** (#413)
  Fixed: promoted to `static LazyLock<Regex>`.

- [x] **Slow WS clients block Redis pubsub task** (#497)
  `tx.send(event).await` on full buffer pauses the Redis subscriber for the connection.
  - `server/src/ws/mod.rs:1172,1811`
  - Fix: use `try_send()` and drop/disconnect on sustained backpressure

- [x] **WS auth doesn't check user exists in DB on connect** (#413)
  Fixed: `find_user_by_id` call added before `on_upgrade()`.

- [x] **Health check always returns HTTP 200 even when degraded** (#413)
  Fixed: returns HTTP 503 when `status != "ok"`.

### Client — Tauri

- [x] **Tauri `ServerEvent` enum missing multiple server events** (#413)
  Fixed: added 10 missing variants (workspace, commands, moderation).

- [x] **`screen_share_started` Tauri event missing `stream_id` field** (#413)
  Fixed: added `stream_id` and `started_at` fields.

### Client — Frontend

- [x] **`console.log` stripping doesn't work in production builds** (#413)
  Fixed: uses Vite `mode` parameter instead of `process.env.NODE_ENV`.

- [x] **Mermaid statically imported — threatens <3s startup target** (#413)
  Fixed: lazy-loaded via dynamic `import()`.

- [x] **Voice panel shows raw UUIDs instead of usernames** (#413)
  Fixed: uses `display_name || username || user_id.slice(0, 8)`.

- [x] **Message edit doesn't decrypt in E2EE channels** (#497)
  `message_edit` patches raw content without running `decryptMessageIfNeeded()`.
  - `client/src/stores/websocket.ts:985-1009` (browser), `262-290` (Tauri)
  - Fix: call `decryptMessageIfNeeded()` on edit content for E2EE channels

- [x] **Login doesn't honor `returnUrl` after auth redirect** (#413)
  Fixed: reads `returnUrl` from query params, validates relative path, navigates to it.

- [x] **Guild load error invisible to user** (#413)
  Fixed: shows error indicator with retry button in `ServerRail`.

- [x] **Optimistic message send race condition** (#497)
  `findIndex(m => m.id.startsWith("pending:"))` matches the *oldest* pending, not the specific one. Rapid sends can corrupt ordering.
  - `client/src/stores/messages.ts:373-381`
  - Fix: match against a specific pending ID (e.g., store pending ID in nonce)

- [ ] **`any[]` types in voice/screen-share handlers**
  Eight handlers bypass TypeScript type checking entirely.
  - `client/src/stores/websocket.ts:459-460,1763-1765,1786,1815,1843,1871,1896`
  - Fix: use existing `VoiceParticipant`, `ScreenShareServerInfo`, `WebcamServerInfo` types

- [x] **`Patch` entity type `"channel"` not implemented client-side** (#413)
  Fixed: added `"channel"` case to `handlePatchEvent` with `patchChannel()`.

### Infrastructure

- [x] **Presence scanner uses `ProcessRefreshKind::everything()` at startup** (#413)
  Fixed: uses `ProcessRefreshKind::new()` (minimal).

---

## Nice-to-Have — Polish for Beta

### Security & Data Hygiene

- [ ] E2EE Megolm session cache never cleared on logout — `client/src/stores/messages.ts:618`
- [ ] `returnUrl` injection risk when feature is implemented — validate relative URL — `client/src/components/auth/AuthGuard.tsx:28-30`
- [ ] TURN credentials are static/long-lived — consider time-limited HMAC credentials — `server/src/voice/handlers.rs:47-66`
- [ ] No virus scanning for file uploads (MIME + magic bytes is current defense) — `server/src/chat/uploads.rs`

### UX Polish

- [ ] Upload error auto-dismisses in 5s (toast convention is 8s) — `client/src/components/messages/MessageInput.tsx:147`
- [ ] `createEffect` without reactive dep should be `onMount` — `client/src/views/Main.tsx:133-144`
- [ ] Long channel names truncate in header without tooltip — `client/src/views/Main.tsx:239-241`
- [ ] `/demo` theme route accessible in production — `client/src/App.tsx:290`
- [ ] `Login.tsx` sets `document.title` but no other view does — `client/src/views/Login.tsx:30-31`

### Accessibility

- [ ] ServerRail buttons lack `aria-current` and `role="navigation"` — `client/src/components/layout/ServerRail.tsx:78-91`
- [ ] ChannelItem uses `div[role=button]` instead of `<button>` — `client/src/components/channels/ChannelItem.tsx:153-183`

### Backend Optimization

- [ ] Per-request PEM key decode on every JWT validation — parse once at startup — `server/src/auth/jwt.rs:131-133`
- [ ] `get_member_permission_context` fires 3 sequential queries where 1 CTE would suffice — `server/src/permissions/helpers.rs:88-135`
- [ ] Session cleanup lacks optimal index for `expires_at < NOW()` range scan — `server/migrations/20260128000000_performance_indexes.sql:30`
- [ ] Graceful shutdown doesn't drain active WS connections — `server/src/main.rs:379-413`
- [ ] Connection pool (max 20) may be tight for hundreds of users — `server/src/db/mod.rs:49-60`
- [ ] No rate limiting on WS typing events specifically — `server/src/ws/mod.rs:1470-1514`

### Infrastructure

- [ ] Deploy scripts use `docker` but dev uses `podman` — `infra/scripts/deploy.sh:16`, `infra/scripts/backup.sh:32`
- [ ] `bitnami/postgresql:latest` in compose — pin to major version — `infra/compose/docker-compose.yml:65`
- [ ] No code splitting for heavy deps (highlight.js, marked) beyond mermaid — `client/vite.config.ts:44-46`
- [ ] `scap` git fork branch dependency — track upstream merge — `client/src-tauri/Cargo.toml:59`
- [ ] No auto-updater for Tauri client (`tauri-plugin-updater` missing)
- [ ] Upload S3 failure leaves orphaned empty message in DB — `server/src/chat/uploads.rs:675-693`
- [ ] Duplicate presence update listeners in Tauri mode — `client/src/stores/presence.ts:49-97`
- [ ] `SetupWizard` uses raw `fetch` instead of `tauri.fetchApi` — `client/src/components/SetupWizard.tsx:44-46`
- [ ] `discovery/handlers.rs` uses `.unwrap()` on guarded Option — use `if let` — `server/src/discovery/handlers.rs:153,207`

---

## Progress Summary

| Category | Total | Done | Remaining |
|----------|-------|------|-----------|
| Critical | 7 | 7 | 0 |
| Important | 19 | 19 | 0 |
| Nice-to-have | 22 | 0 | 22 |
| **Total** | **48** | **26** | **22** |
