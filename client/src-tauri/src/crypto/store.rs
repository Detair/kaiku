//! Local Key Store for E2EE
//!
//! Encrypted `SQLite` storage for Olm accounts and sessions.

use std::path::Path;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;
use vc_crypto::olm::{OlmAccount, OlmSession};
use zeroize::Zeroizing;

/// Key store errors.
#[derive(Debug, Error)]
pub enum KeyStoreError {
    /// Database error.
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// Crypto error from vc-crypto.
    #[error("Crypto error: {0}")]
    Crypto(#[from] vc_crypto::CryptoError),

    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Key store result type.
pub type Result<T> = std::result::Result<T, KeyStoreError>;

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
    /// User ID that owns this key store.
    pub user_id: Uuid,
    /// Device ID for this client instance.
    pub device_id: Uuid,
    /// Unix timestamp when the store was created.
    pub created_at: i64,
}

/// Local encrypted key store.
///
/// Stores Olm accounts and sessions in `SQLite`, encrypted with the provided key.
/// The encryption key is zeroized on drop to prevent sensitive key material from
/// lingering in memory.
pub struct LocalKeyStore {
    conn: Connection,
    encryption_key: Zeroizing<[u8; 32]>,
}

impl LocalKeyStore {
    /// Create or open a key store at the given path.
    ///
    /// The `encryption_key` is used to encrypt/decrypt Olm account and session state.
    ///
    /// # Errors
    ///
    /// Returns an error if the database cannot be opened or the schema cannot be initialized.
    pub fn open(path: &Path, encryption_key: [u8; 32]) -> Result<Self> {
        let conn = Connection::open(path)?;

        let store = Self {
            conn,
            encryption_key: Zeroizing::new(encryption_key),
        };
        store.init_schema()?;
        Ok(store)
    }

    /// Initialize the database schema.
    fn init_schema(&self) -> Result<()> {
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
            ",
        )?;
        Ok(())
    }

