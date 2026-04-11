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

**Backup Codes**: Not yet implemented. TODO: Generate 10 single-use backup codes on MFA setup.

### OIDC Integration

**Supported Providers**: Configurable in `config.oidc_providers` (array of `OidcProvider`).

**Flow**:
1. `GET /auth/oidc/authorize/:provider` redirects to IdP with state parameter
2. IdP redirects back to `GET /auth/oidc/callback?code=...&state=...`
3. Exchange code for tokens, fetch userinfo, create/update user
4. Return access + refresh tokens

**State Parameter**: Prevents CSRF. Generated as random UUID, stored in Redis with 10min TTL (`oidc:state:{uuid}` key).

**User Linking**: If email matches existing user, link OIDC identity. Otherwise create new user with `oidc:{provider}:{sub}` as username.

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
