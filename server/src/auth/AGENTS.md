<!-- Parent: ../../AGENTS.md -->

# Auth Module

**SECURITY CRITICAL** — Authentication, session management, OIDC/SSO, MFA, and password hashing.

## Purpose

- Local authentication (username/password registration and login)
- JWT-based session management (access + refresh tokens)
- OpenID Connect (OIDC) integration for SSO
- Multi-factor authentication (TOTP)
- Password hashing with Argon2id
- Authentication middleware for protected routes

## Key Files

- `mod.rs` — Router setup with rate limiting per category
- `register.rs` — Registration handler (first-user admin grant, transactional)
- `login.rs` — Login, logout, refresh, password reset, OIDC handlers
- `mfa.rs` — MFA setup/verify/disable, backup codes, QR login handlers
- `sessions.rs` — Session listing and revocation handlers
- `identities.rs` — Linked external (OIDC) identity view/link/unlink handlers
- `profile.rs` — Profile read/update, avatar upload, password change handlers
- `helpers.rs` — Shared helpers (token extraction, user-agent parsing)
- `jwt.rs` — JWT token creation, validation, and claims parsing
- `middleware.rs` — `require_auth` middleware extracting AuthUser from JWT
- `password.rs` — Argon2id password hashing and verification
- `mfa_crypto.rs` — TOTP generation, verification, and QR code creation
- `oidc.rs` — OpenID Connect provider configuration and callback handling
- `error.rs` — AuthError and AuthResult types

## For AI Agents

**SECURITY CRITICAL MODULE**: Every change must be reviewed with Faramir mindset. Never skip validation, never weaken crypto parameters, never leak tokens in logs.

### JWT Security

**Token Lifetimes** (REQUIRED):
- Access token: 15 minutes (`JWT_EXPIRY = 900` in jwt.rs)
- Refresh token: 7 days (stored in database `sessions` table)

**Algorithm**: EdDSA (Ed25519) or RS256. NEVER use HS256 for production (symmetric secret is weaker). Current implementation uses HMAC for simplicity but should migrate to EdDSA.

**Claims Structure**:
```rust
{
    "sub": "user_id",        // UUID as string
    "exp": timestamp,        // 15min from issue
    "iat": timestamp,        // issued at
    "username": "..."        // for convenience
}
```

**Token Storage**:
- Access tokens: Client-side only (memory or sessionStorage, NEVER localStorage)
- Refresh tokens: Database `sessions` table with expiry, invalidated on logout

**Validation**: Always use `jwt::validate_access_token()` which checks signature, expiry, and claims format. Middleware `require_auth` does this automatically.

### Password Security

**Hashing**: Argon2id with parameters from OWASP recommendations:
```rust
// In password.rs
Config::default()  // Uses OWASP-recommended defaults
```

**Timing Attacks**: `hash_password()` and `verify_password()` are constant-time. Never implement custom comparison.

**Password Requirements**: Enforced client-side and in handlers:
- Minimum 8 characters (consider 12+ in production)
- No maximum (bcrypt has 72-byte limit, Argon2id does not)

### MFA (TOTP)

**Setup Flow**:
1. `POST /auth/mfa/setup` generates secret, returns QR code data URL
2. User scans QR code in authenticator app
3. `POST /auth/mfa/verify` with first TOTP code enables MFA
4. `mfa_secret` stored in users table (encrypted if `encryption_key` in config)

**Login Flow**:
1. Username/password validates user
2. If `mfa_enabled`, return `AuthError::MfaRequired` (HTTP 428)
3. Client sends second request with `totp_code` in login body
4. `mfa_crypto::verify_totp()` checks code (30s window, ±1 step tolerance)

**Backup Codes**: Implemented (`backup_codes.rs`, `mfa::mfa_generate_backup_codes`) — single-use hashed codes accepted by the login MFA path.

### OIDC Integration

**Supported Providers**: Configurable in `config.oidc_providers` (array of `OidcProvider`).

**Flow**:
1. `GET /auth/oidc/authorize/:provider` redirects to IdP with state parameter
2. IdP redirects back to `GET /auth/oidc/callback?code=...&state=...` — always the server's own callback (`{PUBLIC_URL}/auth/oidc/callback`, `OidcFlowState.redirect_uri`), also for desktop
3. Exchange code for tokens, fetch userinfo, create/update user
4. Return the result: browser → HttpOnly refresh cookie + `postMessage` page; desktop (Tauri, authorize called with `?redirect_uri=http://127.0.0.1:PORT/callback`, stored as `OidcFlowState.client_redirect_uri`, never sent to the IdP) → 307 to that loopback URL with tokens (link flow: `?linked=`/`?link_error=`). Use `oidc::client_return_url` to pick the branch. Desktop return address contract: `http://{localhost|127.0.0.1}[:port]/callback`, no userinfo/query/fragment (`oidc::is_loopback_client_redirect`). `PUBLIC_URL` comes from `Config::public_url` (trailing slash trimmed).
5. Failures after the state is validated (provider `?error=` without code → `OIDC_PROVIDER_ERROR`; code exchange / user-info failure → `OIDC_EXCHANGE_FAILED`) go back to the origin via `flow_failure_response`: desktop login `?error=CODE`, desktop link `?link_error=CODE`, browser the usual `postMessage` payload. Never provider text; unknown/invalid state → 400, no redirect. New flow kinds (e.g. re-auth `?reauth_error=`) extend this one function.

