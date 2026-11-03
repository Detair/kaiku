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
- `session_issue.rs` — `issue_session_tx`: every new session (account lock, session limit, absolute cap by client class); device-key sign-in lives in `crate::approvals::device_login`
- `../cli/` + `../bin/kaiku_admin.rs` — `kaiku-admin` host CLI (enrol a key phone, remove devices, end sessions; audit rows with the system account as actor)

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
- `AuthFlow`: 60 req/60s per IP (phone setup `/auth/enrollments/*`, device-key sign-in `/auth/device-login*`); never behind `check_ip_not_blocked`, 429 `TOO_MANY_ATTEMPTS`
- `ApprovalLogin`: 30 req/60s per user (`/api/approvals/*`), 429 `TOO_MANY_ATTEMPTS`

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

### Session check (B.4 rev 5)

The committed `sessions` row is the authority for every authenticated request, every WebSocket handshake and re-check, the `?token=` download path, refresh and device-key sign-in.

- **Live** = `ended_at IS NULL AND expires_at > NOW() AND absolute_expires_at > NOW()`, read by primary key (`session_check::load_live`: session, user and ban state in one query, DC6).
- **Order:** the Valkey denylist first (`session_check::denylist`): `Hit` rejects at once; `Miss` and `Unavailable` fall through to the row. A missing, empty or unreachable denylist never makes a session valid.
- **Refusals:** no `sid` claim, no row, not live, another user's session, the system account → 401 `INVALID_TOKEN`; banned → 403 `ACCOUNT_BANNED`.
- **No positive cache** of live sessions. A cache added later must be invalidated synchronously inside the revoking transaction's commit path and needs a security review.
- **WebSocket:** the handshake runs the same check (bounded by the auth timeout; a database error closes 1013 / answers 503); the socket loop re-reads the row every `session_recheck_secs` (≤ 60 s): not live → `error SESSION_REVOKED`, close 1008; database error → close 1013.
- **Assurance** (`full` | `probation`) is stored at creation by `session_issue::issue_session_tx` and never changes. The access JWT's `assurance` claim is for client display only; the server decides from `AuthUser::session` (the row). Sessions from before the upgrade are `probation`, `{legacy}`, web, capped at `created_at + 30 d` (DC3).
- **Creation:** every sign-in path goes through `issue_session_tx` (account row lock, absolute cap by client class — web 30 d, desktop and Android 90 d — `expires_at = min(now + 7 d, absolute)`, `first_activity_at`). IP and User-Agent are stored for display and audit only; there is no GeoIP lookup.

### Session end (C25, B.4 revocation order)

A ban (single or bulk), a single-session revoke, "revoke all other sessions", a password change with `revoke_others`, a password reset, logout, account deletion (request or admin) end the affected sessions right away.

- **API:** `revocation::end_sessions_tx(conn, ctx, user_id, sids, reason)` and `end_all_sessions_tx(conn, ctx, user_id, keep, reason)`, called inside the transaction of the change after its locks and re-checks. `EndReason` is stored in `sessions.end_reason`.
- **Order inside the transaction:** `ended_at` + `end_reason` (the row is kept 30 days, DC4) → the sessions' `elevated_sessions` rows deleted and `admin:elevated:{sid}` cleared → each `sid` written to the denylist (`canis:revoked_sid:{sid}`, TTL `jwt_access_expiry` + 60 s; a Valkey error is logged and never stops the transition) → `pg_notify('kaiku_session_ended', {user_id, session_ids, all})` in chunks of ≤ 100 ids (the 8000-byte payload limit; `all` on the first) → the caller commits.
- **After commit:** `spawn_session_end_listener` (one per process, its own connection) republishes the notice as `session_revoked` / `all_sessions_revoked` on `user:{id}`; sockets answer with an `error` event `SESSION_REVOKED` and close 1008. Every socket re-checks its committed row at least every 60 s (the backstop when Valkey or the listener is down). The CLI's ends reach the server through the same `NOTIFY`.
- **Failure directions:** a rolled-back transaction leaves a denylist entry that only over-revokes until it expires (safe). A failed denylist write leaves the commit authoritative: every request reads the row.
- **Cleanup:** `cleanup_expired_sessions` deletes ended rows 30 days after `ended_at` and lapsed rows 30 days after their last expiry (their step-up grants go with them); `cleanup_finished_approvals` deletes finished challenges and ended enrolments 30 days after they ended.
- **Stable session id:** refresh rotates the session row in place, so the `sid` stays the same for the whole login. Do not change refresh back to delete-and-insert.

### Step-up and the two guards (F7, C27; replaces C24)

- **Guards** (`guards.rs`): `require_full` / the `require_full_session` layer → 403 `FULL_SESSION_REQUIRED` unless the committed session row is `full`. The step-up guard is `step_up::consume_tx(conn, auth, action, target)` → 403 `STEP_UP_REQUIRED`, called **inside the action's transaction**.
- **Grants** (`step_up_grants`): one action of the C27 registry (`StepUpAction`) and one canonical target (`StepUpTarget`: lowercase UUID, `others`, `""`; `target_hash = SHA-256(lp(target))`), ≤ 300 s (DB CHECK), single use, bound to the `sid` of a full session. `grant_tx`, `peek_tx` and `consume_tx` lock the session row and re-check it live and full (B.7 rule 4); `consume_tx` also refuses a grant whose authenticator is no longer active. Ending a session or revoking an authenticator voids its open grants (`approvals::cascade`).
- **Methods:** a passkey assertion (LP: `grant_tx(.., StepUpVia::Passkey, AuthenticatorRef::passkey(id))`) or a `device_stepup` signature (C26). Never a password, TOTP, recovery code or OIDC. `GET /auth/reauth/methods` → `{passkey, device}`.
- **Admin area:** every `/api/admin/*` route except `/status` needs a full session (`require_system_admin` checks it after the admin check; C27). Pre-upgrade elevations are deleted by the assurance migration.
- **Admin elevation:** `POST /api/admin/elevate {reason|null}` = `require_full` + `consume_tx(admin.elevate, "")` + the elevation row and audit entry (`details.method` = `step_up:device` / `step_up:passkey`) in one transaction; 15 minutes, bound to the `sid`. Old bodies with `password`/`mfa_code` are ignored (still 403 without a grant). The password/TOTP and OIDC re-auth paths, `POST /auth/reauth/oidc/authorize/{provider_id}` and its callback branch are removed.
- **Still on the old checks until L1b Task 16:** `/auth/me/password`, `/auth/mfa/*`, `/auth/mfa/backup-codes` use `reauth::verify_reauth` / `accept_totp` (TOTP replay store, per-account lockout, `with_reauth_limits`).
- **Elevation binding:** `require_elevated`, `/api/admin/status` and the WebSocket admin subscription look up `(user_id, session_id)`; the cache key is `admin:elevated:{session_id}`, dropped when the session ends. `DELETE /api/admin/elevate` de-elevates only the caller's session.

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
