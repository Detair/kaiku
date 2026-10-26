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
4. `reauth::accept_totp()` checks the code (30s window, ±1 step tolerance) and accepts each code once per user (Valkey replay store, shared with the re-auth paths); a replayed code falls through to the backup-code check and then answers `INVALID_MFA` (401); a replay-store failure answers 503 `SERVICE_UNAVAILABLE`

**Backup Codes**: Implemented (`backup_codes.rs`, `mfa::mfa_generate_backup_codes`) — single-use hashed codes accepted by the login MFA path.

### OIDC Integration

**Supported Providers**: Configurable in `config.oidc_providers` (array of `OidcProvider`).

**Flow**:
1. `GET /auth/oidc/authorize/:provider` redirects to IdP with state parameter
2. IdP redirects back to `GET /auth/oidc/callback?code=...&state=...` — always the server's own callback (`{PUBLIC_URL}/auth/oidc/callback`, `OidcFlowState.redirect_uri`), also for desktop
3. Exchange code for tokens, fetch userinfo, create/update user
4. Return the result: browser → HttpOnly refresh cookie + `postMessage` page; desktop (Tauri, authorize called with `?redirect_uri=http://127.0.0.1:PORT/callback?flow=<per-flow value>`, stored as `OidcFlowState.client_redirect_uri`, never sent to the IdP) → 307 to that loopback URL. Use `oidc::client_return_url` to pick the branch. Desktop return address contract: `http://{localhost|127.0.0.1}[:port]/callback`, optionally with exactly one query pair `flow=<base64url, 22–128 chars>` (handed back unchanged, so the desktop listener can recognise its own flow); no userinfo, fragment or other query (`oidc::is_loopback_client_redirect`). `PUBLIC_URL` comes from `Config::public_url` (trailing slash trimmed).
   - **Desktop starts need the app header.** A request to `/auth/oidc/authorize/{provider}` with a loopback `redirect_uri` or a `client_challenge` is a desktop start: it must carry `X-Kaiku-Client: desktop` (not in the CORS allow-list, so browsers cannot send it cross-site, and a clicked link cannot set it) and is always answered with JSON `{"url"}`, never a 307. Without the header → 400 `OIDC_CLIENT_OUTDATED`. Link and re-auth starts are bearer-authenticated and unchanged.
   - **Desktop login hands back a one-time code, never tokens.** Authorize must carry `client_challenge` (S256: unpadded base64url SHA-256 of a random verifier the desktop keeps, 43 chars; stored as `OidcFlowState.client_challenge`). Missing → 400 `OIDC_CLIENT_OUTDATED` (older app); malformed, or sent for a browser/link/reauth flow → 400 `VALIDATION_ERROR`. The callback stores `OidcHandoffGrant {user_id, slug, client_challenge, created_at}` at `oidc:handoff:{sha256(code)}` (60 s) and 307s to `…/callback?flow=…&code=<code>` with `Cache-Control: no-store` and `Referrer-Policy: no-referrer`. The session is only created on redemption.
   - `POST /auth/oidc/desktop/exchange {code, verifier}` (`check_ip_not_blocked` + `AuthOther`): `GETDEL` the grant (any attempt consumes it), check age ≤ 60 s and S256(verifier) == challenge, re-check the ban, create the session and return `AuthResponse` with `refresh_token` in the body (no cookie). Unknown/expired/used code or wrong verifier → 400 `OIDC_HANDOFF_INVALID`; banned → 403 `ACCOUNT_BANNED`; Valkey down → 503.
   - A desktop login state without a challenge (started before the upgrade) hands back `?error=OIDC_CLIENT_OUTDATED`; failing to store the grant hands back `?error=SERVICE_UNAVAILABLE`.
   - Every loopback 307 (code, errors, link, re-auth) uses `login::loopback_redirect` (`no-store`, `no-referrer`); the global security-headers middleware keeps a handler's own `Referrer-Policy`.
   - Redemption records the client IP and User-Agent on the session (no GeoIP lookup).
   - Link (`?linked=`/`?link_error=`) and re-auth (`?reauth=`/`?reauth_error=`) hand-backs carry no tokens and are unchanged; they also echo `flow=`.
5. Failures after the state is validated (provider `?error=` without code → `OIDC_PROVIDER_ERROR`; code exchange / user-info failure → `OIDC_EXCHANGE_FAILED`) go back to the origin via `flow_failure_response`: desktop login `?error=CODE`, desktop link `?link_error=CODE`, browser the usual `postMessage` payload. Never provider text; unknown/invalid state → 400, no redirect. New flow kinds (e.g. re-auth `?reauth_error=`) extend this one function.

