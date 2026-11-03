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

## Task 9 — step-up grants, guards, elevation

- `consume_tx` picks the oldest **unexpired** open grant (an expired one is simply not
  found → `STEP_UP_REQUIRED`), then re-checks its authenticator is active.
- `grant_tx` refuses a grant whose producing authenticator is not active
  (`STEP_UP_REQUIRED`).
- Elevation consumes the grant, writes the elevation row and its audit entry in one
  transaction (`create_elevated_session` now takes any executor); the old
  `record_elevation` helper (shared with the removed OIDC path) is gone. `ElevateRequest` is
  `{reason}` only; unknown fields (`password`, `mfa_code`) are ignored by serde.
- OIDC re-auth removal also removed `OidcFlowPurpose::Reauth`, `OidcReauthBinding`, the
  `prompt=login` path of `start_oidc_flow` and the `oidc_hand_back` helper (only re-auth used
  it). L1b's account-request "fresh OIDC" factor needs its own flow purpose.
- The mock OIDC provider of the deleted `reauth_oidc.rs` tests is kept as
  `tests/integration/oidc_mock.rs` (shared with `oidc_handoff.rs`), with the one login-flow
  test that did not depend on re-auth.
- `reauth.rs` tests that elevated by password were removed; the rate-limit and TOTP-replay
  tests now drive `/auth/mfa/backup-codes` (still on `verify_reauth` until L1b Task 16).
- Admin guide (`creating-accounts.md`) updated for elevation by step-up.
- Found while running the lib suite: `db::tests::test_cleanup_expired_sessions` had failed
  since Task 6 (30-day retention); fixed in this commit.

## Task 10 — approvals core, key devices, device step-up

- `KeyDeviceSession` is a function (`devices::key_device_session`) rather than an axum
  extractor: it needs the transaction the handler then uses, so the device row is read under
  the same lock. `pending`/`revoked` devices and non-device sessions → 403
  `KEY_DEVICE_REQUIRED`.
