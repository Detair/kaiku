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
