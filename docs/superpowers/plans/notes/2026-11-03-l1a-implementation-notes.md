# L1a implementation notes (v1.0 login, server)

Plan: `docs/superpowers/plans/2026-11-03-v1.0-login-server.md` (Tasks 1–14, Task 30 gates).
Spec rev 5 and contracts C26–C28 win over the plan; every deviation and judgment call
is listed here, per task. "Coordinator" marks items that need a decision.

## Task 1 — primitives and `PUBLIC_URL`

- `Config::auth_hmac_key` is a newtype `AuthHmacKey([u8; 32])` with a redacting `Debug`
  (the plan says a bare `[u8; 32]`). `Config` derives `Debug`; a bare array would print the
  key. `AuthCtx::hmac_key()` still returns `&[u8; 32]`.
- DC1 derivation input: the configured `JWT_PRIVATE_KEY` value (the base64 PEM string) is
  the HKDF input key material. Rotating the JWT key therefore rotates the derived key, as DC1
  states.
- An empty `AUTH_HMAC_KEY=` (compose templates) counts as unset, like every other optional
  variable (`env_opt`).

## Task 3 — system account, read-only DM, security events

- **Migration renames a pre-existing `kaiku` account (coordinator).** C28 reserves the
  username `kaiku`; an existing server may already have a user with that name, and the plain
  `INSERT` would fail the migration and keep the server from starting. The migration renames
  such an account to `kaiku_<first 8 hex of its id>` (id, data and sessions unchanged; the
  owner signs in with the new name). Alternative if the coordinator prefers: fail the
  migration with a clear message and let the operator rename the account by hand.
- Extra DB guards beyond the plan: a partial unique index allows only one `is_system` row and
  a CHECK pins it to the fixed id; `security_events` has a CHECK that `authenticator_type` and
  `authenticator_id` are both set or both null; the immutability trigger also refuses
  re-setting `delivered_at` / `repudiated_at` once set (only NULL → time), and allows the
  `actor_user_id ON DELETE SET NULL` action.
- `DMListResponse.read_only` is derived from the participants (a DM with the system account),
  while `ensure_writable` reads `channels.read_only`. Both hold by construction: only
  `kaiku_dm_tx` creates DMs with the system account, and it sets `read_only`.
- Hide/mute/archive: the server has no per-channel hide, mute or archive routes (they live in
  the client preference blob), so there is nothing to guard server-side. Guarded routes: send
  (incl. upload-with-message), edit, delete, react / unreact, pin / unpin (before the
  guild-only check), typing (WS; drops the hint and answers `CHANNEL_READ_ONLY`), leave,
  rename, icon upload, and `PATCH`/`DELETE /api/channels/{id}` (a DM participant passes the
  channel permission check there).
- Presence: the system account has no friends, guilds or sessions, so no presence audience
  ever includes it; no query change was needed. Mentions: `chat::mentions::resolve_usernames`
  excludes it. Bots: `add_bot_to_guild` already requires `is_bot`. OIDC: identity resolution
  joins `users` and excludes it. Admin: list, count, search, export, details and
  `user_exists` (ban, delete, …) exclude it.
- `UserSummary.is_system` (admin list) is always false because the system account is never
  listed; `UserProfile.is_system` (`GET /auth/me`) is false for every signed-in user.

## Task 4 — notices, push, log, sweep

- New notices are announced to the recipient's own WS channel (`user:{id}`) as `message_new`
  (and `dm_created` when the Kaiku DM was created by this notice), not on `channel:{id}`: a
  freshly created DM has no channel subscriptions yet. Updates send
  `security_notice_updated` (C28) to the recipient only.
- Coalescing uses the event's `created_at`, not delivery time, so a late sweep coalesces the
  same way as the immediate delivery. A notice's `severity` becomes `high` when any of its
  events is high (`authenticator_added` can be `totp`/normal or `key_device`/high).
- `place` is `""` in v1.0 (DC17); on the wire it is `null` when empty (C28 `place|null`).
- `ip`/`place` are shown on the subject's own notices, the own log and the admin log; never
  on an admin's (or, in L1b, the contact's) copy.
- There are no server-side quiet hours or per-channel mute, so "bypass" is inherent: every
  security push goes to every subscription of the recipient through `push::deliver_with`,
  outside the C20 coalescer. `push/AGENTS.md` documents that clients must not suppress `high`.
- `SecurityEvent.authenticator.label` is resolved through `auth::strong::authenticator_label`
  (key devices from Task 8, passkeys from LP).
- The admin log's `user_id` is optional (all accounts when absent).

## Task 5 — assurance, `issue_session()`, committed-session check

- WebSocket with Valkey down: the session check no longer fails closed (the denylist is
  skipped, B.4 rev 5), so header auth is no longer refused with 503 before the upgrade. The
  socket still cannot subscribe without Valkey and closes for a resync (1011/1013) without
  ever becoming ready. `valkey_outage` tests were renamed and updated accordingly
  (`unreachable_denylist_is_skipped`, `websocket_header_auth_skips_unreachable_denylist`,
  `websocket_auth_closes_for_resync_when_valkey_hangs`).
- The identity-link and OIDC re-auth session checks moved to the same rule (row decides,
  denylist only rejects earlier); the link hand-back code `SERVICE_UNAVAILABLE` for an
  unreadable denylist is gone.
- A token without `sid` is refused everywhere (401 `INVALID_TOKEN`), including before
  `/auth/me/password` (`session_revocation` test updated: it expected 400 there).
