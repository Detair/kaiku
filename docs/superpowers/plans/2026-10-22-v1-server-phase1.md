# v1.0 Family Release — Server Stream, Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the server a reliable realtime backend for the native desktop and Android clients (no zombie sockets, no ghost voice peers, live first DMs, catch-up after reconnect, one send path for text and photos), give the maintainer a safe way to create family accounts, and close the outbound/inbound hardening gaps that self-hosters inherit from the repo compose file.

**Architecture:** Twelve PR-sized tasks against `server/` and `infra/`, each on its own branch and squash-merged. Tasks 1–3 touch `ws/handlers.rs` and merge in order; all other tasks are file-disjoint and can run in parallel worktrees. Contract items C1–C4 and C9 from the cross-stream contracts file are produced here; no task needs a live server — verification is `cargo test`/`nextest` against the dev stack (`make docker-up`), CI, and `docker compose config`.

**Tech Stack:** Rust (axum 0.8, tokio, sqlx/PostgreSQL, fred/Valkey, webrtc-rs, reqwest, aws-sdk-s3), tokio-tungstenite (tests), Docker Compose, bash, Python 3 (CI guardrails).

**Spec:** `docs/superpowers/specs/2026-10-20-v1-family-release-goal-design.md`
**Review:** `docs/reviews/2026-10-22-v1-family-release-readiness.md` (IDs B-01..B-06, S-02/R-05, S-08, D-02/D-03 server half; *Before rollout* S-03, S-04, S-15)
**Contracts:** `docs/superpowers/plans/2026-10-22-v1-family-contracts.md` (C1, C2, C3, C4, C9, C10 — names used verbatim below)

## Global Constraints

- Code is read on `main` @ `aa1f319f`; line numbers below refer to that commit and may drift.
- **Never commit to `main`.** One branch per task (`fix/<name>` / `feature/<name>`), squash-merge via `gh pr merge <n> --squash`.
- Commit format `type(scope): subject`, max 72 chars, imperative; scopes only `auth, voice, chat, db, api, ws, ratelimit, infra, client, crypto`. End every commit with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Pre-push gates for every task: `cargo nextest run -p vc-server` (dev stack up via `make docker-up`), `cargo fmt --check`, `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`. Commit new `.sqlx/*.json` if a `query!` macro changes (this plan uses runtime `query`/`query_as` only, matching the surrounding code).
- License: `cargo deny check licenses` must pass; allowed MIT, Apache-2.0, BSD-2/3, ISC, Zlib, MPL-2.0 (only Task 10 adds a crate: `ipnet`, MIT/Apache-2.0, already in `Cargo.lock`).
- Every user-relevant change gets a `CHANGELOG.md` entry under `[Unreleased]` (Keep a Changelog sections), user perspective.
- **Public repo:** security fixes are described neutrally (what changes, what test proves it). No exploit narratives in code comments, commits, PR bodies or CHANGELOG.
- **Review gate (every task):** Fable 5.1 + one external model (Gemini 4 Argon via `agy`, or Codex) review each PR before merge.
- No task may require access to a live server; athene (k3s/Flux) deployment is out of scope — compose changes are for self-hosters.
- C9: every new env var is documented in `docs/admin-guide/configuration/environment.md` (append a section; create the file with an `# Environment variables` heading if the release stream has not landed it yet).
- C10: do **not** create `server/tests/integration/android_contract_fixtures.rs` — it belongs to the Android stream. New test modules added here (`ws_e2e.rs`, `admin_create_user.rs`, `files_http.rs`) each add one `mod` line to `server/tests/integration/main.rs` in alphabetical order; resolve trivial conflicts there by keeping all lines.
- WebSocket close codes the server uses: **1011** (server-side event pipeline failed) and **1013** (client too slow, try again later) — both mean "reconnect and run catch-up". **1008** stays reserved for auth rejection (Android suppresses reconnect on 1008).
- Out of scope: FCM push (Phase 2), Android/desktop client code, S-01 QR-redeem binding, session revocation (S-07), image pinning (S-14).

## Review Focus

