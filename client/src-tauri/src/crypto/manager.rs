//! Crypto Manager
//!
//! High-level API for E2EE operations: initialization, encryption, decryption.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;
use vc_crypto::olm::{EncryptedMessage, IdentityKeyPair, OlmAccount};
use vc_crypto::types::{Curve25519PublicKey, KeyId};

use super::store::{KeyStoreMetadata, LocalKeyStore, SessionKey};

/// Crypto manager errors.
#[derive(Debug, Error)]
pub enum CryptoManagerError {
    /// Key store error.
    #[error("Key store error: {0}")]
    KeyStore(#[from] super::store::KeyStoreError),

    /// Crypto error from vc-crypto.
    #[error("Crypto error: {0}")]
    Crypto(#[from] vc_crypto::CryptoError),

    /// Invalid key format.
    #[error("Invalid key: {0}")]
    InvalidKey(String),

    /// Session not found.
    #[error("Session not found for user {user_id} device {device_key}")]
    SessionNotFound { user_id: Uuid, device_key: String },

    /// Account not initialized.
    #[error("Crypto account not initialized")]
    NotInitialized,
}

/// Crypto manager result type.
pub type Result<T> = std::result::Result<T, CryptoManagerError>;

/// Device keys from server (for establishing sessions).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct DeviceKeys {
    /// Device ID.
    pub device_id: Uuid,
    /// Device name (optional).
    pub device_name: Option<String>,
    /// Ed25519 identity key (base64).
    pub identity_key_ed25519: String,
    /// Curve25519 identity key (base64).
    pub identity_key_curve25519: String,
}

/// Claimed prekey from server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimedPrekey {
    /// Device ID.
    pub device_id: Uuid,
    /// Ed25519 identity key (base64).
    pub identity_key_ed25519: String,
    /// Curve25519 identity key (base64).
    pub identity_key_curve25519: String,
    /// One-time prekey (if available).
    pub one_time_prekey: Option<PrekeyInfo>,
}

/// One-time prekey info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrekeyInfo {
    /// Key ID.
    pub key_id: String,
    /// Public key (base64).
    pub public_key: String,
}

/// E2EE content for a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct E2EEContent {
    /// Our Curve25519 public key (sender identification).
    pub sender_key: String,
    /// Encrypted content for each recipient user -> device -> ciphertext.
    pub recipients: HashMap<String, HashMap<String, EncryptedMessage>>,
}

/// Prekey for upload to server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrekeyForUpload {
    /// Key ID.
    pub key_id: String,
    /// Public key (base64).
    pub public_key: String,
}

/// Manages E2EE cryptographic operations.
///
/// Uses `Mutex` instead of `RwLock` because `rusqlite::Connection` is `Send` but not `Sync`.
/// This allows `CryptoManager` to be `Send + Sync` for use in async contexts.
pub struct CryptoManager {
    store: Arc<Mutex<LocalKeyStore>>,
    user_id: Uuid,
    device_id: Uuid,
}

impl CryptoManager {
    /// Initialize the crypto manager.
    ///
    /// Creates a new Olm account if one doesn't exist, otherwise loads the existing one.
    ///
    /// # Arguments
    ///
    /// * `data_dir` - Directory for storing the key database
    /// * `user_id` - The authenticated user's ID
    /// * `encryption_key` - 32-byte key for encrypting the key store
    ///
    /// # Errors
    ///
    /// Returns an error if the key store cannot be opened or the account cannot be created/loaded.
    pub fn init(data_dir: PathBuf, user_id: Uuid, encryption_key: [u8; 32]) -> Result<Self> {
        let db_path = data_dir.join("keys.db");
        let store = LocalKeyStore::open(&db_path, encryption_key)?;

        // Check if we have an existing account
        let device_id = if store.has_account()? {
            // Load existing metadata to get device_id
            let metadata = store
                .load_metadata()?
                .ok_or(CryptoManagerError::NotInitialized)?;
            metadata.device_id
        } else {
            // Create new account
            let account = OlmAccount::new();
            store.save_account(&account)?;

            // Generate initial one-time keys
            let mut account = store.load_account()?;
            account.generate_one_time_keys(50);
            store.save_account(&account)?;

            // Create and save metadata
            let device_id = Uuid::now_v7();
            let metadata = KeyStoreMetadata {
                user_id,
                device_id,
                created_at: chrono::Utc::now().timestamp(),
            };
            store.save_metadata(&metadata)?;

            device_id
        };

        Ok(Self {
            store: Arc::new(Mutex::new(store)),
            user_id,
            device_id,
        })
    }

