<!-- Parent: ../../AGENTS.md -->

# Approvals Module

**SECURITY CRITICAL** — device signatures (C26), key devices (SM4), enrolment (SM2), device-key sign-in and device step-up. Every change gets a Faramir security review and an external review before merge.

## Purpose

- `secrets.rs` — pre-session secrets: `Secret256` (256-bit CSPRNG, `[REDACTED]` Debug, zeroized on drop), `keyed_hash` (HMAC-SHA256 under the server key, one domain per `SecretPurpose`), `ct_eq`, strict b64u, `random_digits` (rejection sampling).

- `signing.rs`, `context.rs` — the `kaiku-sig-v1` bytes and canonical contexts (C26, golden vectors in `mobile/android/app/src/test/resources/golden/kaiku_sig_v1.json`; never regenerate the signatures).
- `transition.rs` — B.7 rule 1: `lock_accounts` (one statement, ascending id order), `lock_scope`.
- `cascade.rs` — B.7 rule 2: `on_sessions_ended` (grants, enrolments; never challenges, see the lock order), `on_authenticator_revoked` (sessions, grants, enrolments; a key device also voids its open challenges).
- `core.rs` — challenges: `issue` (v4 id, 32-byte nonce, ≤ 10 min, never already past due), `head` (approver and subject without a lock, to learn which accounts to lock), `lock_for_answer` (`FOR UPDATE` by id + approver + install id; another account's or device's → 404; open and past due → expired, 410, the caller commits the mark; not `issued` → 409), `verify`, `set_state`, `expire_due` (minute sweep).
- `handlers.rs` — `/api/approvals/*` (`ApprovalLogin` limit per user): `step-up` (key-device session only), `answer` (dispatch by kind), `deny`, `release`.
- `device_login.rs` — device-key sign-in (B.4, C26 `device_login`): public `POST /auth/device-login/challenge` and `POST /auth/device-login` (`AuthFlow` per IP, never IP-blocked). `sign_in` locks account → device row → challenge, re-checks `active` and the ban, consumes the challenge, ends the device's older sessions (`superseded`), mints a full android session and records `sign_in`.
- `enrollments/` — SM2: `issue_tx` per issuer, the public `open`/`pop-challenge`/`claim`/`status` routes (`AuthFlow`), `confirm_tx` per confirmer (a re-enrolling install's old row goes through the revocation cascade), `after_done`.
- `devices/` — key devices (SM4): list (any session), remove (full + `key_device.remove` grant, never the last strong authenticator), `invalidated` (the device's own session), `revoke_tx` (+ cascade + notice event), `after_revoke` (deliver the notice, then delete the device's push subscriptions).

## Order of checks (answer)

1. The session snapshot names a key device, else 403 `KEY_DEVICE_REQUIRED` (no lock yet).
2. Read the challenge's approver and subject without a lock; another approver's → 404. Lock the caller's and the subject's accounts in one statement.
3. `key_device_session`: the device row (`FOR UPDATE`, `active`, same user, else 403) and the authorising session row (`FOR UPDATE`, live, full, bound to that device, else 401 `INVALID_TOKEN`).
4. Lock the challenge by id, approver and install id (else 404); it must be `issued` and not past due.
5. Per kind: `device_stepup` — approve only, empty response, answered from the session that asked; verify the signature; `consumed`; `grant_tx` in the same transaction. `enroll` — only from the enrolment's issuing device (else 404), signature first, then the code (B.2 rev 4); deny likewise. `login` — L1b (response compared first).

## Rules

- Secrets are 256-bit CSPRNG values (codes: 6 digits), single use, stored only as `keyed_hash` under `Config::auth_hmac_key` (DC1), compared with `ct_eq`. Never a UUID (v4 or v7) as a secret, never a plain SHA-256 of a code.
- Never log a secret, token, code or signature. Errors never echo them.
- The signature origin is `Config::public_origin.origin` (`PUBLIC_URL`, mandatory). Changing the domain invalidates every device signature.
- Every core function takes `auth::ctx::AuthCtx` so HTTP handlers, timers and the host CLI (`crate::cli`, `kaiku-admin`) share it.
- Lock order (B.7): accounts first (`lock_accounts`, every account of the transition in one statement), then device rows, then session rows, then challenges and enrolments. Every revoking caller and every answer, deny, release, issuer challenge and confirm takes the account lock first, so a concurrent device-key sign-in either sees `revoked` or its session is ended by the revocation, and an answer never waits in a cycle with a revocation (pinned by `answer_and_revocation_never_deadlock` and `enroll_answer_and_revocation_never_deadlock`). Paths that end sessions without the account lock (logout, refresh) touch only sessions, grants and enrolments, never challenges or device rows.
- A re-enrolment on the same install as an `active` key revokes the earlier key with the full cascade, `revoke_reason` by variant (`own`/`registration` → `owner`, `admin`, `request`, `cli`), and records `authenticator_removed` for it; its push subscription stays (keyed by account and install id, it now belongs to the new key). Never the enrolment's own issuer, and never for a key that will be `pending` (`admin` of an existing account, `request`): SM4 lets only the account request's completion replace a key, so such a claim is refused with 400 `VALIDATION_ERROR` at `pop-challenge` and again in `confirm_tx` under the lock (`replaceable_on_install`).
- 429 on the v1.0 sign-in categories (`ApprovalLogin`, `AuthFlow`) is `TOO_MANY_ATTEMPTS` with `Retry-After`.
