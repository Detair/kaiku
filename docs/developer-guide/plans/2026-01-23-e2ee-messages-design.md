# E2EE Messages Design

**Date:** 2026-01-23
**Status:** Design Complete
**Phase:** 4.2 (Message Encryption - builds on E2EE Key Infrastructure)
**Depends on:** `2026-01-19-e2ee-key-backup-design.md`

## Overview

Integrate end-to-end encryption into DM message flow. Users can send and receive encrypted messages in 1:1 and Group DMs. Server stores encrypted blobs without access to plaintext.

### Scope

- 1:1 DMs - Olm sessions between two users
- Group DMs - Fan-out encryption (encrypt for each participant's devices)

### Key Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| DM scope | 1:1 and Group DMs | Full coverage per design |
| Session storage | SQLite with encryption | Cross-platform, no OS keychain complexity |
| Browser support | WASM + IndexedDB | Full feature parity |
| No-keys fallback | Block sending | Never silently fall back to plaintext |
| Key initialization | Lazy (first DM) | Only DM users get keys |
| No-prekeys fallback | Session without OTK | Less secure first msg, but still encrypted |

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         Client                                   │
├─────────────────────────────────────────────────────────────────┤
│  UI Layer (Solid.js)                                            │
│  └── DM Chat → calls encrypt/decrypt via Tauri or WASM          │
├─────────────────────────────────────────────────────────────────┤
│  Crypto Layer                                                    │
│  ├── Tauri: src-tauri/src/crypto/ (Rust, SQLite)                │
│  └── Browser: vc-crypto-wasm (WASM, IndexedDB)                  │
├─────────────────────────────────────────────────────────────────┤
│  Storage Layer                                                   │
│  ├── Tauri: encrypted SQLite at app data dir                    │
│  └── Browser: encrypted IndexedDB with password-derived key     │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                         Server                                   │
├─────────────────────────────────────────────────────────────────┤
│  /api/keys/upload      - Store identity + prekeys               │
│  /api/users/:id/keys   - Get user's device keys                 │
│  /api/users/:id/keys/claim - Atomically claim prekey            │
│  /api/messages/...     - Store encrypted message blobs          │
└─────────────────────────────────────────────────────────────────┘
```

Server never sees plaintext DM content - only stores and relays encrypted blobs.

---

## Key Store & Session Management

### LocalKeyStore Structure

```rust
struct LocalKeyStore {
    // Identity (generated once, backed up)
    account: OlmAccount,              // Contains Ed25519 + Curve25519 keys

    // Sessions (per user+device, persisted)
    sessions: HashMap<SessionKey, OlmSession>,

    // Metadata
    user_id: Uuid,
    device_id: Uuid,
    created_at: DateTime<Utc>,
}

struct SessionKey {
    user_id: Uuid,
    device_curve25519: String,  // Their device's public key
}
```

### Storage Encryption

- **Tauri (SQLite):** Encryption key derived from recovery key using Argon2id. Database at `~/.config/canis/e2ee.db`
- **Browser (IndexedDB):** User enters password on session start. Key derived via PBKDF2, stored in sessionStorage for session duration.

### Initialization Flow (Lazy)

```
First DM attempt:
1. Check if LocalKeyStore exists → if yes, load and decrypt
2. If no store exists:
   a. Generate new OlmAccount
   b. Generate 50 one-time prekeys
   c. Upload to server: POST /api/keys/upload
   d. Prompt user to save recovery key
   e. Create encrypted backup: POST /api/keys/backup
   f. Persist LocalKeyStore locally
```

---

## Message Encryption Flow

### Sending an Encrypted DM

```
User types message in DM chat → Send button clicked

1. Get recipient(s) keys:
   GET /api/users/:id/keys → Returns all devices for user

2. For each recipient device:
   a. Check if we have existing OlmSession
   b. If no session: claim prekey and create new session
      POST /api/users/:id/keys/claim { device_id }
      → Returns identity_key + one_time_prekey (or null)
   c. session.encrypt(plaintext) → EncryptedMessage

3. Build message payload:
   {
     "encrypted": true,
     "e2ee_content": {
       "sender_key": "<our Curve25519>",
       "recipients": {
         "<recipient_user_id>": {
           "<device_key>": { "type": 0|1, "body": "<ciphertext>" }
         }
       }
     }
   }

4. Send to server:
   POST /api/messages/channel/:dm_id

5. Persist updated sessions to LocalKeyStore
```

### Group DM Fan-out

For a group DM with 3 participants (excluding self), each with 2 devices = 6 encryption operations. The `recipients` map contains all 6 ciphertexts.

### Prekey Exhaustion Handling

```rust
if claimed_prekey.one_time_prekey.is_some() {
    session = account.create_outbound_session(identity_key, one_time_key);
} else {
    // Fallback: less secure first message, but still encrypted
    session = account.create_outbound_session_without_otk(identity_key);
}
```

---

## Message Decryption Flow

### Receiving an Encrypted DM (via WebSocket)

```
WebSocket event: MESSAGE_CREATE with encrypted: true

1. Parse e2ee_content, find our device's ciphertext:
   ciphertext = e2ee_content.recipients[my_user_id][my_device_key]

2. Determine message type:
   - type 0 (prekey): First message, establishes new session
   - type 1 (normal): Existing session

3. Decrypt:
   If prekey message (type 0):
     (session, plaintext) = account.create_inbound_session(
       sender_identity_key,
       prekey_message
     )
     → Save new session to LocalKeyStore

   If normal message (type 1):
     session = sessions.get(sender_key)
     plaintext = session.decrypt(ciphertext)
     → Update session state in LocalKeyStore

4. Display decrypted plaintext in UI
```

### Decryption Failures

| Scenario | Action |
|----------|--------|
| No ciphertext for our device | Show "Message not encrypted for this device" |
| Session not found (type 1) | Store as undecryptable, retry when session established |
| Decryption error | Show "Unable to decrypt message" placeholder |
| Invalid UTF-8 | Show error, log for debugging |

### Undecryptable Message Queue

```rust
struct UndecryptableMessage {
    message_id: Uuid,
    sender_key: String,
    ciphertext: EncryptedMessage,
    received_at: DateTime<Utc>,
    retry_count: u32,  // Max 3 retries, then give up
}
// Stored locally, retried when new session with sender is created
// Auto-deleted after 30 days
```

---

## Browser Support (WASM)

### Package Structure

```
shared/vc-crypto-wasm/
├── Cargo.toml          # wasm-bindgen, vc-crypto dependency
├── src/lib.rs          # WASM bindings
└── pkg/                # Built output (wasm-pack)
```

### Exposed API (wasm-bindgen)

```rust
#[wasm_bindgen]
pub struct CryptoManager {
    store: LocalKeyStore,  // In-memory, loaded from IndexedDB
}

#[wasm_bindgen]
impl CryptoManager {
    pub fn new() -> Self;
    pub fn load_from_encrypted(data: &[u8], password: &str) -> Result<Self, JsError>;
    pub fn export_encrypted(&self, password: &str) -> Vec<u8>;

    pub fn encrypt_for_recipients(&mut self, plaintext: &str, recipients: JsValue) -> JsValue;
    pub fn decrypt_message(&mut self, sender_key: &str, ciphertext: JsValue) -> Result<String, JsError>;

    pub fn get_identity_keys(&self) -> JsValue;
    pub fn generate_prekeys(&mut self, count: u32) -> JsValue;
}
```

### Browser Storage (IndexedDB)

```typescript
// client/src/lib/crypto/browser-store.ts
const DB_NAME = 'canis-e2ee';
const STORE_NAME = 'keystore';

async function saveEncryptedStore(password: string, store: CryptoManager) {
    const encrypted = store.export_encrypted(password);
    const db = await openDB(DB_NAME);
    await db.put(STORE_NAME, encrypted, 'keys');
}

async function loadStore(password: string): Promise<CryptoManager> {
    const db = await openDB(DB_NAME);
    const encrypted = await db.get(STORE_NAME, 'keys');
    return CryptoManager.load_from_encrypted(encrypted, password);
}
```

### Session Unlock Flow

```
Page load → Check IndexedDB for encrypted store
├── No store → First DM will trigger setup
└── Store exists → Prompt password modal
    └── Decrypt → Store CryptoManager in memory
    └── Optional: "Remember for session" → sessionStorage
```

---

## Prekey Management

### Initial Upload (on key generation)

```
POST /api/keys/upload
{
    "device_name": "Desktop App",
    "identity_key_ed25519": "<base64>",
    "identity_key_curve25519": "<base64>",
    "one_time_prekeys": [
        { "key_id": "AAAAAQ", "public_key": "<base64>" },
        // ... 50 prekeys total
    ]
}
```

### Replenishment Flow

```
Server → WebSocket event: PREKEY_COUNT_LOW { count: 8 }

Client receives event:
1. Generate 50 new one-time prekeys
2. Upload: POST /api/keys/upload (just the prekeys portion)
3. Mark as published locally
4. Persist updated account state
```

### Prekey Count Check (polling fallback)

```
On app startup and every 24 hours:
GET /api/keys/status → { prekey_count: 12 }

If count < 10:
  → Trigger replenishment
```

---

## UI Components

### Encryption Indicator (DM Chat Header)

```tsx
<Show when={isDM && isEncrypted()}>
  <LockIcon class="w-4 h-4 text-green-500" title="End-to-end encrypted" />
</Show>

<Show when={isDM && !isEncrypted()}>
  <LockOpenIcon class="w-4 h-4 text-yellow-500" title="Encryption not set up" />
</Show>
```

### E2EE Setup Prompt (first DM)

- Modal explaining encryption setup
- Shows recovery key after generation
- Requires user acknowledgment before continuing

### Recipient Not Ready Error

- Clear message: "@username hasn't set up encryption yet"
- Blocks sending until recipient sets up E2EE

### Browser Password Prompt

- Password field to unlock encryption
- Option to remember for session
- Fallback link to use recovery key

---

## Implementation Files

### New Files

| File | Purpose |
|------|---------|
| `shared/vc-crypto-wasm/` | WASM package with wasm-bindgen |
| `client/src-tauri/src/crypto/store.rs` | LocalKeyStore with SQLite |
| `client/src-tauri/src/crypto/session.rs` | Session management helpers |
| `client/src/lib/crypto/browser-store.ts` | IndexedDB wrapper |
| `client/src/lib/crypto/manager.ts` | Unified API (Tauri or WASM) |
| `client/src/components/chat/E2EESetupModal.tsx` | Setup prompt |
| `client/src/components/chat/EncryptionIndicator.tsx` | Lock icon |
| `client/src/components/auth/BrowserUnlockModal.tsx` | Password prompt |

### Modified Files

| File | Changes |
|------|---------|
| `client/src-tauri/src/commands/chat.rs` | Add `send_encrypted_message`, `decrypt_message` |
| `client/src-tauri/src/commands/crypto.rs` | Add `init_e2ee`, `get_e2ee_status` |
| `client/src/stores/messages.ts` | Decrypt incoming, encrypt outgoing |
| `client/src/stores/websocket.ts` | Handle PREKEY_COUNT_LOW event |
| `client/src/components/chat/MessageInput.tsx` | Call encryption before send |
| `server/src/ws/events.rs` | Add PrekeyCountLow event |
| `server/src/crypto/handlers.rs` | Add prekey count endpoint |

### Build Changes

| File | Changes |
|------|---------|
| `client/package.json` | Add wasm-pack build script |
| `client/vite.config.ts` | Configure WASM loading |

---

## Testing Strategy

### Unit Tests

- `vc-crypto-wasm`: Encrypt/decrypt roundtrip in WASM
- `LocalKeyStore`: Serialize/deserialize with encryption
- Session creation with and without prekeys

### Integration Tests

- Full flow: Send encrypted DM → Receive → Decrypt
- Multi-device: Same user, two devices, both receive
- Group DM: 3 participants, fan-out encryption
- Prekey exhaustion: Fallback session works

### E2E Tests (Playwright)

- Setup flow: First DM triggers key generation
- Cross-browser: Tauri and web client interop
- Recovery: Restore keys on new device

---

*Design validated through collaborative brainstorming session.*