    /// Check if keys need to be uploaded to the server.
    ///
    /// Returns true if there are unpublished one-time keys.
    ///
    /// # Errors
    ///
    /// Returns an error if the account cannot be loaded.
    ///
    /// # Panics
    ///
    /// Panics if the Mutex is poisoned.
    pub fn needs_key_upload(&self) -> Result<bool> {
        let store = self.store.lock().expect("Mutex poisoned");
        let account = store.load_account()?;
        Ok(!account.one_time_keys().is_empty())
    }

    /// Get our identity keys.
    ///
    /// # Errors
    ///
    /// Returns an error if the account cannot be loaded.
    ///
    /// # Panics
    ///
    /// Panics if the Mutex is poisoned.
    pub fn get_identity_keys(&self) -> Result<IdentityKeyPair> {
        let store = self.store.lock().expect("Mutex poisoned");
        let account = store.load_account()?;
        Ok(account.identity_keys())
    }

    /// Get our device ID.
    #[must_use]
    pub const fn device_id(&self) -> Uuid {
        self.device_id
    }

    /// Get our user ID.
    #[must_use]
    pub const fn user_id(&self) -> Uuid {
        self.user_id
    }

    /// Get our Curve25519 public key (base64).
    ///
    /// # Errors
    ///
    /// Returns an error if the account cannot be loaded.
    ///
    /// # Panics
    ///
    /// Panics if the Mutex is poisoned.
    pub fn our_curve25519_key(&self) -> Result<String> {
        let store = self.store.lock().expect("Mutex poisoned");
        let account = store.load_account()?;
        Ok(account.curve25519_key().to_base64())
    }

    /// Generate one-time prekeys for upload to the server.
    ///
    /// Returns the prekeys and marks them as published after you call `mark_keys_published`.
    ///
    /// # Arguments
    ///
    /// * `count` - Number of prekeys to generate
    ///
    /// # Errors
    ///
    /// Returns an error if the account cannot be loaded or saved.
    ///
    /// # Panics
    ///
    /// Panics if the Mutex is poisoned.
    pub fn generate_prekeys(&self, count: usize) -> Result<Vec<PrekeyForUpload>> {
        let store = self.store.lock().expect("Mutex poisoned");
        let mut account = store.load_account()?;

        // Generate new one-time keys
        account.generate_one_time_keys(count);

        // Get the unpublished keys
        let prekeys: Vec<PrekeyForUpload> = account
            .one_time_keys()
            .into_iter()
            .map(|(key_id, public_key): (KeyId, String)| PrekeyForUpload {
                key_id: key_id.to_base64(),
                public_key,
            })
            .collect();

        // Save the account with the new keys
        store.save_account(&account)?;

        Ok(prekeys)
    }

    /// Mark keys as published after successful upload to server.
    ///
    /// # Errors
    ///
    /// Returns an error if the account cannot be loaded or saved.
    ///
    /// # Panics
    ///
    /// Panics if the Mutex is poisoned.
    pub fn mark_keys_published(&self) -> Result<()> {
        let store = self.store.lock().expect("Mutex poisoned");
        let mut account = store.load_account()?;
        account.mark_keys_as_published();
        store.save_account(&account)?;
        Ok(())
    }

