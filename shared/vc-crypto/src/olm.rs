//! Olm Session Management
//!
//! Double Ratchet protocol for 1:1 encrypted communication.

use serde::{Deserialize, Serialize};
use vodozemac::olm::{
    Account, AccountPickle, OlmMessage, PreKeyMessage, Session, SessionConfig, SessionPickle,
};
use vodozemac::Curve25519PublicKey;
use zeroize::ZeroizeOnDrop;

use crate::{CryptoError, Result};

/// Encrypted message from Olm.
///
/// This is a serializable wrapper around vodozemac's `OlmMessage` that can be
/// transmitted over the network or stored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedMessage {
    /// Message type: 0 = prekey, 1 = normal
    pub message_type: u8,
    /// Base64-encoded ciphertext
    pub ciphertext: String,
}

impl EncryptedMessage {
    /// Create from a vodozemac `OlmMessage`.
    #[must_use]
    pub fn from_olm_message(message: OlmMessage) -> Self {
        match message {
            OlmMessage::PreKey(prekey) => Self {
                message_type: 0,
                ciphertext: prekey.to_base64(),
            },
            OlmMessage::Normal(normal) => Self {
                message_type: 1,
                ciphertext: normal.to_base64(),
            },
        }
    }

    /// Convert to `OlmMessage` for decryption.
    ///
    /// # Errors
    ///
    /// Returns an error if the ciphertext is not valid base64 or the message type is invalid.
    pub fn to_olm_message(&self) -> Result<OlmMessage> {
        match self.message_type {
            0 => {
                let prekey = PreKeyMessage::from_base64(&self.ciphertext)
                    .map_err(|e| CryptoError::Vodozemac(e.to_string()))?;
                Ok(OlmMessage::PreKey(prekey))
            }
            1 => {
                let normal = vodozemac::olm::Message::from_base64(&self.ciphertext)
                    .map_err(|e| CryptoError::Vodozemac(e.to_string()))?;
                Ok(OlmMessage::Normal(normal))
            }
            _ => Err(CryptoError::Vodozemac(format!(
                "Invalid message type: {}",
                self.message_type
            ))),
        }
    }

    /// Try to get as prekey message.
    ///
    /// Returns `Some` if this is a prekey message (`message_type == 0`),
    /// `None` otherwise.
    #[must_use]
    pub fn into_prekey_message(&self) -> Option<PreKeyMessage> {
        if self.message_type == 0 {
            PreKeyMessage::from_base64(&self.ciphertext).ok()
        } else {
            None
        }
    }

    /// Check if this is a prekey message.
    #[must_use]
    pub const fn is_prekey(&self) -> bool {
        self.message_type == 0
    }
}

/// Identity key pair containing both Ed25519 (signing) and Curve25519 (encryption) keys.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IdentityKeyPair {
    /// Ed25519 public key for signing (base64-encoded).
    pub ed25519: String,
    /// Curve25519 public key for encryption (base64-encoded).
    pub curve25519: String,
}

/// A one-time key with its ID.
pub type OneTimeKey = (vodozemac::KeyId, String);

/// User's Olm account containing identity keys.
///
/// This wraps vodozemac's Account and provides secure key management
/// for the Double Ratchet protocol.
#[derive(ZeroizeOnDrop)]
pub struct OlmAccount {
    #[zeroize(skip)] // vodozemac::Account handles its own zeroization
    inner: Account,
}