**Security boundary (desktop SSO)**: the design protects against other websites, browser history/sync and other accounts on the network. It does not protect against software already running on the user's own machine: a local process can listen on loopback ports and observe the system browser, which is the accepted boundary for loopback sign-in (RFC 8252). Within it, the code is useless without the app's verifier, single use and valid for 60 s, and the desktop accepts only its own per-flow value.

**Legacy flow states**: flow states stored before `client_redirect_uri` existed deserialize with `None` (browser). In-flight *desktop* flows from before the upgrade cannot complete and must be restarted (states expire after 10 min).

**State Parameter**: Prevents CSRF. Generated as random UUID, stored in Redis with 10min TTL (`oidc:state:{uuid}` key).

**Identity Resolution & Linking**: An OIDC login resolves the account via the `user_identities` table keyed by `(provider_slug, subject)` (the authoritative lookup; `users.external_id` is retained as the primary-identity marker only). A first-time login provisions a new account + identity row; a known identity logs in. Authenticated users can attach additional identities (`/auth/me/identities/authorize/{provider}` stamps `OidcFlowPurpose::Link {user_id, session_id}` into the encrypted flow state; the callback's `handle_identity_link` binds the identity instead of logging in) and unlink them (`DELETE /auth/me/identities/{id}`, refused if it's the only login method of a passwordless account → `CANNOT_UNLINK_LAST_IDENTITY`). A conflict (identity already bound elsewhere) is reported as a generic `IDENTITY_ALREADY_LINKED` code.

### Rate Limiting Strategy

**Categories** (strictest to most permissive):
- `AuthLogin`: 5 req/60s per IP + IP blocking after 10 failures
- `AuthRegister`: 3 req/3600s per IP (prevent mass registration)
- `AuthOther`: 40 req/60s per IP by default (refresh, OIDC authorize/callback, desktop hand-off exchange)

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
  - When MFA is on, a current TOTP code is required. Backup codes are never accepted.
  - `POST /auth/mfa/backup-codes` takes `{"mfa_code": string}`, which must be a current TOTP code.
- **TOTP replay:** `accept_totp` accepts each 30 s step once per user (`mfa:totp_step:{user_id}`, 90 s TTL).
  - It is used by elevation, the OIDC start, backup-code regeneration and `/auth/mfa/verify` (also behind `/auth/mfa/disable`).
  - A replayed code counts as a wrong code.
  - If Valkey errors, the request gets 503 `SERVICE_UNAVAILABLE` (fail closed).
- **OIDC path (elevation only):** `POST /auth/reauth/oidc/authorize/{provider_id}` (bearer, system admin) with the JSON body `{"redirect_uri": string|null, "reason": string|null, "mfa_code": string|null}`.
  - It answers 200 `{"url": string}` (the provider URL to open); there is no redirect variant.
  - Secrets never go into a URL. A missing or invalid body is 400 `VALIDATION_ERROR`; `reason` has a 255-character limit.
  - When MFA is on, `mfa_code` (TOTP) is verified here, with the same lockout and replay protection. The result is bound into the flow state. `amr` never replaces it.
  - The provider is asked for a fresh login (`prompt=login`, `max_age=0`); normal login flows do not get these parameters.
  - The provider always redirects to `{PUBLIC_URL}/auth/oidc/callback` (`Config::public_url`, trailing slash ignored).
  - A desktop loopback `redirect_uri` is stored as `client_redirect_uri` and used only for the hand-back.
  - The flow state is encrypted and single use (300 s TTL). `OidcFlowState.purpose` is one `OidcFlowPurpose` (`Login`, `Link {user_id, session_id}` or `Reauth(binding)`), so link and reauth exclude each other.
  - Legacy states deserialize as before: a bare `link_user_id` becomes `Link`, nothing becomes `Login`.
  - The binding holds the user, the token's `sid`, the reason and `mfa_verified`.
  - The callback dispatches a reauth state before any login or link work. `complete_oidc_reauth` then checks, in this order:
    1. ID token signature, issuer, audience, expiry and nonce;
    2. `auth_time` present, with no fallback;
    3. `auth_time` at most 300 s old and at most 30 s in the future;
    4. (provider, `sub`) linked to that user;
    5. the session still live and not revoked (fail closed);
    6. the identity eligible for that session (see below);
    7. the user still a system admin;
    8. TOTP verified at the start when MFA is on.
  - Only then does `admin::system::record_elevation` elevate that session. It writes the same row, cache and audit entry as the password path.
  - Hand-back works like the identity-link flow (`login::oidc_hand_back`):
    - desktop: a 307 to `client_redirect_uri` with `reauth=elevated&expires_at=<RFC 3339>` or `reauth_error=<CODE>`;
    - browser: a `postMessage` of `{"type": "oidc-reauth-callback", "success", "error_code", "expires_at"}`.
    - a provider error (callback without `code`) and server faults also hand back, as `reauth_error=OIDC_PROVIDER_ERROR` / `REAUTH_FAILED` (`reauth::reauth_failure_response`, used by `login::flow_failure_response` for reauth states).
