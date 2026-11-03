<!-- Parent: ../../AGENTS.md -->

# Approvals Module

**SECURITY CRITICAL** — device signatures (C26), key devices (SM4), enrolment (SM2), device-key sign-in and device step-up. Every change gets a Faramir security review and an external review before merge.

## Purpose

- `secrets.rs` — pre-session secrets: `Secret256` (256-bit CSPRNG, `[REDACTED]` Debug, zeroized on drop), `keyed_hash` (HMAC-SHA256 under the server key, one domain per `SecretPurpose`), `ct_eq`, strict b64u, `random_digits` (rejection sampling).

## Rules

- Secrets are 256-bit CSPRNG values (codes: 6 digits), single use, stored only as `keyed_hash` under `Config::auth_hmac_key` (DC1), compared with `ct_eq`. Never a UUID (v4 or v7) as a secret, never a plain SHA-256 of a code.
- Never log a secret, token, code or signature. Errors never echo them.
- The signature origin is `Config::public_origin.origin` (`PUBLIC_URL`, mandatory). Changing the domain invalidates every device signature.
- Every core function takes `auth::ctx::AuthCtx` so HTTP handlers, timers and the host CLI share it.