impl OlmAccount {
    /// Create a new Olm account with fresh identity keys.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Account::new(),
        }
    }

    /// Get the account's identity keys.
    #[must_use]
    pub fn identity_keys(&self) -> IdentityKeyPair {
        let keys = self.inner.identity_keys();
        IdentityKeyPair {
            ed25519: keys.ed25519.to_base64(),
            curve25519: keys.curve25519.to_base64(),
        }
    }

    /// Get the Curve25519 public key for session creation.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn curve25519_key(&self) -> Curve25519PublicKey {
        self.inner.curve25519_key()
    }

    /// Generate one-time prekeys.
    ///
    /// These are used for establishing new sessions without requiring
    /// the recipient to be online.
    pub fn generate_one_time_keys(&mut self, count: usize) {
        self.inner.generate_one_time_keys(count);
    }

    /// Get unpublished one-time keys.
    ///
    /// Returns a list of (`KeyId`, base64-encoded key) pairs.
    #[must_use]
    pub fn one_time_keys(&self) -> Vec<OneTimeKey> {
        self.inner
            .one_time_keys()
            .into_iter()
            .map(|(id, key)| (id, key.to_base64()))
            .collect()
    }

    /// Mark one-time keys as published.
    ///
    /// Call this after uploading keys to the server to prevent
    /// them from being returned again.
    pub fn mark_keys_as_published(&mut self) {
        self.inner.mark_keys_as_published();
    }

    /// Serialize the account for secure storage.
    ///
    /// The account is encrypted with the provided key before serialization.
    pub fn serialize(&self, encryption_key: &[u8; 32]) -> Result<String> {
        // vodozemac uses "pickle" terminology for serialized cryptographic state
        let encrypted = self.inner.pickle().encrypt(encryption_key);
        Ok(encrypted)
    }

    /// Deserialize an account from secure storage.
    ///
    /// The account is decrypted with the provided key after deserialization.
    pub fn deserialize(serialized: &str, encryption_key: &[u8; 32]) -> Result<Self> {
        // vodozemac uses "pickle" terminology for serialized cryptographic state
        let account_pickle = AccountPickle::from_encrypted(serialized, encryption_key)
            .map_err(|e| CryptoError::Vodozemac(e.to_string()))?;

        let inner = Account::from_pickle(account_pickle);

        Ok(Self { inner })
    }

    /// Create an outbound session to a recipient.
    ///
    /// Uses the recipient's identity key and one-time key to establish
    /// a new encrypted session.
    pub fn create_outbound_session(
        &mut self,
        recipient_identity_key: &Curve25519PublicKey,
        recipient_one_time_key: &Curve25519PublicKey,
    ) -> OlmSession {
        let session = self.inner.create_outbound_session(
            SessionConfig::version_2(),
            *recipient_identity_key,
            *recipient_one_time_key,
        );
        OlmSession::new(session)
    }

    /// Create an inbound session from a prekey message.
    ///
    /// Used when receiving the first message from a new sender.
    /// Returns the session and the decrypted plaintext from the prekey message.
    ///
    /// Note: The prekey message is automatically decrypted during session creation,
    /// so you should NOT call `session.decrypt()` again on the same message.
    pub fn create_inbound_session(
        &mut self,
        sender_identity_key: &Curve25519PublicKey,
        message: &PreKeyMessage,
    ) -> Result<(OlmSession, String)> {
        let result = self
            .inner
            .create_inbound_session(*sender_identity_key, message)
            .map_err(|e| CryptoError::Vodozemac(e.to_string()))?;

        let plaintext = String::from_utf8(result.plaintext)
            .map_err(|e| CryptoError::DecryptionFailed(e.to_string()))?;

        Ok((OlmSession::new(result.session), plaintext))
    }
}

impl Default for OlmAccount {
    fn default() -> Self {
        Self::new()
    }
}

/// An Olm session for encrypted 1:1 communication.
#[derive(ZeroizeOnDrop)]
pub struct OlmSession {
    #[zeroize(skip)] // vodozemac::Session handles its own zeroization
    inner: Session,
}

impl OlmSession {
    /// Create a new `OlmSession` wrapping a vodozemac Session.
    const fn new(session: Session) -> Self {
        Self { inner: session }
    }

    /// Encrypt a message.
    ///
    /// Returns an `EncryptedMessage` that can be serialized and transmitted.
    pub fn encrypt(&mut self, plaintext: &str) -> EncryptedMessage {
        let olm_message = self.inner.encrypt(plaintext);
        EncryptedMessage::from_olm_message(olm_message)
    }

    /// Decrypt a message.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The message cannot be converted to an `OlmMessage`
    /// - Decryption fails (wrong session, corrupted message, etc.)
    /// - The decrypted plaintext is not valid UTF-8
    pub fn decrypt(&mut self, message: &EncryptedMessage) -> Result<String> {
        let olm_message = message.to_olm_message()?;

        let plaintext = self
            .inner
            .decrypt(&olm_message)
            .map_err(|e| CryptoError::DecryptionFailed(e.to_string()))?;

        String::from_utf8(plaintext).map_err(|e| CryptoError::DecryptionFailed(e.to_string()))
    }

    /// Get the session ID for tracking/storage.
    #[must_use]
    pub fn session_id(&self) -> String {
        self.inner.session_id()
    }

    /// Serialize the session for storage.
    pub fn serialize(&self, encryption_key: &[u8; 32]) -> Result<String> {
        // vodozemac uses "pickle" terminology for serialized cryptographic state
        let encrypted = self.inner.pickle().encrypt(encryption_key);
        Ok(encrypted)
    }