    /// Check if the store has an account.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub fn has_account(&self) -> Result<bool> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM account", [], |row| row.get(0))?;
        Ok(count > 0)
    }

    /// Save the Olm account.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization or database write fails.
    pub fn save_account(&self, account: &OlmAccount) -> Result<()> {
        let serialized = account.serialize(&self.encryption_key)?;

        self.conn.execute(
            "INSERT OR REPLACE INTO account (id, serialized) VALUES (1, ?1)",
            params![serialized],
        )?;

        Ok(())
    }

    /// Load the Olm account.
    ///
    /// # Errors
    ///
    /// Returns an error if no account exists, or if deserialization fails.
    pub fn load_account(&self) -> Result<OlmAccount> {
        let serialized: String =
            self.conn
                .query_row("SELECT serialized FROM account WHERE id = 1", [], |row| {
                    row.get(0)
                })?;

        let account = OlmAccount::deserialize(&serialized, &self.encryption_key)?;
        Ok(account)
    }

    /// Save a session.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization or database write fails.
    pub fn save_session(&self, key: &SessionKey, session: &OlmSession) -> Result<()> {
        let serialized = session.serialize(&self.encryption_key)?;
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
        )?;

        Ok(())
    }

    /// Load a session.
    ///
    /// Returns `None` if no session exists for the given key.
    ///
    /// # Errors
    ///
    /// Returns an error if deserialization fails.
    pub fn load_session(&self, key: &SessionKey) -> Result<Option<OlmSession>> {
        let result: std::result::Result<String, _> = self.conn.query_row(
            "SELECT serialized FROM sessions WHERE user_id = ?1 AND device_key = ?2",
            params![key.user_id.to_string(), key.device_curve25519],
            |row| row.get(0),
        );

        match result {
            Ok(serialized) => {
                let session = OlmSession::deserialize(&serialized, &self.encryption_key)?;
                Ok(Some(session))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Save metadata.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization or database write fails.
    pub fn save_metadata(&self, metadata: &KeyStoreMetadata) -> Result<()> {
        let json = serde_json::to_string(metadata)?;

        self.conn.execute(
            "INSERT OR REPLACE INTO metadata (key, value) VALUES ('info', ?1)",
            params![json],
        )?;

        Ok(())
    }

    /// Load metadata.
    ///
    /// Returns `None` if no metadata exists.
    ///
    /// # Errors
    ///
    /// Returns an error if deserialization fails.
    pub fn load_metadata(&self) -> Result<Option<KeyStoreMetadata>> {
        let result: std::result::Result<String, _> =
            self.conn
                .query_row("SELECT value FROM metadata WHERE key = 'info'", [], |row| {
                    row.get(0)
                });

        match result {
            Ok(json) => {
                let metadata: KeyStoreMetadata = serde_json::from_str(&json)?;
                Ok(Some(metadata))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use vc_crypto::types::Curve25519PublicKey;

    use super::*;

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
        let bob_otk_key = Curve25519PublicKey::from_base64(&bob_otk).unwrap();

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
    fn test_store_session_not_found() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let key = [0u8; 32];

        let store = LocalKeyStore::open(&path, key).unwrap();

        let session_key = SessionKey {
            user_id: Uuid::new_v4(),
            device_curve25519: "nonexistent".to_string(),
        };

        let result = store.load_session(&session_key).unwrap();
        assert!(result.is_none());
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

    #[test]
    fn test_store_persistence() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let key = [0u8; 32];

        let account_identity;
        {
            let store = LocalKeyStore::open(&path, key).unwrap();
            let account = OlmAccount::new();
            account_identity = account.identity_keys();
            store.save_account(&account).unwrap();
        }

        // Reopen store and verify data persisted
        {
            let store = LocalKeyStore::open(&path, key).unwrap();
            assert!(store.has_account().unwrap());
            let loaded = store.load_account().unwrap();
            assert_eq!(loaded.identity_keys(), account_identity);
        }
    }

    #[test]
    fn test_store_wrong_key_fails() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let key = [0u8; 32];
        let wrong_key = [1u8; 32];

        {
            let store = LocalKeyStore::open(&path, key).unwrap();
            let account = OlmAccount::new();
            store.save_account(&account).unwrap();
        }

        // Try to load with wrong key
        {
            let store = LocalKeyStore::open(&path, wrong_key).unwrap();
            let result = store.load_account();
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_store_session_overwrite() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let key = [0u8; 32];

        let store = LocalKeyStore::open(&path, key).unwrap();

        // Create accounts and initial session
        let mut alice = OlmAccount::new();
        let mut bob = OlmAccount::new();
        bob.generate_one_time_keys(1);
        let bob_otk = bob.one_time_keys().pop().unwrap().1;
        let bob_otk_key = Curve25519PublicKey::from_base64(&bob_otk).unwrap();

        let mut session = alice.create_outbound_session(&bob.curve25519_key(), &bob_otk_key);
        let session_id = session.session_id();

        let session_key = SessionKey {
            user_id: Uuid::new_v4(),
            device_curve25519: bob_otk.clone(),
        };

        // Save initial session
        store.save_session(&session_key, &session).unwrap();

        // Advance the ratchet by encrypting a message
        let _ciphertext = session.encrypt("test message");

        // Save updated session with the same SessionKey (should overwrite)
        store.save_session(&session_key, &session).unwrap();

        // Load and verify the session was updated
        let loaded = store.load_session(&session_key).unwrap().unwrap();

        // Session ID should be the same (identifies the session)
        assert_eq!(loaded.session_id(), session_id);

        // Verify only one session exists for this key by checking the database directly
        let count: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE user_id = ?1 AND device_key = ?2",
                params![
                    session_key.user_id.to_string(),
                    session_key.device_curve25519
                ],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "Should have exactly one session after overwrite");
    }
}