1. **Message lost in the connect window** — a message posted between the socket being accepted and the server's pub/sub subscription being live must still reach the client. Expected: `Ready` is sent only after subscriptions are active, and a message posted right after `Ready` is delivered. Test: `ws_message_posted_after_ready_is_delivered` (Task 1).
2. **Catch-up anchor that is not a message of this channel** (other channel's id, random UUID) — expected 400, never an empty "you are caught up" page. Test: `test_list_messages_after_foreign_or_unknown_anchor_is_400` (Task 2).
3. **Same user on phone and PC** — the phone's socket dropping must not remove the PC's voice peer; a stale-peer reap must not remove a peer that re-joined. Tests: `test_socket_close_only_removes_peer_owned_by_that_socket`, `test_reaper_ignores_rejoined_session` (Task 6).
4. **Server behind a reverse proxy with no trusted proxies configured** — expected: the server still works, logs a startup warning when the legacy `RATE_LIMIT_TRUST_PROXY=true` is set, and the shipped compose file configures `RATE_LIMIT_TRUSTED_PROXIES` so the family does not share one rate-limit bucket (one wrong password would otherwise count against everyone). Tests: `legacy_trust_proxy_without_cidrs_trusts_nothing` (Task 10), compose guardrail `RATE_LIMIT_TRUSTED_PROXIES` token (Task 11).
5. **Valkey password containing URL-reserved characters** (`/`, `+`, `=` from `openssl rand -base64`) — embedded in `REDIS_URL` it breaks parsing and the server never connects. Expected: setup generates a URL-safe (hex) password and the docs say so. Test: compose guardrail forbids `rand -base64` for `VALKEY_PASSWORD` in `setup-beta.sh` (Task 11).

---

## File Structure

| File | Responsibility | Tasks |
|---|---|---|
| `server/src/ws/handlers.rs` | socket loop exits on pub/sub death; Ready after subscribe; DmCreated auto-subscribe; WS auth refuses pending-password users; voice teardown on close | 1, 2, 3, 5, 6 |
| `server/src/ws/events.rs` | `OutboundMsg::Close`, `Ready.server_time`, `DmCreated` | 1, 2, 3 |
| `server/src/chat/types.rs`, `chat/messages.rs`, `db/queries.rs` | `after` cursor (C1) | 2 |
| `server/src/chat/dm.rs` | DmCreated broadcast, shared DM list entry builder | 3 |
| `server/src/config.rs`, `api/mod.rs`, `auth/helpers.rs` | tauri origins, `x-refresh-token` | 4 |
| `server/migrations/20261023000000_must_change_password.sql`, `admin/users.rs`, `admin/mod.rs`, `auth/*` | C4 | 5 |
| `server/src/voice/leave.rs` (new), `voice/ws_handler.rs`, `voice/sfu.rs`, `main.rs` | permission-free internal leave, socket-close leave, stale-peer reaper | 6 |
| `server/src/chat/send.rs` (new), `chat/messages.rs`, `chat/uploads.rs`, `db/queries.rs` | shared authorize-send + post-commit dispatch | 7 |
| `server/src/chat/files.rs` | public-prefix allowlist, streaming | 8 |
| `server/src/webhooks/ssrf.rs`, `webhooks/delivery.rs`, `webhooks/handlers.rs`, `push/mod.rs` | pinned, no-redirect outbound client | 9 |
| `server/src/ratelimit/{ip,config,middleware}.rs` | trusted proxy CIDRs | 10 |
| `infra/compose/*`, `infra/scripts/setup-beta.sh`, `scripts/check_compose_hardening.py` | self-hoster compose hardening | 11 |
| `infra/scripts/backup.sh`, `infra/scripts/upgrade-preflight.sh`, `infra/scripts/tests/` | #623 psql auth | 12 |
| `server/tests/integration/helpers/mod.rs` | `ws_connect`, `next_ws_event` harness | 1 |

---

### Task 1: WebSocket closes when its pub/sub pipeline dies (B-01)

Branch `fix/ws-pubsub-zombie`.

**Files:**
- Modify: `server/src/ws/handlers.rs:233-500` (`handle_socket`), `:952-1170` (`handle_pubsub`)
- Modify: `server/src/ws/events.rs:1030` (`OutboundMsg`)
- Modify: `server/tests/integration/helpers/mod.rs` (WS harness)
- Create: `server/tests/integration/ws_e2e.rs` (+ `mod ws_e2e;` in `main.rs`)
- Modify: `server/src/ws/AGENTS.md` (lifecycle section), `CHANGELOG.md`

**Interfaces:**
- Produces:
  - `pub enum OutboundMsg { Event(ServerEvent), Ping, Close { code: u16, reason: &'static str } }` — sender task sends `Message::Close(Some(CloseFrame{code, reason}))` then stops.
  - `pub(crate) enum PubsubExit { ConnectFailed, SubscribeFailed, StreamEnded, Lagged, SlowClient, ClientGone }` with `pub(crate) fn close_code(&self) -> Option<u16>` (`SlowClient` → `Some(1013)`, `ClientGone` → `None`, all others → `Some(1011)`).
  - `async fn handle_pubsub(redis: Client, params: HandlePubsubParams, ready: oneshot::Sender<()>) -> PubsubExit` — fires `ready` after `psubscribe("channel:*")` and all `subscribe` calls returned.
  - `pub(crate) async fn forward_pubsub_messages(rx: tokio::sync::broadcast::Receiver<fred::types::Message>, params: &HandlePubsubParams) -> PubsubExit` — the extracted forwarding loop (testable without Valkey).
  - Test helpers: `pub async fn ws_connect(server: &TestServer, token: &str) -> WsClient` (connects `ws://{addr}/ws`, sends `{"type":"authenticate","token":…}`) and `pub async fn next_ws_event(ws: &mut WsClient, event_type: &str, timeout: Duration) -> serde_json::Value` (skips other events; panics on timeout). `pub type WsClient = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>`.

Decisions:
- The `select!` loop gets a third arm `exit = &mut pubsub_handle => { … }`: on `close_code() == Some(c)` send `OutboundMsg::Close{code:c, reason:"resync"}`, give the sender ≤1 s to flush, then break.
- `broadcast::error::RecvError::Lagged(_)` now returns `PubsubExit::Lagged` (today `while let Ok` silently ends the loop).
- The slow-client branch closes on the **first** `TrySendError::Full` (no silent drops any more — clients resync via C1). Raise the outbound mpsc capacity from 100 to 256 to absorb normal bursts.
- Connection order becomes: load friends/guilds/blocks → spawn pubsub → await `ready` (timeout 5 s → close 1011) → send `Ready` → send presence snapshot. This closes the connect-window gap (Review Focus 1).

- [ ] **Step 1: Write failing unit tests** in `#[cfg(test)] mod pubsub_tests` in `handlers.rs`:

```rust
#[tokio::test]
async fn forward_returns_stream_ended_when_sender_dropped() {
    let (btx, brx) = tokio::sync::broadcast::channel(8);
    let (params, _rx) = test_params(16);
    drop(btx);
    assert!(matches!(forward_pubsub_messages(brx, &params).await, PubsubExit::StreamEnded));
}

#[tokio::test]
async fn forward_returns_slow_client_on_first_full_send() {
    let (btx, brx) = tokio::sync::broadcast::channel(8);
    let (params, _rx_never_drained) = test_params(1); // outbound capacity 1
    params.subscribed_channels.write().await.insert(CHANNEL);
    for _ in 0..2 { btx.send(channel_message(CHANNEL, &typing_event())).unwrap(); }
    assert!(matches!(forward_pubsub_messages(brx, &params).await, PubsubExit::SlowClient));
}

#[tokio::test]
async fn forward_returns_lagged_when_broadcast_overflows() { /* capacity 1, send 3 before first recv → Lagged */ }

#[test]
fn close_codes() {
    assert_eq!(PubsubExit::SlowClient.close_code(), Some(1013));
    assert_eq!(PubsubExit::StreamEnded.close_code(), Some(1011));
    assert_eq!(PubsubExit::ClientGone.close_code(), None);
}
```
(`test_params(cap)` builds `HandlePubsubParams` with a fresh mpsc of capacity `cap`; `channel_message` builds a `fred::types::Message` on `channel:{id}`.)

- [ ] **Step 2: Write failing e2e test** in `ws_e2e.rs` using `spawn_test_server(app.router.clone())`, `ws_connect`, `next_ws_event`:

```rust
#[sqlx::test]
async fn ws_message_posted_after_ready_is_delivered(pool: PgPool) {
    // user A + B, DM channel via helpers::create_dm_channel; B connects, waits "ready",
    // sends {"type":"subscribe","channel_id":dm}, waits "subscribed";
    // A POSTs /api/messages/channel/{dm} immediately
    let ev = next_ws_event(&mut ws_b, "message_new", Duration::from_secs(5)).await;
    assert_eq!(ev["message"]["content"], "hello");
}
```

- [ ] **Step 3: Run tests to verify they fail**
Run: `cargo test -p vc-server --lib ws::handlers::pubsub_tests && cargo test -p vc-server --test integration ws_e2e`
Expected: FAIL (`forward_pubsub_messages` / `PubsubExit` / `ws_connect` not defined).

- [ ] **Step 4: Implement** `OutboundMsg::Close`, `PubsubExit`, `forward_pubsub_messages`, the `ready` oneshot, the third `select!` arm and the new connect ordering as decided above.

- [ ] **Step 5: Run tests to verify they pass**
Run: same as Step 3, then `cargo nextest run -p vc-server -E 'test(websocket) | test(ws_)'`
Expected: PASS, existing `websocket_integration` tests unchanged.

- [ ] **Step 6: Docs + CHANGELOG** — `ws/AGENTS.md`: lifecycle (Ready after subscribe; close 1011/1013 ⇒ reconnect + catch-up). CHANGELOG `### Fixed`: "Realtime updates no longer stop silently when the server's internal event stream hiccups — the connection is closed so apps reconnect and catch up."

- [ ] **Step 7: Gates + commit**
Run: `cargo fmt --check && SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`
```bash
git commit -m "fix(ws): close socket when pub/sub forwarder exits"
```

---

### Task 2: Catch-up API and server time on connect (B-04, C1, C2)

Branch `feature/message-catchup`. Depends on Task 1 (merge order; touches `handle_socket`).

**Files:**
- Modify: `server/src/chat/types.rs:254-260` (`ListMessagesQuery`)
- Modify: `server/src/chat/messages.rs:53-128` (`list`)
- Modify: `server/src/db/queries.rs:937-980` (add `list_messages_after`)
- Modify: `server/src/ws/events.rs:343` (`Ready`), `server/src/ws/handlers.rs` (Ready construction)
- Test: `server/tests/integration/messages_http.rs`, `server/tests/integration/ws_e2e.rs`
- Modify: `CHANGELOG.md`, `server/src/ws/AGENTS.md`

**Interfaces:**
- Consumes: Task 1 `ws_connect`, `next_ws_event`.
- Produces:
  - `ListMessagesQuery { before: Option<Uuid>, around: Option<Uuid>, after: Option<Uuid>, limit: i64 }` (C1).
  - `pub async fn list_messages_after(pool: &PgPool, channel_id: Uuid, after: Uuid, limit: i64) -> sqlx::Result<Option<Vec<Message>>>` — `None` when the anchor id is not a message (deleted or not) **of this channel**; otherwise rows with `deleted_at IS NULL AND parent_id IS NULL AND (created_at,id) > anchor ORDER BY created_at ASC, id ASC LIMIT $limit`.
  - `ServerEvent::Ready { user_id: Uuid, server_time: chrono::DateTime<chrono::Utc> }` (C2, serialises RFC 3339 UTC).

Decisions (C1 clarifications, see report):
- `after` combined with `before` or `around` → `ChatError::Validation` (400 `VALIDATION_ERROR`).
- Unknown/foreign anchor → 400 `VALIDATION_ERROR` ("Unknown cursor"). Soft-deleted anchors of the same channel are valid anchors.
- With `after`: fetch `limit+1`, `has_more = len > limit`, items ascending, `next_cursor` = id of the **newest** returned item when `has_more` (pass it as the next `after`). Block filtering identical to the `before` path.
- Edits/deletes that happened during the gap are not part of C1 (clients reload the open channel as today).

- [ ] **Step 1: Write failing tests** in `messages_http.rs` (use `insert_message_at` with fixed timestamps):

```rust
#[sqlx::test]
async fn test_list_messages_after_returns_ascending_page(pool: PgPool) {
    // m1..m5 at t1<..<t5; GET ?after={m2}&limit=2
    assert_eq!(ids(&body["items"]), vec![m3, m4]);
    assert_eq!(body["has_more"], true);
    assert_eq!(body["next_cursor"], m4.to_string());
    // GET ?after={m4}&limit=2 → [m5], has_more false, no next_cursor
}

#[sqlx::test]
async fn test_list_messages_after_excludes_deleted_and_thread_replies(pool: PgPool) { /* insert_deleted_message + a reply with parent_id → not returned */ }

#[sqlx::test]
async fn test_list_messages_after_and_before_is_400(pool: PgPool) { /* ?after=..&before=.. → 400 */ }

#[sqlx::test]
async fn test_list_messages_after_foreign_or_unknown_anchor_is_400(pool: PgPool) {
    // anchor = message id from another channel → 400; anchor = Uuid::now_v7() → 400
}
```
In `ws_e2e.rs`:
```rust
#[sqlx::test]
async fn ws_ready_carries_server_time(pool: PgPool) {
    let ready = next_ws_event(&mut ws, "ready", Duration::from_secs(5)).await;
    let t = chrono::DateTime::parse_from_rfc3339(ready["server_time"].as_str().unwrap()).unwrap();
    assert!((chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_seconds().abs() < 5);
}
```

- [ ] **Step 2: Run to verify failure**
Run: `cargo test -p vc-server --test integration list_messages_after ws_ready_carries_server_time`
Expected: FAIL (field ignored → wrong items; `server_time` missing).

- [ ] **Step 3: Implement** `after` in `ListMessagesQuery` (+ utoipa `IntoParams`/doc for the query), the branch in `messages::list`, `db::list_messages_after`, and `Ready.server_time = Utc::now()` taken when `Ready` is sent (after subscriptions are live, per Task 1).

- [ ] **Step 4: Run to verify pass** — same command, then `cargo nextest run -p vc-server -E 'test(messages)'`. Expected: PASS.

- [ ] **Step 5: CHANGELOG** `### Added`: "Apps can fetch messages they missed while offline (`GET /api/messages/channel/{id}?after=…`), and the realtime connection now reports the server time on connect so reconnecting apps catch up reliably."

- [ ] **Step 6: Gates + commit**
```bash
git commit -m "feat(chat): add after-cursor catch-up and server_time on ready"
```

---

### Task 3: First DM reaches the recipient live (B-03, C3)

Branch `feature/dm-created-event`. Depends on Task 1 (harness, pubsub loop).

**Files:**
- Modify: `server/src/chat/dm.rs:30-54` (`get_or_create_dm`), `:57-97` (`create_group_dm`), `:128-217` (`create_dm`), `:230-268` (`list_dms`)
- Modify: `server/src/ws/events.rs` (variant), `server/src/ws/handlers.rs` (user-channel branch of the pubsub loop, ~`:1098-1118`)
- Test: `server/tests/integration/dm_http.rs`, `server/tests/integration/ws_e2e.rs`
- Modify: `CHANGELOG.md`, `server/src/ws/AGENTS.md`

**Interfaces:**
- Produces:
  - `ServerEvent::DmCreated { channel: serde_json::Value }` (wire `type: "dm_created"`). `channel` is exactly one element of the `GET /api/dm` array (`DMListResponse` JSON: flattened channel fields + `participants` + `last_message` + `unread_count`), computed per recipient. `Value` because `DMListResponse` is `Serialize`-only and `ServerEvent` must deserialize in the pubsub loop.
  - `pub async fn get_or_create_dm(pool, user1_id, user2_id) -> Result<(Channel, bool), ChatError>` — `bool` = newly created.
  - `pub(crate) async fn dm_list_entry(pool: &PgPool, viewer_id: Uuid, channel: Channel) -> Result<DMListResponse, ChatError>` — used by `list_dms` and the event.
  - `async fn announce_dm_created(state: &AppState, channel: &Channel, participant_ids: &[Uuid])` — `broadcast_to_user(pid, DmCreated{…})` for **every** participant including the creator (their other devices); failures logged, never fail the request.

Decisions:
- Sent only when the channel is newly created (existing 1:1 DM → no event); group DMs are always new.
- Auto-subscribe: in the pubsub loop's `user_channel` branch, on `DmCreated` parse `channel["id"]` as `Uuid` and insert it into `subscribed_channels` **before** forwarding the event. Redis preserves publish order on the subscriber connection, so a following `MessageNew` on `channel:{id}` is delivered.

- [ ] **Step 1: Failing tests.** `ws_e2e.rs`:

```rust
#[sqlx::test]
async fn first_dm_is_pushed_and_auto_subscribed(pool: PgPool) {
    // B connects (no subscribe), A POST /api/dm {participant_ids:[B]}
    let ev = next_ws_event(&mut ws_b, "dm_created", secs(5)).await;
    assert_eq!(ev["channel"]["id"], dm_id.to_string());
    assert!(ev["channel"]["participants"].as_array().unwrap().len() == 2);
    // A posts a message → B receives it without ever sending "subscribe"
    let msg = next_ws_event(&mut ws_b, "message_new", secs(5)).await;
    assert_eq!(msg["channel_id"], dm_id.to_string());
}

#[sqlx::test]
async fn existing_dm_does_not_emit_dm_created(pool: PgPool) {
    // create DM first, then connect B, then A POSTs /api/dm again → 201, same id;
    expect_no_ws_event(&mut ws_b, "dm_created", Duration::from_secs(1)).await;
}
```
(add `pub async fn expect_no_ws_event(ws, event_type, timeout)` to helpers.) `dm_http.rs`: `test_group_dm_creation_still_returns_201_with_participants` (regression for the signature change).

- [ ] **Step 2: Run to verify failure** — `cargo test -p vc-server --test integration dm_created dm_http`. Expected: FAIL (timeout waiting for `dm_created`).

- [ ] **Step 3: Implement** the variant, `(Channel, bool)` return, `dm_list_entry` (refactor `list_dms` to use it), `announce_dm_created` call in `create_dm` after the response data is built, and the auto-subscribe in the pubsub loop.

- [ ] **Step 4: Run to verify pass** — same command + `cargo nextest run -p vc-server -E 'test(dm)'`.

- [ ] **Step 5: CHANGELOG** `### Fixed`: "The first direct message to someone now appears immediately on all of their open devices, instead of only after reloading the conversation list."

- [ ] **Step 6: Gates + commit** — `git commit -m "feat(chat): push DmCreated to participants and auto-subscribe"`

---

### Task 4: Desktop webview origins — CORS and refresh-token delivery (D-02, D-03 server half)

Branch `fix/desktop-cors-refresh`. Independent.

**Files:**
- Modify: `server/src/config.rs:346-354` (CORS parsing; add const + helper)
- Modify: `server/src/api/mod.rs:115-178` (CORS layer)
- Modify: `server/src/auth/helpers.rs:44-57` (`should_return_refresh_token`)
- Test: in-module tests in `auth/helpers.rs`, `config.rs`; `server/tests/integration/auth.rs` (CORS preflight via `spawn_test_server`)
- Modify: `CHANGELOG.md`

**Interfaces:**
- Produces:
  - `pub const TAURI_ORIGINS: [&str; 3] = ["tauri://localhost", "http://tauri.localhost", "https://tauri.localhost"];` in `config.rs`.
  - `impl Config { pub fn effective_cors_origins(&self) -> Vec<String> }` — wildcard config returns `["*"]` unchanged; an explicit list returns the list ∪ `TAURI_ORIGINS` (deduplicated). Used by the CORS layer.
  - `pub(super) fn should_return_refresh_token(headers: &HeaderMap) -> bool` — true when `Origin` is absent **or** equals one of `TAURI_ORIGINS`.
  - CORS `allowed_headers` adds `x-refresh-token`.

Decision: tauri origins are auto-included (self-hosters cannot be expected to know them); they are only presentable by the Tauri webview, so a website cannot obtain body refresh tokens through this.

- [ ] **Step 1: Failing tests**

```rust
#[test] fn refresh_token_returned_for_tauri_origin() {
    for o in TAURI_ORIGINS { assert!(should_return_refresh_token(&origin(o))); }
}
#[test] fn refresh_token_withheld_for_web_origin() {
    assert!(!should_return_refresh_token(&origin("https://kaiku.example")));
}
#[test] fn effective_origins_add_tauri_to_explicit_list() { /* ["https://a"] → contains all 3 + "https://a" */ }
#[test] fn effective_origins_keep_wildcard() { /* ["*"] → ["*"] */ }
```
Integration (`auth.rs`): `test_cors_preflight_allows_tauri_origin_with_credentials` — config with `cors_allowed_origins = ["https://kaiku.example"]`, `OPTIONS /auth/refresh` with `Origin: tauri://localhost`, `Access-Control-Request-Headers: x-refresh-token, content-type` → `access-control-allow-origin == "tauri://localhost"`, `access-control-allow-credentials == "true"`, allow-headers contains `x-refresh-token`. And `test_login_from_tauri_origin_returns_refresh_token_in_body` (POST `/auth/login` with that Origin → `refresh_token` present).

- [ ] **Step 2: Run to verify failure** — `cargo test -p vc-server --lib auth::helpers config && cargo test -p vc-server --test integration tauri_origin`.
- [ ] **Step 3: Implement** as specified.
- [ ] **Step 4: Run to verify pass.**
- [ ] **Step 5: CHANGELOG** `### Fixed`: "The desktop app can sign in and stay signed in against servers that restrict allowed origins."
- [ ] **Step 6: Gates + commit** — `git commit -m "fix(api): allow tauri origins and deliver refresh token to desktop"`

---

### Task 5: Admin-created accounts with forced password change (S-02/R-05, C4)

Branch `feature/admin-create-user`. Independent (touches `ws/handlers.rs` WS-auth only — rebase over Tasks 1–3 if they merged first).

**Files:**
- Create: `server/migrations/20261023000000_must_change_password.sql` — `ALTER TABLE users ADD COLUMN must_change_password BOOLEAN NOT NULL DEFAULT false;`
- Modify: `server/src/db/models.rs:10-43` (`User.must_change_password: bool`), `server/src/chat/messages.rs:1686` (test literal)
- Modify: `server/src/admin/users.rs` (handler), `admin/mod.rs:94-111` (route), `admin/error.rs` (`Conflict`), `admin/types.rs` (request), `openapi.rs`
- Modify: `server/src/auth/types.rs:13` (`USERNAME_REGEX` → `pub(crate)`), `auth/types.rs:84-97` (`AuthResponse`), `auth/login.rs:233,360`, `auth/register.rs:260`, `auth/mfa.rs:524`
- Modify: `server/src/auth/middleware.rs:63-105`, `auth/mod.rs:174-200`, `auth/error.rs`, `auth/profile.rs:303-362`, `auth/queries.rs` (`update_password_hash_tx`)
- Modify: `server/src/ws/handlers.rs` (both WS auth paths)
- Create: `server/tests/integration/admin_create_user.rs` (+ `mod` line)
- Modify: `docs/admin-guide/` (new `ops` note: "Creating family accounts"), `CHANGELOG.md`

**Interfaces:**
- Produces:
  - `POST /api/admin/users` → `pub async fn create_user(State, Extension<SystemAdminUser>, Extension<ElevatedAdmin>, ConnectInfo<SocketAddr>, Json<CreateUserRequest>) -> Result<(StatusCode, Json<UserSummary>), AdminError>`; `CreateUserRequest { username: String /* 3-32, USERNAME_REGEX */, display_name: Option<String> /* ≤64, validate_unicode_text */, temporary_password: String /* 8-128 */ }`. 201 with `UserSummary`; duplicate username → `AdminError::Conflict` (409 `CONFLICT`); audit log action `admin.users.create` (never logs the password).
  - Route wiring: `.route("/users", get(users::list_users).post(users::create_user.layer(from_fn_with_state(state.clone(), require_elevated))))` inside `admin_routes` (handler-level elevation layer; avoids an overlapping path between the two routers).
  - `AuthResponse.password_change_required: bool` (set from `user.must_change_password` in login, refresh, register, QR redeem).
  - `AuthError::PasswordChangeRequired` → 403 `PASSWORD_CHANGE_REQUIRED`.
  - `require_auth` rejects users with `must_change_password` with that error; new `pub async fn require_auth_allow_password_change(...)` (same as `require_auth` without that check) layers a separate protected router containing only `POST /auth/me/password` and `POST /auth/logout`. `POST /auth/refresh` is a public route and stays allowed (C4 update) — covered by a test.
  - WebSocket: both auth paths refuse such users (frame path: `Error{code:"PASSWORD_CHANGE_REQUIRED"}` then close 1008; header path: HTTP 403).
  - `update_password`: when `must_change_password`, reject `new_password == current_password` (400 validation) and clear the flag in the same transaction.

- [ ] **Step 1: Failing tests** in `admin_create_user.rs` (use `make_admin`, `create_elevated_session`, `generate_access_token`):

```rust
#[sqlx::test] async fn admin_creates_user_201(pool: PgPool) { /* body → 201, ["username"]=="oma", user row must_change_password=true */ }
#[sqlx::test] async fn create_user_requires_elevation(pool: PgPool) { /* admin w/o elevated session → 403 */ }
#[sqlx::test] async fn create_user_non_admin_forbidden(pool: PgPool) { /* → 403 */ }
#[sqlx::test] async fn create_user_duplicate_username_409(pool: PgPool) {}
#[sqlx::test] async fn create_user_works_with_registration_closed(pool: PgPool) { /* set registration_policy "closed" → still 201; POST /auth/register still 403 */ }
#[sqlx::test] async fn login_reports_password_change_required(pool: PgPool) { /* login as new user → body["password_change_required"]==true */ }
#[sqlx::test] async fn pending_user_only_reaches_allowed_routes(pool: PgPool) {
    // GET /auth/me → 403, body["error"]=="PASSWORD_CHANGE_REQUIRED" (match existing AuthError JSON shape)
    // GET /api/guilds → 403 same code
    // POST /auth/refresh with the login refresh token → 200, password_change_required==true
    // POST /auth/me/password {current: temp, new: temp} → 400
    // POST /auth/me/password {current: temp, new: other} → 200
    // GET /auth/me → 200; re-login → password_change_required==false
}
#[sqlx::test] async fn pending_user_websocket_refused(pool: PgPool) { /* ws_connect (if Task 1 merged; else raw tungstenite) → error event code PASSWORD_CHANGE_REQUIRED, close 1008 */ }
```

- [ ] **Step 2: Run to verify failure** — `cargo test -p vc-server --test integration admin_create_user`. Expected: FAIL (405/404 on POST).
- [ ] **Step 3: Implement** migration, model field, error variants, middleware split, route, handler, response field, WS checks, password-change flag clearing.
- [ ] **Step 4: Run to verify pass** — same + `cargo nextest run -p vc-server -E 'test(auth) | test(admin)'` (all `AuthResponse` constructors compile; existing auth tests green).
- [ ] **Step 5: Docs** — `docs/admin-guide/ops/creating-accounts.md`: keep registration `closed`/`invite_only`, elevate, create account, hand over temporary password or QR, user must change it on first login. CHANGELOG `### Added`: "Server admins can create accounts with a temporary password (Admin → Users); the new user is asked to set their own password at first sign-in. Registration can stay closed."
- [ ] **Step 6: Gates + commit** — `git commit -m "feat(auth): admin-created accounts with forced password change"`

---

### Task 6: Remove voice peers on socket loss and failed connections (B-02)

Branch `fix/voice-ghost-peers`. Rebase over Task 1 if merged (cleanup block in `handle_socket`).

**Files:**
- Create: `server/src/voice/leave.rs` (+ `pub mod leave;` in `voice/mod.rs`)
- Modify: `server/src/voice/ws_handler.rs:280-428` (`handle_leave` → delegate)
- Modify: `server/src/voice/sfu.rs:278-300` (fields), `:474-514` (`create_peer`), `:788-805` (`register_pc_state_handler`)
- Modify: `server/src/ws/handlers.rs:486-500` (cleanup), `server/src/main.rs:~390` (spawn reaper)
- Test: `server/src/voice/ws_handler_test.rs`
- Modify: `server/src/voice/AGENTS.md`, `CHANGELOG.md`

**Interfaces:**
- Produces:
  - `pub async fn leave_voice_internal(sfu: &Arc<SfuServer>, pool: &PgPool, user_id: Uuid, channel_id: Uuid, screen_share_limiter: Option<&ScreenShareLimiter>, expected_session: Option<Uuid>) -> Result<bool, VoiceError>` — the body of today's `handle_leave` **without** `require_channel_access`; when `expected_session` is `Some(s)` and the current peer's `session_id != s`, it does nothing and returns `Ok(false)`. Returns whether a peer was removed. Broadcasts `ScreenShareStopped`/`WebcamStopped`/`VoiceUserLeft`, finalizes the session, `forget_peer`, `cleanup_room_if_empty` — same as today.
  - `handle_leave` = `leave_voice_internal(.., None)`; the permission check is removed (leaving never requires access — a user whose access was revoked mid-call must still be able to leave).
  - `impl SfuServer { pub async fn find_peer_owned_by(&self, user_id: Uuid, tx: &mpsc::Sender<OutboundMsg>) -> Option<(Uuid /*channel*/, Uuid /*session*/)> }` — matches `peer.signal_tx.same_channel(tx)`.
  - `pub async fn leave_voice_on_socket_close(sfu: &Arc<SfuServer>, pool: &PgPool, user_id: Uuid, tx: &mpsc::Sender<OutboundMsg>, limiter: Option<&ScreenShareLimiter>)` — called from the WS cleanup before presence is set offline.
  - `pub struct StalePeer { pub user_id: Uuid, pub channel_id: Uuid, pub session_id: Uuid }`; `SfuServer` gains `stale_tx: mpsc::UnboundedSender<StalePeer>` and `stale_rx: Mutex<Option<mpsc::UnboundedReceiver<StalePeer>>>`; `pub fn stale_peer_sender(&self) -> mpsc::UnboundedSender<StalePeer>`; `pub fn spawn_stale_peer_reaper(self: &Arc<Self>, pool: PgPool, limiter: Option<ScreenShareLimiter>) -> JoinHandle<()>` (takes the receiver once; each item → `leave_voice_internal(.., Some(session_id))`).
  - `pub const PEER_FAILED_GRACE: Duration = Duration::from_secs(15);` — on `RTCPeerConnectionState::Failed` the state handler spawns a task: sleep the grace, and if the peer still exists and that PC's `connection_state()` is still `Failed`, send `StalePeer`. `Disconnected` stays log-only.
  - `main.rs`: `state.sfu.spawn_stale_peer_reaper(state.db.clone(), state.screen_share_limiter.clone())` after `AppState::new`.

- [ ] **Step 1: Failing tests** in `ws_handler_test.rs` (reuse its `create_test_user`/guild/channel helpers and `handle_voice_event(VoiceJoin)`):

```rust
#[sqlx::test] async fn test_socket_close_removes_peer_and_notifies_room(pool: PgPool) {
    // u1 joins with tx1, u2 joins with tx2; leave_voice_on_socket_close(&sfu,&pool,u1,&tx1,None)
    assert!(room.get_peer(u1).await.is_none());
    // drain rx2 → contains ServerEvent::VoiceUserLeft{user_id:u1,..}
}
#[sqlx::test] async fn test_socket_close_only_removes_peer_owned_by_that_socket(pool: PgPool) {
    // u1 joins with tx1; call with a different tx_other → peer still present
}
#[sqlx::test] async fn test_leave_succeeds_after_access_revoked(pool: PgPool) {
    // u2 joins; delete u2's guild membership; VoiceLeave → Ok, peer removed
}
#[sqlx::test] async fn test_reaper_removes_matching_session(pool: PgPool) {
    // spawn_stale_peer_reaper; send StalePeer{session_id: peer.session_id}; wait ≤2s → peer gone
}
#[sqlx::test] async fn test_reaper_ignores_rejoined_session(pool: PgPool) {
    // send StalePeer with an old session_id after u1 re-joined → peer still present
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test -p vc-server --lib voice::ws_handler_test`. Expected: FAIL (functions not defined).
- [ ] **Step 3: Implement** `voice/leave.rs`, SFU fields/methods, state-handler grace task, WS cleanup call, `main.rs` spawn.
- [ ] **Step 4: Run to verify pass** — same + `cargo nextest run -p vc-server -E 'test(voice)'`.
- [ ] **Step 5: CHANGELOG** `### Fixed`: "People who lose connection, close their laptop or crash no longer stay visible in the voice channel; they are removed automatically."
- [ ] **Step 6: Gates + commit** — `git commit -m "fix(voice): remove peers on socket close and failed connections"`

---

### Task 7: One send path for text and file messages (B-05)

Branch `fix/upload-send-path`. Independent.

**Files:**
- Create: `server/src/chat/send.rs` (+ `pub mod send;` in `chat/mod.rs`)
- Modify: `server/src/chat/messages.rs:138-782` (`create` uses the helpers)
- Modify: `server/src/chat/uploads.rs:328-646` (`upload_message_with_file`)
- Modify: `server/src/db/queries.rs:1033` (add `create_message_with_id`), `:1667` (`create_file_attachment` takes an executor)
- Test: `server/tests/integration/uploads_http.rs`, `messages_http.rs`; `server/tests/integration/helpers/mod.rs` (`TestApp::with_pool_and_unreachable_s3`)
- Modify: `server/src/chat/AGENTS.md`, `CHANGELOG.md`

**Interfaces:**
- Produces (in `chat/send.rs`):
  - `pub struct SendIntent<'a> { pub content: &'a str, pub encrypted: bool, pub has_attachment: bool }`
  - `pub async fn authorize_send(state: &AppState, user_id: Uuid, channel_id: Uuid, intent: &SendIntent<'_>) -> Result<db::Channel, ChatError>` — in this order: channel exists (`ChannelNotFound`), `require_channel_access` (`Forbidden`), guild: `SEND_MESSAGES`, plus `ATTACH_FILES` when `has_attachment`; announcement: `SEND_ANNOUNCEMENTS`; DM: block check both directions honoring `block_check_fail_open` (`Blocked`); guild + `@everyone`/`@here`: `MENTION_EVERYONE` (`Validation`); guild + `!encrypted` + non-empty content: content filter with moderation logging and admin event (`ContentFiltered`). Logic moved verbatim from `messages::create:144-280`.
  - `pub struct NewMessageDispatch<'a> { pub channel: &'a db::Channel, pub author_id: Uuid, pub message_id: Uuid, pub content: &'a str, pub encrypted: bool, pub is_crosspost: bool, pub message_json: serde_json::Value }`
  - `pub async fn dispatch_new_message(state: &AppState, d: NewMessageDispatch<'_>)` — `MessageNew` broadcast, announcement crosspost (`!is_crosspost`), DM push wake to other participants, then `dispatch_bot_events`. Never returns an error (logs).
  - `pub fn dispatch_bot_events(state: &AppState, channel: &db::Channel, author_id: Uuid, message_id: Uuid, content: &str, encrypted: bool)` — the existing spawned bot/webhook fan-out (guild channels, non-encrypted). The thread-reply path in `create` keeps its own `ThreadReplyNew` broadcast and calls only this.
  - `pub async fn create_message_with_id(exec: impl sqlx::PgExecutor<'_>, id: Uuid, channel_id: Uuid, user_id: Uuid, content: &str) -> sqlx::Result<Message>`; `create_file_attachment(exec: impl sqlx::PgExecutor<'_>, …)` (other args unchanged).

Upload handler order (decided): parse multipart → size/MIME/magic-byte validation → `authorize_send(content, has_attachment: true)` → S3-configured check (503) → `message_id = Uuid::now_v7()`, key `attachments/{channel}/{message_id}/{file_id}.{ext}` → variants + original upload → one transaction inserting message (with that id) and attachment → on any DB error delete the uploaded objects → build response → `dispatch_new_message`. An S3 failure therefore leaves **no** message row. Content-filter rejection on uploads becomes 403 `CONTENT_FILTERED` (was 400), same as text.

- [ ] **Step 1: Failing tests** (`uploads_http.rs`; no RustFS needed because authorization precedes the S3 check):

```rust
#[sqlx::test] async fn test_upload_requires_attach_files(pool: PgPool) {
    // guild default role VIEW_CHANNEL|SEND_MESSAGES (no ATTACH_FILES) → 403
}
#[sqlx::test] async fn test_upload_blocked_in_dm(pool: PgPool) { /* recipient blocked sender → 403 BLOCKED */ }
#[sqlx::test] async fn test_upload_to_announcement_requires_send_announcements(pool: PgPool) {}
#[sqlx::test] async fn test_upload_everyone_mention_requires_permission(pool: PgPool) { /* content "@everyone" → 400 */ }
#[sqlx::test] async fn test_upload_s3_failure_leaves_no_message(pool: PgPool) {
    // TestApp::with_pool_and_unreachable_s3 (s3_endpoint = "http://127.0.0.1:9", dummy keys)
    // authorized upload → 5xx; SELECT count(*) FROM messages WHERE channel_id=$1 == 0
}
```
`test_upload_returns_503_without_s3` must keep passing (authorized user). Live delivery for uploads is covered by a RustFS-guarded test (skips when `rustfs_available()` is false, same pattern as existing S3 tests): `test_upload_message_broadcasts_message_new` in `ws_e2e.rs` if Task 1 merged, otherwise in `uploads_http.rs` via a raw Valkey subscriber on `channel:{id}` — asserts one `message_new` with one attachment. Push wake for uploads has no observable sink in tests before FCM (Phase 2); reviewers check that the upload path calls `dispatch_new_message`.

- [ ] **Step 2: Run to verify failure** — `cargo test -p vc-server --test integration uploads_http`. Expected: the new tests FAIL (upload currently 503 before auth / message row created).
- [ ] **Step 3: Implement** `chat/send.rs`, refactor `create` to call `authorize_send` + `dispatch_new_message`/`dispatch_bot_events`, rewrite the upload flow in the decided order, executor-generic DB functions.
- [ ] **Step 4: Run to verify pass** — `cargo nextest run -p vc-server -E 'test(upload) | test(messages) | test(announcement) | test(mention) | test(blocking) | test(filters)'`.
- [ ] **Step 5: CHANGELOG** `### Fixed`: "Photos and files now follow the same rules as text messages (blocking, attach-files, announcement and @everyone permissions) and trigger the same notifications; a failed upload no longer leaves an empty message behind."
- [ ] **Step 6: Gates + commit** — `git commit -m "fix(chat): share send authorization and dispatch with uploads"`

---

### Task 8: Public file endpoint serves only public prefixes, streamed (S-08)

Branch `fix/public-files-allowlist`. Independent. (Review suggests shipping with the security hotfix — can merge first.)

**Files:**
- Modify: `server/src/chat/files.rs:39-78`
- Create: `server/tests/integration/files_http.rs` (+ `mod` line)
- Modify: `CHANGELOG.md`

**Interfaces:**
- Produces: `pub fn is_public_file_key(key: &str) -> bool` — true only for `avatars/{…}` and `guilds/{uuid}/banner-{…}`; false for anything containing `..`, `//`, a leading `/`, or any other prefix (`attachments/`, `exports/`, `emojis/`, …). `serve` returns 404 "File not found" for non-public keys **before** touching S3, streams the body via `axum::body::Body::new(stream.into_inner())` (same pattern as `uploads.rs:782`), and adds `X-Content-Type-Options: nosniff`.

- [ ] **Step 1: Failing tests** — unit tests in `files.rs`:

```rust
#[test] fn public_keys() {
    assert!(is_public_file_key("avatars/0190.../1700_me.png"));
    assert!(is_public_file_key("guilds/0190f.../banner-1700_b.png"));
    assert!(!is_public_file_key("attachments/c/m/f.png"));
    assert!(!is_public_file_key("exports/u/j.zip"));
    assert!(!is_public_file_key("avatars/../exports/u/j.zip"));
    assert!(!is_public_file_key("guilds/not-a-uuid/banner-x.png"));
    assert!(!is_public_file_key("/avatars/x.png"));
}
```
`files_http.rs`: `test_files_endpoint_rejects_private_prefix_without_s3` — `TestApp::with_pool` (S3 `None`): `GET /api/files/exports/x/y.zip` → 404 (today 503), `GET /api/files/avatars/x.png` → 503 (allowed prefix reaches the storage check).

- [ ] **Step 2: Run to verify failure** — `cargo test -p vc-server --lib chat::files && cargo test -p vc-server --test integration files_http`.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run to verify pass.**
- [ ] **Step 5: CHANGELOG** `### Security`: "The public file endpoint now serves only avatars and server banners and streams files instead of buffering them."
- [ ] **Step 6: Gates + commit** — `git commit -m "fix(api): restrict public file endpoint to public prefixes"`

---

### Task 9: Outbound requests never follow redirects and stay pinned (SSRF guard, webhooks + push)

Branch `fix/ssrf-redirects`. Independent.

**Files:**
- Modify: `server/src/webhooks/ssrf.rs` (builder + IP ranges)
- Modify: `server/src/webhooks/delivery.rs:298-311`, `server/src/webhooks/handlers.rs:416-426`, `server/src/push/mod.rs:146-185` (send) and `:240-270` (`create_subscription` validation)
- Test: in-module tests in `ssrf.rs`; `server/tests/integration/push_http.rs`
- Modify: `server/src/webhooks/AGENTS.md`, `CHANGELOG.md`

**Interfaces:**
- Produces:
  - `pub fn pinned_client(verified: &VerifiedUrl, timeout: Duration) -> reqwest::Result<reqwest::Client>` — `.resolve(&verified.host, verified.addr)`, `.redirect(reqwest::redirect::Policy::none())`, `.no_proxy()`, `.timeout(timeout)`. The only way user-controlled URLs are fetched.
  - Webhook delivery, webhook test endpoint and `send_unifiedpush` all call `verify_resolved_ip` then `pinned_client(&verified, 10s)`; push no longer discards the `VerifiedUrl`. A 3xx response is treated as failure (`Delivery::Failed` / webhook retry), never followed.
  - `is_private_ip` additionally blocks `0.0.0.0/8` and NAT64 `64:ff9b::/96` with a private embedded IPv4.
  - `create_subscription`: `unifiedpush` endpoints must be `https` and pass `is_blocked_host` (400 `Validation` otherwise).

- [ ] **Step 1: Failing tests** (`ssrf.rs`, `#[tokio::test]`):

```rust
#[tokio::test]
async fn pinned_client_does_not_follow_redirects() {
    // axum server A on 127.0.0.1:pa: GET /  → 302 Location: http://127.0.0.1:{pb}/hit
    // server B counts hits on /hit
    let v = VerifiedUrl { host: "hook.test".into(), addr: a_addr };
    let resp = pinned_client(&v, Duration::from_secs(2)).unwrap()
        .get(format!("http://hook.test:{pa}/")).send().await.unwrap();
    assert_eq!(resp.status(), 302);
    assert_eq!(b_hits.load(SeqCst), 0);
}
#[test] fn blocks_zero_network_and_nat64_private() {
    assert!(is_private_ip(&"0.1.2.3".parse().unwrap()));
    assert!(is_private_ip(&"64:ff9b::a00:1".parse().unwrap())); // 10.0.0.1
}
```
`push_http.rs`: `test_create_push_subscription_rejects_http_and_private` — `endpoint: "http://push.example/x"` → 400; `"https://127.0.0.1/x"` → 400; `"https://push.example/x"` → 201.

- [ ] **Step 2: Run to verify failure** — `cargo test -p vc-server --lib webhooks::ssrf && cargo test -p vc-server --test integration push_http`.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run to verify pass** + `cargo nextest run -p vc-server -E 'test(webhook) | test(push)'`.
- [ ] **Step 5: CHANGELOG** `### Security`: "Outgoing webhook and push requests no longer follow redirects and always connect to the address that passed validation; push endpoints must use HTTPS."
- [ ] **Step 6: Gates + commit** — `git commit -m "fix(api): pin outbound webhook and push requests, no redirects"`

---

### Task 10: Rate limiting trusts forwarded headers only from configured proxies

Branch `fix/ratelimit-trusted-proxies`. Independent; Task 11 consumes the env var name.

**Files:**
- Modify: `Cargo.toml` (workspace dep `ipnet = "2"`), `server/Cargo.toml`
- Modify: `server/src/ratelimit/ip.rs:14-40`, `ratelimit/config.rs:15,86,170-198`, `ratelimit/middleware.rs:96-97`, `ratelimit/limiter.rs:433`, `voice/ws_handler_test.rs:223`
- Modify: `docs/admin-guide/configuration/rate-limiting.md`, `docs/admin-guide/configuration/environment.md` (C9), `CHANGELOG.md`

**Interfaces:**
- Produces:
  - `#[derive(Clone, Debug, Default)] pub struct TrustedProxies(Vec<ipnet::IpNet>)` with `pub fn parse(list: &str) -> Result<Self, String>` (comma-separated CIDRs or bare IPs → /32, /128; empty → none) and `pub fn contains(&self, ip: IpAddr) -> bool`.
  - `RateLimitConfig.trusted_proxies: TrustedProxies` replaces `trust_proxy: bool`. Env `RATE_LIMIT_TRUSTED_PROXIES` (default empty = trust none). Legacy `RATE_LIMIT_TRUST_PROXY=true` without CIDRs → `warn!` at startup ("ignored, set RATE_LIMIT_TRUSTED_PROXIES") and trusts nothing; invalid CIDR → `warn!` and that entry is skipped.
  - `pub fn extract_client_ip(headers: &HeaderMap, connect_info: Option<&ConnectInfo<SocketAddr>>, trusted: &TrustedProxies) -> IpAddr` — if the TCP peer is not trusted → peer IP. Otherwise walk all `X-Forwarded-For` values right-to-left, skipping trusted entries; the first untrusted, parseable IP is the client. No usable XFF → `X-Real-IP` (only when the peer is trusted) → peer IP.

Scope note: session/audit IPs (`ConnectInfo` in auth/admin handlers) are unchanged.

- [ ] **Step 1: Failing tests** in `ip.rs` / `config.rs` (replace the four `trust_proxy` tests):

```rust
#[test] fn untrusted_peer_headers_ignored() { /* peer 203.0.113.9, XFF "1.2.3.4", trusted none → 203.0.113.9 */ }
#[test] fn trusted_peer_uses_rightmost_untrusted() {
    // trusted 10.42.0.0/16; peer 10.42.0.7; XFF "6.6.6.6, 198.51.100.4, 10.42.1.2" → 198.51.100.4
}
#[test] fn trusted_peer_all_trusted_falls_back_to_peer() {}
#[test] fn trusted_peer_real_ip_when_no_xff() {}
#[test] fn parse_accepts_ips_and_cidrs_and_rejects_garbage() { /* "10.0.0.0/8, 192.0.2.1, ::1" ok; "nope" → Err */ }
#[test] fn legacy_trust_proxy_without_cidrs_trusts_nothing() {
    // RateLimitConfig::from_env_map(&[("RATE_LIMIT_TRUST_PROXY","true")]) → trusted_proxies empty
}
```
(Add `pub(crate) fn from_env_map(vars: &[(&str, &str)]) -> Self` behind `from_env()` so parsing is testable without mutating process env.)

- [ ] **Step 2: Run to verify failure** — `cargo test -p vc-server --lib ratelimit`.
- [ ] **Step 3: Implement**; `cargo deny check licenses`.
- [ ] **Step 4: Run to verify pass** + `cargo nextest run -p vc-server -E 'test(ratelimit)'`.
- [ ] **Step 5: Docs** — rate-limiting.md + environment.md: `RATE_LIMIT_TRUSTED_PROXIES` with examples (compose network subnet; k3s Traefik pod CIDR `10.42.0.0/16` for the athene deployment plan). CHANGELOG `### Changed`: "Rate limiting now only trusts `X-Forwarded-For` from proxies listed in `RATE_LIMIT_TRUSTED_PROXIES`. `RATE_LIMIT_TRUST_PROXY` is no longer used — self-hosters behind a reverse proxy must set the new variable."
- [ ] **Step 6: Gates + commit** — `git commit -m "fix(ratelimit): honor forwarded headers only from trusted proxies"`

---

### Task 11: Compose hardening for self-hosters

Branch `fix/compose-hardening`. Depends on Tasks 4 and 10 being merged (env names, tauri origins).

**Files:**
- Modify: `infra/compose/docker-compose.yml`, `infra/compose/.env.beta`, `infra/scripts/setup-beta.sh:132-145`
- Create: `scripts/check_compose_hardening.py`
- Modify: `.github/workflows/ci.yml:73-74` (one step after "Validate CI guardrails")
- Modify: `infra/compose/AGENTS.md`, `docs/admin-guide/self-hosting.md`, `CHANGELOG.md`

**Interfaces / decisions:**
- `coturn`: add `--no-multicast-peers`, `--no-tcp-relay` (TURN/TCP relay fallback is given up; TURN over TCP to the server still works), and `--denied-peer-ip=` for `0.0.0.0-0.255.255.255`, `10.0.0.0-10.255.255.255`, `100.64.0.0-100.127.255.255`, `127.0.0.0-127.255.255.255`, `169.254.0.0-169.254.255.255`, `172.16.0.0-172.31.255.255`, `192.168.0.0-192.168.255.255`, `::1`, `fc00::-fdff:ffff:ffff:ffff:ffff:ffff:ffff:ffff`, `fe80::-febf:ffff:ffff:ffff:ffff:ffff:ffff:ffff`; move it to its own network `turn` (not `voicechat`).
- `server` env: `REDIS_URL=redis://:${VALKEY_PASSWORD:?set VALKEY_PASSWORD}@valkey:6379`, `CORS_ALLOWED_ORIGINS=${CORS_ALLOWED_ORIGINS:?set to https://<your domain>}` (tauri origins are added by the server, Task 4), `RATE_LIMIT_TRUSTED_PROXIES=${KAIKU_NET_SUBNET:-172.31.250.0/24}` (replacing `RATE_LIMIT_TRUST_PROXY`), `PUBLIC_IP=${PUBLIC_IP:-}` (the SFU must advertise its public address now that TURN cannot relay to private ones), `AWS_ACCESS_KEY_ID=${S3_ACCESS_KEY}`, `AWS_SECRET_ACCESS_KEY=${S3_SECRET_KEY}`.
- `networks.voicechat.ipam.config: [{ subnet: ${KAIKU_NET_SUBNET:-172.31.250.0/24} }]`; port 8080 stays unpublished.
- `valkey`: `VALKEY_PASSWORD=${VALKEY_PASSWORD:?…}`, drop `ALLOW_EMPTY_PASSWORD`; healthcheck `["CMD-SHELL", "REDISCLI_AUTH=\"$$VALKEY_PASSWORD\" valkey-cli ping | grep -q PONG"]`.
- `setup-beta.sh`: `VALKEY_PASSWORD=$(openssl rand -hex 24)`; `.env.beta` comments say hex/URL-safe, drop `RATE_LIMIT_TRUST_PROXY`, add `KAIKU_NET_SUBNET`.
- `check_compose_hardening.py`: token checks in the style of `check_ci_guardrails.py` (`require_token` / `forbid_pattern`, exits 1 with the list of errors): requires `--denied-peer-ip=172.16.0.0-172.31.255.255`, `--no-tcp-relay`, `REDIS_URL=redis://:${VALKEY_PASSWORD`, `REDISCLI_AUTH`, `CORS_ALLOWED_ORIGINS=`, `RATE_LIMIT_TRUSTED_PROXIES=`, `PUBLIC_IP=`, `AWS_ACCESS_KEY_ID=`; forbids `RATE_LIMIT_TRUST_PROXY` in compose and `.env.beta`, forbids `rand -base64` on the `VALKEY_PASSWORD` line of `setup-beta.sh`, forbids `voicechat` in the coturn service block (regex over the block between `coturn:` and the next top-level service).

- [ ] **Step 1: Write `scripts/check_compose_hardening.py`** and run it against the unchanged files.
Run: `python3 scripts/check_compose_hardening.py`
Expected: FAIL listing every missing token.
- [ ] **Step 2: Edit compose, `.env.beta`, `setup-beta.sh`** as decided.
- [ ] **Step 3: Verify**
Run: `python3 scripts/check_compose_hardening.py && (cd infra/compose && VALKEY_PASSWORD=x CORS_ALLOWED_ORIGINS=https://a POSTGRES_PASSWORD=x docker compose config -q)`
Expected: both exit 0.
Run (optional local smoke, dev machine only): `cd infra/compose && docker compose --env-file .env.test up -d valkey && docker compose ps valkey` → `healthy`.
- [ ] **Step 4: Wire into CI** — `.github/workflows/ci.yml` docs-governance job: `- name: Validate compose hardening` / `run: python3 scripts/check_compose_hardening.py`.
- [ ] **Step 5: Docs + CHANGELOG** — self-hosting.md: required variables, hex password, `KAIKU_NET_SUBNET` collision note, TURN/TCP relay trade-off. CHANGELOG `### Security`: "The self-hosting compose file now isolates the TURN server, requires a Valkey password, forwards the CORS allow-list and trusted-proxy settings, and passes storage credentials to the server."
- [ ] **Step 6: Commit** — `git commit -m "fix(infra): harden compose for self-hosters"`

---

### Task 12: Backup and preflight scripts authenticate to Postgres (#623, B-06)

Branch `fix/backup-psql-auth`. Independent.

> These scripts serve compose self-hosters. The athene deployment will replace them with a k8s CronJob (separate infra plan); do not invest beyond correctness.

**Files:**
- Modify: `infra/scripts/backup.sh:43-56`, `infra/scripts/upgrade-preflight.sh:64-105`
- Modify: `docs/ops/backup-restore-runbook.md:53-67,134-156` (only the `psql`/`pg_dump` command lines; the restore drill itself belongs to the release stream)
- Create: `infra/scripts/tests/run.sh`, `infra/scripts/tests/fake-docker` (executable shim)
- Modify: `.github/workflows/ci.yml` (one step), `CHANGELOG.md`

**Interfaces / decisions:**
- All in-container Postgres calls use `docker exec "$CONTAINER" sh -c 'PGPASSWORD="$POSTGRESQL_PASSWORD" pg_dump -U "$POSTGRESQL_USERNAME" "$POSTGRESQL_DATABASE"'` (resp. `psql … -tAc …`), reading the Bitnami env inside the container; `POSTGRES_USER`/`POSTGRES_DB` script overrides are removed.
- Dump completeness: after gzip, `zcat "$BACKUP_FILE" | tail -n 5 | grep -q "PostgreSQL database dump complete"` or fail (also in preflight check 4), so a ~20-byte empty gzip from a failed dump is rejected.
- Preflight check 5 compares migration **versions** (`SELECT version FROM _sqlx_migrations` vs. the numeric prefixes of `server/migrations/*.sql`): fail if the DB has a version the repo lacks; pending = repo − DB.
- Preflight check 6: `docker image inspect --format '{{index .RepoDigests 0}}' "$(docker inspect -f '{{.Config.Image}}' canis-server)"`, falling back to the image id when `RepoDigests` is empty.
- `fake-docker` records its argv to `$FAKE_DOCKER_LOG` and emulates `exec … sh -c …` by asserting the command string contains `PGPASSWORD=` (exit 2 otherwise) and printing a minimal dump ending with `-- PostgreSQL database dump complete`; a `FAKE_DOCKER_MODE=fail` variant prints nothing and exits 1.

- [ ] **Step 1: Write failing `infra/scripts/tests/run.sh`** — puts `fake-docker` first on `PATH` as `docker`, runs `BACKUP_DIR=$(mktemp -d) BACKUP_RUSTFS=false infra/scripts/backup.sh`:
  - case ok: exit 0, one `kaiku-*.sql.gz` exists, `zcat` contains `dump complete`;
  - case `FAKE_DOCKER_MODE=fail`: exit ≠ 0 and **no** `.sql.gz` left behind;
  - `bash -n` on both scripts.
Run: `bash infra/scripts/tests/run.sh`
Expected: FAIL (current script calls `pg_dump` without `PGPASSWORD`, shim exits 2).
- [ ] **Step 2: Fix both scripts and the runbook command lines** as decided.
- [ ] **Step 3: Verify** — `bash infra/scripts/tests/run.sh && shellcheck infra/scripts/backup.sh infra/scripts/upgrade-preflight.sh` (shellcheck if installed). Expected: PASS.
- [ ] **Step 4: CI** — add `- name: Test ops scripts` / `run: bash infra/scripts/tests/run.sh` to the docs-governance job.
- [ ] **Step 5: CHANGELOG** `### Fixed`: "Nightly backups and the upgrade preflight check now authenticate to the database correctly and reject incomplete dumps (#623)."
- [ ] **Step 6: Commit** — `git commit -m "fix(infra): authenticate backup and preflight psql calls (#623)"`

---

## Contract notes for the coordinator (not edits to the contracts file)

- **C1 clarifications:** unknown or other-channel `after` anchor → 400 `VALIDATION_ERROR`; with `after`, `next_cursor` is the id of the newest returned item (pass it as the next `after`); edits/deletes during the gap are not covered.
- **C2 clarification:** `Ready` is sent only after the server's subscriptions are live; clients should run catch-up after receiving `Ready` (no gap, dedupe by id).
- **C3 clarification:** `channel` = one element of the `GET /api/dm` array (`DMListResponse`: flattened channel + `participants` + `last_message` + `unread_count`), computed per recipient; also sent to the creator's other sessions.
- **C4 clarifications:** WS connections of pending users are refused (`error` event `PASSWORD_CHANGE_REQUIRED`, close 1008); restored sessions detect the state via 403 `PASSWORD_CHANGE_REQUIRED` on `GET /auth/me`; `POST /auth/refresh` stays allowed and its response also carries `password_change_required`.
- **New shared rule (proposed C11):** server close codes 1011/1013 mean "reconnect + catch-up"; 1008 remains auth rejection.

---

### Task 13: Fixed SFU UDP port range (contract C13) — added by coordinator 2026-10-22

**Why:** the athene deployment (host firewall, hostNetwork) can only open a
fixed UDP range; today the SFU uses ephemeral ports.

**Files:** `server/src/config.rs` (two new env vars), `server/src/voice/sfu.rs`
(SettingEngine port range where the API/peer connections are built),
`docs/admin-guide/configuration/environment.md` (C9), CHANGELOG `### Added`
("Operators can pin voice media to a UDP port range").

**Interfaces:** `VOICE_UDP_PORT_MIN: Option<u16>`, `VOICE_UDP_PORT_MAX:
Option<u16>` in `Config`; both-or-neither, `min <= max`, else startup error
`"VOICE_UDP_PORT_MIN and VOICE_UDP_PORT_MAX must be set together and min <= max"`.

- [ ] Test `config_rejects_half_set_udp_range` and `config_rejects_inverted_udp_range`.
- [ ] Test `sfu_candidates_use_configured_port_range`: build the SFU API with range 50000–50010, create a peer connection, gather host candidates, assert every UDP candidate port ∈ [50000, 50010].
- [ ] Implement via `SettingEngine::set_udp_network(UDPNetwork::Ephemeral(EphemeralUDP::new(min, max)?))` (or the webrtc-rs 0.17 equivalent — verify in the crate source).
- [ ] Verify: `cargo test -p vc-server voice::` + clippy; commit `feat(voice): allow pinning SFU media to a UDP port range`.