- The OIDC desktop exchange records client class `desktop` unless the request says
  `X-Kaiku-Client: android` (Android redeems the same code, C27).
- Interim refresh (until Task 7): locks the live row by token hash, keeps the stable `sid`,
  caps `expires_at` at the absolute cap, sets `last_used_at`, returns the new
  `AuthResponse` fields; a denylist hit refuses, an unreachable denylist is skipped.
- DC7 test harness: `test_session_id` is a deterministic SHA-256-derived UUID (version 8)
  instead of `Uuid::new_v5` (the workspace `uuid` has no `v5` feature; adding one would add a
  dependency). Test users created without `create_test_user` call
  `helpers::ensure_test_session` (governance, identities, admin-created users).
- `MAX_SESSIONS_PER_USER` is read from the environment (1–1000, default 50) and documented;
  `session_recheck_secs` is a config field only (60, tests set 1).

## Task 6 — ending sessions in the revocation order

- The elevation cache (`admin:elevated:{sid}`) is a positive cache, so it is dropped inside
  the revoking path (before commit, best effort) as well as by the listener after commit
  (B.4: a cache needs synchronous invalidation in the revoking path). A concurrent request
  that re-caches between the drop and the commit is harmless: the ended session is refused
  by the row check before any elevation check.
- The listener uses its own one-connection pool (same options), so it never holds one of
  the application pool's connections. The integration-test `TestApp` spawns one listener per
  app and aborts it on drop (`AbortOnDrop`), so `#[sqlx::test]` can drop its database
  (PostgreSQL otherwise waits 5 s per test and then refuses the drop).
- `valkey_flush_keeps_ended_sessions_refused` deletes the session's denylist key instead of
  `FLUSHALL`, which would disturb every other test sharing the test Valkey.
- Semantics change pinned by updated tests: a failed denylist write no longer turns a ban
  into a 500 (the ban, its audit row and the ended sessions commit; B.4 rev 5);
  `test_ban_is_audited_when_ending_sessions_fails` became
  `test_ban_commits_when_the_denylist_write_fails`.
- Admin user deletion now ends the sessions (reason `account_deleted`) and deletes the user in
  one transaction; the admin `find_user`/`delete_user` queries exclude the system account
  (the admin could otherwise delete it).
- Cleanup deletes ended rows 30 days after `ended_at` and never-ended rows 30 days after their
  last expiry (`LEAST(expires_at, absolute_expires_at)`), so a lapsed key-device session is
  still recognised for a month.

## For L1b (messages from the client implementers, 2026-11-03)

- (1) Enrolment status of a void/expired enrolment: see Task 11 below for the code L1a uses.
- (2) Clients take the step-up `sid` from `GET /auth/me` → `session.id` (C27); L1a's
  `/auth/me` returns it.
- (3) Admin user views should carry `is_admin` so clients can hide admin-on-admin actions —
  additive; L1a adds it with `may_approve` in Task 8 if in scope, otherwise L1b with the C27
  admin view fields.
- (4) Clients name the `/auth/me` session object `AuthSession`; the server keeps the C27 field
  names (`id, assurance, auth_methods, auth_time, client_class, absolute_expires_at,
  key_device_id`).
- (5) Desktop OIDC callback messages `oidc-callback {status, attempt}` and
  `oidc-request-callback`: L1b/LP territory, not touched in L1a.

## Task 7 — refresh rotation, caps, session limit

- Refresh order under the row lock: signature (expiry not checked — the row decides
  inactivity, answered `SESSION_EXPIRED`) → row exists and belongs to `sub` → **ban** (403
  `ACCOUNT_BANNED`, as before: a banned user's client is told why) → ended or denylist hit
  (401 `INVALID_TOKEN`; an unreachable denylist is skipped) → absolute cap (ends the row,
  `SESSION_EXPIRED`) / inactivity (`SESSION_EXPIRED`, the row lapses) → generation logic. The
  plan lists "ended" before "banned"; neither the spec nor C27 fixes the order, and the
  existing C25 tests pin `ACCOUNT_BANNED` for a banned user's refresh.
- New-format refresh JWT: `{sub, exp, iat, typ, jti, sid, gen}`; the row stores
  `refresh_generation`, `refresh_jti`, `refresh_issued_at` (second precision, the JWT `iat`)
  and `refresh_retry_at`. The retry re-encodes the identical token (Ed25519 is deterministic)
  and checks it against the stored hash before answering; a mismatch (only if
  `JWT_REFRESH_EXPIRY` changed within those 10 s) answers `INVALID_TOKEN`.
- A token with a higher `gen` than the row, or the current `gen` with a different hash, is
  "unrecognised": 401 `INVALID_TOKEN`, nothing changes.
- A spent legacy refresh token (no `gen`) is not recognisable as a predecessor: after its one
  rotation it answers `INVALID_TOKEN` and ends nothing.
- The refresh cookie is always `SameSite=Strict` (B.4); `COOKIE_SAMESITE` no longer applies to
  it (documented). Name `__Secure-kaiku_refresh` with `COOKIE_SECURE`, else `kaiku_refresh`
  (local development); the old name is read once and cleared.
- Session-limit notices (`session_ended_limit`) are recorded in the issuing transaction;
  `IssuedSession.event_ids` lets the sign-in handlers deliver them right after commit (the
  minute sweep is the fallback). The reuse notice is delivered by a spawned
  `deliver_pending` after the 401.
