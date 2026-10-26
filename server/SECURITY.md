# Security Implementation Guide

This document outlines critical security requirements and implementation guidelines for the Kaiku server.

## Overview

The Kaiku platform implements multiple layers of security:
- **Authentication**: JWT-based with refresh token rotation
- **Password Storage**: Argon2id hashing
- **Text E2EE**: Olm (1:1 DMs) + Megolm (group DMs) via vodozemac
- **Voice Security**: DTLS-SRTP (server-trusted), with future MLS support
- **Session Management**: Token hashing, expiration, cleanup

## Critical: Encryption at Rest Requirements

⚠️ **IMPORTANT**: The following sensitive data fields **MUST** be encrypted before storing in the database.

### 1. MFA Secrets (`users.mfa_secret`)

**Risk**: Database breach exposes all MFA secrets, allowing complete MFA bypass.

**Status**: ✅ **IMPLEMENTED** - Stored using AES-256-GCM encryption.

**Implementation**:
- Secrets are encrypted using `aes-gcm` before storage.
- Key is derived from `MFA_ENCRYPTION_KEY` environment variable.
- Implementation located in `src/auth/mfa_crypto.rs`.

**Key Management**:
Ensure `MFA_ENCRYPTION_KEY` (32-byte hex string) is set in the environment.
```bash
# Generate a secure 256-bit key
openssl rand -hex 32
```

### 2. Megolm Session Data (`megolm_sessions.session_data`)

**Risk**: All "encrypted" messages can be decrypted if database is breached.

**Status**: 🟡 **CLIENT-SIDE** — Megolm sessions on the client are stored encrypted in SQLCipher. Server-side Megolm session data (if any) should also be encrypted at rest.

**Implementation Required**:

```rust
// Before INSERT
let encrypted_session_data = encrypt_megolm_session(
    &session_data,
    &channel_key,
    channel_id
)?;
db::create_megolm_session(&pool, channel_id, &encrypted_session_data).await?;

// When loading
let encrypted = megolm_session.session_data;
let session_data = decrypt_megolm_session(&encrypted, &channel_key, channel_id)?;
```

**Recommended Approach**:
- Use **AES-256-GCM** with channel-specific keys
- Derive channel keys from master key + channel_id (HKDF)
- Store master key in environment variable (`MEGOLM_ENCRYPTION_KEY`)
- Rotate session keys when membership changes (forward secrecy)

**Key Derivation**:
```rust
use hkdf::Hkdf;
use sha2::Sha256;

fn derive_channel_key(master_key: &[u8], channel_id: Uuid) -> Result<[u8; 32]> {
    let hk = Hkdf::<Sha256>::new(None, master_key);
    let mut okm = [0u8; 32];
    hk.expand(channel_id.as_bytes(), &mut okm)?;
    Ok(okm)
}
```

**Important**: Megolm session rotation on user leave:
```rust
// When user leaves channel
async fn on_user_leave_channel(channel_id: Uuid, user_id: Uuid) {
    // Invalidate old session
    db::expire_megolm_sessions(&pool, channel_id).await?;

    // Broadcast session rotation event
    broadcast_session_rotation(channel_id).await?;
}
```

### 3. One-Time Keys Array Limit (`user_keys.one_time_keys`)

**Risk**: Unbounded JSONB array can grow to gigabytes, causing DoS.

**Status**: ✅ **Enforced at application layer**

**Implementation**:

```rust
const MAX_ONE_TIME_KEYS: usize = 100;

async fn add_one_time_key(
    pool: &PgPool,
    user_id: Uuid,
    new_key: &str
) -> Result<()> {
    let mut keys: Vec<String> = db::get_user_one_time_keys(pool, user_id).await?;

    // Prune old keys if at limit
    if keys.len() >= MAX_ONE_TIME_KEYS {
        keys.drain(0..keys.len() - MAX_ONE_TIME_KEYS + 1);
    }

    keys.push(new_key.to_string());
    db::update_user_one_time_keys(pool, user_id, &keys).await?;
    Ok(())
}
```

**Automatic Pruning**: Implement background job to remove keys older than 30 days.

## Security Constraints Enforced

### Database Level (Migration 20240102000000)

✅ **Encrypted messages must have nonce**
```sql
CONSTRAINT encrypted_requires_nonce CHECK (
    encrypted = FALSE OR nonce IS NOT NULL
)
```

✅ **Message content length limited to 4000 chars**
```sql
CONSTRAINT message_length CHECK (LENGTH(content) <= 4000)
```

✅ **File size limited to 100MB**
```sql
CONSTRAINT valid_file_size CHECK (
    size_bytes > 0 AND size_bytes <= 104857600
)
```