- A `device_stepup` answer must come from the session that asked for the challenge
  (`requester_id` = the caller's `sid`), approve only, with an empty response; otherwise 404 /
  400. The grant is bound to that same `sid` (the one signed in the context).
- "Voiding" a challenge (revoked device) sets `state = 'expired'`: the challenge state CHECK
  has no separate `void` value (B.2 lists `expired` as the terminal for open challenges).
- `lock_for_answer` marks a past-due challenge `expired` and the handler commits that mark
  before answering 410.
- Challenge `expires_at` is stored in whole seconds (it is signed as i64 Unix seconds).
- `target_label` (signed): device label for key-device targets, username for account targets,
  the parsed User-Agent for a session id, "all other sessions" for `others`, `""` otherwise.
- The device list returns every row of the account (pending, active and revoked), newest
  first. `ready_at` is null in L1a (it comes from the account request in L1b).
- Rate limits: `ApprovalLogin` (`approval_login`, 30/60 s per user) on `/api/approvals/*`;
  `AuthFlow` (`auth_flow`, 60/60 s per IP) added here for Tasks 11–12. Both count as auth
  categories (`is_auth`), i.e. they fail closed on a Valkey outage like the other auth limits.
- `queries.rs` files of the plan's file list were not split out; the queries live next to the
  functions that use them (small modules).

## Task 11 — enrolment core (SM2) and the attestation minimum

- **Status of a void/expired enrolment (client question 1):** `POST
  /auth/enrollments/status` answers **200** with `state` for every known enrolment whose claim
  secret matches, including `cancelled`, `expired` and `failed` (C26 status returns the
  state). **404 `ENROLLMENT_TOKEN_INVALID`** only for an unknown id or a wrong claim secret.
  `open`/`pop-challenge`/`claim` with a token of a cancelled or expired enrolment answer 404
  `ENROLLMENT_TOKEN_INVALID`; of a claimed or done one 409 `ENROLLMENT_ALREADY_CLAIMED`. A
  confirm after the code expired answers 409 `ENROLLMENT_STATE` (and marks it expired).
- The code hash is `keyed_hash(EnrollCode, enrollment_id ‖ code)`, so a code's hash is bound
  to its enrolment.
- Attestation fixtures are generated at test time with `rcgen` (dev-dependency) rather than
  stored under `server/tests/fixtures/attestation/`. The `KeyDescription` reader is a small
  bounded DER reader (x509-parser only parses the certificate and finds the extension). The
  security level used is the lower of `attestationSecurityLevel` and `keyMintSecurityLevel`;
  user authentication = no `noAuthRequired` and a `userAuthType`; the timeout is
  `authTimeout` (absent or 0 for `per_use`, 1..=10 for `window_10s`).
- An empty label after cleaning becomes "Android phone" (the column needs 1..64 characters).
- A device that re-enrols on the same install replaces its own earlier live row (revoked with
  `owner`) — the `(user_id, device_id)` live-row index allows one. The replaced row goes through
  the revocation cascade (its sessions end `authenticator_revoked`, grants and open challenges
  void; fixed after Task 13), without a notice of its own (the `authenticator_added` notice
  covers it).
- `≤ 5 open per issuer`: per key device / session / request / ticket; for the CLI per subject
  account.
- `OwnClaim` (the issuing client's own claim) answers `VALIDATION_ERROR` in L1a; L1b Tasks 19
  and 22 implement it. `Confirmer::Pc`/`Request`/`Ticket` answer 404 in L1a.
- The answer route commits a wrong-code try (`ENROLL_CODE_WRONG`, `ENROLLMENT_FAILED`) and
  the spent challenge even though it answers an error.

## Task 12 — device-key sign-in

- **429 code:** the existing limiter answered `RATE_LIMITED`; the contract ("Errors") and the
  plan's test want `TOO_MANY_ATTEMPTS` with `Retry-After`. Ruling: the v1.0 sign-in categories
  (`ApprovalLogin`, `AuthFlow`) answer `TOO_MANY_ATTEMPTS` (same body otherwise); every older
  category keeps `RATE_LIMITED` so today's clients are unchanged.
- **`auth_methods` in the response:** C26's route line says the `AuthResponse` carries
  `auth_methods: ["device_key"]`, but C27 defines `AuthResponse` without it (only `session_id`,
  `assurance`, `absolute_expires_at` were added). Followed C27: the session's `auth_methods`
  are on `GET /auth/me` → `session.auth_methods`. **Coordinator:** if clients want the field in
  the `AuthResponse`, it is an additive change for every sign-in.
- `sign_in` returns `DeviceSignIn {session, password_change_required}` instead of the bare
  `IssuedSession`, so the response carries the account's real `password_change_required` (as
  `/auth/login`).
- Lock order: account (`lock_accounts`), then the device row `FOR UPDATE`, then the challenge.
  Every revoking caller takes the account lock first, so a revocation either commits before
  (sign-in sees `revoked`) or after (and ends the minted session). The challenge itself is
  read once without a lock to learn the account.
- The device's older sessions are those with `authenticator = key_device(id)` and no
  `ended_at` (also already-expired rows); they end `superseded`.
- Unknown device and the system account → 404 `KEY_DEVICE_UNKNOWN`; only revoked rows → 403
  `KEY_DEVICE_REVOKED`; then ban → 403 `ACCOUNT_BANNED`; then `pending` → 409
  `KEY_DEVICE_PENDING {ready_at: null}` (`ready_at` comes from account requests, L1b). The
  ban is re-checked under the lock at sign-in.
- A wrong signature does not spend the challenge (rolled back, as the answer route).
- Each device-key sign-in records `sign_in {auth_methods ["device_key"], assurance "full"}`
  with the device's label and is delivered as a normal notice (B.4 "never silent").

## Task 13 — `kaiku-admin` host CLI

- `CliIo::read_line` is async (`fn read_line(..) -> impl Future + Send`) and `run` takes
  `&mut impl CliIo` instead of `&mut dyn CliIo`: the scripted test terminal must wait for the
  phone's claim on the same runtime; a blocking read would deadlock it. Stdin is read in
  `spawn_blocking`.
- Events: `enroll` records `authenticator_added` (actor `cli`, via `confirm_tx`) and `devices
  remove` records `authenticator_removed {type: key_device, by: cli}` (via `revoke_tx`), both with
  their `security.*` audit rows. Lists and `sessions end` write only their own audit row
  (`cli.devices.list`, `cli.sessions.list`, `cli.sessions.end`, plus `cli.enroll`,
  `cli.devices.remove`; actor = the system account): C28 has no event type for "sessions
  ended by the host" or for reads. **For L1b/coordinator:** add an event type if host-ended
  sessions should notify the account.
- After commit the CLI runs the same steps as the HTTP routes (`after_done`, `after_revoke`:
  notice delivery, WS hint, push clean-up after the last notice) on a server state it builds
  itself (no listener, no limiter). Undelivered notices stay in the outbox for the server's
  minute sweep (DC11).
- `sessions end <user>` needs an explicit `--all` (the plan's `None = all`) so a missing id
  never ends every session by accident.
- `devices remove` takes the key device id from `devices list` (the `approval_devices.id`), not
  the per-install id, and refuses a device of another account.
- The enrolment code prompt ignores spaces and re-asks (without spending a try) when the input
  is not 6 digits.
- The binary parses its arguments before reading any configuration, so a usage error exits 2
  without touching the database (tested on the built binary).

## Task 14 / Task 30 — gate results (2026-11-03)

- `cargo test --workspace --lib --bins --tests`: every unit suite green (vc-server lib 682,
  vc-client lib 248, vc-crypto 24, vp8 decoder 4); `--test integration` 1020 passed, 2 failed,
  33 ignored. The two (`role_channel_overrides::pending_member_ignores_everyone_override_allow`,
  `unread::test_viewable_filter_matches_require_channel_access`) are the known flaky
  pending-member family and pass on every rerun (3/3). The lib Valkey tests ran against a
  temporary Valkey on `127.0.0.1:6379`.
- Plain `cargo test --workspace` does not build: the client example
  `client/src-tauri/examples/capture_vp8_fixture.rs` fails against `vpx_encode` (untouched by
  this branch).
- `SQLX_OFFLINE=true cargo clippy -- -D warnings`: the server and shared crates are clean; the
  workspace run fails only in `vc-client` (`voice/video_decoder.rs`, `voice/connection_stats.rs`;
  untouched by this branch, local clippy 1.97).
- `cargo +nightly fmt --check` clean; `cargo deny check`: `advisories ok, bans ok, licenses ok,
  sources ok`; `.sqlx` refreshed (two admin user queries) and `cargo clean -p vc-server &&
  SQLX_OFFLINE=true cargo build -p vc-server` builds; offline `--lib error_code` (9) and
  `android_contract_fixtures` (17) pass with the committed goldens; `git diff --check` clean; the
  IP grep shows only OIDs and documentation ranges.
- The server image (`infra/docker/Dockerfile`) builds and contains `/usr/local/bin/kaiku-admin`
  (no arguments → usage, exit 2).
- Size: `git diff --stat origin/main...HEAD` → 202 files, ~19.6k insertions (≈17.3k in `.rs`),
  far above the ~7k threshold; the split point is the commit marked "L1a-1 boundary" (Task 7).
- Not done here (coordinator): the whole-branch Faramir review, the external reviews and the PR.

## Review round 1 (2026-11-03)

Reviews: Fable (APPROVE WITH FIXES, M1–M4, m1–m9), Codex (REQUEST CHANGES, 1–5 major, 6 minor),
Argon (REQUEST CHANGES, B1–B3, M1–M3, m1–m2). Every finding was checked against the code first;
every blocker and major fix, and the m5, m8, m9 and Argon m1 fixes, are test-first (the test
failed before the fix); m1, m4 and the m6 bounds have no dedicated test. The three lock-order tests reproduced
`40P01` deterministically before the fix (PostgreSQL log: `deadlock detected` in
`lock_full_session`, `DELETE FROM users` and `lock_accounts`). Commits: **(A)** `faefb63f` (migrations),
**(B)** `264baaf1` (admin guard, ban/delete locks), **(C)** `803c2e9e` (approvals, revocation, minors).

### Argon B1 decision: probation admin sessions on `/api/admin/*` → guard (fixed)

The spec forbids it. C27 "What each may do" lists `/api/admin/*` (with the elevation window where
it applies) in the **full** row, and its "unchanged" row reads B.4's "cannot" list as exhaustive
only for *every other route*; B.4's "cannot" list includes "elevate as admin". Fable's "no
privilege-escalation path" holds for creation (`POST /api/admin/users` was already behind
`require_elevated`, and elevation already needed `require_full`), but a probation admin session
(every pre-upgrade session, QR, password/OIDC + phone, TOTP) could read the user list with emails,
the CSV export, user details, the audit log, stats, guilds, diagnostics and observability, and an
elevation row from before the upgrade (Codex 5) opened the elevated routes to it. The plan had put
the guard in L1b Task 16; it moves to L1a: `require_system_admin` now also requires `full`
(403 `FULL_SESSION_REQUIRED`; non-admins still get `NOT_ADMIN`; `/api/admin/status` stays open to
any session). `elevate_session` checks the assurance before it parses the body. Tests:
`guarded_routes::{admin_routes_need_full, probation_elevation_opens_nothing}`. Still L1b Task 16:
bot and incoming-webhook token creation behind `full` (C27 same row), the own-account routes.

### Findings

| Finding | Reviewer(s) | Result |
|---|---|---|
| `first_activity_at` backfill leaves dormant OIDC-only accounts "new" | Fable M1, Argon B2 | Fixed (A): `20261120000100` edited in place: every non-system account existing at the upgrade gets `first_activity_at = created_at` (local accounts had a password, OIDC accounts are created at their first sign-in). Test `dormant_oidc_account_is_existing_after_the_upgrade` |
| Legacy elevations survive the assurance migration | Codex 5 | Fixed (A + B): the migration deletes every `elevated_sessions` row; the admin guard refuses probation anyway. Tests `legacy_elevations_end_at_the_upgrade`, `probation_elevation_opens_nothing` |
| `kaiku` rename can collide; owner not told | Codex 6, Fable m8 | Fixed (A): `20261120000000` edited in place: a DO block picks the first free `kaiku_` + 8/12/16/20/24 hex digits (fails with a clear message only if all are taken) and writes a `system.username_reserved` audit row (actor: the system account). The rename stays (coordinator item from Task 3 unchanged); admin guide "After the upgrade" tells admins to tell the owner. Test `pre_existing_kaiku_is_renamed_to_a_free_name` |
| Kaiku rename is case-sensitive | Argon M2.1 | Rejected: `username_format` is `^[a-z0-9_]{3,32}$`, so no `Kaiku`/`KAIKU` can exist |
| `last_used_at = created_at` expires active legacy sessions | Argon B3 | Rejected: inactivity is decided by `expires_at` (refresh: `row.expires_at <= now`; `session_check`: `s.expires_at > NOW()`), which the migration keeps; `last_used_at` only orders the session-limit LRU and is displayed. Already expired rows are not live for any check and the cleanup deletes them |
| Probation admins on `/api/admin/*` | Argon B1 | Fixed (B), see the decision above |
| Lock order answer/deny/release/issuer challenge/confirm vs revocation (40P01) | Fable M2, Codex 2 | Fixed (C): `lock_challenge` reads the challenge's approver and subject unlocked, locks both accounts in one statement, then `key_device_session` (account → device row `FOR UPDATE` → session row `FOR UPDATE`), then the challenge; `confirm_tx` reads the subject unlocked, locks subject (+ answering account) in one statement, then the enrolment. Tests `answer_and_revocation_never_deadlock`, `enroll_answer_and_revocation_never_deadlock` (both produced 40P01 before) |
| Ban/delete without the account lock; delete was sessions → users | Argon M3 | Fixed (B): ban, bulk ban, re-tell and delete lock the admin's and the subject's accounts first. Tests `admin_delete_and_a_transition_never_deadlock` (40P01 before), `ban_takes_the_account_lock` (a lapsed ban renewed in place takes no FK lock, so the ban ran past a held account lock before) |
| Challenge bound only to the client-chosen install id; `enroll` answer not bound to the issuer | Fable M3, Codex 1, Argon M1.2 | Fixed (C): `lock_for_answer(conn, id, approver_id, device_id)` filters in SQL; an `enroll` answer or deny must come from the enrolment's issuing device row (`lock_issued_by`), checked again in `confirm_tx`; `Confirmer::KeyDeviceAnswer` carries the locked `KeyDeviceSession`. Tests `same_install_id_of_another_account_cannot_answer` (it enrolled a key before) and `…_cannot_deny` (it cancelled before) |
| Stale request session in `key_device_session` | Codex 3 | Fixed (C): the session row is re-read under the account lock (`FOR UPDATE`, live, full, bound to that device) → 401 `INVALID_TOKEN` if it ended; the account's ban is re-checked. Test `ended_session_cannot_act_after_waiting_for_the_lock` (`/invalidated` revoked the key after a concurrent logout before) |
| Re-enrolment on the same install silently revokes a key | Fable M4 | Fixed in part (C): each replaced row records `authenticator_removed {type: key_device, by}` (high; `by` from the confirmer: `cli`, `self`, `admin`, `request`) and is delivered with the `authenticator_added` notice. Push clean-up rejected: push subscriptions are keyed by (account, install id), so deleting them would delete the new key's own subscription on the same install. Refusing the claim when a live row has that install id is **not** done (coordinator): it changes the claim interface for L2 and would not stop the case where the victim's own phone issued the enrolment. Test `re_enrolment_records_the_removed_key` |
| `NOTIFY` payload unbounded (~200 ids max): large revocations roll back | Codex 4 | Fixed (C): `after_update` sends ≤ 100 ids per `pg_notify` (`NOTIFY_CHUNK`), `all` on the first chunk. Test `ending_a_thousand_sessions_commits` (failed with the payload error before) |
| Cascades do not void open challenges | Argon M1.1 | Rejected in substance, consolidated (C): `revoke_tx` and the re-enrol branch already voided the device's challenges (`void_for_device`); this moved into `on_authenticator_revoked` so every key-device revocation gets it. `on_sessions_ended` deliberately leaves challenges alone: a `device_stepup` challenge is answerable only from its requesting session, re-read live under the lock, and `enroll`/`enroll_pop` effects re-check the enrolment state under the lock; voiding them there would add challenge rows to the lock set of logout and refresh, which do not take the account lock, and recreate a cycle with the `enroll` answer |
| Expired mark rolled back by `?` | Argon M1.3 | Answer and deny already committed it (notes, Task 10); fixed (C) for `claim` and `release` |
| `bulk_ban_users` can ban the system account | Argon M2.2 | Rejected: the loop calls `queries::user_exists`, which is `… AND NOT is_system` (`admin/queries.rs`); unknown → "User not found" |
| Sub-second TTL issues a past-due challenge | Argon m1 | Fixed (C): `issue` answers 410 `APPROVAL_EXPIRED` when the floored `expires_at` is not after now. Test `sub_second_lifetime_never_issues_a_past_due_challenge` |
| `chmod 0750 kaiku-admin` | Argon m2 | Rejected: the guide runs it as `docker compose exec server kaiku-admin` (the image's service user); anyone who can exec into the container already has the database credentials in its environment, so a root-only mode adds nothing and breaks the documented use |
| `target_label` reads other accounts' rows | Fable m1 | Fixed (C) for `sessions.end` and `key_device.remove` (`AND user_id = $2`); admin and recovery-contact targets name other accounts by design. **LP:** scope the passkey label of `passkey.remove` to the caller when adding its arm |
| Legacy refresh tokens recognised only from the second rotation | Fable m2 | Documented (A): admin guide "After the upgrade to v1.0" |
| Dead legacy token helpers in `jwt.rs` | Fable m3 | Deferred: the integration tests mint their tokens with `generate_token_pair_for_session`; moving them under `#[cfg(test)]` needs a test-support feature (L1b clean-up) |
| `Debug` on secret-bearing types | Fable m4 | Fixed (C): redacting `Debug` for `AnswerRequest`, `EnrollmentIssued`, `CliEnrollment` |
| No clean-up of terminal challenges and enrolments | Fable m5 | Fixed (C): `db::cleanup_finished_approvals` in the hourly job (finished challenges and ended enrolments after 30 days; grants go with their sessions). Test `finished_challenges_and_enrolments_are_cleaned_up` |
| Attestation chain decoded before it is bounded | Fable m6 | Fixed in part (C): ≤ 8 entries of ≤ 8 KiB DER (b64u length) checked before decoding → 400 `VALIDATION_ERROR`. Deferred: comparing the leaf SPKI with the claimed key (the `rcgen` fixtures certify another key; needs fixtures that certify the phone key, L1b with the L2 attestation fixtures) |
| `grant_tx` trusts its caller for `by`'s owner | Fable m7 | Deferred to **LP**: L1a's only caller passes the session's own device (now locked and re-checked in `key_device_session`); LP adds the passkey arm and should add an owner check for `by` in `grant_tx` with it (no L1a interface change now) |
| `approved` past due answered 409 | Fable m9 | Fixed (C): open (`issued`/`approved`) and past due → expired, 410. Test `approved_past_due_is_expired` |

### Interface changes (LP, L1b, L2)

- Wire: `/api/admin/*` (except `/status`) answers 403 `FULL_SESSION_REQUIRED` to probation
  sessions (C27 as frozen; clients show the admin area only on full sessions). Re-enrolment adds an
  `authenticator_removed` notice. An attestation chain above the bounds → 400 `VALIDATION_ERROR`.
  An `answer`/`deny` whose session ended while it waited → 401 `INVALID_TOKEN`.
- Rust: `approvals::core::lock_for_answer(conn, id, approver_id, device_id)` (new argument),
  `core::head`, `devices::require_key_device_snapshot`, `enrollments::lock_issued_by`;
  `Confirmer::KeyDeviceAnswer(&KeyDeviceSession)` (was `&ChallengeRow`); `key_device_session`
  now takes the caller's account lock itself and may answer `Auth(InvalidToken)`;
  `cascade::on_authenticator_revoked` also voids a key device's open challenges (callers no longer
  call `void_for_device`); `revocation::NOTIFY_CHUNK`; `db::cleanup_finished_approvals`. LP's
  listed interfaces (`issue_session*`, `step_up::*`, `guards::*`, `strong::*`,
  `on_authenticator_revoked`, `security_events::record`) keep their signatures.
- Migrations `20261120000000` and `20261120000100` were edited in place (timestamps kept): a
  database that already ran the branch's migrations (e.g. a local LP test database) must be
  recreated; `#[sqlx::test]` databases are fresh.

### Gate results (round 1, 2026-11-03)

- `cargo test --workspace --lib --bins --tests` (test DB `:55433`, integration Valkey `:56379`,
  lib Valkey tests against a temporary Valkey on `127.0.0.1:6379`): vc-server lib 682, vc-client
  lib 248 (2 ignored), vc-crypto 24, vp8 decoder 4 + 1; `--test integration` 1037 passed, 2
  failed, 33 ignored. The two (`screening_http::pending_member_is_blocked_then_unlocked_on_accept`,
  `unread::test_viewable_filter_matches_require_channel_access`) are the known flaky
  pending-member family: 3/3 green on rerun, and the integration-only run of the same code before
  had 1038 passed, 0 failed (the clean-up test was added after it).
- `cargo +nightly fmt --check` clean; `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`
  clean; `cargo deny check`: advisories, bans, licenses, sources ok.
- `.sqlx`: no `query!` macro changed, so no cache file was added or removed; `cargo sqlx prepare`
  rewrites 38 existing files only with the `origin` metadata of the local sqlx-cli (no semantic
  difference, checked by a JSON comparison without `origin`), so the cache is left as committed.
  `git diff --check` clean.

### Review round 2 (confirmation, 2026-11-03)

- Codex m6 (PARTLY in round 2): the `kaiku` rename no longer aborts after five taken hex candidates; it falls back to `kaiku_` + 20 hex + `_` + counter within `username_format`. Test `pre_existing_kaiku_rename_survives_all_hex_candidates_taken` (red before, green after).
- Argon: APPROVE (all three rejections accepted).

## Review round 3 (Argon coverage, 2026-11-03)

Argon's first full review had seen only the first ~190 KB of the diff (58 of 150 files); it
reviewed the 92 missed files in six parts against `267e4c78`. Argon saw only a diff, so every
finding was checked against the code first; each "real" fix is test-first (the test failed before
the fix) unless the row says otherwise. Every claimed deadlock was tried with a concurrent test:
four reproduced `40P01` (or the 500 it causes) before the fix (P2 M1, P4 M1, P5 M2, P6 M1), the
refresh pool self-deadlock (P3 M1) reproduced as `PoolTimedOut`, and P1 M3 deadlocked only with
the existing up-front lock removed, which shows the code already prevents it.

**Blocker verdict (part 1 B1): real, fixed.** `confirm_tx` revoked any live row with the subject's
`(user_id, device_id)`, and the install id is the claimant's own choice (listed to every session of
the account). In L1a only the CLI reaches `confirm_tx` in production (the issuer routes are L1b),
so the live effect was the wrong `revoke_reason`; L1b's `admin` and `request` routes would have
inherited the immediate removal of another account's key.

