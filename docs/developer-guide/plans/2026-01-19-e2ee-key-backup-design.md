# E2EE + Key Backup Design

**Date:** 2026-01-19
**Status:** Design Complete
**Phase:** 4 (Advanced Features)

## Overview

End-to-end encryption for DMs and Group DMs using vodozemac (Olm protocol), with cross-device key backup and recovery. Guild channels remain server-encrypted since they're semi-public by nature.

### Scope

**Encrypted:**
- 1:1 DMs - Olm sessions between two users
- Group DMs - Olm sessions with each participant (fan-out encryption)

**Not encrypted (server-side):**
- Guild channels - admins can moderate, semi-public by nature

### Key Hierarchy

```
Identity Key (Ed25519)
├── Signing operations, never leaves device
└── Generates one-time prekeys

Curve25519 Key Pair
├── Used for Olm session establishment
└── New prekeys uploaded periodically

Recovery Key (256-bit)
├── Derives encryption key for identity backup
└── User saves this for disaster recovery
```

### Trust Model

- Server never sees plaintext DM content
- Server holds encrypted key backup blob (can't decrypt without recovery key)
- Device-to-device verification for adding new devices
- Recovery key as fallback when all devices lost

---

## Local Key Storage

### Primary: OS Keychain

Identity keys stored in platform-native secure storage:
- **macOS:** Keychain Services
- **Windows:** Credential Manager (DPAPI)
- **Linux:** Secret Service (libsecret) or KWallet

Tauri provides `tauri-plugin-keyring` for cross-platform access.

### Fallback: Encrypted Local Backup

Auto-generated encrypted SQLite database as safety net:
```
~/.config/canis/keys.db (Linux)
~/Library/Application Support/canis/keys.db (macOS)
%APPDATA%/canis/keys.db (Windows)
```

Encryption: AES-256-GCM with key derived from recovery key via Argon2id.

### Key Data Structure

```rust
struct LocalKeyStore {
    identity_key: Ed25519KeyPair,
    curve25519_key: Curve25519KeyPair,
    one_time_prekeys: Vec<Curve25519PublicKey>,
    olm_sessions: HashMap<(UserId, DeviceKey), SerializedOlmSession>,
    created_at: DateTime<Utc>,
    last_backup_at: Option<DateTime<Utc>>,
}
```

### Startup Flow

1. Try OS Keychain → success → done
2. Keychain fails → prompt for recovery key
3. Recovery key entered → decrypt local backup → restore to keychain

---

## Recovery Key & Backup

### Recovery Key Format

256-bit random, encoded as Base58 in 12 groups of 4 characters:
```
EsTJ 91ks N7kL 4xPm  QwRt 8vBn 2hYc 6dFj  KpLm 3nXz 9aTb 5wGr
```

No ambiguous characters (0/O, l/1/I). Easy to read aloud or write down.

### Generation

```rust
fn generate_recovery_key() -> RecoveryKey {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).unwrap();
    RecoveryKey(bytes)
}

fn format_for_display(key: &RecoveryKey) -> String {
    bs58::encode(&key.0)
        .into_string()
        .chars()
        .collect::<Vec<_>>()
        .chunks(4)
        .map(|c| c.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(" ")
}
```

### Server-side Encrypted Backup

```rust
struct EncryptedBackup {
    salt: [u8; 16],              // Random, unique per backup
    nonce: [u8; 12],             // AES-GCM nonce
    ciphertext: Vec<u8>,         // Encrypted keys
    version: u32,                // Backup version for rotation
}

fn derive_backup_key(recovery_key: &[u8; 32], salt: &[u8; 16]) -> [u8; 32] {
    argon2id(recovery_key, salt, t=3, m=64MB, p=1)
}

// Verification via magic bytes in plaintext
const MAGIC: &[u8] = b"CANIS_KEYS_V1";
```

### Backup API

```
POST /api/keys/backup
{
    "salt": "<base64>",
    "nonce": "<base64>",
    "ciphertext": "<base64>",
    "version": 1
}
```

Server stores blob but cannot decrypt.

---

## Device-to-Device Verification

### QR Code Flow (Primary)

1. **New device** shows QR code containing:
   ```json
   {
     "device_id": "uuid",
     "public_key": "<base64 Curve25519>",
     "timestamp": 1705678900
   }
   ```

2. **Existing device** scans QR, verifies timestamp (<60s old)

3. **PIN verification** (mandatory):
   ```
   New Device shows:  [QR CODE]  PIN: 7294
   Existing device:   "Transfer keys to device showing PIN: [____]"
   ```
   Both display/confirm same 4-digit PIN derived from shared secret.

4. **Existing device** encrypts identity keys:
   ```rust
   let shared_secret = x25519(my_private, their_public);
   let transfer_key = hkdf(shared_secret, "canis-device-transfer");
   let encrypted_keys = aes_gcm_encrypt(transfer_key, serialized_keys);
   ```

5. **Transfer via server** (encrypted blob):
   ```
   POST /api/keys/device-transfer
   {
     "target_device_id": "uuid",
     "encrypted_keys": "<base64>",
     "nonce": "<base64>",
     "signature": "<Ed25519>"
   }
   ```

6. **New device** polls, decrypts, verifies

### Device Registration Flow

```
1. New device: POST /api/devices/register { device_id, public_key }
   → Returns challenge
2. Existing device: POST /api/keys/device-transfer (authenticated)
   → Server marks new device as "pending verification"
3. New device decrypts keys, signs challenge
4. New device: POST /api/devices/verify { signature }
   → Server marks device as "trusted"
```

### Timeouts

- QR code valid: 60 seconds
- Transfer blob deleted: 5 minutes or after retrieval
- Polling timeout: 2 minutes with exponential backoff

### Fallback

When no existing device available:
- Prompt for recovery key
- Fetch encrypted backup from server
- Decrypt locally

---

## User Experience Flow

### After Registration Prompt

```
┌─────────────────────────────────────────────────────────┐
│  🔐 Secure Your Messages                                │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  Your messages are end-to-end encrypted. Save your      │
│  recovery key to restore them if you lose all devices.  │
│                                                         │
│  ┌───────────────────────────────────────────────────┐  │
│  │ EsTJ 91ks N7kL 4xPm  QwRt 8vBn 2hYc 6dFj        │  │
│  │ KpLm 3nXz 9aTb 5wGr                              │  │
│  └───────────────────────────────────────────────────┘  │
│                                                         │
│  [Copy to Clipboard]    [Download as File]              │
│                                                         │
│  ☐ I have saved my recovery key somewhere safe          │
│                                                         │
│             [Continue]        [Skip for Now]            │
└─────────────────────────────────────────────────────────┘
```

### Verification

- Checkbox required to enable "Continue"
- Optionally: ask user to enter 4 random characters to confirm they saved it

### Persistent Reminder (if skipped)

- Yellow banner in settings: "⚠️ Recovery key not saved"
- Badge on settings icon
- Re-prompt after 7 days, then 30 days
- Cleared once user completes backup setup

### Recovery Key Re-access

- Settings → Security → "View Recovery Key"
- Requires password confirmation before showing

---

## Olm Encryption Flow

### Prekey Upload

Users upload prekeys on registration:
```
POST /api/keys/upload
{
    "identity_key": "<Ed25519 public>",
    "curve25519_key": "<Curve25519 public>",
    "one_time_prekeys": ["<key1>", "<key2>", ...]  // 50 keys
}
```

### Starting an Encrypted DM

1. Alice fetches Bob's keys (all devices):
   ```
   GET /api/users/:bob_id/keys
   {
       "devices": [
           { "device_id": "...", "identity_key": "...", "curve25519_key": "...", "prekey": "..." },
           { "device_id": "...", "identity_key": "...", "curve25519_key": "...", "prekey": "..." }
       ]
   }
   ```

2. Alice claims prekey atomically:
   ```
   POST /api/users/:id/keys/claim
   → Returns keys AND removes one-time prekey (no race condition)
   ```

3. Alice creates Olm session, encrypts message for each device

### Message Format

```json
{
    "type": "olm",
    "sender_key": "<Curve25519>",
    "recipients": {
        "user_uuid_bob": {
            "device_key": "<Bob's Curve25519>",
            "type": 0,
            "body": "<ciphertext>"
        },
        "user_uuid_carol": {
            "device_key": "<Carol's Curve25519>",
            "type": 1,
            "body": "<ciphertext>"
        }
    }
}
```

Message type: 0 = prekey message (session establishment), 1 = normal message

### Prekey Replenishment

```rust
// Server tracks prekey count, notifies client when low
// Client uploads more when < 10 remaining

// Fallback when no one-time prekeys available:
let session = if let Some(otk) = bob_keys.one_time_prekey {
    OlmSession::create_with_prekey(..., otk)
} else {
    OlmSession::create_without_prekey(...)  // Less secure first msg
};
```

---

## Error Handling

### Key-related Errors

| Scenario | Behavior |
|----------|----------|
| Recipient has no keys | Show "User hasn't set up encryption" - message fails |
| Olm session corrupted | Delete session, establish new one with prekey |
| Decryption fails | Show "Unable to decrypt" placeholder, store for retry |
| Recovery key incorrect | 3 local attempts, then require email verification |
| Device transfer timeout | Auto-cancel after 2 min, prompt retry or recovery key |

### Undecryptable Message Storage

```rust
struct UndecryptableMessage {
    message_id: Uuid,
    ciphertext: Vec<u8>,
    sender_key: Curve25519PublicKey,
    received_at: DateTime<Utc>,
    retry_count: u32,
}
// Retry when new session established with sender
// Delete after 30 days or successful decryption
```

### Graceful Degradation

- If E2EE unavailable: refuse to send rather than send unencrypted
- Clear UI indication: 🔒 icon on encrypted DMs
- Never silently fall back to plaintext

### Secure Key Deletion

```rust
fn logout() {
    identity_key.zeroize();
    olm_sessions.values_mut().for_each(|s| s.zeroize());
    keyring.delete_all("canis")?;
    secure_delete(&keys_db_path)?;
}
```

### Rate Limits

| Endpoint | Limit |
|----------|-------|
| `/keys/claim` | 20/min per user |
| `/keys/upload` | 5/min per device |
| `/keys/device-transfer` | 3/hour per device |
| `/keys/backup` | 5/day per user |

---

## Web Browser Support (WASM)

### Architecture

```
┌─────────────────────────────────────────┐
│           Browser (Web UI)              │
├─────────────────────────────────────────┤
│  Solid.js UI                            │
│         ↓                               │
│  vc-crypto-wasm (vodozemac compiled)    │
│         ↓                               │
│  IndexedDB (encrypted key storage)      │
└─────────────────────────────────────────┘
```

### vodozemac WASM Build

```toml
# shared/vc-crypto-wasm/Cargo.toml
[lib]
crate-type = ["cdylib"]

[dependencies]
vc-crypto = { path = "../vc-crypto" }
wasm-bindgen = "0.2"
```

### Key Storage in Browser

```typescript
// Keys encrypted with password-derived key before IndexedDB storage
const dbKey = await deriveKey(password, salt);
const encryptedKeys = await encrypt(dbKey, serializedKeys);
await idb.put('keys', encryptedKeys);
```

### Session Unlock Flow

- On page load: prompt for password to unlock keys
- Keys decrypted into memory for session duration
- Optional "Remember for 24h": store derived key in sessionStorage

### Security Comparison

| Aspect | Native App | Web Browser |
|--------|------------|-------------|
| Key storage | OS Keychain (hardware-backed) | IndexedDB (encrypted) |
| Memory protection | Process isolation | JavaScript heap |
| XSS risk | None | Keys exposed if XSS |
| Recommendation | Preferred | Acceptable for convenience |

### UI Indicators

- 🔒 Native app: "End-to-end encrypted"
- 🔓 Web browser: "End-to-end encrypted (web)" with security tooltip

---

## Implementation Components

### New Backend Components

| Component | Description |
|-----------|-------------|
| `/api/keys/upload` | Upload identity + prekeys |
| `/api/keys/claim` | Atomically claim prekey |
| `/api/keys/backup` | Store/retrieve encrypted backup |
| `/api/devices/*` | Device registration and verification |
| `/api/keys/device-transfer` | Relay encrypted key transfer |
| Prekey storage | Track prekeys per device |
| Device registry | Track trusted devices per user |

### Shared Library Updates

| Component | Description |
|-----------|-------------|
| `vc-crypto` | Complete Olm implementation (replace TODOs) |
| `vc-crypto-wasm` | WASM bindings for browser |

### Client (Tauri) Components

| Component | Description |
|-----------|-------------|
| Keychain integration | OS-native key storage |
| Local key store | Encrypted SQLite backup |
| Crypto operations | Key generation, Olm sessions |

### Client (UI) Components

| Component | Description |
|-----------|-------------|
| Recovery key modal | Display, copy, download |
| Device verification | QR scanner, PIN display |
| Encryption indicators | Lock icons, security badges |
| Key management settings | View recovery key, manage devices |

---

## Not In Scope (Future)

- Guild channel encryption (Megolm)
- Voice E2EE (MLS)
- Cross-signing / trust-on-first-use UI
- Key history / message search over encrypted content
- Public key verification (fingerprint comparison)

---

*Design reviewed through collaborative brainstorming session.*
