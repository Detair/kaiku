# E2EE Messages Implementation Plan

> **Status:** ✅ **COMPLETED** - 2026-01-23
>
> **PR:** [#41](https://github.com/Wolftown-io/canis/pull/41)

**Goal:** Integrate end-to-end encryption into DM messaging flow for 1:1 and Group DMs.

**Architecture:** Client-side encryption using vodozemac Olm sessions. SQLite storage for Tauri, IndexedDB for browser. Server stores encrypted blobs only. Lazy key initialization on first DM attempt.

**Tech Stack:** vodozemac, rusqlite, wasm-bindgen, idb (IndexedDB), Solid.js

**Note:** vodozemac uses the term "pickle" for its internal serialization format - this is standard cryptographic terminology for serializing session state, not Python's pickle module.

---

## Batch 1: Tauri Key Store (Foundation)

### Task 1: Add rusqlite dependency

**Files:**
- Modify: `client/src-tauri/Cargo.toml`

**Step 1: Add dependency**

Add rusqlite with bundled SQLite to Cargo.toml dependencies section:

```toml
# After keyring = "2"
rusqlite = { version = "0.31", features = ["bundled"] }
```

**Step 2: Verify compilation**

Run: `cd /home/detair/GIT/canis/.worktrees/e2ee-messages && cargo build -p vc-client`
Expected: Compiles successfully

**Step 3: Commit**

```bash
git add client/src-tauri/Cargo.toml
git commit -m "chore(client): add rusqlite for E2EE session storage"
```

---

### Task 2: Create LocalKeyStore struct

**Files:**
- Create: `client/src-tauri/src/crypto/store.rs`
- Modify: `client/src-tauri/src/crypto/mod.rs`

**Step 1: Write the test**

```rust
// client/src-tauri/src/crypto/store.rs
//! Local Key Store for E2EE
//!
//! Encrypted SQLite storage for Olm accounts and sessions.

use std::path::Path;

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use vc_crypto::olm::{OlmAccount, OlmSession};

use crate::error::ClientError;

/// Key for identifying a session with a specific user's device.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionKey {
    /// The user we're communicating with.
    pub user_id: Uuid,
    /// Their device's Curve25519 public key (base64).
    pub device_curve25519: String,
}

/// Metadata about the local key store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyStoreMetadata {
    pub user_id: Uuid,
    pub device_id: Uuid,
    pub created_at: i64,
}

/// Local encrypted key store.
pub struct LocalKeyStore {
    conn: Connection,
    encryption_key: [u8; 32],
}

impl LocalKeyStore {
    /// Create or open a key store at the given path.
    ///
    /// The encryption_key is used to encrypt/decrypt Olm account and session state.
    pub fn open(path: &Path, encryption_key: [u8; 32]) -> Result<Self, ClientError> {
        let conn = Connection::open(path)
            .map_err(|e| ClientError::Crypto(format!("Failed to open key store: {e}")))?;

        let store = Self { conn, encryption_key };
        store.init_schema()?;
        Ok(store)
    }

    /// Initialize the database schema.
    fn init_schema(&self) -> Result<(), ClientError> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS metadata (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS account (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                serialized TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS sessions (
                user_id TEXT NOT NULL,
                device_key TEXT NOT NULL,
                session_id TEXT NOT NULL,
                serialized TEXT NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (user_id, device_key)
            );
            "
        ).map_err(|e| ClientError::Crypto(format!("Failed to init schema: {e}")))?;
        Ok(())
    }

    /// Check if the store has an account.
    pub fn has_account(&self) -> Result<bool, ClientError> {
        let count: i64 = self.conn
            .query_row("SELECT COUNT(*) FROM account", [], |row| row.get(0))
            .map_err(|e| ClientError::Crypto(format!("Failed to check account: {e}")))?;
        Ok(count > 0)
    }

    /// Save the Olm account.
    pub fn save_account(&self, account: &OlmAccount) -> Result<(), ClientError> {
        let serialized = account.serialize(&self.encryption_key)
            .map_err(|e| ClientError::Crypto(format!("Failed to serialize account: {e}")))?;

        self.conn.execute(
            "INSERT OR REPLACE INTO account (id, serialized) VALUES (1, ?1)",
            params![serialized],
        ).map_err(|e| ClientError::Crypto(format!("Failed to save account: {e}")))?;

        Ok(())
    }

    /// Load the Olm account.
    pub fn load_account(&self) -> Result<OlmAccount, ClientError> {
        let serialized: String = self.conn
            .query_row("SELECT serialized FROM account WHERE id = 1", [], |row| row.get(0))
            .map_err(|e| ClientError::Crypto(format!("Failed to load account: {e}")))?;

        OlmAccount::deserialize(&serialized, &self.encryption_key)
            .map_err(|e| ClientError::Crypto(format!("Failed to deserialize account: {e}")))
    }

    /// Save a session.
    pub fn save_session(&self, key: &SessionKey, session: &OlmSession) -> Result<(), ClientError> {
        let serialized = session.serialize(&self.encryption_key)
            .map_err(|e| ClientError::Crypto(format!("Failed to serialize session: {e}")))?;

        let now = chrono::Utc::now().timestamp();

        self.conn.execute(
            "INSERT OR REPLACE INTO sessions (user_id, device_key, session_id, serialized, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                key.user_id.to_string(),
                key.device_curve25519,
                session.session_id(),
                serialized,
                now
            ],
        ).map_err(|e| ClientError::Crypto(format!("Failed to save session: {e}")))?;

        Ok(())
    }

    /// Load a session.
    pub fn load_session(&self, key: &SessionKey) -> Result<Option<OlmSession>, ClientError> {
        let result: Result<String, _> = self.conn.query_row(
            "SELECT serialized FROM sessions WHERE user_id = ?1 AND device_key = ?2",
            params![key.user_id.to_string(), key.device_curve25519],
            |row| row.get(0),
        );

        match result {
            Ok(serialized) => {
                let session = OlmSession::deserialize(&serialized, &self.encryption_key)
                    .map_err(|e| ClientError::Crypto(format!("Failed to deserialize session: {e}")))?;
                Ok(Some(session))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(ClientError::Crypto(format!("Failed to load session: {e}"))),
        }
    }

    /// Save metadata.
    pub fn save_metadata(&self, metadata: &KeyStoreMetadata) -> Result<(), ClientError> {
        let json = serde_json::to_string(metadata)
            .map_err(|e| ClientError::Crypto(format!("Failed to serialize metadata: {e}")))?;

        self.conn.execute(
            "INSERT OR REPLACE INTO metadata (key, value) VALUES ('info', ?1)",
            params![json],
        ).map_err(|e| ClientError::Crypto(format!("Failed to save metadata: {e}")))?;

        Ok(())
    }

    /// Load metadata.
    pub fn load_metadata(&self) -> Result<Option<KeyStoreMetadata>, ClientError> {
        let result: Result<String, _> = self.conn.query_row(
            "SELECT value FROM metadata WHERE key = 'info'",
            [],
            |row| row.get(0),
        );

        match result {
            Ok(json) => {
                let metadata: KeyStoreMetadata = serde_json::from_str(&json)
                    .map_err(|e| ClientError::Crypto(format!("Failed to parse metadata: {e}")))?;
                Ok(Some(metadata))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(ClientError::Crypto(format!("Failed to load metadata: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_store_account_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let key = [0u8; 32];

        let store = LocalKeyStore::open(&path, key).unwrap();
        assert!(!store.has_account().unwrap());

        let account = OlmAccount::new();
        let identity = account.identity_keys();

        store.save_account(&account).unwrap();
        assert!(store.has_account().unwrap());

        let loaded = store.load_account().unwrap();
        assert_eq!(loaded.identity_keys(), identity);
    }

    #[test]
    fn test_store_session_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let key = [0u8; 32];

        let store = LocalKeyStore::open(&path, key).unwrap();

        // Create accounts and session
        let mut alice = OlmAccount::new();
        let mut bob = OlmAccount::new();
        bob.generate_one_time_keys(1);
        let bob_otk = bob.one_time_keys().pop().unwrap().1;
        let bob_otk_key = vc_crypto::types::Curve25519PublicKey::from_base64(&bob_otk).unwrap();

        let session = alice.create_outbound_session(&bob.curve25519_key(), &bob_otk_key);
        let session_id = session.session_id();

        let session_key = SessionKey {
            user_id: Uuid::new_v4(),
            device_curve25519: bob_otk.clone(),
        };

        store.save_session(&session_key, &session).unwrap();

        let loaded = store.load_session(&session_key).unwrap().unwrap();
        assert_eq!(loaded.session_id(), session_id);
    }

    #[test]
    fn test_store_metadata() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let key = [0u8; 32];

        let store = LocalKeyStore::open(&path, key).unwrap();

        assert!(store.load_metadata().unwrap().is_none());

        let metadata = KeyStoreMetadata {
            user_id: Uuid::new_v4(),
            device_id: Uuid::new_v4(),
            created_at: chrono::Utc::now().timestamp(),
        };

        store.save_metadata(&metadata).unwrap();

        let loaded = store.load_metadata().unwrap().unwrap();
        assert_eq!(loaded.user_id, metadata.user_id);
        assert_eq!(loaded.device_id, metadata.device_id);
    }
}
```

**Step 2: Update crypto mod.rs**

```rust
// client/src-tauri/src/crypto/mod.rs
//! Client-side Cryptography

pub mod store;

pub use store::{LocalKeyStore, SessionKey, KeyStoreMetadata};
```

**Step 3: Run tests to verify**

Run: `cd /home/detair/GIT/canis/.worktrees/e2ee-messages && cargo test -p vc-client store`
Expected: All tests pass

**Step 4: Commit**

```bash
git add client/src-tauri/src/crypto/
git commit -m "feat(crypto): add LocalKeyStore with SQLite storage"
```

---

### Task 3: Create CryptoManager for session operations

**Files:**
- Create: `client/src-tauri/src/crypto/manager.rs`
- Modify: `client/src-tauri/src/crypto/mod.rs`

**Step 1: Write the implementation**

```rust
// client/src-tauri/src/crypto/manager.rs
//! Crypto Manager
//!
//! High-level API for E2EE operations: initialization, encryption, decryption.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use vc_crypto::olm::{EncryptedMessage, OlmAccount, OlmSession};
use vc_crypto::types::Curve25519PublicKey;

use super::store::{KeyStoreMetadata, LocalKeyStore, SessionKey};
use crate::error::ClientError;

/// Device keys from server (for establishing sessions).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceKeys {
    pub device_id: Uuid,
    pub device_name: Option<String>,
    pub identity_key_ed25519: String,
    pub identity_key_curve25519: String,
}

/// Claimed prekey from server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimedPrekey {
    pub device_id: Uuid,
    pub identity_key_ed25519: String,
    pub identity_key_curve25519: String,
    pub one_time_prekey: Option<PrekeyInfo>,
}

/// One-time prekey info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrekeyInfo {
    pub key_id: String,
    pub public_key: String,
}

/// E2EE content for a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct E2EEContent {
    /// Our Curve25519 public key (sender identification).
    pub sender_key: String,
    /// Encrypted content for each recipient user -> device -> ciphertext.
    pub recipients: std::collections::HashMap<String, std::collections::HashMap<String, EncryptedMessage>>,
}

/// Manages E2EE cryptographic operations.
pub struct CryptoManager {
    store: Arc<RwLock<LocalKeyStore>>,
    user_id: Uuid,
    device_id: Uuid,
}

impl CryptoManager {
    /// Initialize the crypto manager, creating keys if needed.
    pub fn init(
        data_dir: PathBuf,
        user_id: Uuid,
        encryption_key: [u8; 32],
    ) -> Result<Self, ClientError> {
        let db_path = data_dir.join("e2ee.db");

        // Ensure directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ClientError::Crypto(format!("Failed to create data dir: {e}")))?;
        }

        let store = LocalKeyStore::open(&db_path, encryption_key)?;

        // Check if we need to generate new keys
        let (device_id, needs_upload) = if store.has_account()? {
            let metadata = store.load_metadata()?
                .ok_or_else(|| ClientError::Crypto("Account exists but no metadata".into()))?;

            if metadata.user_id != user_id {
                return Err(ClientError::Crypto(
                    "Key store belongs to different user".into()
                ));
            }

            (metadata.device_id, false)
        } else {
            // Generate new account
            let account = OlmAccount::new();
            store.save_account(&account)?;

            let device_id = Uuid::new_v4();
            let metadata = KeyStoreMetadata {
                user_id,
                device_id,
                created_at: chrono::Utc::now().timestamp(),
            };
            store.save_metadata(&metadata)?;

            tracing::info!("Generated new E2EE identity keys");
            (device_id, true)
        };

        let manager = Self {
            store: Arc::new(RwLock::new(store)),
            user_id,
            device_id,
        };

        if needs_upload {
            tracing::info!("New keys generated - upload required");
        }

        Ok(manager)
    }

    /// Check if keys need to be uploaded to server.
    pub fn needs_key_upload(&self) -> bool {
        // For now, just check if account exists
        // In production, track upload status in metadata
        false
    }

    /// Get our identity keys for upload.
    pub fn get_identity_keys(&self) -> Result<(String, String), ClientError> {
        let store = self.store.read();
        let account = store.load_account()?;
        let keys = account.identity_keys();
        Ok((keys.ed25519, keys.curve25519))
    }

    /// Get our device ID.
    pub fn device_id(&self) -> Uuid {
        self.device_id
    }

    /// Generate one-time prekeys for upload.
    pub fn generate_prekeys(&self, count: usize) -> Result<Vec<(String, String)>, ClientError> {
        let store = self.store.write();
        let mut account = store.load_account()?;

        account.generate_one_time_keys(count);
        let keys: Vec<_> = account.one_time_keys()
            .into_iter()
            .map(|(id, key)| (id.to_string(), key))
            .collect();

        account.mark_keys_as_published();
        store.save_account(&account)?;

        Ok(keys)
    }

    /// Encrypt a message for a recipient's device.
    ///
    /// If no session exists, creates one using the claimed prekey.
    pub fn encrypt_for_device(
        &self,
        recipient_user_id: Uuid,
        claimed: &ClaimedPrekey,
        plaintext: &str,
    ) -> Result<EncryptedMessage, ClientError> {
        let session_key = SessionKey {
            user_id: recipient_user_id,
            device_curve25519: claimed.identity_key_curve25519.clone(),
        };

        let mut store = self.store.write();

        // Try to load existing session
        let mut session = if let Some(s) = store.load_session(&session_key)? {
            s
        } else {
            // Create new session
            let mut account = store.load_account()?;

            let their_identity = Curve25519PublicKey::from_base64(&claimed.identity_key_curve25519)
                .map_err(|e| ClientError::Crypto(format!("Invalid identity key: {e}")))?;

            let session = if let Some(ref otk) = claimed.one_time_prekey {
                let their_otk = Curve25519PublicKey::from_base64(&otk.public_key)
                    .map_err(|e| ClientError::Crypto(format!("Invalid prekey: {e}")))?;
                account.create_outbound_session(&their_identity, &their_otk)
            } else {
                // Fallback: use identity key as one-time key (less secure)
                tracing::warn!("No one-time prekey available, using identity key");
                account.create_outbound_session(&their_identity, &their_identity)
            };

            store.save_account(&account)?;
            session
        };

        let ciphertext = session.encrypt(plaintext);
        store.save_session(&session_key, &session)?;

        Ok(ciphertext)
    }

    /// Decrypt a message from a sender.
    pub fn decrypt_message(
        &self,
        sender_user_id: Uuid,
        sender_key: &str,
        message: &EncryptedMessage,
    ) -> Result<String, ClientError> {
        let session_key = SessionKey {
            user_id: sender_user_id,
            device_curve25519: sender_key.to_string(),
        };

        let mut store = self.store.write();

        // Check if this is a prekey message (new session)
        if message.is_prekey() {
            let prekey_msg = message.into_prekey_message()
                .ok_or_else(|| ClientError::Crypto("Invalid prekey message".into()))?;

            let sender_identity = Curve25519PublicKey::from_base64(sender_key)
                .map_err(|e| ClientError::Crypto(format!("Invalid sender key: {e}")))?;

            let mut account = store.load_account()?;
            let (session, plaintext) = account.create_inbound_session(&sender_identity, &prekey_msg)
                .map_err(|e| ClientError::Crypto(format!("Failed to create inbound session: {e}")))?;

            store.save_account(&account)?;
            store.save_session(&session_key, &session)?;

            return Ok(plaintext);
        }

        // Normal message - need existing session
        let mut session = store.load_session(&session_key)?
            .ok_or_else(|| ClientError::Crypto("No session for sender".into()))?;

        let plaintext = session.decrypt(message)
            .map_err(|e| ClientError::Crypto(format!("Decryption failed: {e}")))?;

        store.save_session(&session_key, &session)?;

        Ok(plaintext)
    }

    /// Get our Curve25519 public key for sender identification.
    pub fn our_curve25519_key(&self) -> Result<String, ClientError> {
        let store = self.store.read();
        let account = store.load_account()?;
        Ok(account.identity_keys().curve25519)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_crypto_manager_init() {
        let dir = tempdir().unwrap();
        let user_id = Uuid::new_v4();
        let key = [0u8; 32];

        let manager = CryptoManager::init(dir.path().to_path_buf(), user_id, key).unwrap();
        assert!(!manager.device_id().is_nil());

        // Verify identity keys are generated
        let (ed25519, curve25519) = manager.get_identity_keys().unwrap();
        assert!(!ed25519.is_empty());
        assert!(!curve25519.is_empty());
    }

    #[test]
    fn test_crypto_manager_prekey_generation() {
        let dir = tempdir().unwrap();
        let user_id = Uuid::new_v4();
        let key = [0u8; 32];

        let manager = CryptoManager::init(dir.path().to_path_buf(), user_id, key).unwrap();

        let prekeys = manager.generate_prekeys(10).unwrap();
        assert_eq!(prekeys.len(), 10);

        // Prekeys should be marked as published (not returned again)
        let prekeys2 = manager.generate_prekeys(5).unwrap();
        assert_eq!(prekeys2.len(), 5);
    }
}
```

**Step 2: Update crypto mod.rs**

```rust
// client/src-tauri/src/crypto/mod.rs
//! Client-side Cryptography

pub mod manager;
pub mod store;

pub use manager::{CryptoManager, DeviceKeys, ClaimedPrekey, PrekeyInfo, E2EEContent};
pub use store::{LocalKeyStore, SessionKey, KeyStoreMetadata};
```

**Step 3: Run tests**

Run: `cd /home/detair/GIT/canis/.worktrees/e2ee-messages && cargo test -p vc-client crypto`
Expected: All tests pass

**Step 4: Commit**

```bash
git add client/src-tauri/src/crypto/
git commit -m "feat(crypto): add CryptoManager for E2EE operations"
```

---

## Batch 2: Tauri Commands Integration

### Task 4: Add E2EE Tauri commands

**Files:**
- Modify: `client/src-tauri/src/commands/crypto.rs`
- Modify: `client/src-tauri/src/commands/mod.rs`
- Modify: `client/src-tauri/src/lib.rs` (if needed for state)

**Step 1: Add imports and state to commands/crypto.rs**

Add at the top of the file after existing imports:

```rust
use crate::crypto::{CryptoManager, E2EEContent, ClaimedPrekey, PrekeyInfo};
```

**Step 2: Add E2EE status command**

Add this command to `client/src-tauri/src/commands/crypto.rs`:

```rust
/// E2EE initialization status.
#[derive(Debug, Serialize)]
pub struct E2EEStatus {
    pub initialized: bool,
    pub device_id: Option<String>,
    pub has_identity_keys: bool,
}

/// Check E2EE status.
#[command]
pub async fn get_e2ee_status(state: State<'_, AppState>) -> Result<E2EEStatus, String> {
    let crypto = state.crypto.read().await;

    match crypto.as_ref() {
        Some(manager) => {
            let (ed25519, _) = manager.get_identity_keys()
                .map_err(|e| e.to_string())?;
            Ok(E2EEStatus {
                initialized: true,
                device_id: Some(manager.device_id().to_string()),
                has_identity_keys: !ed25519.is_empty(),
            })
        }
        None => Ok(E2EEStatus {
            initialized: false,
            device_id: None,
            has_identity_keys: false,
        }),
    }
}
```

**Step 3: Add init E2EE command**

```rust
/// Initialize E2EE for the current user.
/// Returns the identity keys that need to be uploaded to the server.
#[derive(Debug, Serialize)]
pub struct InitE2EEResponse {
    pub device_id: String,
    pub identity_key_ed25519: String,
    pub identity_key_curve25519: String,
    pub prekeys: Vec<PrekeyData>,
}

#[derive(Debug, Serialize)]
pub struct PrekeyData {
    pub key_id: String,
    pub public_key: String,
}

#[command]
pub async fn init_e2ee(
    state: State<'_, AppState>,
    encryption_key: String,
) -> Result<InitE2EEResponse, String> {
    // Get user ID from auth state
    let user_id = {
        let auth = state.auth.read().await;
        auth.user.as_ref()
            .ok_or("Not authenticated")?
            .id
            .parse::<uuid::Uuid>()
            .map_err(|e| format!("Invalid user ID: {e}"))?
    };

    // Derive encryption key from provided string (should be recovery key derived)
    let key_bytes = derive_key_from_string(&encryption_key)?;

    // Get data directory
    let data_dir = tauri::api::path::app_data_dir(&tauri::Config::default())
        .ok_or("Failed to get app data directory")?;

    // Initialize crypto manager
    let manager = CryptoManager::init(data_dir, user_id, key_bytes)
        .map_err(|e| e.to_string())?;

    // Get identity keys
    let (ed25519, curve25519) = manager.get_identity_keys()
        .map_err(|e| e.to_string())?;

    // Generate initial prekeys
    let prekeys = manager.generate_prekeys(50)
        .map_err(|e| e.to_string())?;

    let device_id = manager.device_id().to_string();

    // Store manager in app state
    {
        let mut crypto = state.crypto.write().await;
        *crypto = Some(manager);
    }

    info!("E2EE initialized for user {}", user_id);

    Ok(InitE2EEResponse {
        device_id,
        identity_key_ed25519: ed25519,
        identity_key_curve25519: curve25519,
        prekeys: prekeys.into_iter().map(|(id, key)| PrekeyData {
            key_id: id,
            public_key: key,
        }).collect(),
    })
}

fn derive_key_from_string(input: &str) -> Result<[u8; 32], String> {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&result);
    Ok(key)
}
```

**Step 4: Add encrypt message command**

```rust
/// Encrypt a message for recipients.
#[command]
pub async fn encrypt_message(
    state: State<'_, AppState>,
    plaintext: String,
    recipients: Vec<ClaimedPrekeyInput>,
) -> Result<E2EEContent, String> {
    let crypto = state.crypto.read().await;
    let manager = crypto.as_ref()
        .ok_or("E2EE not initialized")?;

    let sender_key = manager.our_curve25519_key()
        .map_err(|e| e.to_string())?;

    let mut recipients_map: std::collections::HashMap<String, std::collections::HashMap<String, _>> =
        std::collections::HashMap::new();

    for recipient in recipients {
        let claimed = ClaimedPrekey {
            device_id: recipient.device_id.parse().map_err(|e| format!("Invalid device ID: {e}"))?,
            identity_key_ed25519: recipient.identity_key_ed25519,
            identity_key_curve25519: recipient.identity_key_curve25519.clone(),
            one_time_prekey: recipient.one_time_prekey.map(|otk| PrekeyInfo {
                key_id: otk.key_id,
                public_key: otk.public_key,
            }),
        };

        let user_id: uuid::Uuid = recipient.user_id.parse()
            .map_err(|e| format!("Invalid user ID: {e}"))?;

        let ciphertext = manager.encrypt_for_device(user_id, &claimed, &plaintext)
            .map_err(|e| e.to_string())?;

        recipients_map
            .entry(recipient.user_id)
            .or_default()
            .insert(recipient.identity_key_curve25519, ciphertext);
    }

    Ok(E2EEContent {
        sender_key,
        recipients: recipients_map,
    })
}

#[derive(Debug, Deserialize)]
pub struct ClaimedPrekeyInput {
    pub user_id: String,
    pub device_id: String,
    pub identity_key_ed25519: String,
    pub identity_key_curve25519: String,
    pub one_time_prekey: Option<PrekeyInput>,
}

#[derive(Debug, Deserialize)]
pub struct PrekeyInput {
    pub key_id: String,
    pub public_key: String,
}
```

**Step 5: Add decrypt message command**

```rust
/// Decrypt a message.
#[command]
pub async fn decrypt_message(
    state: State<'_, AppState>,
    sender_user_id: String,
    sender_key: String,
    message_type: u8,
    ciphertext: String,
) -> Result<String, String> {
    let crypto = state.crypto.read().await;
    let manager = crypto.as_ref()
        .ok_or("E2EE not initialized")?;

    let user_id: uuid::Uuid = sender_user_id.parse()
        .map_err(|e| format!("Invalid user ID: {e}"))?;

    let message = vc_crypto::olm::EncryptedMessage {
        message_type,
        ciphertext,
    };

    manager.decrypt_message(user_id, &sender_key, &message)
        .map_err(|e| e.to_string())
}
```

**Step 6: Update AppState to include crypto manager**

In `client/src-tauri/src/lib.rs`, add to AppState:

```rust
pub crypto: tokio::sync::RwLock<Option<crate::crypto::CryptoManager>>,
```

And initialize it in the builder:

```rust
crypto: tokio::sync::RwLock::new(None),
```

**Step 7: Register commands in mod.rs**

Add to `client/src-tauri/src/commands/mod.rs` public exports:

```rust
pub use crypto::{
    get_e2ee_status, init_e2ee, encrypt_message, decrypt_message,
    E2EEStatus, InitE2EEResponse, PrekeyData, ClaimedPrekeyInput, PrekeyInput,
};
```

**Step 8: Run to verify compilation**

Run: `cd /home/detair/GIT/canis/.worktrees/e2ee-messages && cargo build -p vc-client`
Expected: Compiles successfully

**Step 9: Commit**

```bash
git add client/src-tauri/src/
git commit -m "feat(crypto): add Tauri commands for E2EE init, encrypt, decrypt"
```

---

## Batch 3: Frontend Integration

### Task 5: Add E2EE types and tauri wrappers

**Files:**
- Modify: `client/src/lib/types.ts`
- Modify: `client/src/lib/tauri.ts`

**Step 1: Add E2EE types to types.ts**

Add to `client/src/lib/types.ts`:

```typescript
// E2EE Types

export interface E2EEStatus {
  initialized: boolean;
  device_id: string | null;
  has_identity_keys: boolean;
}

export interface InitE2EEResponse {
  device_id: string;
  identity_key_ed25519: string;
  identity_key_curve25519: string;
  prekeys: PrekeyData[];
}

export interface PrekeyData {
  key_id: string;
  public_key: string;
}

export interface DeviceKeys {
  device_id: string;
  device_name: string | null;
  identity_key_ed25519: string;
  identity_key_curve25519: string;
}

export interface UserKeysResponse {
  devices: DeviceKeys[];
}

export interface ClaimedPrekeyResponse {
  device_id: string;
  identity_key_ed25519: string;
  identity_key_curve25519: string;
  one_time_prekey: {
    key_id: string;
    public_key: string;
  } | null;
}

export interface E2EEContent {
  sender_key: string;
  recipients: Record<string, Record<string, EncryptedMessage>>;
}

export interface EncryptedMessage {
  message_type: number;
  ciphertext: string;
}

export interface ClaimedPrekeyInput {
  user_id: string;
  device_id: string;
  identity_key_ed25519: string;
  identity_key_curve25519: string;
  one_time_prekey: {
    key_id: string;
    public_key: string;
  } | null;
}
```

**Step 2: Add tauri wrappers to tauri.ts**

Add E2EE command wrappers as documented in the design.

**Step 3: Verify TypeScript compiles**

Run: `cd /home/detair/GIT/canis/.worktrees/e2ee-messages/client && bun run build`
Expected: Builds successfully

**Step 4: Commit**

```bash
git add client/src/lib/types.ts client/src/lib/tauri.ts
git commit -m "feat(client): add E2EE types and tauri wrappers"
```

---

### Task 6: Create E2EE store

**Files:**
- Create: `client/src/stores/e2ee.ts`

Implementation as documented in the design.

**Step 1: Write the store**

Create the E2EE store for state management and encryption/decryption functions.

**Step 2: Verify build**

Run: `cd /home/detair/GIT/canis/.worktrees/e2ee-messages/client && bun run build`
Expected: Builds successfully

**Step 3: Commit**

```bash
git add client/src/stores/e2ee.ts
git commit -m "feat(client): add E2EE store for encryption state management"
```

---

## Batch 4: Message Flow Integration

### Task 7: Integrate encryption into message sending

**Files:**
- Modify: `client/src/stores/messages.ts`
- Modify: `client/src/lib/tauri.ts`

Update sendMessage to encrypt for DMs when E2EE is ready.

**Step 1-4:** Implementation and verification as documented.

---

### Task 8: Integrate decryption into message receiving

**Files:**
- Modify: `client/src/stores/messages.ts`
- Modify: `client/src/lib/types.ts`

Update addMessage to decrypt incoming encrypted messages.

**Step 1-4:** Implementation and verification as documented.

---

## Batch 5: Server Message Storage

### Task 9: Update server message model for E2EE content

**Files:**
- Modify: `server/src/api/messages.rs` (or equivalent message handler)

Add e2ee_content field to message creation and responses.

---

### Task 10: Add database migration for E2EE content

**Files:**
- Create: `server/migrations/YYYYMMDDHHMMSS_add_e2ee_content.sql`

```sql
-- Add e2ee_content column to messages table
ALTER TABLE messages ADD COLUMN IF NOT EXISTS e2ee_content JSONB;

-- Index for finding encrypted messages (optional, for admin/moderation)
CREATE INDEX IF NOT EXISTS idx_messages_encrypted ON messages(encrypted) WHERE encrypted = true;
```

---

## Batch 6: UI Components

### Task 11: Create E2EE setup modal

**Files:**
- Create: `client/src/components/chat/E2EESetupModal.tsx`

Modal for first-time E2EE setup with recovery key display.

---

### Task 12: Add encryption indicator to DM header

**Files:**
- Create: `client/src/components/chat/EncryptionIndicator.tsx`

Lock icon showing encryption status.

---

## Summary

| Batch | Tasks | Focus |
|-------|-------|-------|
| 1 | 1-3 | Tauri Key Store foundation (SQLite, LocalKeyStore, CryptoManager) |
| 2 | 4 | Tauri commands (init, encrypt, decrypt) |
| 3 | 5-6 | Frontend types, tauri wrappers, E2EE store |
| 4 | 7-8 | Message flow integration (send/receive encryption) |
| 5 | 9-10 | Server message storage for E2EE content |
| 6 | 11-12 | UI components (setup modal, indicator) |

**Total Tasks:** 12
**Estimated Commits:** 12

**After completing all tasks:**
- Run full test suite: `cargo test --workspace && cd client && bun run build`
- Update CHANGELOG.md with E2EE messaging entry
- Create PR using commit-push-pr skill