**Legacy flow states**: flow states stored before `client_redirect_uri` existed deserialize with `None` (browser). In-flight *desktop* flows from before the upgrade cannot complete and must be restarted (states expire after 10 min).

**State Parameter**: Prevents CSRF. Generated as random UUID, stored in Redis with 10min TTL (`oidc:state:{uuid}` key).

**Identity Resolution & Linking**: An OIDC login resolves the account via the `user_identities` table keyed by `(provider_slug, subject)` (the authoritative lookup; `users.external_id` is retained as the primary-identity marker only). A first-time login provisions a new account + identity row; a known identity logs in. Authenticated users can attach additional identities (`/auth/me/identities/authorize/{provider}` stamps `link_user_id` into the encrypted flow state; the callback's `handle_identity_link` binds the identity instead of logging in) and unlink them (`DELETE /auth/me/identities/{id}`, refused if it's the only login method of a passwordless account → `CANNOT_UNLINK_LAST_IDENTITY`). A conflict (identity already bound elsewhere) is reported as a generic `IDENTITY_ALREADY_LINKED` code.

### Rate Limiting Strategy

**Categories** (strictest to most permissive):
- `AuthLogin`: 5 req/60s per IP + IP blocking after 10 failures
- `AuthRegister`: 3 req/3600s per IP (prevent mass registration)
- `AuthOther`: 10 req/60s per IP (refresh, OIDC authorize)

**IP Blocking**: `check_ip_not_blocked` middleware reads `ratelimit:block:{ip}` key from Redis. If exists, return 403. Login failures increment `ratelimit:login_failures:{ip}`, block at 10 failures for 1 hour.

### Common Security Mistakes to Avoid

**DO NOT**:
- Log tokens or secrets (use `[REDACTED]` in debug output)
- Return detailed error messages (e.g., "invalid password" vs "invalid credentials")
- Skip HTTPS in production (JWT transmitted in Authorization header)
- Use `unwrap()` on crypto operations (always handle errors gracefully)
- Implement custom crypto (use vetted crates: `jsonwebtoken`, `argon2`, `totp-lite`)

**DO**:
- Validate all input (username length, email format, password strength)
- Use prepared statements (sqlx already does this)
- Rotate JWT secret on compromise (invalidates all sessions)
- Audit token expiry changes (longer = more risk)
- Test auth flows with expired/malformed tokens

### Session end (C25)

A ban (single or bulk), a single-session revoke, "revoke all other sessions", a password change with `revoke_others`, a password reset, logout and the account-deletion logout end the affected sessions right away.

- **Order:**
  1. Commit the durable change first: the ban row, the session-row deletion, the password, and the admin audit entry.
  2. Then call `revocation::end_sessions` (a list of ids) or `end_all_sessions` (every socket of the user).
- **What ending does:**
  - It denylists each session id (`canis:revoked_sid:{sid}`, TTL `jwt_access_expiry` + 60 s).
  - It publishes the internal `session_revoked` / `all_sessions_revoked` notice on `user:{id}`. Sockets consume the notice (it is never forwarded) and answer with an `error` event `SESSION_REVOKED`, then close 1008.
- **Bans:** every authenticated request and every WS handshake re-checks the ban in the DB. A Valkey problem can never undo a ban.
- **Bounded failure window:** if Valkey fails after the commit, the request answers 500 and the durable change stands.
  - The session rows are gone, so refresh is impossible.
  - Open sockets close anyway, because their pub/sub pipeline fails as well (close 1011), and no new socket can become ready while Valkey is down.
  - The residual exposure is an access token that is not denylisted, until it expires (`jwt_access_expiry`, 15 min).
  - Every ending call is idempotent, so a retry finishes the job.
- **Stable session id:** refresh rotates the session row in place, so the `sid` stays the same for the whole login. Do not change refresh back to delete-and-insert: sockets and tokens from before a refresh would then escape revocation.
- **Possible follow-up:** a durable revocation outbox (a DB row per pending end, drained to Valkey) would close the residual window.

### Re-authentication (C24)

Admin elevation and backup-code regeneration ask the caller to prove the account again (`reauth.rs`).

