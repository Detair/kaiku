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
