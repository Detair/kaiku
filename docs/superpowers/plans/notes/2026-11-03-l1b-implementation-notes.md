# L1b implementation notes (v1.0 login, server flows)

Plan: `docs/superpowers/plans/2026-11-03-v1.0-login-server.md` (Tasks 15–29, Task 30 gates).
Branch `feat/v1.0-login-l1b`, stacked on LP (`feat/v1.0-login-lp`, draft PR #744) at `6fd3ab54`,
which is stacked on L1a (draft PR #743). Spec rev 5 and contracts C26–C29 win over the plan; every
deviation and judgment call is listed here, per task. "Coordinator" marks items that need a
decision. "For L2" marks interface facts the clients need.

## Task 15 — credential states, generations, blocklist, recovery codes

- `factors::mark_compromised_tx(conn, ctx, user_id, used, cause, actor)`: an extra `actor`
  (the plan has none), so the `*_compromised` event names who caused it.
- `recovery_codes::replace_tx(conn, ctx, user_id)` takes `ctx`: replacing the set starts a new
  generation and runs `cascade::on_generation_ended` (B.7 rule 2).
- `factors::clear_throttle_tx` returns whether the state changed and applies the whole B.1 exit
  table (`factors::exits`): `compromised` leaves only by replacement/removal, a completed request
  or the CLI; an admin clears `throttled` only.
- `StateCause::Cli` sends `cause: "cli"`, appended to C28's list.
- Legacy backup codes stop working now: `/auth/login` and `/auth/mfa/verify` no longer accept them
  (F8); `/auth/mfa/backup-codes[/count]` answer 404. The `mfa_backup_codes` table stays until the
  upgrade migration (Task 25) drops it. `reauth::verify_reauth` had only the backup-code route as
  caller and is deleted here (the plan deletes it in Task 16).
- Blocklist: SecLists renamed the planned file; the bundled list is
  `Passwords/Common-Credentials/xato-net-10-million-passwords-100000.txt` (MIT, commit
  `c205c36a`), stored as `server/data/password-blocklist-top100k.txt.gz` and embedded with
  `include_bytes!` (new direct dependency `flate2`, already in the tree). Context words: `kaiku`
  and the username, display name and server name, whole, without spaces and per word, ≥ 4
  characters, case-insensitive substring.
- `PASSWORD_REJECTED` is a 400 with `reason` (`length`, `breached`, `context`).
- Recovery codes: 13 characters from the RFC 4648 alphabet (5 bits of an OS-random byte each),
  shown `ABCDE-FGHIJ-KLM`; a malformed input costs one dummy Argon2 verify.

## Task 16 — own-account routes behind the guards

- Every guarded route takes `lock_accounts([caller])`, consumes its grant (`step_up::consume_tx`)
  and records `step_up_action {action, target_label}` (`step_up::record_action_tx`) in one
  transaction; labels come from `approvals::handlers::target_label` (now `pub(crate)`, plus an
  `identity.unlink` arm = the provider slug).
- `DELETE /auth/me/password` answers 409 `LAST_STRONG_AUTHENTICATOR` before the step-up when the
  account has no active strong authenticator (after the step-up it could never trigger, because
  `consume_tx` re-checks the grant's authenticator).
- `/auth/mfa/verify` only confirms the pending secret of `/auth/mfa/setup` (no pending secret → 400
  `VALIDATION_ERROR`); the first verify no longer returns backup codes. `/auth/mfa/disable`
  ignores a code in the body (L2a sends one).
- Identity unlink: a passwordless account with an active strong authenticator may drop its last
  OIDC identity.
- Account deletion: `password` in the body is ignored; the duplicate and guild-ownership checks
  answer before the step-up.
- **Interim (coordinator):** an admin-created account with a temporary password signs in with
  probation and can no longer change the password itself (403 `FULL_SESSION_REQUIRED`); Task 23
  drops temporary passwords and Task 25 clears `must_change_password` (no RC is cut between).
- Long-lived tokens: `require_full_session` on `POST /api/applications/{id}/bot`,
  `/reset-token` and `POST /api/channels/{id}/webhooks` (handler-level layer).
- **L1a review item (WebSocket):** `admin_subscribe` re-reads the socket's session row (live,
  full, a system admin) before the elevation check, so a cached `admin:elevated:{sid}` of a
  probation session subscribes nothing. Test `guarded_routes::probation_admin_cannot_subscribe_to_admin_events`.
- **L1a review m3 (jwt clean-up):** `generate_token_pair`, `generate_token_pair_for_session`,
  `build_token_pair` and `validate_refresh_token` are `#[cfg(test)]` in `auth::jwt`; integration
  tests mint their legacy-format tokens in `tests/integration/helpers/tokens.rs` from the public
  `Claims`. Test helpers `grant_step_up(pool, sid, action, target)` and `make_session_full` serve
  existing tests of guarded routes.

## Task 17 — login attempts (SM1), QR sign-in (F2), the `login` approval

- Module `auth::login_attempts::{core, handlers, types}` (no separate `queries.rs`); routes
  `POST /auth/login-attempts`, `/{id}/poll`, `/{id}/collect`, `/{id}/cancel` (`AuthFlow` per IP,
  never IP-blocked) and `POST /api/approvals/claim` (`ApprovalLogin` per user).
- **Lock order:** device-side transitions (claim, answer, deny, release) lock accounts → device →
  session → challenge → attempt; collect (and Task 18's TOTP step) lock the attempt's account →
  attempt, retrying (≤ 3, then 503) if the attempt's account changed while unlocked; poll, cancel
  and the sweep update only attempt rows and never a challenge. An answer, deny or release whose
  attempt ended or moved on voids its own challenge and answers 409 `APPROVAL_STATE`.
- A claim whose challenge expired or was released returns the attempt to `open` lazily (next
  poll, claim or sweep); a QR attempt then forgets its claimer. A token used by a claim is
  rotated by the next poll.
- The claim challenge lives `min(2 min, attempt deadline)`; at approve the challenge is
  `consumed` and the attempt `approved` (collect within 60 s while the approving device is active
  and, for password attempts, the password generation unchanged; otherwise the attempt becomes
  `expired` and collect answers 410 `LOGIN_ATTEMPT_ENDED {state: "expired"}`).
- Extra columns: `collection_digest` (the signed `collection_hash`: the S256 challenge, or
  SHA-256 of the cookie secret; the cookie itself is stored only as a keyed hash),
  `session_id` (the collected session, for repudiation), `unfinished_recorded` (Task 18).
- `client_label` from the User-Agent: desktop "Kaiku desktop on <OS>", Android "Kaiku on <OS>",
  web "<Browser> on <OS>", cleaned (controls, bidi overrides) and ≤ 64 characters.
- New codes: `LOGIN_CODE_STALE` 404, `LOGIN_ATTEMPT_CLAIMED` 409, `LOGIN_ATTEMPT_OTHER_ACCOUNT` 409,
  `APPROVAL_RESPONSE_MISMATCH` 400 (approvals); `LOGIN_ATTEMPT_UNKNOWN` 404,
  `LOGIN_ATTEMPT_NOT_READY` 409, `LOGIN_ATTEMPT_ENDED {state}` 410 (auth).
- **For L2:** poll answers 200 with the attempt in every state (also terminal); `qr` only when a
  token was minted in that response. Golden files `login_attempt.json`,
  `approval_challenge_login.json`.
- The `sign_in` event of a QR sign-in carries the PC's class/label/IP and session; a QR sign-in
  ends a password throttle (B.1 exit "another method").

## Task 18 — password and OIDC sign-in with phone confirmation (F3, F4)

- `POST /auth/login {username, password, client_challenge?}` → 202 `LoginStep` or 401
  `INVALID_CREDENTIALS` with F3's text (`AuthError::NeutralSignIn`, `login::neutral_answer()`):
  one Argon2 verify (off the runtime) in every case; wrong password, unknown user, the system
  account, a passwordless account, a `throttled`/`compromised` password and an account without an
  eligible second step look the same. `mfa_code` → 400 `CLIENT_OUTDATED`. A banned account with a
  correct password and a second step still answers 403 `ACCOUNT_BANNED`.
- `confirmation_required` needs an active key device **and** `may_approve`; `totp_required` an
  active TOTP (`totp_state = active`). A `totp_required` step carries `qr: null`.
- `POST /auth/login-attempts/{id}/totp {verifier?, code}` (SM1): `password`/`oidc` attempts in
  `open`/`claimed`; first factor unchanged and active (else 410 `LOGIN_ATTEMPT_ENDED {expired}`);
  TOTP `active` (else 403 `TOTP_UNAVAILABLE`); wrong/replayed code → 401 `INVALID_MFA` and
  `second_factor_failed {factor: totp, count: 1}`. A claimed challenge becomes void.
- `sign_in_not_finished {factor, count: 1}` is recorded once per F3/F4 attempt that ends
  `denied`, `reported`, `cancelled` or `expired` (flag `unfinished_recorded`); notices coalesce.
- **OIDC (C27, C29 step 8).** Desktop start: `X-Kaiku-Client: desktop` with `loopback_port`,
  `callback_path` (43 b64u) and `client_challenge` → `{url, flow_id}`; the callback answers LP's
  hand-off page (form POST of `flow_id` + `code`, or `error`); the old `redirect_uri` login start
  → 400 `OIDC_CLIENT_OUTDATED`. Android: `X-Kaiku-Client: android` + `client_challenge` →
  `{url}`; the callback redirects to `kaiku://auth/callback?code=…` only. `POST
  /auth/oidc/desktop/exchange {code, verifier}` → 202 `LoginStep` (an `oidc` attempt bound to the
  start's `client_challenge`) or the neutral 401. Browser: `postMessage {type: "oidc-callback",
  status, attempt}` with the attempt cookie; failures `{type, status: "error", error}`.
- **For L2a (desktop):** read `flow_id` from the authorize JSON; the listener receives `flow_id`
  and `code` (or `error`) by form POST.
- **Coordinator:** a new account created by OIDC self-registration has no second step and gets the
  neutral answer; C27 has no registration-ticket path for OIDC sign-up (Task 19 decides).
- LP's passkey routes already record `sign_in` and `authenticator_added`/`authenticator_removed`;
  nothing added.

## Task 19 — registration tickets and passwordless accounts (F8)

- `POST /auth/register {username, password?, email?, display_name?}` → 201 `{user_id, username,
  ticket_expires_at, registration_ticket?}`: native clients (`X-Kaiku-Client`) get the ticket in
  the body, browsers the cookie `__Secure-kaiku_ticket` (`Path=/auth/registration`). Never a
  session. A password, when given, goes through the policy (400 `PASSWORD_REJECTED`).
- Ticket routes (`AuthFlow` per IP, header `X-Kaiku-Ticket` or the cookie): `GET
  /auth/registration` → `{expires_at, bound}`; `POST /auth/registration/totp` → `{secret,
  otpauth_uri}`; `POST …/totp/verify {code}` → `{recovery_codes}` (sets TOTP, recovery codes and
  `bootstrap_open`); `POST /auth/registration/passkey/{start,finish}` (C29 shapes, `active`);
  `POST /auth/registration/enrollments`, `GET|DELETE …/{id}`, `POST …/{id}/confirm {code}`
  (variant `registration`, `active`). The issuer routes are written per issuer (ticket here,
  request in Task 22) instead of the plan's generic `issuer_routes<E>`.
- The Android app's own key: `POST /auth/enrollments/claim` with `X-Kaiku-Ticket` (or, Task 22,
  `X-Kaiku-Request-Secret`) → 201 `{enrollment_id, state: "done", authenticator_state, ready_at}`
  without a code (`ClaimResponse` is an untagged enum; the code path is unchanged).
- New code `REGISTRATION_TICKET_INVALID` (404) for an unknown or expired ticket (**for L2**: not
  in C27's list).
- The sweep deletes accounts whose ticket bound nothing 30 minutes after creation; the first
  admin's TOTP routes answer 403 `TOTP_UNAVAILABLE`.
- `enrollments::finish_done_tx` holds SM2's `done` effects for codes and own claims; any `done`
  marks the subject's ticket bound. LP's `RegistrationBinding`/`RegistrationVia` gain `Ticket`
  and `Request` (appended).
- **L1b-1 boundary:** the Task 19 commit (`861d5778`) is the split point (≈ 8.9k insertions
  incl. tests up to there).
- Identity link (Task 16 follow-up): the `identity.link` grant is spent and committed before the
  provider round trip (a test showed a pool timeout when a transaction was held across it).

## Task 20 — recovery contact (C28)

- `PUT /api/me/recovery-contact {user_id}` (full + `recovery_contact.set`/user_id) → 200
  `RecoveryContact`; `DELETE` (`recovery_contact.set`/`""`) → 204; `GET
  /api/me/recovery-contact-for` → `{accounts}`; `POST
  /api/me/recovery-contact-for/{user_id}/confirm` (full, no step-up) → 204 (404 when the caller is
  not that account's contact); `PUT /api/admin/users/{user_id}/recovery-contact {user_id}` (full +
  `admin.recovery_contact.set`/user_id; an admin target → 403 `ADMIN_TARGET_IS_ADMIN`; in the
  non-elevated admin group).
- An admin, the account itself, the system account or an unknown id → 400
  `RECOVERY_CONTACT_INVALID`. A new contact clears the confirmation; `counts_from =
  confirmed_at + ACCOUNT_REQUEST_DELAY_HOURS` (new config, 24..=72, default 24, documented).
- Events: `recovery_contact_named` (first), `recovery_contact_changed` (later; details
  `{contact_user_id, previous_contact_user_id}`), `recovery_contact_confirmed` (actor
  `recovery_contact`). Recipients follow C28: the subject and the contacts named in the details
  (not all admins, as the plan said). For every `high` event the confirmed contact gets a copy
  (role `Contact`, never `ip`/`place`).
- `cascade::on_contact_changed` is the hook Task 21 fills (requests that recorded the old
  contact).

## Task 21 — account requests with recorded cancellers (SM3)

- `POST /auth/account-requests` (public, `AuthFlow` per IP): bootstrap now; recovery start is
  filled in Task 27. → 201 `{request, request_secret?}` (native body; web cookie
  `__Secure-kaiku_request`, Path `/auth/account-requests`). Request client header
  `X-Kaiku-Request-Secret`. `GET/DELETE /auth/account-requests/current` (DELETE = withdraw, no
  effects). Bearer: `GET /api/me/account-requests`, `POST /api/account-requests/{id}/cancel`
  (owner rule B.2: part 1/contact/admin with the grant `request.cancel`/id; `part2` only while the
  account has no active strong authenticator); `GET /api/admin/account-requests?state=open`
  (elevated).
- Cancellers are recorded as JSON routes `{route, id, type?}` (admin, contact, own authenticator)
  and re-evaluated live: an admin route needs the admin to still be admin with an active strong
  authenticator, a contact route a confirmed contact (confirmed ≥ one delay period before the
  start) with one, an authenticator route the authenticator still active. No route at start →
  409 `NO_INDEPENDENT_CANCELLER`; none left → `void`.
- `lock_accounts` and the factor generation locks take `FOR NO KEY UPDATE` (was `FOR UPDATE`):
  `FOR UPDATE` conflicts with the `FOR KEY SHARE` of FK inserts, and a notice delivery inserting
  rows for two accounts deadlocked (40P01, reproduced) with a cancel holding both account locks.
  Transitions still serialise. **L1a review:** this changes `approvals/transition.rs`.
- Cascades for requests of other accounts (an admin's or contact's authenticator revoked) update
  those rows without their account locks (void is idempotent; completion re-checks routes under
  its own lock and the row lock serialises the two).
- WS hint `account_request_updated {request_id, subject_id, state}` to the subject, the contact
  and all admins. New codes (C27 list): `ACCOUNT_REQUEST_OPEN`, `NO_INDEPENDENT_CANCELLER`,
  `ACCOUNT_REQUEST_COMPLETED`, `CANCEL_NOT_ALLOWED`, `REQUEST_FACTOR_INVALID
  {recovery_codes_left}`, `ACCOUNT_REQUEST_NOT_READY {state}` (appended; **for L2**).
- The start budget table `account_request_starts` exists now and is used in Task 27.
- Not written (time): tests for demoted admins voiding their admin requests, notices to lost
  devices, the WS hint, and the cancel-vs-completion / contact-change race tests
  (`concurrent_starts_one_open` exists).

## Task 22 — request client: staging and Finish

- Routes (request secret, else 404; state `ready`, else 409 `ACCOUNT_REQUEST_NOT_READY {state}`;
  past the deadline before the sweep → `state: "expired"`): `PUT …/current/password {password}`
  (blocklist; Argon2id hash staged), `POST …/current/totp` → `{secret, otpauth_uri}`, `POST
  …/current/totp/verify {code}` (the encrypted secret staged), `PUT …/current/recovery-contact
  {username}` (exists, not system, not admin, not self), issuer routes `POST
  …/current/enrollments`, `GET`/`DELETE …/current/enrollments/{id}`, `POST
  …/current/enrollments/{id}/confirm {code}`, `POST …/current/passkey/{start,finish}` → 201
  `Passkey` (`pending`, `ready_at` = the request's), `POST …/current/finish` → 200 `{request,
  recovery_codes}` and clears the request cookie. Staging answers the updated `AccountRequest`.
- One staged authenticator: a new phone or passkey replaces the previous pending one (a pending
  device is revoked with reason `request`, a pending passkey deleted); a new setup link voids the
  old one.
- The Android app's own key: a claim with `X-Kaiku-Request-Secret` of a `ready`, unexpired
  request → `{state: "done", authenticator_state: "pending", ready_at}`, no code.
- `KEY_DEVICE_PENDING {ready_at}` (device login) and the own-claim/status `ready_at` come from the
  request that staged the device (coordinator item 4).
- Finish locks the subject and the staged contact in ascending order, then runs
  `complete_locked_tx`. Two Task 21 bugs fixed here, both found by the new tests: the request is
  set `completed` before its effects (replacing the staged password's generation otherwise
  cascaded onto the request and voided it mid-completion), and the session sweep now also ends
  PC sessions without an authenticator (`NOT (NULL AND …)` kept them). Completion also cancels
  the request's open child enrolments.
- A second Finish → 409 `{state: "completed"}`, no new codes. Rule 4 failures (generation
  changed, no route left) → 409 `{state: "void"}`, nothing applied.
- A contact that became admin between staging and Finish fails Finish with 400
  `RECOVERY_CONTACT_INVALID` (stage another).
- The TOTP replay store is per account: a staged TOTP code in the same 30 s step as an old-secret
  code is refused (harmless: a real client stages at least a day after the start).

## Task 23 — "Add a phone", admin setup (F5c), admin routes

- `POST /auth/enrollments` (`own`): from a key device with `may_approve` (else 403
  `APPROVAL_NOT_ALLOWED`) by its `enrollment.issue` grant, or from a full PC session by a passkey
  grant (recorded as `Issuer::PcSession`). `GET`/`DELETE /auth/enrollments/{id}` for the issuer (any
  admin for `admin` enrolments); `POST /auth/enrollments/{id}/confirm {code}` for the PC, which
  re-checks the session (live, full) and the recorded passkey (active) — otherwise the enrolment
  expires (409 `ENROLLMENT_STATE`).
- `POST /api/admin/users/{user_id}/enrollments` from the admin's own key device (403
  `KEY_DEVICE_REQUIRED`), full + `admin.enrollment.issue`/user id per code; `400
  ENROLLMENT_TARGET_SELF` (new `AuthError` code); for an existing account (B.0 =
  `first_activity_at` set) 409 `ACCOUNT_REQUEST_OPEN` / `NO_INDEPENDENT_CANCELLER` at issue and
  again at `done` (rolls the done back). `mode` is stored in the new column
  `enrollments.admin_mode`. At `done`: a new account is active + `admin_setup {mode,
  admin_user_id, new_account: true}`; an existing account gets an `admin` request with the device
  pending (`admin_setup` / `admin_recovery`), lost devices revoked at completion.
- Admin routes (non-elevated admin group, full + grant): `PUT …/may-approve`, `PUT …/factors`
  (`compromised` marks the current generation; `active` clears `throttled` only, a compromised
  factor → 400 `VALIDATION_ERROR`), `POST …/authenticators/{id}/remove` (key device or passkey; no
  last-authenticator check; the removed phone's last notice goes before its push rows).
  `ensure_not_admin_target` refuses other admins' accounts (403 `ADMIN_TARGET_IS_ADMIN`), the
  admin's own account is allowed.
- `POST /api/admin/users`: `temporary_password` → 400; passwordless, `may_approve` off,
  `recovery_contact_id` optional. Admin views: the list carries `is_admin` and `may_approve`
  (coordinator item 3); details flatten the sign-in view (`key_devices`/`passkeys` as arrays
  `{id, label, state, ready_at, created_at, last_used_at}` as L2a's panel reads them).
- **Not done:** the browser setup-link passkey claim (`/auth/enrollments/passkey/{start,finish}`,
  `claimed_passkey`, the `passkeys` row at `done`) — L2a calls it; `admin_role_granted` (no server
  route grants the role).

## Task 24 — "That wasn't me"

- `POST /api/me/security-events/{id}/repudiate {part2|null}` → `{event, ended_session_ids,
  revoked_authenticator_ids, compromised, voided_request_ids}`. Repudiable: `sign_in`,
  `authenticator_added`, `password_set`, `password_changed`. A password sign-in's generation comes
  from its login attempt; TOTP uses the current generation. The event's own authenticator never
  vouches; part 2 never revokes.
- `actions_for`: `repudiate` (`step_up` / `part2`, warnings `voids_own_request` /
  `voids_own_bootstrap`), `remove_device`, `confirm_contact`.

## Task 25 — upgrade, security overview, removed routes

- Migration `20261121000600_upgrade_login_v1.sql` (000500 is `admin_mode`): ends sessions of
  accounts with neither TOTP nor a strong authenticator (`upgrade`), opens bootstrap for
  password/OIDC + TOTP accounts without one, clears `must_change_password`, adds
  `users.bootstrap_reminded_at`, drops `mfa_backup_codes` and `password_reset_tokens`.
- `/auth/forgot-password` and `/auth/reset-password` are gone (404); the reset e-mail, queries and
  cleanup job too. `RateLimitCategory::AuthPasswordReset` stays (config compatibility).
- `GET /api/me/security` with the C27 prompts; `bootstrap_reminder` every 30 days
  (`COALESCE(bootstrap_reminded_at, created_at)`).
- **For L2:** the `client/` code on this branch still has the reset views.

## Task 26 — per-account throttling (B.5)

- `ratelimit/account.rs`: counters `ratelimit:acct:{password|totp}:{key}`; from the 5th failure
  429 `TOO_MANY_ATTEMPTS` + `Retry-After` (30 s · 2^(n−5), max 1 h); `guessing` every 20; at 100
  `throttle_tx` (TOTP also compromises the attempt's password generation). Only wrong passwords
  count; a started attempt resets. Valkey down → 503. TOTP failures do not feed the per-IP
  counter.

## Task 27 — self-service recovery

- `purpose: "recovery"`: budget (3 per delay period) → recovery code (invalid = unknown-user
  answer and work, `recovery_codes::dummy_check`) → consumed + start counted → password (`active`)
  or TOTP (any state); failure → 401 `REQUEST_FACTOR_INVALID {recovery_codes_left}`.
  `lost: "all_key_devices"`.
- **Not done:** OIDC as a request factor (`/auth/account-requests/oidc/authorize/{provider_id}`,
  `oidc-request-callback`, desktop/Android hand-back); `redeem_oidc_code` refuses every code. L2a
  calls the authorize route.

## Task 28 — host CLI recovery

- `factor reset`, `throttle reset`, `may-approve`, `recovery-contact`, `admin demote`, `request
  cancel` (`OwnerAuthority::Cli`). CLI session ends record a new event `sessions_ended` (high,
  `{count, by: "cli"}`) — appended to C28 (coordinator item 5). Factor/throttle resets and
  `may-approve` are audit-only (no C28 event).

## Coordinator round (after d7e7c03e)

- (a) Done: `POST /auth/enrollments/passkey/{start,finish}` (C29 browser claim; the verified
  passkey is held in `enrollments.claimed_passkey`, the `passkeys` row is created at `done`,
  `active` or `pending` like a key device; the browser long-polls `/auth/enrollments/status` with
  the `__Secure-kaiku_enroll` cookie instead of `claim_secret`). `admin_role_granted` /
  `admin_role_removed` are recorded where `system_admins` rows change (first registered admin,
  `grant_system_admin`, `revoke_system_admin`, CLI demote).
- (b) Done: `POST /auth/account-requests/oidc/authorize/{provider_id}` (`prompt=login`,
  `max_age=0`). L2a's assumption holds: the desktop start answers `{url, flow_id}` like the
  sign-in start (here the loopback fields come in a JSON body, as L2a's Rust sends them). The
  callback checks the verified ID token's `auth_time` (≤ 300 s) and the linked identity and hands
  back a one-time `oidc_code` (60 s): web `postMessage {type: "oidc-request-callback",
  oidc_code}`, desktop hand-off page, Android `kaiku://auth/callback?code=…`. A stale or unlinked
  sign-in still gets a code (no oracle) that redeems to nothing. Native codes need the verifier.
- Repudiation: sign-in events record `factors: [{factor, generation}]` for the password and TOTP
  they used; "That wasn't me" compromises exactly those. A generation replaced since is left
  alone (the repudiation is still recorded); an event without a recorded generation compromises
  nothing.
- `lock_accounts` takes `FOR NO KEY UPDATE`: this still conflicts with itself, so two transitions
  on one account serialise exactly as before, and the ascending-id order of B.7 still decides
  between accounts. Only `FOR KEY SHARE` is no longer blocked, and that is what the FK checks of
  child inserts (notices, events, sessions) take. They change no account row and take no account
  lock, so they cannot form a cycle with a transition. `DELETE` of a users row still conflicts.
- Mutation checks (guarded line removed → the named test must fail): KILLED — request completion
  generation re-check and route re-check (`finish_rechecks_routes_and_generations`), request
  client `ready` check (`staging_only_while_ready`), recovery start budget, active password,
  code consumed first (`recovery_requests::*`), per-account delay and throttle at 100
  (`throttling::*`), attempt TOTP-step generation re-check, owner rule part 2 without a strong
  authenticator. SURVIVED at first and strengthened: registration ticket expiry (new
  `expired_ticket_is_refused`), attempt collect generation re-check (new
  `collect_rechecks_window_and_generation`, also the 60 s window and `LOGIN_ATTEMPT_NOT_READY`);
  both KILLED on the re-run.
- C28 follow-up (later): CLI factor reset, throttle reset and `may-approve` — superseded by the
  review round below (the contracts now define the events).

## Review round 1 (Codex, Fable) at d7e7c03e

| Finding | Verdict | Commit | Test |
|---|---|---|---|
| Fable B1 / Codex M3 — TOTP counter missing on OIDC attempt, bootstrap, part 2, recovery | fixed: `auth::totp_guard` (pre-check, outcome, 20/100 effects in their own transaction; bootstrap compromises its password generation at 100) | 103ac2f5 | `throttling::{oidc_attempt_totp_step_is_throttled, bootstrap_totp_is_throttled, part2_totp_is_throttled, recovery_totp_is_throttled_before_a_code_is_consumed}` |
| Codex M4 — bootstrap bypasses the start budget | fixed: shared budget (3 per delay period) and the password counter | 103ac2f5 | `throttling::bootstrap_starts_share_the_budget` |
| Codex B2 — old credential recorded as a newer generation | fixed: bootstrap and recovery re-read hash/secret under the lock; `/auth/login` re-checks hash + generation before the attempt | 103ac2f5, 6aaf439f | none deterministic (interleaving); `account_requests::concurrent_starts_one_open` rewritten |
| Fable M1 / Codex M5 — repudiation uses current or attempt-row generations | fixed: sign-in, unfinished sign-in, password and TOTP events record their generation; only that one is compromised, a replaced one is left alone, unknown → nothing | e37ac4a7, 633dc446 | `repudiation::{a_sign_in_records_the_generations_it_used, repudiation_compromises_exactly_the_recorded_generation, a_factor_replaced_since_is_left_alone, a_password_change_compromises_only_the_generation_it_made}` |
| Codex M10 — no repudiation for `sign_in_not_finished`, `step_up_action` | fixed: unfinished → its recorded password generation; step-up → ends its session, revokes nothing | 633dc446 | `repudiation::{an_unfinished_sign_in_compromises_its_password, a_step_up_action_ends_its_session_and_revokes_nothing}` |
| Fable M2 / Codex M6 — recovery enumeration | fixed: `lost` validated before the lookup; `consume_tx` always checks `CODE_COUNT` hashes (dummy-padded, also for a malformed code) | 103ac2f5 | `recovery_requests::*` (timing not asserted) |
| Fable M3 — attempt long-poll holds a connection | fixed: a connection per step, at most 32 concurrent long-polls (then answered at once) | 594bd2d6 | none (pool behaviour) |
| Codex M8 — no re-check after the locks | fixed: admin routes re-check the actor's role under the lock (403 `NOT_ADMIN`); part 2 re-checks its session is live | 594bd2d6 | none deterministic |
| Codex M9 — bound ticket still binds | fixed: binding routes refuse a bound ticket (also re-checked under the lock) | 594bd2d6 | `registration_tickets::a_bound_ticket_binds_nothing_more` |
| Codex 11 / Fable m6 — CLI notices (coordinator correction) | fixed: `factor_restored`, `throttle_reset`, `may_approve_changed` (`by: admin|cli`) for CLI and admin routes; `sessions_ended` kept | 6aaf439f | `cli_recovery::every_cli_action_is_audited_and_notified`, `admin_setup::may_approve_and_factor_toggles_are_audited` |
| Fable m1 — bootstrap_open after a strong bind | fixed | 594bd2d6 | `registration_tickets::a_bound_ticket_binds_nothing_more` |
| Fable m2 — banned accounts run requests | fixed: starts answer neutrally, Finish 403 `ACCOUNT_BANNED`, admin completion waits | 103ac2f5, 9dde2e6e | `account_request_client::a_banned_account_cannot_finish` |
| Fable m3 — recovery-code generation not re-checked | fixed | 9dde2e6e | `account_request_client::finish_rechecks_the_recovery_code_generation` |
| Fable m4 — lost devices in setup mode | fixed: 400 | 9dde2e6e | `admin_setup::lost_devices_only_in_recovery_mode` |
| Fable m5 — `ACCOUNT_BANNED` on a correct password | kept (coordinator decision: a banned person is told so) | — | `login_password::passwordless_banned_and_system_accounts` |
| Fable m7 — WS hint to an unconfirmed contact | fixed | 9dde2e6e | none |
| Fable m8 — Argon2 under the account lock | fixed: policy + hash before the transaction | 9dde2e6e | `account_request_client::stage_password_totp_and_contact` |
| Fable m9 — admin contact can confirm | fixed: 400 `RECOVERY_CONTACT_INVALID` | 9dde2e6e | `recovery_contact::a_contact_who_became_admin_cannot_confirm` |
| Fable M4 — test gaps | partly: collect window/generation/`LOGIN_ATTEMPT_NOT_READY`, ticket expiry, probation Add a phone, `/auth/login` passwordless/system/banned, admin `STEP_UP_REQUIRED`, repudiation negatives, error codes in the named bare-status tests | ac868475, 5904aca5 | as listed |
| Fable M4 — rest (request voids and races, throttle completion paths, TOTP-step refusals, probation cancel/repudiate without part 2) | deferred (next round) | — | — |
| Codex M7 — canceller cascades without cross-account serialisation | deferred: needs the discover–lock–recheck protocol across accounts (a fresh transaction when the lock set grows); the row lock keeps each request's own transition atomic today | — | — |
| Codex B1 — pending enrolment revokes the same install's phone | deferred to the LP merge (L1a b8814823); L1b's Pc/Request/Ticket confirmers and request completion use the shared `finish_done_tx`; recovery-case tests after the merge | — | — |
| Flash note — step-up consume before both locks | verified: admin routes, admin setup, admin contact and Cancel take one ascending `lock_accounts([actor, target])` (or `lock_scope`) before any consume | — | — |

## LP merge (35fcadf8) and the second coordinator round

- Merge `6167214c` (`git merge --no-ff origin/feat/v1.0-login-lp`). Conflicts and resolutions:
  `enrollments/mod.rs` — L1b's extracted `finish_done_tx` (passkey branch, request staging,
  admin start) now uses L1a's `subject_is_new`, `replaceable_on_install` (a pending key never
  replaces a live one, `ENROLLMENT_DEVICE_IN_USE`, never the issuer's own device) and
  `replaced_reason`; status combines L1a's expired-code `CASE` with L1b's passkey state.
  `enrollments/handlers.rs` — L1a's `notify_issuer`. `notices.rs` — L1a's sorted recipients
  without the system account and its recipient locks before the event row, plus L1b's contact
  recipients (event type and details read with the head row). `timers.rs` — L1a's independent
  steps plus L1b's login-attempt expiry and unbound clean-up. `governance/error.rs` — both
  variants. `approvals/handlers.rs`, `login.rs` — LP's comment and refresh helpers. Tests: both
  sides' enrolment tests; L1b's own-claim helper binds the attestation to the phone key (SPKI);
  LP's passkey step-up lock test now holds `FOR NO KEY UPDATE` (L1b's account lock mode, which
  a key-share lock no longer blocks).
- Codex B1: every L1b confirmer (Pc, Request, Ticket, own claims) and request completion go
  through `finish_done_tx` with L1a's rule; a recovery's pending phone on a live install is
  refused at the proof of possession and the live key stays (`a_recovery_phone_never_replaces_a_live_key`).
  Completion activates the pending row; there is never a replacement to do at completion.
- Codex M7 (`58781fac`): `lock_for_start` (subject ∪ each admin ∪ the contact) for bootstrap,
  recovery and admin-setup starts; `lock_route_change` (user ∪ actors ∪ every account whose
  open requests record the user as admin or contact or one of its authenticators) for owner and
  admin device/passkey removal, key invalidation, CLI device removal, CLI demote and
  repudiation; completions with lost devices lock their recording accounts. All over L1a's
  `lock_scope_with` (one ascending statement, savepoint retry). Test
  `account_requests::start_vs_route_loss_serialise` (12 concurrent rounds, no deadlock, never an
  open request without a live route). Not covered: the admin request started at `done` inside
  the confirm (it holds the subject and the answering device's account only).
- Fable M4 rest (`review_gaps.rs`): late cancel → 409 `ACCOUNT_REQUEST_COMPLETED`; voids on
  device removal, demotion and contact change; TOTP renewal before Finish → void; counter reset
  on success; TOTP-step refusals (a QR attempt now answers 403 `TOTP_UNAVAILABLE` instead of
  409 `LOGIN_ATTEMPT_NOT_READY` — for L2), compromised TOTP, claimed challenge voided; probation
  Cancel/repudiation without part 2 → 403 `FULL_SESSION_REQUIRED`.

## Review round 2 (Codex, Fable, Flash) at 166c35cc

| Finding | Verdict | Commit | Test |
|---|---|---|---|
| Codex M9 — bound ticket / two links / own claim after bind | fixed: own claim and ticket confirmer require an unexpired, unbound ticket under the account lock; a bind cancels the ticket's other open links | 2b73b7d4 | `registration_tickets::a_ticket_binds_one_phone_even_with_two_links` (sequential, concurrent, own claim; RED seen) |
| Fable M-A — Argon2 under every admin's lock | fixed: code found without a lock with constant `CODE_COUNT` work, then `lock_for_start`, consumed by id while unused and current | a6821e02 | `recovery_requests::*` |
| Codex M6 (rest) — 429 before code verification | fixed: an invalid code is neutral also while delayed; bootstrap's delay answers neutrally (Fable i) | a6821e02 | `throttling::an_invalid_code_is_neutral_while_delayed`, `bootstrap_totp_is_throttled` (now neutral) |
| Fable m-d — OIDC code spent before the checks | fixed: TOTP delay and budget before redeeming | a6821e02 | — |
| Fable M-B — same-install re-enrolment cascades first | fixed: the old key leaves the live index, the new row exists, then the cascade; `confirm_tx` takes `lock_route_change` when a live key is on the claimed install | 44312523 | `review_gaps::same_install_re_enrolment_keeps_the_admin_route` (RED seen) |
| Codex M7 / Fable / Flash — admin at `done` | fixed: `confirm_tx` takes `lock_for_start` for `admin` enrolments (key device and passkey claims); route discovery includes `admin` requests the admin or the device started | 44312523 | `admin_setup::*`, `enrollment_issuers::existing_account_browser_passkey_waits_for_its_request` |
| Codex M7 — a restored route resurrecting a request | verified: route losses void at once under the locks (cascades), so no request survives a loss to be revived | — | `review_gaps::the_only_routes_gone_voids_the_request` |
| Codex — stale TOTP failure throttles a new TOTP | fixed: the effects carry the TOTP generation and re-check it and the counter under the lock | f3ccce12 | `throttling::a_stale_totp_failure_does_not_throttle_the_new_totp` |
| Codex M8 (rest) — session predicate | fixed: `absolute_expires_at` too | f3ccce12 | — |
| Codex M11 / Flash — CLI and role events | fixed: a delay-only throttle reset records `throttle_reset`; `revoke_system_admin` records the admin; CLI demote already recorded `admin_role_removed` | f3ccce12 | `cli_recovery::a_delay_only_reset_is_recorded`, `admin_setup::admin_role_changes_are_recorded` |
| Flash — TOTP notices | fixed: the effects deliver pending notices when the caller has a server state (part 2 has none: minute sweep) | f3ccce12 | — |
| Flash / Fable m-c — RequestFactor native failures | verified: `flow_failure_response` routes by `flow_state.native` first (hand-off page / Android error) | — | `recovery_requests::oidc_factor::a_desktop_provider_error_reaches_the_app` |
| Codex minor — auth_time at redemption | fixed: re-checked at redemption (≤ 300 s) | cbb62665 | `oidc_factor::a_code_whose_sign_in_aged_is_refused` |
| Flash verdict 1 — OIDC subject vs target | verified (no gap): the redeemed identity must be the typed account's | — | `oidc_factor::an_identity_of_another_account_is_refused` |
| Flash verdict 2 — passkey finish binding | verified: another link's token → 404 | — | `existing_account_browser_passkey_waits_for_its_request` |
| Fable m-a / m-b / Flash banned start | fixed: claim-time duplicate and limit checks; admin actor on `authenticator_added`; banned subject refused at passkey start | 5df23b23 | `existing_account_browser_passkey_waits_for_its_request` (actor; RED seen) |
| Fable tests — OIDC bootstrap, single redeem, reused ceremony | added | cbb62665, 5df23b23 | `oidc_factor::{bootstrap_with_oidc_as_the_first_factor, a_code_redeems_once}`, reused ceremony → 400 |
| Fable m-e — notes line | the QR attempt's TOTP step answers 403 `TOTP_UNAVAILABLE` (C27 lists it; L2a/L2b map the text) | — | `review_gaps::totp_step_refusals` |

Tests written after the fix (RED not observed): the neutral-delay, stale-TOTP, aged-code, delay-only reset
and existing-account passkey tests; the M-A and session-predicate changes have no dedicated test.

## Review round 3 (Codex, Fable, Flash) at c4e6be33

| Finding | Verdict | Commit | Test |
|---|---|---|---|
| Codex Major — confirm deadlock (40P01) | fixed: `lock_confirm_scope` finds the subject, the answering admin, every possible canceller of an `admin` enrolment and the accounts recording a live key on the claimed install, then locks them in one ascending statement before any device, challenge or enrolment row. It re-reads the scope under the locks and retries from a savepoint if the scope changed. Both the enroll answer and `confirm_tx` use it | 391a073e | `review_gaps::an_admin_confirmation_and_a_canceller_transition_never_deadlock` (deterministic; RED: answer 500 → 200) |
| Codex/Fable — `live_on_install` read before the lock | fixed: the live key on the install is part of the re-read scope; `replaceable_on_install` at `done` runs under those locks | 391a073e | `review_gaps::same_install_re_enrolment_keeps_the_admin_route` |
| Codex/Flash/Fable — TOTP counter races | fixed: <ul><li>the counter is keyed by account and generation (`ratelimit:acct:totp:{user}:{generation}`), so a renewal starts a fresh counter;</li><li>`totp_tx` reports the generation it checked;</li><li>a failure is counted atomically with the counter's epoch;</li><li>a success starts a new epoch;</li><li>the 20/100 effects apply under the account lock only while the epoch, the count and the generation still match.</li></ul> | 56d1290a | `throttling::a_totp_renewal_starts_a_fresh_counter` (RED 429), `throttling::effects_of_an_old_epoch_never_apply`, `throttling::a_stale_totp_failure_does_not_throttle_the_new_totp` (r2) |
| Codex — OIDC bootstrap freshness after the lock wait | fixed: `auth_time` re-checked (≤ 300 s) after `lock_for_start` | fcdb8088 | `oidc_factor::bootstrap_oidc_freshness_is_rechecked_after_the_lock_wait` (RED 201 → 401) |
| Codex 5b — one live-session predicate | fixed: `live_clause!` (with `absolute_expires_at`) is exported and used by the part-2 check, the Pc confirmer and the security view's session count | daf43a60 | `review_gaps::part2_refuses_a_session_past_its_absolute_cap` (regression guard; r2 had already added the cap) |
| Codex 5a — same-code recovery race | verified and tested: two concurrent starts give one 201 and one neutral 401, one `used_at` and one start row. `consume_id_tx` with a bumped generation returns `false` | 2f0bfe65 | `recovery_requests::{one_code_used_twice_at_once_starts_one_request, a_found_code_of_an_old_generation_is_not_consumed}` (RED with the guards removed: 201/409; consumed) |
| Fable — Argon2 holding a pool connection | fixed: `recovery_codes::find` takes the pool, fetches the rows and holds no connection during the Argon2 work | b9745283 | — (structural) |
| Fable — demoted issuer completes an `admin` enrolment | fixed: at `done` the issuer's `system_admins` row is read `FOR SHARE`, so a concurrent demotion waits. A demoted issuer's answer gets 403 `APPROVAL_NOT_ALLOWED` and the enrolment stays `claimed` | b9745283 | `enrollment_issuers::a_demoted_issuer_cannot_complete_an_admin_enrolment` (RED: 200, `done`) |
| Fable — passkey `done` order | fixed: `done` before `mark_bound_tx`, as on the key-device path; no observable difference, since the bind sets only `issued`/`claimed` links to `cancelled` | b9745283 | existing passkey enrolment tests |
| Fable — banned admin request | notes: the timer skips a banned account's due `admin` request (`complete_locked_tx` → `AccountBanned` → `continue`). The request stays `waiting` and completes on the first tick after the ban ends. Known limit: such requests stay in the timer's `LIMIT 200` selection | — | — |
| Contract — bootstrap 401 | done by the coordinator | — | — |
