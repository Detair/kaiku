# Crypto Module

**Parent:** [Tauri Source](../AGENTS.md)

**Purpose:** End-to-end encryption (E2EE) for text messages using vodozemac (Olm/Megolm protocol). Provides encrypted local key storage, session management, and Tauri IPC commands for frontend integration.

## Architecture

```
Text Message (plaintext)
    ↓ CryptoManager::encrypt() / encrypt_group_message()
Encrypted Message (Olm 1:1 or Megolm Group)
    ↓ Server (stores encrypted, cannot read)
Recipient Client
    ↓ CryptoManager::decrypt() / decrypt_group_message()
Text Message (plaintext)
```

## Current State

### Implemented

| File | Purpose |
|------|---------|
| `mod.rs` | Module root — exports `CryptoManager`, `ClaimedPrekey`, `PrekeyForUpload`, `PrekeyInfo` |
| `manager.rs` | `CryptoManager` — session management, encrypt/decrypt for Olm and Megolm |
| `store.rs` | `LocalKeyStore` — encrypted SQLite storage (SQLCipher) for accounts, sessions, and metadata |

### Protocol Support

| Protocol | Use Case | Status |
|----------|----------|--------|
| **Olm** (Double Ratchet) | 1:1 DM encryption | ✅ Implemented |
| **Megolm** (Ratchet) | Group DM encryption (3+ participants) | ✅ Implemented |
| **MLS** (future) | Voice E2EE ("Paranoid Mode") | ❌ Planned |

## CryptoManager API

### Olm (1:1 Messaging)
- `init()` — Generate identity keys (Ed25519 + Curve25519), create Olm account
- `encrypt()` — Encrypt plaintext for specific recipients via Olm sessions
- `decrypt()` — Decrypt incoming Olm ciphertext
- `generate_prekeys()` — Generate one-time prekeys for session establishment
- `claim_prekey()` — Claim a one-time prekey from another user

### Megolm (Group Messaging)
- `create_outbound_group_session(room_id)` — Create new outbound Megolm session, returns exportable session key
- `encrypt_group_message(room_id, plaintext)` — Encrypt with current outbound session (ratchet advances)
- `add_inbound_group_session(room_id, sender_key, session_key)` — Store received session key
- `decrypt_group_message(room_id, sender_key, ciphertext)` — Decrypt with stored inbound session

### Key Backup & Recovery
- `create_backup()` / `restore_backup()` — AES-256-GCM encrypted key backup with Argon2id KDF
- `generate_recovery_key()` — Base58-encoded recovery key for user backup

## LocalKeyStore (store.rs)

Encrypted SQLite database using SQLCipher with Argon2id key derivation.

**Tables:**
- `olm_account` — Serialized Olm account (one per device)
- `olm_sessions` — Per-user Olm sessions (keyed by `user_id:device_id`)
- `megolm_outbound_sessions` — Outbound Megolm sessions (keyed by `room_id`)
- `megolm_inbound_sessions` — Inbound Megolm sessions (keyed by `room_id:sender_key`)
- `metadata` — Encrypted key-value store for device IDs, prekey counters, etc.

## Megolm Group Encryption Flow

### Sending (Group DM)
1. **Create session**: `create_outbound_group_session(channel_id)` → returns session key
2. **Distribute key**: Session key encrypted via Olm for each group member's device, sent as a real message
3. **Encrypt message**: `encrypt_group_message(channel_id, plaintext)` → Megolm ciphertext
4. **Send**: Message sent with `encrypted: true` flag and `MegolmE2EEContent` JSON envelope

### Receiving (Group DM)
1. **Key receipt**: Olm-encrypted session key message arrives via WebSocket
2. **Auto-process**: `processMegolmKeyIfPresent()` detects `__megolm_session_key__` marker
3. **Store**: `add_inbound_group_session(room_id, sender_key, session_key)`
4. **Decrypt**: `decrypt_group_message(room_id, sender_key, ciphertext)` → plaintext

### Session Rotation
- Sessions rotate every **100 messages** (matching Matrix protocol)
- Sessions rotate when **participant list changes**
- Session state cached in-memory (`megolmSessionCache` in `messages.ts`)

## Key Library: vodozemac

**Why vodozemac?**
- **License:** Apache 2.0 (compatible with our MIT/Apache dual license)
- **Alternative:** libsignal (AGPL-3.0) — incompatible with our license
- **Protocol:** Implements Signal's Olm/Megolm, battle-tested crypto

**Crate:** `vodozemac = "0.9"`

## Security Considerations

### Threat Model
- **Server compromise**: Attacker gains access to server DB
  - **Mitigation:** E2EE ensures server only sees ciphertext
- **Client compromise**: Attacker gains access to client device
  - **No mitigation:** E2EE cannot protect against local access
- **Network eavesdropping**: Attacker intercepts traffic
  - **Mitigation:** TLS + E2EE (defense in depth)

### Key Storage
- **Identity Keys**: Long-lived, encrypted at rest with Argon2id-derived key
- **Session Keys**: Ephemeral, rotated per session
- **One-time Keys**: Pre-generated, deleted after use
- **Local DB**: SQLCipher-encrypted SQLite

### Forward Secrecy
- **Olm:** Built-in via Double Ratchet
- **Megolm:** Forward secrecy via ratcheting (compromising a session key doesn't reveal past messages)
- **Voice DTLS-SRTP (current):** No forward secrecy (server holds keys)
- **Voice MLS (future):** Forward secrecy

## Testing

### Unit Tests (vc-crypto)
```bash
cargo test -p vc-crypto --features megolm
```
Tests: `test_megolm_encrypt_decrypt`, `test_megolm_serialization`, `test_olm_session_encrypt_decrypt`, + 21 more

### Integration Tests (store.rs)
```bash
cargo test -p client -- crypto::store
```
Tests: `test_store_account_roundtrip`, `test_store_session_roundtrip`, `test_store_persistence`, etc.

## Performance Targets

| Operation | Target | Notes |
|-----------|--------|-------|
| Key generation | <100ms | One-time on first run |
| Olm session setup | <50ms | Per new 1-to-1 conversation |
| Encrypt (Olm) | <5ms | Per message |
| Decrypt (Olm) | <5ms | Per message |
| Encrypt (Megolm) | <2ms | Per message (amortized) |
| Decrypt (Megolm) | <2ms | Per message |

## Compliance

### Licenses
- **vodozemac:** Apache-2.0 ✅
- **aes-gcm:** MIT/Apache-2.0 ✅
- **argon2:** MIT/Apache-2.0 ✅

## Common Pitfalls

### Don't Roll Your Own Crypto
- **Bad:** Implementing custom encryption protocol
- **Good:** Using vodozemac (peer-reviewed, audited)

### Key Management is Hard
- **Bad:** Storing keys in plaintext
- **Good:** SQLCipher-encrypted local storage with Argon2id KDF

### Fallback to Plaintext
- **Bad:** Disabling E2EE if keys fail to load
- **Good:** Block message send, require key recovery (current: graceful fallback with warning)

## Related Documentation

- [vc-crypto AGENTS.md](../../../../shared/vc-crypto/AGENTS.md) — Core crypto library
- [vc-crypto/src AGENTS.md](../../../../shared/vc-crypto/src/AGENTS.md) — Detailed source docs
- [Commands AGENTS.md](../commands/AGENTS.md) — Tauri crypto commands
- [Stores AGENTS.md](../../../src/stores/AGENTS.md) — Frontend e2ee.ts store