| Finding | Verdict | Commit | Test or reason |
|---|---|---|---|
| P1 B1 `confirm_tx` replaces a live key for `pending` enrolments, its own issuer, reason always `owner` | real | `b8814823` | `pending_admin_enrolment_never_replaces_a_live_key`, `…_another_admins_key`, `pending_request_enrolment_never_replaces_a_live_key`, `own_enrolment_cannot_replace_its_issuer`, `replaced_key_carries_the_variants_reason`. A `pending` key on a live install and the issuer's own install answer 400 `VALIDATION_ERROR` at `pop-challenge` and again in `confirm_tx` under the lock; replacement only for an `active` key, reason by variant |
| P1 M2 attestation: `userAuthType` from `softwareEnforced` or 0, unreadable `authTimeout` read as absent | real | `003dfbd4` | unit tests `attestation::tests::*` (4 red before) |
| P1 M2 leaf SPKI not compared with the claimed key | deferred | – | same as round 1 Fable m6 (fixtures certify another key; L1b with the L2 fixtures); no new path |
| P1 M3 lock order in `answer_enroll_tx`/`confirm_tx` for cross-account enrolments | not real | `7256fdaa` (test) | `approvals/handlers.rs` `lock_challenge` locks `[auth.id, subject_id]` in one statement before `key_device_session`; later `lock_accounts` calls re-lock held rows. `admin_enroll_answer_never_deadlocks_{subject,admin}_lower`; with the up-front lock removed the lower-subject case deadlocks |
| P1 M4 no `enrollment_updated` for PC-session issuers | real | `cddbed4c` | `pc_session_issuer_gets_the_claimed_hint` (`notify_issuer` falls back to the issuing session's account) |
| P1 m5 `after_revoke` deletes the push subscription even if the last notice failed | deferred | – | keeping it would keep pushing to a removed phone until something deletes it; the correct fix is an outbox-driven clean-up after the sweep delivers the notice (L1b) |
| P1 m6 open count uses `expires_at` for claimed rows; `status` of an overdue claim | real | `fbd042b1` | `claimed_enrolment_is_open_while_its_code_is` |
| P1 m7 `invalidated` answers `is_current: false` | real | `4bb42264` | `invalidated_device_is_revoked_with_notice` (new assertion) |
| P1 m7 `cancel_tx` ignores `_by`, leaves open challenges | not real | – | callers authorise first (`handlers.rs` `deny` → `lock_issued_by`); `enroll`/`enroll_pop` effects re-check the enrolment state under the lock (round 1 decision on challenges) |
| P2 M1 `lock_scope` retry takes a lower account after a higher one | real (dormant until L1b Task 21) | `c66f7ae5` | `lock_scope_retry_never_takes_a_lower_account_after_a_higher_one` (40P01 before) via the new `lock_scope_with` reader seam; a savepoint per attempt |
| P2 M2 cross-account labels in `step_up`/`target_label` | fixed in LP `6c8a4444` | – | the `admin.*` gate runs before `key_device_session` there |
| P2 M2 passkey fallback for `admin.authenticator.remove`; `passkey.remove` unscoped | LP | – | real gap (C27: the target is "key device or passkey id"); LP adds the passkey arms, scoped to the caller for `passkey.remove` (round 1 Fable m1) |
| P2 m3 `identity.unlink` has no `target_label` | deferred | – | L1b Task 16 puts the route behind the step-up; add the arm there (avoids a conflict with LP in `target_label`) |
| P2 m4 logout leaves the legacy cookie | real | `aaccc72c` | `logout_clears_the_legacy_cookie` |
| P3 M1 `rotate` takes a second pool connection under its row lock; Valkey under the lock | real | `b34b3561` | `rotation_needs_only_its_own_connection` (pool of one: `PoolTimedOut` before). The ban check runs in the transaction; a `sid` token's denylist is read before the lock, its verdict still after the ban (pinned order) |
| P3 M2 two Valkey round-trips per ended session inside the transaction | real | `7c1bd642` | `ending_many_sessions_waits_for_a_hung_valkey_once` (48 s for 8 sessions before; one pipeline now) |
| P3 m1 `reauth/methods` `device` from `active_strong_count` | real (latent for LP) | `62143ccf` | no test: identical in L1a; becomes wrong once LP counts passkeys |
| P3 m2 `is_live`/`is_live_for_user` skip bans and `is_system` | not real | – | the WS handshake and re-check also run `account_status` (`ws/handlers.rs:174`, `:432`, `:671`); a ban ends the session rows in its transaction; the system account has no sessions |
| P3 m3 expiry checked before reuse detection | not real | – | the documented order (Task 7); a lapsed row authenticates nothing, so a presented predecessor gains nothing |
| P4 M1 grant operations without the account lock | real | `9b41b012` | `elevation_and_revocation_never_deadlock` (40P01 before: elevation held the session row and waited for the key-share lock its `elevated_sessions`/audit rows take on `users`). `lock_full_session` locks the account first. The `is_active` snapshot read is serialisable (first commit wins) and the account lock now orders it anyway |
| P4 M1 `grant_tx` does not check `by`'s owner or kind | deferred to LP | – | round 1 Fable m7 |
| P4 M2 `/auth/sessions` `authenticator.label` always null | real | `08716983` | `session_list_names_the_key_device` (LP adds passkey labels) |
| P4 m1 pin/unpin reveal another user's Kaiku DM | real | `101c4e19` | `pin_never_reveals_another_users_kaiku_dm` |
| P4 m2 CLI builds its state after the commit | real | `a686cd10` | no test (the voice setup cannot be made to fail from the CLI suite) |
| P4 m2 CLI exits before spawned deliveries; no audit row at confirm | not real | – | the CLI awaits `notices::deliver` (no spawn); `record` writes `security.authenticator_added` in the same transaction (`security_events/record.rs:54`) |
| P4 m3 session-ending routes without the account lock | not real | – | by design (approvals AGENTS.md): they touch sessions, grants and enrolments only; a session committed after the end-all statement is a later sign-in |
| P4 m3 `is_current` true for two sessions | real | `08716983` | `session_list_marks_only_the_tokens_session_current` |
| P4 m4 sweep skips enrolment expiry when challenge expiry fails | real | `ab65c651` | no test (a failing step 2 cannot be produced without a fault hook) |
| P5 M1 concurrent deliveries race past coalescing | not real | `9b7ce5d4` (test) | `kaiku_dm_created_tx` takes the recipient's advisory lock before the notice lookup (`notices.rs` `deliver_one`); `concurrent_deliveries_coalesce` |
| P5 M2 deadlock between two admins' high-severity deliveries | real | `9b7ce5d4` | `admin_deliveries_never_deadlock` (40P01 before; recipients sorted by id) |
| P5 M3 coalescing window has no upper bound | real | `9b7ce5d4` | `coalescing_window_is_bounded_both_ways` |
| P5 M4 one failing event blocks the outbox | real | `9b7ce5d4` | `one_failing_event_never_blocks_the_outbox` (test-only trigger as the failing event) |
| P5 m1 system account as notice subject | real | `5ad67620` | `system_account_event_never_reaches_another_users_dm` (with P6 M1) |
| P5 m2 deletion maps end-all errors to 500 with the error text | not real | – | `GovernanceError::Internal` renders only "Internal server error" (`governance/error.rs:72-82`); a Valkey outage never fails `end_all_sessions_tx` (best effort) |
| P5 m3 unbounded event fetch and N+1 labels per notice | real (performance) | `2397f951` | covered by `coalesced_events_truncate_at_50` and the notice tests |
| P6 M1 system-account self-lookup returns another user's Kaiku DM | real | `5ad67620` | the same test: the event was posted into the user's DM before |
| P6 M1 multi-recipient advisory lock order | real | `9b7ce5d4` | as P5 M2 |
| P6 M1 FK key-share vs `FOR UPDATE` on `users` | partly | `c6d2e493` | Argon's scenario (notices created inside a transition) does not exist: deliveries run in their own transaction after the commit. A related cycle did: admin deletion (account lock, then cascade into the account's events) vs a delivery holding that event and waiting for the account; `notice_delivery_and_account_deletion_never_deadlock` (deletion 500 before). `deliver_one` now key-share-locks the event's accounts in id order before the event row |
| P6 m1 Typing read-only check without a timeout | not real | – | the same handler's subscribe checks (`find_channel_by_id`, `require_channel_access`) are equally unbounded; the pool's 5 s acquire timeout applies to all; bounding `handle_client_message` as a whole is out of scope |
| P6 m2 `DmCreated` doc comment detached | real | `7d615893` | – |