    /// Deserialize a session from storage.
    pub fn deserialize(serialized: &str, encryption_key: &[u8; 32]) -> Result<Self> {
        // vodozemac uses "pickle" terminology for serialized cryptographic state
        let session_pickle = SessionPickle::from_encrypted(serialized, encryption_key)
            .map_err(|e| CryptoError::Vodozemac(e.to_string()))?;

        let inner = Session::from(session_pickle);

        Ok(Self { inner })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_creation() {
        let account = OlmAccount::new();
        let identity_keys = account.identity_keys();
        assert!(!identity_keys.ed25519.is_empty());
        assert!(!identity_keys.curve25519.is_empty());
    }

    #[test]
    fn test_account_generates_one_time_keys() {
        let mut account = OlmAccount::new();
        account.generate_one_time_keys(10);
        let otks = account.one_time_keys();
        assert_eq!(otks.len(), 10);
    }

    #[test]
    fn test_account_marks_keys_as_published() {
        let mut account = OlmAccount::new();
        account.generate_one_time_keys(5);
        assert_eq!(account.one_time_keys().len(), 5);

        account.mark_keys_as_published();
        assert_eq!(account.one_time_keys().len(), 0);
    }

    #[test]
    fn test_account_serialization() {
        let account = OlmAccount::new();
        let identity_keys = account.identity_keys();

        let encryption_key = [0u8; 32];
        let serialized = account.serialize(&encryption_key).unwrap();

        let restored = OlmAccount::deserialize(&serialized, &encryption_key).unwrap();
        assert_eq!(restored.identity_keys(), identity_keys);
    }

    #[test]
    fn test_account_serialization_wrong_key_fails() {
        let account = OlmAccount::new();

        let encryption_key = [0u8; 32];
        let wrong_key = [1u8; 32];

        let serialized = account.serialize(&encryption_key).unwrap();
        let result = OlmAccount::deserialize(&serialized, &wrong_key);

        assert!(result.is_err());
    }

    #[test]
    fn test_session_encrypt_decrypt() {
        // Create Alice and Bob accounts
        let mut alice = OlmAccount::new();
        let mut bob = OlmAccount::new();

        // Bob generates one-time keys
        bob.generate_one_time_keys(1);
        let bob_otk = bob.one_time_keys().pop().unwrap().1;
        let bob_otk_key = Curve25519PublicKey::from_base64(&bob_otk).unwrap();

        // Alice creates outbound session to Bob
        let mut alice_session = alice.create_outbound_session(&bob.curve25519_key(), &bob_otk_key);

        // Alice encrypts a message
        let plaintext = "Hello, Bob!";
        let ciphertext = alice_session.encrypt(plaintext);

        // Verify it's a prekey message
        assert!(ciphertext.is_prekey());

        // Bob creates inbound session from prekey message
        // Note: create_inbound_session decrypts the prekey message automatically
        let message = ciphertext.into_prekey_message().unwrap();
        let (mut bob_session, decrypted) = bob
            .create_inbound_session(&alice.curve25519_key(), &message)
            .unwrap();

        assert_eq!(decrypted, plaintext);

        // Bob can now respond
        let response = "Hello, Alice!";
        let response_ciphertext = bob_session.encrypt(response);

        // Normal message (not prekey)
        assert!(!response_ciphertext.is_prekey());

        // Alice decrypts the response
        let alice_decrypted = alice_session.decrypt(&response_ciphertext).unwrap();
        assert_eq!(alice_decrypted, response);
    }

    #[test]
    fn test_session_serialization() {
        let mut alice = OlmAccount::new();
        alice.generate_one_time_keys(1);
        let alice_identity_key = alice.curve25519_key();
        let alice_otk = Curve25519PublicKey::from_base64(&alice.one_time_keys()[0].1)
            .expect("valid base64 key");

        let mut bob = OlmAccount::new();
        let mut session = bob.create_outbound_session(&alice_identity_key, &alice_otk);

        let encryption_key = [42u8; 32];
        let session_id = session.session_id();

        // Encrypt a message to advance ratchet state
        let _ = session.encrypt("test message");

        let serialized = session.serialize(&encryption_key).unwrap();
        let restored = OlmSession::deserialize(&serialized, &encryption_key).unwrap();

        assert_eq!(restored.session_id(), session_id);
    }

    #[test]
    fn test_identity_keys_are_unique() {
        let account1 = OlmAccount::new();
        let account2 = OlmAccount::new();

        assert_ne!(account1.identity_keys(), account2.identity_keys());
    }

    #[test]
    fn test_default_impl() {
        let account = OlmAccount::default();
        let identity_keys = account.identity_keys();
        assert!(!identity_keys.ed25519.is_empty());
    }
}