✅ **Session expiration is in the future**
```sql
CONSTRAINT valid_expiration CHECK (expires_at > created_at)
```

✅ **Email format validation**
```sql
CONSTRAINT email_format CHECK (
    email IS NULL OR
    email ~* '^[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}$'
)
```

✅ **Role color hex format**
```sql
CONSTRAINT valid_color CHECK (
    color IS NULL OR color ~ '^#[0-9A-Fa-f]{6}$'
)
```

✅ **Non-negative position values**
```sql
CONSTRAINT valid_position CHECK (position >= 0)
```

### Application Level

✅ **Message content validation** (`src/chat/messages.rs:97`)
- Length: 1-4000 characters
- Encrypted messages require nonce

✅ **File upload validation** (`src/chat/uploads.rs:235-292`)
- Size limit: Configurable (default 50MB, max 100MB)
- MIME type whitelist
- Filename sanitization

✅ **User agent truncation** (`src/auth/handlers.rs:124-137`)
- Truncated to 512 characters to prevent DoS

✅ **Username format validation** (`src/auth/handlers.rs:110`)
- Regex: `^[a-z0-9_]{3,32}$`
- Matches database constraint

✅ **@everyone permission validation** (`src/guild/roles.rs`)
- Prevents assignment of dangerous permissions (e.g., `MANAGE_GUILD`, `BAN_MEMBERS`, `ADMINISTRATOR`) to the default role
- Enforced at API level regardless of client checks

✅ **Password requirements** (`src/auth/handlers.rs:40`)
- Length: 8-128 characters
- Hashed with Argon2id before storage

## Deleted Messages and GDPR Compliance

**Current Implementation**: ✅ **GDPR Compliant** - Soft delete with content scrubbing.

**Mechanism**:
When a message is deleted, the `deleted_at` timestamp is set, and the `content` is replaced with `[deleted]`. This preserves the message ID/thread structure while removing the user's data.

```rust
// In db/queries.rs
pub async fn delete_message(pool: &PgPool, id: Uuid, user_id: Uuid) -> Result<bool> {
    let result = sqlx::query(
        "UPDATE messages
         SET deleted_at = NOW(),
             content = '[deleted]',
             nonce = NULL
         WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL"
    )
    .bind(id)
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}
```

## Presence Privacy

Who may see a user's online status, activity and custom status (friends and members of a shared server, never blocked users) is enforced per WebSocket under one lock, at the moment each event is queued. Friendship, block and server-membership changes notify the affected sockets, which stop forwarding the user at once. See `src/ws/AGENTS.md` ("Presence System").

Revoking changes (block, unfriend, leaving or being removed from a server, server deletion) record a durable *pending revocation* in Valkey **before** the database change and remove it after the change (or its error). Every socket, including one connecting meanwhile, checks these records whenever it validates who it may show. A user stays hidden from the other side while a revocation is pending and is shown again only when none is pending and a fresh read authorizes. Notices sent before and after the change make already-open sockets react at once. For a block this covers channel events too.

**Known limits (v1.0):**
- **Infrastructure failure:** the records and notices depend on Valkey. If Valkey is unreachable when a revocation starts, or a server crashes mid-operation, an observer can keep receiving that user's presence until its socket's periodic revalidation (about 60 seconds) or until it reconnects. A record left behind by a crash expires after 5 minutes.
- **Concurrency residual:** what remains only occurs with concurrent operations, connects or lock waits in the millisecond-to-second range. It affects only status and activity, never message content.
- **Pending block at connect:** a block that is still pending while a socket connects is applied to presence immediately. Hiding that user's channel messages starts only with the block's completion notice, or, in a rare case of a concurrent leave and block, only after a reconnect.
- **Very slow revoking write:** if a revoking database write waits longer than the 5-minute lifetime of its pending-revocation record, the user's status can be disclosed briefly until the write completes.

**Blocking and channel messages:** hiding a blocked user's channel messages is a user-experience feature, not a confidentiality boundary. Their messages in shared server channels remain reachable through pins and search.

## Rate Limiting

### Current Implementation

✅ **Voice channel joins** (`src/voice/rate_limit.rs`)
- In-memory per-user limits
- Prevents rapid join/leave spam

✅ **Authentication endpoints** (`src/auth/handlers.rs`)
- Redis-backed distributed rate limiting
- Protects Login, Register, and Password Reset
- Tracks failed attempts and blocks IPs

### Missing Implementation

🔴 **Message creation** - No rate limiting
- Risk: Spam flooding
- Recommendation: 10 messages per 10 seconds per user

🔴 **File uploads** - No rate limiting
- Risk: Storage exhaustion
- Recommendation: 5 uploads per minute per user

## Session Security

### Current Implementation