Also in this round (coordinator, CI only): `d566b740` raises the "Run tests" step of the Rust
Tests job to 40 minutes (no job-level timeout exists).

### Interface changes (LP, L1b, L2)

- Wire: `POST /auth/enrollments/pop-challenge` (and the confirm) answers 400 `VALIDATION_ERROR`
  when the install id is the issuer's own key device, or a live key of the account while the new
  key would be `pending` (`admin` of an existing account, `request`). L2: a phone whose stale row
  is still live sets up again with a new install id, or the row is removed first. L1b: replacing
  a live install at the request's completion needs `approval_devices_live` to allow a `pending`
  row beside the `active` one (and device-key sign-in to pick the active row).
- Wire: `GET /auth/sessions` fills `authenticator.label` (key devices); `is_current` follows the
  token's `sid`. `POST /api/approval-devices/{id}/invalidated` answers `is_current: true`. Pin and
  unpin in someone else's Kaiku DM answer like any other DM. Enrolment `status` reads `expired`
  for a code past due. Logout clears the legacy refresh cookie.
- Attestation: `userAuthType` must be hardware-enforced and non-zero; an unreadable
  `authTimeout` → `DEVICE_UNSUITABLE {reason: "attestation_missing"}`.
- Rust: `transition::lock_scope_with` (reader seam); `step_up::{grant_tx, peek_tx, consume_tx}`
  take the caller's account lock (a no-op for callers that hold it); `enrollments::notify_issuer`;
  `queries::events_of_notices(conn, ids, per_notice)`; `revocation::revoke_session` removed;
  `system_account::kaiku_dm_created_tx` refuses the system account; `notices::deliver` returns
  the first error after trying every event. L1b Task 21: `accounts_recording` plugs into
  `lock_scope`'s savepoint retry as is.

