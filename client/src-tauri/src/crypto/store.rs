//! Local Key Store for E2EE
//!
//! Encrypted `SQLite` storage for Olm accounts and sessions.

use std::path::Path;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use hmac::{Hmac, Mac};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;
#[cfg(feature = "megolm")]
use vc_crypto::megolm::{MegolmInboundSession, MegolmOutboundSession};
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

/// Key for identifying a Megolm inbound group session.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct MegolmInboundKey {
    /// The channel or group room ID.
    pub room_id: String,
    /// The sender's device Curve25519 public key (base64).
    pub sender_key: String,
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
    const METADATA_ENCRYPTION_DOMAIN: &'static [u8] = b"vc-client:metadata_encryption:v1";

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
            CREATE TABLE IF NOT EXISTS megolm_outbound_sessions (
                room_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                serialized TEXT NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (room_id)
            );
            CREATE TABLE IF NOT EXISTS megolm_inbound_sessions (
                room_id TEXT NOT NULL,
                sender_key TEXT NOT NULL,
                session_id TEXT NOT NULL,
                serialized TEXT NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (room_id, sender_key)
            );
            ",
        )?;
        Ok(())
    }

    /// Derive a deterministic keyed hash of a value.
    ///
    /// Used to store session lookup keys (`user_id`, `device_key`) as opaque
    /// hashes in the database so the communication graph is not exposed
    /// in plaintext on disk.
    fn keyed_hash(&self, domain: &str, value: &str) -> String {
        let mut mac = match <Hmac<Sha256> as Mac>::new_from_slice(self.encryption_key.as_ref()) {
            Ok(mac) => mac,
            Err(_) => unreachable!("HMAC-SHA256 accepts keys of any length"),
        };
        mac.update(domain.as_bytes());
        mac.update(&[0u8]);
        mac.update(value.as_bytes());
        STANDARD.encode(mac.finalize().into_bytes())
    }

    fn keyed_hash_legacy(&self, domain: &str, value: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.encryption_key.as_ref());
        hasher.update(domain.as_bytes());
        hasher.update(value.as_bytes());
        STANDARD.encode(hasher.finalize())
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

        // Hash lookup keys so plaintext user_id and device_key are not stored on disk
        let hashed_user_id = self.keyed_hash("session:user_id", &key.user_id.to_string());
        let hashed_device_key = self.keyed_hash("session:device_key", &key.device_curve25519);

        self.conn.execute(
            "INSERT OR REPLACE INTO sessions (user_id, device_key, session_id, serialized, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![hashed_user_id, hashed_device_key, session.session_id(), serialized, now],
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
        let hashed_user_id = self.keyed_hash("session:user_id", &key.user_id.to_string());
        let hashed_device_key = self.keyed_hash("session:device_key", &key.device_curve25519);

        let result: std::result::Result<String, _> = self.conn.query_row(
            "SELECT serialized FROM sessions WHERE user_id = ?1 AND device_key = ?2",
            params![hashed_user_id, hashed_device_key],
            |row| row.get(0),
        );

        match result {
            Ok(serialized) => {
                let session = OlmSession::deserialize(&serialized, &self.encryption_key)?;
                return Ok(Some(session));
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {}
            Err(e) => return Err(e.into()),
        }

        let legacy_hashed_user_id =
            self.keyed_hash_legacy("session:user_id", &key.user_id.to_string());
        let legacy_hashed_device_key =
            self.keyed_hash_legacy("session:device_key", &key.device_curve25519);

        let legacy_result: std::result::Result<String, _> = self.conn.query_row(
            "SELECT serialized FROM sessions WHERE user_id = ?1 AND device_key = ?2",
            params![legacy_hashed_user_id, legacy_hashed_device_key],
            |row| row.get(0),
        );

        match legacy_result {
            Ok(serialized) => {
                let session = OlmSession::deserialize(&serialized, &self.encryption_key)?;
                let now = chrono::Utc::now().timestamp();

                let tx = self.conn.unchecked_transaction()?;
                tx.execute(
                    "INSERT OR REPLACE INTO sessions (user_id, device_key, session_id, serialized, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![hashed_user_id, hashed_device_key, session.session_id(), serialized, now],
                )?;
                tx.execute(
                    "DELETE FROM sessions WHERE user_id = ?1 AND device_key = ?2",
                    params![legacy_hashed_user_id, legacy_hashed_device_key],
                )?;
                tx.commit()?;

                Ok(Some(session))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Save a Megolm outbound group session.
    #[cfg(feature = "megolm")]
    pub fn save_megolm_outbound_session(
        &self,
        room_id: &str,
        session: &MegolmOutboundSession,
    ) -> Result<()> {
        let serialized = session.serialize(&self.encryption_key)?;
        let encrypted = self.encrypt_metadata_value(&serialized)?;
        let now = chrono::Utc::now().timestamp();
        let hashed_room_id = self.keyed_hash("megolm:room_outbound", room_id);

        self.conn.execute(
            "INSERT OR REPLACE INTO megolm_outbound_sessions (room_id, session_id, serialized, updated_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![hashed_room_id, session.session_id(), encrypted, now],
        )?;
        Ok(())
    }

    /// Load a Megolm outbound group session.
    #[cfg(feature = "megolm")]
    pub fn load_megolm_outbound_session(
        &self,
        room_id: &str,
    ) -> Result<Option<MegolmOutboundSession>> {
        let hashed_room_id = self.keyed_hash("megolm:room_outbound", room_id);
        let result: std::result::Result<String, _> = self.conn.query_row(
            "SELECT serialized FROM megolm_outbound_sessions WHERE room_id = ?1",
            params![hashed_room_id],
            |row| row.get(0),
        );

        match result {
            Ok(serialized) => {
                let json = self
                    .decrypt_metadata_value(&serialized)
                    .unwrap_or(serialized);
                let session = MegolmOutboundSession::deserialize(&json, &self.encryption_key)?;
                return Ok(Some(session));
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {}
            Err(e) => return Err(e.into()),
        }

        let legacy_hashed_room_id = self.keyed_hash_legacy("megolm:room_outbound", room_id);
        let legacy_result: std::result::Result<String, _> = self.conn.query_row(
            "SELECT serialized FROM megolm_outbound_sessions WHERE room_id = ?1",
            params![legacy_hashed_room_id],
            |row| row.get(0),
        );

        match legacy_result {
            Ok(serialized) => {
                let json = self
                    .decrypt_metadata_value(&serialized)
                    .unwrap_or_else(|| serialized.clone());
                let session = MegolmOutboundSession::deserialize(&json, &self.encryption_key)?;
                let now = chrono::Utc::now().timestamp();

                let tx = self.conn.unchecked_transaction()?;
                tx.execute(
                    "INSERT OR REPLACE INTO megolm_outbound_sessions (room_id, session_id, serialized, updated_at)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![hashed_room_id, session.session_id(), serialized, now],
                )?;
                tx.execute(
                    "DELETE FROM megolm_outbound_sessions WHERE room_id = ?1",
                    params![legacy_hashed_room_id],
                )?;
                tx.commit()?;

                Ok(Some(session))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Save a Megolm inbound group session.
    #[cfg(feature = "megolm")]
    pub fn save_megolm_inbound_session(
        &self,
        key: &MegolmInboundKey,
        session: &MegolmInboundSession,
    ) -> Result<()> {
        let serialized = session.serialize(&self.encryption_key)?;
        let encrypted = self.encrypt_metadata_value(&serialized)?;
        let now = chrono::Utc::now().timestamp();
        let hashed_room_id = self.keyed_hash("megolm:room_inbound", &key.room_id);
        let hashed_sender = self.keyed_hash("megolm:sender", &key.sender_key);

        self.conn.execute(
            "INSERT OR REPLACE INTO megolm_inbound_sessions (room_id, sender_key, session_id, serialized, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![hashed_room_id, hashed_sender, session.session_id(), encrypted, now],
        )?;
        Ok(())
    }

    /// Load a Megolm inbound group session.
    #[cfg(feature = "megolm")]
    pub fn load_megolm_inbound_session(
        &self,
        key: &MegolmInboundKey,
    ) -> Result<Option<MegolmInboundSession>> {
        let hashed_room_id = self.keyed_hash("megolm:room_inbound", &key.room_id);
        let hashed_sender = self.keyed_hash("megolm:sender", &key.sender_key);

        let result: std::result::Result<String, _> = self.conn.query_row(
            "SELECT serialized FROM megolm_inbound_sessions WHERE room_id = ?1 AND sender_key = ?2",
            params![hashed_room_id, hashed_sender],
            |row| row.get(0),
        );

        match result {
            Ok(serialized) => {
                let json = self
                    .decrypt_metadata_value(&serialized)
                    .unwrap_or(serialized);
                let session = MegolmInboundSession::deserialize(&json, &self.encryption_key)?;
                return Ok(Some(session));
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {}
            Err(e) => return Err(e.into()),
        }

        let legacy_hashed_room_id = self.keyed_hash_legacy("megolm:room_inbound", &key.room_id);
        let legacy_hashed_sender = self.keyed_hash_legacy("megolm:sender", &key.sender_key);
        let legacy_result: std::result::Result<String, _> = self.conn.query_row(
            "SELECT serialized FROM megolm_inbound_sessions WHERE room_id = ?1 AND sender_key = ?2",
            params![legacy_hashed_room_id, legacy_hashed_sender],
            |row| row.get(0),
        );

        match legacy_result {
            Ok(serialized) => {
                let json = self
                    .decrypt_metadata_value(&serialized)
                    .unwrap_or_else(|| serialized.clone());
                let session = MegolmInboundSession::deserialize(&json, &self.encryption_key)?;
                let now = chrono::Utc::now().timestamp();

                let tx = self.conn.unchecked_transaction()?;
                tx.execute(
                    "INSERT OR REPLACE INTO megolm_inbound_sessions (room_id, sender_key, session_id, serialized, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![hashed_room_id, hashed_sender, session.session_id(), serialized, now],
                )?;
                tx.execute(
                    "DELETE FROM megolm_inbound_sessions WHERE room_id = ?1 AND sender_key = ?2",
                    params![legacy_hashed_room_id, legacy_hashed_sender],
                )?;
                tx.commit()?;

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
        let encrypted = self.encrypt_metadata_value(&json)?;

        self.conn.execute(
            "INSERT OR REPLACE INTO metadata (key, value) VALUES ('info', ?1)",
            params![encrypted],
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
            Ok(stored) => {
                // Try decrypting first (new format), fall back to plaintext (old format)
                let json = self
                    .decrypt_metadata_value(&stored)
                    .unwrap_or_else(|| stored.clone());
                let metadata: KeyStoreMetadata = serde_json::from_str(&json)?;
                Ok(Some(metadata))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn encrypt_metadata_value(&self, plaintext: &str) -> Result<String> {
        let key = self.derive_metadata_encryption_key();

        let cipher = match Aes256Gcm::new_from_slice(&key) {
            Ok(cipher) => cipher,
            Err(_) => unreachable!("SHA-256 output size matches AES-256 key size"),
        };

        let mut nonce_bytes = [0u8; 12];
        getrandom::getrandom(&mut nonce_bytes).map_err(|e| {
            vc_crypto::CryptoError::InvalidKey(format!("Nonce generation failed: {e}"))
        })?;

        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher.encrypt(nonce, plaintext.as_bytes()).map_err(|e| {
            vc_crypto::CryptoError::InvalidKey(format!("Metadata encryption failed: {e}"))
        })?;

        let mut combined = Vec::with_capacity(nonce_bytes.len() + ciphertext.len());
        combined.extend_from_slice(&nonce_bytes);
        combined.extend_from_slice(&ciphertext);

        Ok(format!("enc2:{}", STANDARD.encode(combined)))
    }

    fn decrypt_metadata_value(&self, stored: &str) -> Option<String> {
        if let Some(encoded) = stored.strip_prefix("enc2:") {
            let encrypted = STANDARD.decode(encoded).ok()?;
            return self.decrypt_metadata_value_aes(&encrypted);
        }

        if let Some(encoded) = stored.strip_prefix("enc:") {
            let encrypted = STANDARD.decode(encoded).ok()?;
            return self.decrypt_metadata_value_legacy_xor(&encrypted);
        }

        None
    }

    fn derive_metadata_encryption_key(&self) -> [u8; 32] {
        let mut mac = match <Hmac<Sha256> as Mac>::new_from_slice(self.encryption_key.as_ref()) {
            Ok(mac) => mac,
            Err(_) => unreachable!("HMAC-SHA256 accepts keys of any length"),
        };
        mac.update(Self::METADATA_ENCRYPTION_DOMAIN);
        let mut key = [0u8; 32];
        key.copy_from_slice(&mac.finalize().into_bytes());
        key
    }

    fn decrypt_metadata_value_aes(&self, encrypted: &[u8]) -> Option<String> {
        let key = self.derive_metadata_encryption_key();
        let cipher = Aes256Gcm::new_from_slice(&key).ok()?;

        if encrypted.len() > 12 {
            let (nonce_bytes, ciphertext) = encrypted.split_at(12);
            let nonce = Nonce::from_slice(nonce_bytes);

            if let Ok(plaintext) = cipher.decrypt(nonce, ciphertext) {
                if let Ok(decoded) = String::from_utf8(plaintext) {
                    return Some(decoded);
                }
            }
        }

        None
    }

    fn decrypt_metadata_value_legacy_xor(&self, encrypted: &[u8]) -> Option<String> {
        let mut stream_key = Sha256::new();
        stream_key.update(self.encryption_key.as_ref());
        stream_key.update(b"metadata_encryption");
        let key_hash = stream_key.finalize();

        let decrypted: Vec<u8> = encrypted
            .iter()
            .enumerate()
            .map(|(i, b)| b ^ key_hash[i % 32])
            .collect();

        String::from_utf8(decrypted).ok()
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use vc_crypto::types::Curve25519PublicKey;

    use super::*;

    fn legacy_encrypt_metadata_value(encryption_key: &[u8; 32], plaintext: &str) -> String {
        let mut stream_key = Sha256::new();
        stream_key.update(encryption_key);
        stream_key.update(b"metadata_encryption");
        let key_hash = stream_key.finalize();

        let encrypted: Vec<u8> = plaintext
            .as_bytes()
            .iter()
            .enumerate()
            .map(|(i, b)| b ^ key_hash[i % 32])
            .collect();

        format!("enc:{}", STANDARD.encode(encrypted))
    }

    fn legacy_keyed_hash(encryption_key: &[u8; 32], domain: &str, value: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(encryption_key);
        hasher.update(domain.as_bytes());
        hasher.update(value.as_bytes());
        STANDARD.encode(hasher.finalize())
    }

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
    fn test_store_session_legacy_hash_migration() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let key = [0u8; 32];

        let store = LocalKeyStore::open(&path, key).unwrap();

        let mut alice = OlmAccount::new();
        let mut bob = OlmAccount::new();
        bob.generate_one_time_keys(1);
        let bob_otk = bob.one_time_keys().pop().unwrap().1;
        let bob_otk_key = Curve25519PublicKey::from_base64(&bob_otk).unwrap();
        let session = alice.create_outbound_session(&bob.curve25519_key(), &bob_otk_key);
        let session_id = session.session_id();
        let serialized = session.serialize(&key).unwrap();

        let session_key = SessionKey {
            user_id: Uuid::new_v4(),
            device_curve25519: bob_otk,
        };

        let legacy_hashed_uid =
            legacy_keyed_hash(&key, "session:user_id", &session_key.user_id.to_string());
        let legacy_hashed_dk =
            legacy_keyed_hash(&key, "session:device_key", &session_key.device_curve25519);

        store
            .conn
            .execute(
                "INSERT INTO sessions (user_id, device_key, session_id, serialized, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    legacy_hashed_uid,
                    legacy_hashed_dk,
                    session_id,
                    serialized,
                    chrono::Utc::now().timestamp()
                ],
            )
            .unwrap();

        let loaded = store.load_session(&session_key).unwrap().unwrap();
        assert_eq!(loaded.session_id(), session_id);

        let new_hashed_uid = store.keyed_hash("session:user_id", &session_key.user_id.to_string());
        let new_hashed_dk = store.keyed_hash("session:device_key", &session_key.device_curve25519);
        let new_count: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE user_id = ?1 AND device_key = ?2",
                params![new_hashed_uid, new_hashed_dk],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(new_count, 1);

        let legacy_count: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE user_id = ?1 AND device_key = ?2",
                params![legacy_hashed_uid, legacy_hashed_dk],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(legacy_count, 0);
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

        let stored: String = store
            .conn
            .query_row("SELECT value FROM metadata WHERE key = 'info'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(stored.starts_with("enc2:"));

        let loaded = store.load_metadata().unwrap().unwrap();
        assert_eq!(loaded.user_id, metadata.user_id);
        assert_eq!(loaded.device_id, metadata.device_id);
    }

    #[test]
    fn test_store_metadata_legacy_xor_migration() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let key = [0u8; 32];

        let store = LocalKeyStore::open(&path, key).unwrap();

        let metadata = KeyStoreMetadata {
            user_id: Uuid::new_v4(),
            device_id: Uuid::new_v4(),
            created_at: chrono::Utc::now().timestamp(),
        };

        let json = serde_json::to_string(&metadata).unwrap();
        let legacy_encrypted = legacy_encrypt_metadata_value(&key, &json);

        store
            .conn
            .execute(
                "INSERT OR REPLACE INTO metadata (key, value) VALUES ('info', ?1)",
                params![legacy_encrypted],
            )
            .unwrap();

        let loaded = store.load_metadata().unwrap().unwrap();
        assert_eq!(loaded.user_id, metadata.user_id);
        assert_eq!(loaded.device_id, metadata.device_id);
        assert_eq!(loaded.created_at, metadata.created_at);
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

        // Verify only one session exists for this key by checking the database directly.
        // Keys are now stored as keyed hashes, so use the store's hashing method.
        let hashed_uid = store.keyed_hash("session:user_id", &session_key.user_id.to_string());
        let hashed_dk = store.keyed_hash("session:device_key", &session_key.device_curve25519);
        let count: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE user_id = ?1 AND device_key = ?2",
                params![hashed_uid, hashed_dk],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "Should have exactly one session after overwrite");
    }
}
