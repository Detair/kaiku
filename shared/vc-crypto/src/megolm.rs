//! Megolm Session Management
//!
//! Efficient group encryption for channel messages.
//!
//! This module is only compiled when the `megolm` feature is enabled.

use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::{CryptoError, Result};

/// Outbound Megolm session for encrypting messages to a group.
#[cfg(feature = "megolm")]
pub struct MegolmOutboundSession {
    /// The underlying vodozemac `GroupSession`.
    session: vodozemac::megolm::GroupSession,
}

#[cfg(feature = "megolm")]
impl MegolmOutboundSession {
    /// Create a new outbound session.
    #[must_use]
    pub fn new() -> Self {
        Self {
            session: vodozemac::megolm::GroupSession::new(
                vodozemac::megolm::SessionConfig::version_1(),
            ),
        }
    }

    /// Get the session key (to share with group members via Olm).
    /// Returns a base64 encoded string representing the exportable key.
    #[must_use]
    pub fn session_key(&self) -> String {
        let exportable_key = self.session.session_key();
        exportable_key.to_base64()
    }

    /// Get the unique session ID.
    #[must_use]
    pub fn session_id(&self) -> String {
        self.session.session_id()
    }

    /// Get the current message index for ratcheting.
    #[must_use]
    pub const fn message_index(&self) -> u32 {
        self.session.message_index()
    }

    /// Encrypt a message payload.
    /// Returns the ciphertext as a base64 string.
    pub fn encrypt(&mut self, plaintext: &str) -> String {
        let ciphertext = self.session.encrypt(plaintext);
        ciphertext.to_base64()
    }

    pub fn serialize(&self, encryption_key: &[u8; 32]) -> Result<String> {
        let pickle_key = derive_pickle_key(encryption_key);
        Ok(self.session.pickle().encrypt(&pickle_key))
    }

    pub fn deserialize(serialized: &str, encryption_key: &[u8; 32]) -> Result<Self> {
        let pickle_key = derive_pickle_key(encryption_key);
        let pickle = vodozemac::megolm::GroupSessionPickle::from_encrypted(serialized, &pickle_key)
            .map_err(|e| CryptoError::Vodozemac(e.to_string()))?;

        Ok(Self {
            session: vodozemac::megolm::GroupSession::from(pickle),
        })
    }
}

#[cfg(feature = "megolm")]
impl Default for MegolmOutboundSession {
    fn default() -> Self {
        Self::new()
    }
}

/// Inbound Megolm session for decrypting messages from a group member.
#[cfg(feature = "megolm")]
pub struct MegolmInboundSession {
    /// The underlying vodozemac `InboundGroupSession`.
    session: vodozemac::megolm::InboundGroupSession,
}

#[cfg(feature = "megolm")]
impl MegolmInboundSession {
    /// Create an inbound session from a base64 encoded session key.
    pub fn new(session_key_b64: &str) -> Result<Self> {
        let session_key = vodozemac::megolm::SessionKey::from_base64(session_key_b64)
            .map_err(|e| CryptoError::InvalidKey(format!("Invalid megolm session key: {e}")))?;

        let session = vodozemac::megolm::InboundGroupSession::new(
            &session_key,
            vodozemac::megolm::SessionConfig::version_1(),
        );
        Ok(Self { session })
    }

    /// Get the unique session ID.
    #[must_use]
    pub fn session_id(&self) -> String {
        self.session.session_id()
    }

    /// Get the first known message index for this inbound session.
    #[must_use]
    pub const fn first_known_index(&self) -> u32 {
        self.session.first_known_index()
    }

    /// Decrypt a base64-encoded ciphertext message.
    pub fn decrypt(&mut self, ciphertext_b64: &str) -> Result<String> {
        let message = vodozemac::megolm::MegolmMessage::from_base64(ciphertext_b64)
            .map_err(|e| CryptoError::DecryptionFailed(format!("Malformed Megolm message: {e}")))?;

        let decrypt_result = self
            .session
            .decrypt(&message)
            .map_err(|e| CryptoError::DecryptionFailed(format!("Megolm decryption failed: {e}")))?;

        String::from_utf8(decrypt_result.plaintext).map_err(|e| {
            CryptoError::DecryptionFailed(format!("Decrypted payload is not valid UTF-8: {e}"))
        })
    }

    pub fn serialize(&self, encryption_key: &[u8; 32]) -> Result<String> {
        let pickle_key = derive_pickle_key(encryption_key);
        Ok(self.session.pickle().encrypt(&pickle_key))
    }

    pub fn deserialize(serialized: &str, encryption_key: &[u8; 32]) -> Result<Self> {
        let pickle_key = derive_pickle_key(encryption_key);
        let pickle =
            vodozemac::megolm::InboundGroupSessionPickle::from_encrypted(serialized, &pickle_key)
                .map_err(|e| CryptoError::Vodozemac(e.to_string()))?;

        Ok(Self {
            session: vodozemac::megolm::InboundGroupSession::from(pickle),
        })
    }
}

const PICKLE_KEY_DOMAIN: &[u8] = b"vodozemac-pickle-key";

fn derive_pickle_key(encryption_key: &[u8; 32]) -> [u8; 32] {
    let mut mac = match Hmac::<Sha256>::new_from_slice(encryption_key) {
        Ok(mac) => mac,
        Err(_) => unreachable!("HMAC-SHA256 accepts keys of any length"),
    };
    mac.update(PICKLE_KEY_DOMAIN);

    let mut key = [0u8; 32];
    key.copy_from_slice(&mac.finalize().into_bytes());
    key
}

#[cfg(all(test, feature = "megolm"))]
mod tests {
    use super::*;

    #[test]
    fn test_megolm_encrypt_decrypt() {
        // 1. Sender creates an outbound session
        let mut outbound = MegolmOutboundSession::new();
        let session_id = outbound.session_id();
        let session_key = outbound.session_key();

        assert_eq!(outbound.message_index(), 0);

        // 2. Sender encrypts a message
        let plaintext = "Hello group chat!";
        let ciphertext = outbound.encrypt(plaintext);

        assert_eq!(outbound.message_index(), 1); // Ratchet advanced

        // 3. Receiver creates an inbound session using the session key
        let mut inbound =
            MegolmInboundSession::new(&session_key).expect("Failed to create inbound session");

        // Session IDs must match
        assert_eq!(inbound.session_id(), session_id);
        assert_eq!(inbound.first_known_index(), 0);

        // 4. Receiver decrypts the ciphertext
        let decrypted = inbound.decrypt(&ciphertext).expect("Failed to decrypt");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_megolm_serialization() {
        let encryption_key = [42u8; 32];

        // Create an outbound session
        let outbound = MegolmOutboundSession::new();
        let session_key = outbound.session_key();

        // Serialize and Deserialize Outbound
        let serialized_outbound = outbound.serialize(&encryption_key).unwrap();
        let deserialized_outbound =
            MegolmOutboundSession::deserialize(&serialized_outbound, &encryption_key).unwrap();
        assert_eq!(deserialized_outbound.session_id(), outbound.session_id());

        // Create an inbound session
        let inbound = MegolmInboundSession::new(&session_key).unwrap();

        // Serialize and Deserialize Inbound
        let serialized_inbound = inbound.serialize(&encryption_key).unwrap();
        let deserialized_inbound =
            MegolmInboundSession::deserialize(&serialized_inbound, &encryption_key).unwrap();
        assert_eq!(deserialized_inbound.session_id(), inbound.session_id());
    }
}