- **Eligible identity:** linked no later than the session's original start (`sessions.created_at`, stable across refresh), or linked from an elevated session (`user_identities.linked_while_elevated`).
  - An identity linked in a non-elevated session can confirm only sessions that start later.
  - **Admin links need elevation:** a system admin may link an identity only from an elevated session. `GET /auth/me/identities/authorize/{provider}` answers 403 `elevation_required` (same body as the admin middleware) otherwise; non-admins are unaffected. Without this, a stolen non-elevated admin token could link an attacker's identity, sign in through it (a new, later session) and elevate from there. With it, every identity an admin links carries `linked_while_elevated`.
  - **Link callback re-checks the starting session** (`identities::link_refusal`, before anything is inserted). Refusals go back through the link hand-back (`?link_error=` / `oidc-link-callback`):
    - `LINK_SESSION_INVALID`: the state names no session (legacy `link_user_id` states), or the session is missing, expired or denylisted;
    - `SERVICE_UNAVAILABLE`: the denylist cannot be read (fail closed);
    - `ACCOUNT_BANNED`: the account is banned;
    - `elevation_required`: a system admin's session is no longer elevated.
  - "Usable" means: an enabled provider with OIDC discovery (plain OAuth2 has no ID token, so no `auth_time`), and OIDC sign-in allowed.
- **Methods:** `GET /auth/reauth/methods` (bearer) returns `{"password": bool, "mfa_required": bool, "oidc": [{"provider_id": string, "display_name": string}]}`.
  - `mfa_required` applies to both paths.
  - `oidc` lists the usable, eligible providers for the calling session.
- **Audit:** `details.method` names the factors actually verified: `password`, `password+totp`, `oidc` or `oidc+totp`.
- **Errors:**
  - 400 `VALIDATION_ERROR` (missing field or body)
  - 400 `MFA_CODE_REQUIRED` (both paths)
  - 401 `INVALID_CREDENTIALS` (wrong or replayed password or code; it never says which)
  - 400 `REAUTH_OIDC_REQUIRED` (no password, but a usable, eligible provider exists)
  - 400 `REAUTH_UNAVAILABLE` (neither)
  - 400 `REAUTH_PROVIDER_UNAVAILABLE` (OIDC start for a provider that is not linked, not usable or not eligible)
  - 403 (OIDC start by a non-admin; `GET` on the start path is 405)
  - 429 (rate limited or locked)
  - 503 `SERVICE_UNAVAILABLE` (replay store, re-auth limiter or session denylist unavailable)
- **Callback codes:**
  - `REAUTH_AUTH_TIME_MISSING`
  - `REAUTH_STALE`
  - `REAUTH_IDENTITY_MISMATCH`
  - `REAUTH_SESSION_INVALID`
  - `REAUTH_IDENTITY_NOT_ELIGIBLE`
  - `REAUTH_NOT_ALLOWED`
  - `MFA_CODE_REQUIRED`
  - `OIDC_EXCHANGE_FAILED` (bad signature, wrong nonce, failed exchange)
  - `OIDC_PROVIDER_ERROR` (the provider returned an error instead of a code)
  - `REAUTH_FAILED` (server fault, logged)
  - A missing or reused state is HTTP 400 `OIDC_STATE_MISMATCH`.
- **Session binding:** an elevation belongs to the access token's `sid`. It comes from the flow state, never from cookies.
  - `require_elevated`, `/api/admin/status` and the WebSocket admin subscription look up `(user_id, session_id)`.
  - The Valkey cache key is `admin:elevated:{session_id}`. Ending a session (`revocation::end_sessions` / `end_all_sessions`) deletes it too (best effort, logged).
  - `DELETE /api/admin/elevate` de-elevates only the caller's session.
- **Limits:**
  - These routes have `check_ip_not_blocked`, `AuthOther` and `rate_limit_by_user` (`with_reauth_limits`): `/auth/me/password`, `/auth/mfa/verify`, `/auth/mfa/disable`, `/auth/mfa/backup-codes`, `/auth/qr/create`, `/auth/reauth/oidc/authorize/*` and `POST /api/admin/elevate`.
  - The login MFA step is part of `/auth/login` (`AuthLogin`, with IP blocking).
  - Each wrong password or code is recorded per IP and per account (`reauth:{user_id}`). Ten failures lock both, including the OIDC start.
  - Limiter errors fail closed with 503 `SERVICE_UNAVAILABLE` (`RateLimiter::allow_during_outage(FailedAuth)`, so `RATE_LIMIT_FAIL_OPEN=true` opens them like the other auth limits).

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