✅ **Token hashing**: Refresh tokens hashed with SHA256 before storage
✅ **Token rotation**: Old refresh token invalidated when refreshing
✅ **Expiration**: Automatic cleanup of expired sessions
✅ **Performance**: Index on `token_hash` for fast lookups

### Best Practices

**Session Cleanup**:
```rust
// Run daily
async fn cleanup_expired_sessions(pool: &PgPool) -> Result<u64> {
    let result = sqlx::query(
        "DELETE FROM sessions WHERE expires_at < NOW()"
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}
```

**Logout all devices**:
```rust
pub async fn logout_all_sessions(pool: &PgPool, user_id: Uuid) -> Result<()> {
    sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await?;

    Ok(())
}
```

## Monitoring and Logging

### Security Events to Log

✅ **User registration** (`auth/handlers.rs:201`)
✅ **User login** (`auth/handlers.rs:270`)
✅ **Token refresh** (`auth/handlers.rs:338`)
✅ **User logout** (`auth/handlers.rs:361`)
✅ **File upload** (`chat/uploads.rs:335-341`)

### Missing Security Logs

🔴 **Failed login attempts** - Not logged
🔴 **MFA setup/disable** - Not implemented yet
🔴 **Password changes** - Not implemented yet
🔴 **Permission changes** - Not logged
🔴 **Admin actions** (message delete, user ban) - Not logged

**Recommendation**: Create audit log table:
```sql
CREATE TABLE audit_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID REFERENCES users(id),
    action VARCHAR(64) NOT NULL,
    resource_type VARCHAR(64),
    resource_id UUID,
    ip_address INET,
    metadata JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_audit_user ON audit_log(user_id);
CREATE INDEX idx_audit_action ON audit_log(action);
CREATE INDEX idx_audit_created ON audit_log(created_at DESC);
```

## Dependency Security

### Current Approach

✅ **License compliance**: `cargo-deny` configured
✅ **Forbidden licenses**: GPL, AGPL, LGPL blocked
✅ **Allowed licenses**: MIT, Apache-2.0, BSD, ISC, Zlib, MPL-2.0

### Recommended Additions

**Audit dependencies regularly**:
```bash
# Check for known vulnerabilities
cargo audit

# Update dependencies
cargo update

# Review new dependencies
cargo deny check
```

**CI/CD Integration**:
```yaml
# .github/workflows/security.yml
- name: Security Audit
  run: |
    cargo audit
    cargo deny check licenses
    cargo deny check advisories
```

## Backup and Disaster Recovery

### Encryption Key Backup

**CRITICAL**: If MFA/Megolm encryption keys are lost, data is unrecoverable!

**Backup Strategy**:
1. Store keys in secure vault (HashiCorp Vault, AWS Secrets Manager)
2. Keep encrypted offline backup
3. Document key rotation procedure
4. Test recovery process regularly

**Key Rotation**:
```bash
# 1. Generate new key
NEW_KEY=$(openssl rand -hex 32)

# 2. Deploy code that can decrypt with both old and new key
# 3. Re-encrypt all data with new key
# 4. Remove old key from code
# 5. Archive old key securely for 90 days
```

## Penetration Testing Checklist

- [ ] SQL injection (SQLx prepared statements protect against this)
- [ ] XSS in message content (client-side sanitization required)
- [ ] CSRF (SameSite cookies + token validation)
- [ ] Session fixation (token rotation implemented)
- [ ] Brute force login (rate limiting needed)
- [ ] File upload vulnerabilities (MIME type validation, size limits)
- [ ] Path traversal (filename sanitization implemented)
- [ ] Timing attacks on password verification (Argon2 constant-time)
- [ ] JWT vulnerabilities (HS256 with strong secret, exp validation)
- [ ] WebSocket hijacking (JWT in upgrade request)
- [ ] Voice signaling attacks (channel membership verification)

## Security Roadmap

### Phase 1 (Critical - Next Sprint)
- [x] Implement MFA secret encryption
- [ ] Implement Megolm session data encryption (server-side, if applicable)
- [x] Add rate limiting for auth endpoints
- [x] Clear message content on delete (GDPR)
- [x] Add failed login tracking

### Phase 2 (High Priority)
- [x] Implement audit logging
- [ ] Add rate limiting for messages and uploads
- [x] Implement MFA verification (TOTP)
- [x] Add password reset flow
- [ ] Implement account lockout after failed attempts

### Phase 3 (Future)
- [ ] Implement E2EE voice (MLS)
- [ ] Add device fingerprinting
- [ ] Implement Content Security Policy headers
- [ ] Add security headers (HSTS, X-Frame-Options, etc.)
- [ ] Implement anomaly detection for suspicious activity

## Contact

For security issues, please email: security@voicechat.example.com

**Do not open public issues for security vulnerabilities!**
