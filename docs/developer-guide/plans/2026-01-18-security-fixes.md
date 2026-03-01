# Security Implementation Plan: Critical Vulnerability Fixes

**Date:** 2026-01-18
**Status:** Complete (except 2.2 Megolm — deferred until E2EE messaging is implemented)
**Target:** Immediate Execution (Hotfix) & Short-term Hardening

This plan outlines the steps to remediate critical security vulnerabilities identified during the code review, specifically focusing on Access Control (IDOR), Data Encryption, and Input Validation.

---

## Phase 1: Critical Access Control Fixes (Hotfix)

**Objective:** Prevent unauthorized access to channels, messages, and attachments.

### 1.1 Fix Channel IDOR in Message Handlers
**Vulnerability:** `list` and `create` handlers in `server/src/chat/messages.rs` do not verify if the user has access to the channel.
**Implementation:**
1.  **Create Helper**: In `server/src/chat/mod.rs` or `server/src/db/queries.rs`, create a reusable function `verify_channel_access(pool, channel_id, user_id)`.
    *   **Logic**:
        *   Fetch channel details.
        *   **If DM (guild_id is NULL)**: Check if `user_id` exists in `channel_members` for this channel.
        *   **If Guild Channel (guild_id is SET)**: Check if `user_id` exists in `guild_members` for the channel's guild.
        *   *(Future)*: Check `VIEW_CHANNEL` permission bit for the user in that guild/channel.
2.  **Apply to Handlers**:
    *   In `server/src/chat/messages.rs`:
        *   `list` handler: Call `verify_channel_access` before fetching messages.
        *   `create` handler: Call `verify_channel_access` before creating message.
    *   In `server/src/chat/uploads.rs`:
        *   `upload_message_with_file`: Call `verify_channel_access`.

### 1.2 Fix Attachment IDOR
**Vulnerability:** `check_attachment_access` in `server/src/db/queries.rs` ignores `user_id`, allowing global download access.
**Implementation:**
1.  **Update Query**: Modify `check_attachment_access` in `server/src/db/queries.rs`.
    *   **Logic**:
        ```sql
        SELECT m.channel_id, c.guild_id
        FROM file_attachments fa
        JOIN messages m ON fa.message_id = m.id
        JOIN channels c ON m.channel_id = c.id
        WHERE fa.id = $1
        ```
    *   Use the result to perform the same checks as Step 1.1 (DM participant or Guild Member).

---

## Phase 2: Data Protection (Encryption at Rest)

**Objective:** Encrypt sensitive secrets in the database to prevent compromise in case of a leak.

### 2.1 MFA Secret Encryption
**Context:** MFA secrets are currently stored in plaintext.
**Implementation:**
1.  **Infrastructure**: Ensure `MFA_ENCRYPTION_KEY` (32-byte hex) is loaded in `server/src/config.rs`.
2.  **Crypto Utilities**:
    *   Use `aes-gcm` crate (already in Cargo.toml).
    *   Create `server/src/auth/crypto.rs` with `encrypt_data(data, key)` and `decrypt_data(data, key)`.
3.  **Handlers**:
    *   `server/src/auth/handlers.rs`:
        *   `mfa_setup`: Encrypt secret before calling `db::set_mfa_secret`.
        *   `mfa_verify`: Decrypt secret after fetching from DB to verify TOTP.
        *   `mfa_disable`: No change needed (sets to NULL).

### 2.2 Megolm Session Encryption
**Context:** E2EE session keys stored in plaintext.
**Implementation:**
1.  Follow similar pattern to MFA secrets using `MEGOLM_ENCRYPTION_KEY`.
2.  Update `server/src/chat/e2ee.rs` (or equivalent storage handler) to encrypt/decrypt session data on Read/Write.

---

## Phase 3: Hardening & Validation

**Objective:** Prevent abuse and malicious uploads.

### 3.1 Rate Limit Registration
**Vulnerability:** `register` endpoint allows unlimited account creation.
**Implementation:**
1.  **Rate Limiter**: In `server/src/auth/handlers.rs`, inside `register` handler.
2.  **Logic**:
    ```rust
    if let Some(limiter) = &state.rate_limiter {
        // Limit: 5 per 1 hour per IP
        if !limiter.check_rate_limit("register", ip, 5, 3600).await? {
            return Err(AuthError::RateLimited);
        }
    }
    ```

### 3.2 Magic Byte Validation
**Vulnerability:** File uploads rely on client-provided MIME types.
**Implementation:**
1.  **Dependencies**: Add `infer = "0.15"` to `server/Cargo.toml`.
2.  **Update Handler**: In `server/src/chat/uploads.rs` (`upload_file` and `upload_message_with_file`):
    *   Read first 1KB of file stream (or full buffer if small).
    *   Use `infer::get(&buffer)` to detect type.
    *   Compare detected MIME type against allowed whitelist.
    *   Reject if mismatch (e.g., `image/png` header but `application/x-executable` detected).

---

## Verification Checklist

### Access Control
- [ ] Attempt to list messages in a Guild Channel I am NOT in -> Should return 403.
- [ ] Attempt to list messages in a DM I am NOT in -> Should return 403.
- [ ] Attempt to download an attachment from a private channel I am NOT in -> Should return 403.

### Data Protection
- [ ] Register new MFA. Check DB `users.mfa_secret`. Should be non-readable (encrypted blob).
- [ ] Verify MFA login still works.

### Hardening
- [ ] Spam `/auth/register` -> Should receive 429 after limit.
- [ ] Upload a fake PNG (executable renamed) -> Should be rejected.
