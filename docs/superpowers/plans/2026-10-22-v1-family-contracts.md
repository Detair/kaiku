# v1.0 Family Release — Cross-Stream Contracts (Phase 1)

Shared interfaces between the four Phase-1 plans (server, desktop, Android,
release). Each plan **must** use these names; if code reading shows a name
clashes with existing code, the server plan adapts the name here first and
the other plans follow.

Spec: `docs/superpowers/specs/2026-10-20-v1-family-release-goal-design.md`
Review: `docs/reviews/2026-10-22-v1-family-release-readiness.md`

## C1 — Message catch-up (server → Android, desktop)

`GET /api/messages/channel/{channel_id}?after={message_id}&limit={n}`
- Returns the existing `CursorPaginatedResponse<MessageResponse>`
  (`{items, has_more, next_cursor}`), **items ascending** (oldest first)
  when `after` is given; `has_more` true if more newer messages exist.
- `after` and `before`/`around` are mutually exclusive → 400.
- `limit` default 50, max 100 (same as existing).

## C2 — Server time on connect (server → clients)

`ServerEvent::Ready { user_id, server_time }` — `server_time` is an RFC 3339
UTC timestamp. Clients store it as their resync watermark.

## C3 — DM creation is pushed live (server → clients)

`ServerEvent::DmCreated { channel }` (wire `type: "dm_created"`), where
`channel` is the same DM channel object returned by the DM list endpoint.
Sent via `broadcast_to_user` to **every participant** on create /
get-or-create (only when newly created) / group DM create. The server also
auto-subscribes each participant's open sockets to the new channel.

## C4 — Admin-created accounts (server → desktop/web UI, ops)

`POST /api/admin/users` (system admin, elevated session)
- Body: `{ "username": string, "display_name": string|null, "temporary_password": string }`
- Response `201`: the admin user view of the created user.
- Created user has `must_change_password = true`.
- Login response gains `password_change_required: bool`; while true, only
  `POST /auth/me/password` (existing change-password endpoint),
  `POST /auth/refresh` and logout are allowed (others → 403 `PASSWORD_CHANGE_REQUIRED`).
- Registration policy stays `closed`/`invite_only` for the family server.

## C5 — Unknown enum tolerance (server ↔ Android)

Android must deserialize unknown `channel_type` (and other wire enums) to an
`UNKNOWN` value and hide such channels; server adds no new types in Phase 1.

## C6 — Tauri detection (desktop internal, but referenced by release)

Single helper `isTauri()` exported from `client/src/lib/tauri/common.ts`,
implemented via `@tauri-apps/api/core` `isTauri()`. The literal
`"__TAURI__" in window` is banned by lint. Desktop REST goes through
`httpRequest` with base URL + bearer token provided by Rust
(`invoke("get_auth_info")`), tokens owned by Rust/keyring.

## C7 — Versioning (release → all)

Single source of truth: git tag `vX.Y.Z[-rc.N]`. `scripts/bump-version.sh
X.Y.Z[-rc.N]` updates `client/src-tauri/tauri.conf.json`, root
`Cargo.toml` workspace version, `client/package.json`, and Android
`versionName`/`versionCode` in `mobile/android/version.properties` (`MAJOR*1_000_000 + MINOR*10_000 + PATCH*100 +
rc`, rc 1–98, 99 for final). CI fails a tag build whose files disagree.

## C8 — Server container image (release → athene deployment)

`ghcr.io/detair/kaiku-server:<version>` (`<version>` = tag without the
leading `v`; multi-arch not required, amd64),
built from `infra/docker/Dockerfile`, published by `release.yml` on tags.
Configuration only via environment variables (documented in
`docs/admin-guide/configuration/`); no config files baked in.

## C9 — Documented configuration (release CI → server)

Every `env::var` read in `server/src/config.rs` must appear in
`docs/admin-guide/configuration/environment.md`; a CI check enforces it.
Server PRs that add env vars (e.g. trusted proxy CIDRs) document them there.

## C10 — Android golden fixtures (server tests → Android tests)

Test-only file `server/tests/integration/android_contract_fixtures.rs`
(+ one `mod` line) serialises real server response types to JSON fixtures
consumed by Android tests. Owned by the Android stream, lands in the server
tree — the server stream must not create a conflicting file of that name.

## Reconciliation 2026-10-22 (after all four plans were written) — binding

- **C1:** an `after` anchor that is unknown or belongs to another channel → 400.
  `next_cursor` (with `after`) = id of the newest returned item.
- **C2:** `Ready` is sent only after the socket's subscriptions are live;
  clients run catch-up after receiving `Ready`.
- **C3:** `channel` = one element of the `GET /api/dm` array (`DMListResponse`).
- **C4 (final):** the 201 body is `UserSummary` (same shape as items of
  `GET /api/admin/users`). `password_change_required: bool` is carried by the
  login response, the `/auth/refresh` response and `GET /auth/me`. While it is
  true the server answers other REST calls with 403 code
  `PASSWORD_CHANGE_REQUIRED` (clients treat that as a fallback trigger) and
  refuses the WebSocket with close code 1008. Allowed: change password,
  refresh, logout, `GET /auth/me`.
- **C6 (desktop-internal):** `get_auth_info` returns
  `{server_url, access_token, expires_at_ms}`; new Tauri commands
  `refresh_session`, `session_request`.
- **C11 — WebSocket close codes:** 1011 and 1013 from the server mean
  "reconnect with backoff, then catch up"; 1008 means "auth problem — refresh,
  or show login / password change".
- **C12 — Desktop origins (server config + athene deployment):**
  `CORS_ALLOWED_ORIGINS` for the family server includes
  `https://kaiku.pmind.de`, `tauri://localhost`, `http://tauri.localhost`
  (and the compose file forwards the variable). Refresh-token delivery to
  tauri origins per server plan Task 4; `x-refresh-token` header no longer
  required by the desktop.
- **Shared files:** `ci.yml` is edited by release Tasks 1/2/4/5 and desktop
  Tasks 9/10 — the later-merging stream rebases; never reformat unrelated jobs.

## C13 — Fixed SFU UDP port range (server → athene deployment) — added 2026-10-22

The SFU (webrtc-rs) must bind ICE/media only within a configurable UDP port
range so the host firewall can open exactly that range:
`VOICE_UDP_PORT_MIN` / `VOICE_UDP_PORT_MAX` (both required together; unset =
current ephemeral behaviour for dev), applied via the webrtc-rs
`SettingEngine` ephemeral UDP port range; optional `VOICE_PUBLIC_IP` for
NAT 1:1 announcement if not already present. Documented per C9. Server
stream: add as **Task 13** (after Task 6), with a test that a peer
connection's local candidates use ports inside the range.