### Gate results (round 3, 2026-11-03)

- `cargo test -p vc-server --lib`: 688 passed (the Valkey lib tests hard-code `localhost:6379`;
  they ran through a local forwarder to the L1a test Valkey).
- Approvals, enrolment, device sign-in, step-up, transition locks, session (assurance, end,
  revocation, refresh), Valkey outage, security events and notices, system account, CLI,
  elevation, guarded routes and re-auth suites: 210 passed, 0 failed.
- Full `--test integration`: 1061 passed, 2 failed, 33 ignored. The two
  (`role_channel_overrides::pending_member_ignores_everyone_override_allow`,
  `unread::test_viewable_filter_matches_require_channel_access`) are the known flaky
  pending-member family (rounds 1 and 2) and pass 3/3 with `--test-threads=1`.
- `cargo +nightly fmt --all -- --check` clean; `SQLX_OFFLINE=true cargo clippy -p vc-server --
  -D warnings` clean; `cargo deny check`: advisories, bans, licenses, sources ok; `git diff
  --check` clean. No `query!` macro changed, so `.sqlx` is unchanged.

## Review round 4 (2026-11-03)

Round 3 confirmations: Fable APPROVE, Gemini Flash (for Argon) APPROVE, Codex REQUEST CHANGES (the
`admin.*` gate and the leaf SPKI). Every fix is test-first (the test failed before the fix).