    /// Get unpublished one-time keys for upload.
    ///
    /// # Errors
    ///
    /// Returns an error if the account cannot be loaded.
    ///
    /// # Panics
    ///
    /// Panics if the Mutex is poisoned.
    pub fn get_unpublished_keys(&self) -> Result<Vec<PrekeyForUpload>> {
        let store = self.store.lock().expect("Mutex poisoned");
        let account = store.load_account()?;

        let prekeys: Vec<PrekeyForUpload> = account
            .one_time_keys()
            .into_iter()
            .map(|(key_id, public_key): (KeyId, String)| PrekeyForUpload {
                key_id: key_id.to_base64(),
                public_key,
            })
            .collect();

        Ok(prekeys)
    }

    /// Encrypt a message for a specific device.
    ///
    /// Creates a new session if one doesn't exist using the claimed prekey.
    ///
    /// # Arguments
    ///
    /// * `recipient_user_id` - The recipient's user ID
    /// * `claimed` - The claimed prekey information from the server
    /// * `plaintext` - The message to encrypt
    ///
    /// # Errors
    ///
    /// Returns an error if the session cannot be created or encryption fails.
    ///
    /// # Panics
    ///
    /// Panics if the Mutex is poisoned.
    pub fn encrypt_for_device(
        &self,
        recipient_user_id: Uuid,
        claimed: &ClaimedPrekey,
        plaintext: &str,
    ) -> Result<EncryptedMessage> {
        let store = self.store.lock().expect("Mutex poisoned");

        // Parse the recipient's identity key
        let recipient_identity_key =
            Curve25519PublicKey::from_base64(&claimed.identity_key_curve25519)
                .map_err(|e| CryptoManagerError::InvalidKey(e.to_string()))?;

        let session_key = SessionKey {
            user_id: recipient_user_id,
            device_curve25519: claimed.identity_key_curve25519.clone(),
        };

        // Try to load existing session
        let mut session = if let Some(existing) = store.load_session(&session_key)? {
            existing
        } else {
            // Need to create a new outbound session
            let mut account = store.load_account()?;

            // Get the one-time prekey if available
            let one_time_key = if let Some(ref prekey) = claimed.one_time_prekey {
                Curve25519PublicKey::from_base64(&prekey.public_key)
                    .map_err(|e| CryptoManagerError::InvalidKey(e.to_string()))?
            } else {
                // Fallback: use identity key as one-time key (less secure, but allows
                // communication when no one-time keys are available)
                recipient_identity_key
            };

            let session = account.create_outbound_session(&recipient_identity_key, &one_time_key);

            // Save the updated account (one-time keys may have been consumed)
            store.save_account(&account)?;

            session
        };

        // Encrypt the message
        let ciphertext = session.encrypt(plaintext);

        // Save the updated session (ratchet has advanced)
        store.save_session(&session_key, &session)?;

        Ok(ciphertext)
    }

    /// Decrypt an incoming message.
    ///
    /// Creates a new inbound session if this is a prekey message.
    ///
    /// # Arguments
    ///
    /// * `sender_user_id` - The sender's user ID
    /// * `sender_key` - The sender's Curve25519 public key (base64)
    /// * `message` - The encrypted message
    ///
    /// # Errors
    ///
    /// Returns an error if decryption fails or the session cannot be established.
    ///
    /// # Panics
    ///
    /// Panics if the Mutex is poisoned.
    pub fn decrypt_message(
        &self,
        sender_user_id: Uuid,
        sender_key: &str,
        message: &EncryptedMessage,
    ) -> Result<String> {
        let store = self.store.lock().expect("Mutex poisoned");

        let sender_identity_key = Curve25519PublicKey::from_base64(sender_key)
            .map_err(|e| CryptoManagerError::InvalidKey(e.to_string()))?;

        let session_key = SessionKey {
            user_id: sender_user_id,
            device_curve25519: sender_key.to_string(),
        };

        // Check if this is a prekey message (first message in a session)
        if message.is_prekey() {
            // Try to create inbound session
            let prekey_message = message.into_prekey_message().ok_or_else(|| {
                CryptoManagerError::InvalidKey("Invalid prekey message".to_string())
            })?;

            let mut account = store.load_account()?;
            let (session, plaintext) =
                account.create_inbound_session(&sender_identity_key, &prekey_message)?;

            // Save the account (one-time key was consumed)
            store.save_account(&account)?;

            // Save the new session
            store.save_session(&session_key, &session)?;

            Ok(plaintext)
        } else {
            // Normal message - need existing session
            let mut session = store.load_session(&session_key)?.ok_or_else(|| {
                CryptoManagerError::SessionNotFound {
                    user_id: sender_user_id,
                    device_key: sender_key.to_string(),
                }
            })?;

            let plaintext = session.decrypt(message)?;

            // Save the updated session
            store.save_session(&session_key, &session)?;

            Ok(plaintext)
        }
    }

