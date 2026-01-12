//! Olm Session Management
//!
//! Double Ratchet protocol for 1:1 encrypted communication.

use crate::Result;

/// User's Olm account containing identity keys.
pub struct OlmAccount {
    // TODO: vodozemac::olm::Account
}

impl OlmAccount {
    /// Create a new Olm account.
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for OlmAccount {
    fn default() -> Self {
        Self::new()
    }
}

/// An Olm session for encrypted 1:1 communication.
pub struct OlmSession {
    // TODO: vodozemac::olm::Session
}

impl OlmSession {
    /// Encrypt a message.
    pub fn encrypt(&mut self, _plaintext: &str) -> String {
        todo!()
    }

    /// Decrypt a message.
    pub fn decrypt(&mut self, _ciphertext: &str) -> Result<String> {
        todo!()
    }
}