| Item | Verdict | Commit | Test or note |
|---|---|---|---|
| `admin.*` step-up gate before `key_device_session` and `target_label` (Codex r3 P2 M2: L1a merges first, so LP's fix must be here) | real | `23450390` | Ported from LP `6c8a4444` + `e03bdb27`, L1a files only and textually identical (`git apply` of LP's hunks): `StepUpAction::is_admin`, `guards::require_admin_for_step_up` (403 `NOT_ADMIN`, then `FULL_SESSION_REQUIRED`), `AuthError::NotAdmin`, the gate in `step_up`, unit test `every_admin_action_is_gated`, admin `admin.elevate` fixtures in `approvals_core`. `device_admin_step_up_needs_an_admin` lives in `approvals_core` here (its LP file is passkey-only; the body is LP's with the L1a helpers). Beyond LP: `AuthError::NotAdmin` in `error_code_tests` (AGENTS.md rule). Passkey files untouched |
| Leaf SPKI bound to the claimed key (Codex r3 P1 M2, round 1 Fable m6) | real | `6bbaefad` | `attestation_leaf_must_certify_the_claimed_key`. `check` takes the claimed SPKI DER; a different leaf SPKI → 400 `DEVICE_UNSUITABLE` `attestation_properties` (after the extension check, so a leaf without it stays `attestation_missing`). Fixtures: `chain_for` signs the leaf with the phone's own key; `chain` keeps a key of its own for the mismatch case |
| Dedicated code for a pending claim on a live install id (Fable) | done | `b573c898` | 400 `ENROLLMENT_DEVICE_IN_USE` (no fields) at pop-challenge and confirm; the three `pending_*_never_replaces_*` tests assert it. The issuer's own install keeps 400 `VALIDATION_ERROR` (an app bug: the issuing app never claims its own link). Coordinator amends C26 (additive) |
| Deletion: a DB or Valkey failure ending the sessions answers 500 (Codex r3 P5 m2) | real | `edbae8d3` | `deletion_answers_503_when_its_sessions_cannot_end` (test-only trigger); begin, end and commit map to the new `GovernanceError::ServiceUnavailable` (503 `SERVICE_UNAVAILABLE`), logged, never echoed |
| Persistently failing events monopolise the bounded sweep (Codex r3 P5 M4, optional) | real, small | `7052e7f3` | `failing_events_never_take_the_whole_sweep`. Half the batch from the oldest end, half from the newest (no per-event attempt count: `security_events` is append-only, a counter needs a migration and a trigger change); delivery stays oldest first |
| `refresh.rs` `reuse` holds the session row, then `record()` takes KEY SHARE on `users` (Fable) | closed by L1b | – | A concurrent revocation of the same account (account `FOR UPDATE`, then the session row) can deadlock with it (40P01). L1b's `lock_accounts` with `FOR NO KEY UPDATE` does not conflict with KEY SHARE and closes it. No code here |

Corrections and notes on the round 3 answers:

- P6 m1 (Typing read-only check): Codex rejects the "pool acquire timeout" argument (it does not
  bound an executing query) and accepts the deferral as existing WebSocket scope. Corrected: the
  deferral stands because the same handler's other database calls are equally unbounded.
- P3 M1: a legacy token's one-time denylist read still runs under the session row lock (Codex
  PARTLY). Accepted residual: once per legacy token, bounded by the 3 s Valkey command timeout.