- **Password path:** `POST /api/admin/elevate` with `{"password": string, "mfa_code": string|null, "reason": string|null}`.
  - A current TOTP code is required when MFA is on. Backup codes are never accepted.
  - `POST /auth/mfa/backup-codes` takes `{"mfa_code": string}`, which must be a current TOTP code.
- **OIDC path (elevation only):** `GET /auth/reauth/oidc/authorize/{provider}?redirect_uri=&response=json&reason=` (bearer auth, system admin).
  - It starts a fresh provider login (`prompt=login`, `max_age=0`).
  - The flow state is encrypted, single use, with a 300 s TTL. It is bound to purpose `reauth`, the user and the token's `sid`.
  - The shared `/auth/oidc/callback` hands the flow to `complete_oidc_reauth`, which checks in this order:
    1. purpose;
    2. ID token signature, issuer, audience, expiry and nonce;
    3. `auth_time` present, with no fallback;
    4. `auth_time` at most 300 s old (30 s skew);
    5. (provider, `sub`) linked to that user;
    6. the session still live and not revoked;
    7. the user still a system admin.
  - Only then does `admin::system::record_elevation` elevate that session. It is the same record, cache and audit entry as the password path.
  - The result is handed back like the identity-link flow (`login::oidc_hand_back`):
    - desktop: a localhost redirect with `reauth=elevated&expires_at=<RFC 3339>` or `reauth_error=<CODE>`;
    - browser: a `postMessage` of `{"type": "oidc-reauth-callback", "success", "error_code", "expires_at"}`.
  - Only enabled providers with OIDC discovery qualify. Plain OAuth2 providers have no ID token and so no `auth_time`.
- **Methods:** `GET /auth/reauth/methods` (bearer) returns `{"password": bool, "mfa_required": bool, "oidc": [{"provider_id": string, "display_name": string}]}`.
  - `mfa_required` applies to the password path.
  - `oidc` lists linked, usable providers. It is empty when OIDC sign-in is off.
- **Errors:**
  - 400 `VALIDATION_ERROR` (missing field or body)
  - 400 `MFA_CODE_REQUIRED`
  - 401 `INVALID_CREDENTIALS` (wrong password or code; it never says which)
  - 400 `REAUTH_OIDC_REQUIRED` (no password, but a linked provider exists)
  - 400 `REAUTH_UNAVAILABLE` (neither a password nor a linked provider)
  - 400 `REAUTH_PROVIDER_UNAVAILABLE` (OIDC start for a provider that is not linked or not usable)
  - 429 (rate limited or locked)
  - Callback codes: `REAUTH_AUTH_TIME_MISSING`, `REAUTH_STALE`, `REAUTH_IDENTITY_MISMATCH`, `REAUTH_SESSION_INVALID`, `REAUTH_NOT_ALLOWED`, `OIDC_EXCHANGE_FAILED`. A missing or reused state is HTTP 400 `OIDC_STATE_MISMATCH`.
- **Session binding:** an elevation belongs to the access token's `sid`.
  - `require_elevated`, `/api/admin/status` and the WebSocket admin subscription look up `(user_id, session_id)`.
  - The Valkey cache key is `admin:elevated:{session_id}`.
- **Limits:**
  - `/auth/me/password`, `/auth/mfa/verify`, `/auth/mfa/disable`, `/auth/mfa/backup-codes`, `/auth/reauth/oidc/authorize/*` and `POST /api/admin/elevate` have `check_ip_not_blocked`, `AuthOther` and `rate_limit_by_user` (`with_reauth_limits`).
  - Each wrong password or code is recorded per IP and per account (`reauth:{user_id}`). Ten failures lock both for the failed-auth block time.
  - Limiter errors fail closed. The 503 mapping for Valkey errors comes with the Valkey hardening change.

### Error Handling

**AuthError** variants map to HTTP status codes in handlers:
- `Unauthorized` → 401 (invalid credentials, token expired)
- `Forbidden` → 403 (valid token, insufficient permissions)
- `MfaRequired` → 428 Precondition Required
- `Database`, `Other` → 500 Internal Server Error

**Never leak internal errors to client**. Log details server-side, return generic message.

### Testing

**Required Tests**:
- [ ] Token expiry (generate with custom `exp`, verify fails)
- [ ] Invalid signature (tamper with token)
- [ ] Password hash verification (correct vs incorrect)
- [ ] MFA code validation (current, past, future codes)
- [ ] OIDC state mismatch
- [ ] Rate limit enforcement (hit limit, get 429)

**Test Users**: Use separate database or transactions (`#[sqlx::test]`) to avoid polluting production data.

### Migration Checklist (if changing auth)

- [ ] Test with existing sessions (backwards compatibility)
- [ ] Plan token invalidation strategy (force re-login?)
- [ ] Update client-side token refresh logic
- [ ] Document breaking changes in CHANGELOG
- [ ] Review with Faramir (security implications)
