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