- Flash (round 4 part A): the recovery case on the same install now answers
  `ENROLLMENT_DEVICE_IN_USE`, so the phone offers to sign in with its key; a phone whose key is
  gone sets up with a new install id (L2), or L1b lets a `pending` row sit beside the live one.
  For L1b cross-account step-up actions: lock `[auth.id, target]` together before `consume_tx`
  (its own account lock is then a no-op), never the target alone first. The CI step timeout is
  the coordinator's call (round 3).

### Interface changes (round 4)

- Wire: `POST /api/approvals/step-up` with an `admin.*` action answers 403 `NOT_ADMIN` to a
  non-admin (before `KEY_DEVICE_REQUIRED`), then `FULL_SESSION_REQUIRED`; pop-challenge and
  confirm answer 400 `ENROLLMENT_DEVICE_IN_USE` for a pending claim on a live install id; an
  attestation leaf for another key → `DEVICE_UNSUITABLE` `attestation_properties`;
  `POST /api/me/delete-account` (and its retry) answers 503 `SERVICE_UNAVAILABLE` when the
  sessions cannot be ended.
- Rust: `attestation::check(chain, nonce, uv_class, public_key)`;
  `ApprovalError::EnrollmentDeviceInUse`; `GovernanceError::ServiceUnavailable`;
  `notices::{SWEEP_BATCH, deliver_pending_bounded}`; the LP gate items above (same signatures as
  LP).