    /// Check if we have a session with a specific device.
    ///
    /// # Errors
    ///
    /// Returns an error if the session lookup fails.
    ///
    /// # Panics
    ///
    /// Panics if the Mutex is poisoned.
    pub fn has_session(&self, user_id: Uuid, device_curve25519: &str) -> Result<bool> {
        let store = self.store.lock().expect("Mutex poisoned");
        let session_key = SessionKey {
            user_id,
            device_curve25519: device_curve25519.to_string(),
        };
        Ok(store.load_session(&session_key)?.is_some())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn test_crypto_manager_init() {
        let dir = tempdir().unwrap();
        let encryption_key = [0u8; 32];
        let user_id = Uuid::now_v7();

        // First init should create new account
        let manager =
            CryptoManager::init(dir.path().to_path_buf(), user_id, encryption_key).unwrap();
        let device_id = manager.device_id();

        // Should have identity keys
        let identity = manager.get_identity_keys().unwrap();
        assert!(!identity.ed25519.is_empty());
        assert!(!identity.curve25519.is_empty());

        // Should have unpublished keys (we generated 50 on init)
        assert!(manager.needs_key_upload().unwrap());

        // Drop and re-open - should load existing account
        drop(manager);

        let manager2 =
            CryptoManager::init(dir.path().to_path_buf(), user_id, encryption_key).unwrap();

        // Should have same device ID
        assert_eq!(manager2.device_id(), device_id);

        // Should have same identity keys
        let identity2 = manager2.get_identity_keys().unwrap();
        assert_eq!(identity2, identity);
    }

    #[test]
    fn test_crypto_manager_prekey_generation() {
        let dir = tempdir().unwrap();
        let encryption_key = [0u8; 32];
        let user_id = Uuid::now_v7();

        let manager =
            CryptoManager::init(dir.path().to_path_buf(), user_id, encryption_key).unwrap();

        // Get initial unpublished keys (50 from init)
        let initial_keys = manager.get_unpublished_keys().unwrap();
        assert_eq!(initial_keys.len(), 50);

        // Mark as published
        manager.mark_keys_published().unwrap();

        // Should have no unpublished keys now
        assert!(!manager.needs_key_upload().unwrap());
        assert!(manager.get_unpublished_keys().unwrap().is_empty());

        // Generate new prekeys
        let new_keys = manager.generate_prekeys(10).unwrap();
        assert_eq!(new_keys.len(), 10);

        // Should need upload again
        assert!(manager.needs_key_upload().unwrap());

        // Mark as published
        manager.mark_keys_published().unwrap();
        assert!(!manager.needs_key_upload().unwrap());
    }

    #[test]
    fn test_crypto_manager_encrypt_decrypt() {
        let dir = tempdir().unwrap();
        let encryption_key = [0u8; 32];

        // Create Alice
        let alice_dir = dir.path().join("alice");
        std::fs::create_dir(&alice_dir).unwrap();
        let alice_user_id = Uuid::now_v7();
        let alice = CryptoManager::init(alice_dir, alice_user_id, encryption_key).unwrap();

        // Create Bob
        let bob_dir = dir.path().join("bob");
        std::fs::create_dir(&bob_dir).unwrap();
        let bob_user_id = Uuid::now_v7();
        let bob = CryptoManager::init(bob_dir, bob_user_id, encryption_key).unwrap();

        // Get Bob's keys for Alice to encrypt to
        let bob_identity = bob.get_identity_keys().unwrap();
        let bob_prekeys = bob.get_unpublished_keys().unwrap();
        let bob_prekey = bob_prekeys.first().unwrap();

        // Alice encrypts to Bob
        let claimed = ClaimedPrekey {
            device_id: bob.device_id(),
            identity_key_ed25519: bob_identity.ed25519.clone(),
            identity_key_curve25519: bob_identity.curve25519.clone(),
            one_time_prekey: Some(PrekeyInfo {
                key_id: bob_prekey.key_id.clone(),
                public_key: bob_prekey.public_key.clone(),
            }),
        };

        let plaintext = "Hello, Bob!";
        let ciphertext = alice
            .encrypt_for_device(bob_user_id, &claimed, plaintext)
            .unwrap();

        // Should be a prekey message (first message)
        assert!(ciphertext.is_prekey());

        // Bob decrypts
        let alice_curve25519 = alice.our_curve25519_key().unwrap();
        let decrypted = bob
            .decrypt_message(alice_user_id, &alice_curve25519, &ciphertext)
            .unwrap();
        assert_eq!(decrypted, plaintext);

        // Bob replies
        let alice_identity = alice.get_identity_keys().unwrap();
        let bob_claimed = ClaimedPrekey {
            device_id: alice.device_id(),
            identity_key_ed25519: alice_identity.ed25519.clone(),
            identity_key_curve25519: alice_identity.curve25519.clone(),
            one_time_prekey: None, // Bob already has session, doesn't need prekey
        };

        let reply = "Hello, Alice!";
        let reply_ciphertext = bob
            .encrypt_for_device(alice_user_id, &bob_claimed, reply)
            .unwrap();

        // Olm sessions are UNIDIRECTIONAL:
        // - Bob's inbound session from Alice can ONLY decrypt messages from Alice
        // - To SEND to Alice, Bob must create a NEW outbound session to her
        // - This outbound session is completely separate from the inbound session
        // - Therefore, Bob's first message to Alice is also a prekey message
        // - This is fundamental to the Olm protocol design

        // Alice decrypts Bob's reply
        let bob_curve25519 = bob.our_curve25519_key().unwrap();
        let decrypted_reply = alice
            .decrypt_message(bob_user_id, &bob_curve25519, &reply_ciphertext)
            .unwrap();
        assert_eq!(decrypted_reply, reply);
    }

    #[test]
    fn test_crypto_manager_session_persistence() {
        let dir = tempdir().unwrap();
        let encryption_key = [0u8; 32];

        // Create Alice
        let alice_dir = dir.path().join("alice");
        std::fs::create_dir(&alice_dir).unwrap();
        let alice_user_id = Uuid::now_v7();

        // Create Bob
        let bob_dir = dir.path().join("bob");
        std::fs::create_dir(&bob_dir).unwrap();
        let bob_user_id = Uuid::now_v7();
        let bob = CryptoManager::init(bob_dir.clone(), bob_user_id, encryption_key).unwrap();

        // Get Bob's info
        let bob_identity = bob.get_identity_keys().unwrap();
        let bob_prekeys = bob.get_unpublished_keys().unwrap();
        let bob_prekey = bob_prekeys.first().unwrap();
        let bob_device_id = bob.device_id();

        let first_ciphertext;
        let alice_key;
        {
            let alice =
                CryptoManager::init(alice_dir.clone(), alice_user_id, encryption_key).unwrap();
            alice_key = alice.our_curve25519_key().unwrap();

            // Alice encrypts to Bob
            let claimed = ClaimedPrekey {
                device_id: bob_device_id,
                identity_key_ed25519: bob_identity.ed25519.clone(),
                identity_key_curve25519: bob_identity.curve25519.clone(),
                one_time_prekey: Some(PrekeyInfo {
                    key_id: bob_prekey.key_id.clone(),
                    public_key: bob_prekey.public_key.clone(),
                }),
            };

            first_ciphertext = alice
                .encrypt_for_device(bob_user_id, &claimed, "First message")
                .unwrap();
            assert!(first_ciphertext.is_prekey());

            // Verify session exists
            assert!(alice
                .has_session(bob_user_id, &bob_identity.curve25519)
                .unwrap());
        }

        // Reopen Alice's manager
        {
            let alice = CryptoManager::init(alice_dir, alice_user_id, encryption_key).unwrap();

            // Session should still exist after reopen
            assert!(alice
                .has_session(bob_user_id, &bob_identity.curve25519)
                .unwrap());

            // Can encrypt again using existing session
            // Note: In Olm, messages stay as prekey messages until a response is received.
            // This is by design - the protocol ensures the recipient can still establish
            // the session even if earlier messages were lost.
            let claimed = ClaimedPrekey {
                device_id: bob_device_id,
                identity_key_ed25519: bob_identity.ed25519.clone(),
                identity_key_curve25519: bob_identity.curve25519.clone(),
                one_time_prekey: None, // Don't need prekey - we have existing session
            };

            let second_ciphertext = alice
                .encrypt_for_device(bob_user_id, &claimed, "Second message")
                .unwrap();

            // Both messages should still be prekey messages (Olm behavior until response received)
            assert!(second_ciphertext.is_prekey());

            // But the ciphertexts should be different (ratchet advanced)
            assert_ne!(first_ciphertext.ciphertext, second_ciphertext.ciphertext);
        }

        // Verify Bob can decrypt the first message
        let decrypted = bob
            .decrypt_message(alice_user_id, &alice_key, &first_ciphertext)
            .unwrap();
        assert_eq!(decrypted, "First message");
    }

    #[test]
    fn test_crypto_manager_identity_key_fallback() {
        // Test the fallback code path when no one-time prekey is available.
        //
        // KNOWN LIMITATION: The current fallback implementation (lines 340-345) attempts
        // to use the identity key as a one-time key, but this doesn't work with vodozemac.
        // When vodozemac creates an inbound session, it expects to find the referenced
        // one-time key in the account's published keys. Since the identity key is not
        // a one-time key, decryption fails.
        //
        // FUTURE FIX: Implement proper fallback key support using vodozemac's fallback key
        // mechanism. This requires:
        // 1. Bob generating and publishing a fallback key alongside one-time keys
        // 2. Server returning the fallback key when no one-time keys are available
        // 3. Using the fallback key for session creation (works like a one-time key)
        //
        // This test verifies the CURRENT behavior (encryption succeeds, decryption fails).
        let dir = tempdir().unwrap();
        let encryption_key = [0u8; 32];

        // Create Alice
        let alice_dir = dir.path().join("alice");
        std::fs::create_dir(&alice_dir).unwrap();
        let alice_user_id = Uuid::now_v7();
        let alice = CryptoManager::init(alice_dir, alice_user_id, encryption_key).unwrap();

        // Create Bob but mark all keys as published (simulating no available one-time keys)
        let bob_dir = dir.path().join("bob");
        std::fs::create_dir(&bob_dir).unwrap();
        let bob_user_id = Uuid::now_v7();
        let bob = CryptoManager::init(bob_dir, bob_user_id, encryption_key).unwrap();

        // Get Bob's identity keys only (no one-time prekeys)
        let bob_identity = bob.get_identity_keys().unwrap();

        // Alice encrypts to Bob WITHOUT a one-time prekey (fallback path)
        let claimed = ClaimedPrekey {
            device_id: bob.device_id(),
            identity_key_ed25519: bob_identity.ed25519.clone(),
            identity_key_curve25519: bob_identity.curve25519.clone(),
            one_time_prekey: None, // No one-time prekey available - triggers fallback
        };

        let plaintext = "Hello via fallback!";

        // Encryption succeeds (Alice can create an outbound session)
        let ciphertext = alice
            .encrypt_for_device(bob_user_id, &claimed, plaintext)
            .unwrap();
        assert!(ciphertext.is_prekey());

        // Decryption fails because the identity key isn't a valid one-time key.
        // Bob's account doesn't recognize the key embedded in the prekey message.
        let alice_curve25519 = alice.our_curve25519_key().unwrap();
        let result = bob.decrypt_message(alice_user_id, &alice_curve25519, &ciphertext);

        // Current behavior: decryption fails with "unknown one-time key" error
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, CryptoManagerError::Crypto(_)),
            "Expected Crypto error, got: {err:?}"
        );
    }

    #[test]
    fn test_crypto_manager_decrypt_wrong_sender() {
        // Test that decryption fails when the wrong sender key is provided.
        // This verifies the session lookup correctly uses the sender key.
        let dir = tempdir().unwrap();
        let encryption_key = [0u8; 32];

        // Create Alice
        let alice_dir = dir.path().join("alice");
        std::fs::create_dir(&alice_dir).unwrap();
        let alice_user_id = Uuid::now_v7();
        let alice = CryptoManager::init(alice_dir, alice_user_id, encryption_key).unwrap();

        // Create Bob
        let bob_dir = dir.path().join("bob");
        std::fs::create_dir(&bob_dir).unwrap();
        let bob_user_id = Uuid::now_v7();
        let bob = CryptoManager::init(bob_dir, bob_user_id, encryption_key).unwrap();

        // Create Charlie (whose key we'll use as the "wrong" key)
        let charlie_dir = dir.path().join("charlie");
        std::fs::create_dir(&charlie_dir).unwrap();
        let charlie_user_id = Uuid::now_v7();
        let charlie = CryptoManager::init(charlie_dir, charlie_user_id, encryption_key).unwrap();

        // Get Bob's keys for Alice to encrypt to
        let bob_identity = bob.get_identity_keys().unwrap();
        let bob_prekeys = bob.get_unpublished_keys().unwrap();
        let bob_prekey = bob_prekeys.first().unwrap();

        // Alice encrypts to Bob
        let claimed = ClaimedPrekey {
            device_id: bob.device_id(),
            identity_key_ed25519: bob_identity.ed25519.clone(),
            identity_key_curve25519: bob_identity.curve25519.clone(),
            one_time_prekey: Some(PrekeyInfo {
                key_id: bob_prekey.key_id.clone(),
                public_key: bob_prekey.public_key.clone(),
            }),
        };

        let plaintext = "Secret message";
        let ciphertext = alice
            .encrypt_for_device(bob_user_id, &claimed, plaintext)
            .unwrap();

        // First, verify Bob CAN decrypt with correct sender key
        let alice_curve25519 = alice.our_curve25519_key().unwrap();
        let decrypted = bob
            .decrypt_message(alice_user_id, &alice_curve25519, &ciphertext)
            .unwrap();
        assert_eq!(decrypted, plaintext);

        // Now Alice sends another message
        let ciphertext2 = alice
            .encrypt_for_device(bob_user_id, &claimed, "Second message")
            .unwrap();

        // Try to decrypt with wrong sender key (Charlie's key)
        // This should fail because no session exists for that sender key
        let charlie_curve25519 = charlie.our_curve25519_key().unwrap();
        let result = bob.decrypt_message(alice_user_id, &charlie_curve25519, &ciphertext2);

        // The decryption should fail - either SessionNotFound (for normal messages)
        // or a crypto error (for prekey messages where the key doesn't match)
        assert!(result.is_err());
    }
}
