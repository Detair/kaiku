<!-- Parent: ../../AGENTS.md -->

# Approvals Module

**SECURITY CRITICAL** — device signatures (C26), key devices (SM4), enrolment (SM2), device-key sign-in and device step-up. Every change gets a Faramir security review and an external review before merge.

## Purpose

- `secrets.rs` — pre-session secrets: `Secret256` (256-bit CSPRNG, `[REDACTED]` Debug, zeroized on drop), `keyed_hash` (HMAC-SHA256 under the server key, one domain per `SecretPurpose`), `ct_eq`, strict b64u, `random_digits` (rejection sampling).

- `signing.rs`, `context.rs` — the `kaiku-sig-v1` bytes and canonical contexts (C26, golden vectors in `mobile/android/app/src/test/resources/golden/kaiku_sig_v1.json`; never regenerate the signatures).
- `transition.rs` — B.7 rule 1: `lock_accounts` (one statement, ascending id order), `lock_scope`.
- `cascade.rs` — B.7 rule 2: `on_sessions_ended` (grants, enrolments), `on_authenticator_revoked` (sessions, grants; devices also void their challenges).
- `core.rs` — challenges: `issue` (v4 id, 32-byte nonce, ≤ 10 min), `lock_for_answer` (`FOR UPDATE`; another device's → 404; past due → expired, 410; not `issued` → 409), `verify`, `set_state`, `expire_due` (minute sweep).
- `handlers.rs` — `/api/approvals/*` (`ApprovalLogin` limit per user): `step-up` (key-device session only), `answer` (dispatch by kind), `deny`, `release`.
- `device_login.rs` — device-key sign-in (B.4, C26 `device_login`): public `POST /auth/device-login/challenge` and `POST /auth/device-login` (`AuthFlow` per IP, never IP-blocked). `sign_in` locks account → device row → challenge, re-checks `active` and the ban, consumes the challenge, ends the device's older sessions (`superseded`), mints a full android session and records `sign_in`.
- `enrollments/` — SM2: `issue_tx` per issuer, the public `open`/`pop-challenge`/`claim`/`status` routes (`AuthFlow`), `confirm_tx` per confirmer (a re-enrolling install's old row goes through the revocation cascade), `after_done`.
- `devices/` — key devices (SM4): list (any session), remove (full + `key_device.remove` grant, never the last strong authenticator), `invalidated` (the device's own session), `revoke_tx` (+ cascade + notice event), `after_revoke` (deliver the notice, then delete the device's push subscriptions).

## Order of checks (answer)

1. The caller's session is a key-device session (its authenticator is an `active` key device of the same user), else 403 `KEY_DEVICE_REQUIRED`.
2. Lock the challenge; it must name this device's install id (else 404), be `issued` and not past due.
3. Per kind: `device_stepup` — approve only, empty response, answered from the session that asked; verify the signature; `consumed`; `grant_tx` in the same transaction. `enroll` — signature first, then the code (B.2 rev 4). `login` — L1b (response compared first).

## Rules

- Secrets are 256-bit CSPRNG values (codes: 6 digits), single use, stored only as `keyed_hash` under `Config::auth_hmac_key` (DC1), compared with `ct_eq`. Never a UUID (v4 or v7) as a secret, never a plain SHA-256 of a code.
- Never log a secret, token, code or signature. Errors never echo them.
- The signature origin is `Config::public_origin.origin` (`PUBLIC_URL`, mandatory). Changing the domain invalidates every device signature.
- Every core function takes `auth::ctx::AuthCtx` so HTTP handlers, timers and the host CLI (`crate::cli`, `kaiku-admin`) share it.
- Lock order (B.7): accounts first (`lock_accounts`), then device rows, then challenges and enrolments. Every revoking caller takes the account lock before `revoke_tx`, so a concurrent device-key sign-in either sees `revoked` or its session is ended by the revocation.
- 429 on the v1.0 sign-in categories (`ApprovalLogin`, `AuthFlow`) is `TOO_MANY_ATTEMPTS` with `Retry-After`.