### Gate results (round 4, 2026-11-03)

- `cargo test -p vc-server --lib`: 689 passed (Valkey lib tests through the local forwarder, as
  in round 3).
- Approvals, approval devices, enrolment, device sign-in, step-up, transition locks, admin (create
  user, diagnostics, elevation, reports), guarded routes, CLI, governance, security events and
  notices: 180 passed, 0 failed.
- Full `--test integration`: 1064 passed, 3 failed, 33 ignored. The three
  (`role_channel_overrides::pending_member_ignores_everyone_override_allow`,
  `screening_http::pending_member_is_blocked_then_unlocked_on_accept`,
  `unread::test_viewable_filter_matches_require_channel_access`) are the known flaky
  pending-member family and pass 3/3 with `--test-threads=1`.
- `cargo +nightly fmt --all -- --check` clean; `SQLX_OFFLINE=true cargo clippy -p vc-server --
  -D warnings` clean; `cargo deny check`: advisories, bans, licenses, sources ok. No `query!`
  macro changed (`.sqlx` unchanged).

## Review round 5 (Codex r4 on 690df404..b2391e08; 2026-11-03)

Codex: gate, SPKI and 503 FIXED; two PARTLY, fixed by the coordinator. Gemini Flash (Argon stand-in): APPROVE.

| Finding | Verdict | Commit | Test |
|---|---|---|---|
| Own-issuer collision still `VALIDATION_ERROR`; the C26 amendment (09fa5da1) names `ENROLLMENT_DEVICE_IN_USE` for it too | fixed | see `git log` ("issuer's own phone") | `enrollments::own_enrolment_cannot_replace_its_issuer` (red before) |
| Notice sweep: failing events at both ends starve a middle one forever | fixed: the second half continues from a rotating cursor (`system_settings` key `security.notice_sweep_cursor`) and wraps around, so every leftover is reached within `ceil(undelivered / half)` sweeps; both halves are index range scans (a first random-sample version was refused by Codex r5: unbounded scan, no fairness bound) | see `git log` ("rotating cursor") | `security_notices::failing_events_at_both_ends_never_starve_the_middle` (red before; deterministic, 5 sweeps); the round-4 test loops 4 sweeps |

Gates: `enrollments::`, `security_notices::`, `approvals*` integration 72 passed; after the cursor change `security_notices::` + `enrollments::` 53 passed; fmt and `clippy -p vc-server -D warnings` clean.

## Review round 6 (Codex r5 on the rotating cursor; 2026-11-03)

| Finding | Verdict | Commit | Test |
|---|---|---|---|
| Concurrent sweeps (several processes) can write a stale cursor back | fixed: the rotating half runs in a transaction holding `pg_try_advisory_xact_lock(hashtext('security.notice_sweep_cursor'))`; a sweep without the lock delivers only its oldest half | see `git log` ("advisory lock") | `security_notices::a_concurrent_sweep_never_moves_the_cursor` (red with the lock bypassed: checked) |
| The nullable `OR` and the `(created_at, id)` comparison do not match the `created_at`-only partial index | fixed: separate queries with and without a cursor; `idx_security_events_undelivered` is now `(created_at, id) WHERE delivered_at IS NULL` (the migration is new in L1a and unreleased, so it is changed in place) | same | covered by the sweep tests |

Gates: `security_notices::` + `enrollments::` 54 passed; fmt and `clippy -p vc-server -D warnings` clean (`--tests` lints in other files are pre-existing).
