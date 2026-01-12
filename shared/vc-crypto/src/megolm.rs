//! Megolm Session Management
//!
//! Efficient group encryption for channel messages.

use crate::Result;

/// Outbound Megolm session for encrypting messages to a group.
pub struct MegolmOutboundSession {
    // TODO: vodozemac::megolm::GroupSession
}

impl MegolmOutboundSession {
    /// Create a new outbound session.
    pub fn new() -> Self {
        Self {}
    }

    /// Get the session key (to share with group members).
    pub fn session_key(&self) -> String {
        todo!()
    }

    /// Encrypt a message.
    pub fn encrypt(&mut self, _plaintext: &str) -> String {
        todo!()
    }
}

impl Default for MegolmOutboundSession {
    fn default() -> Self {
        Self::new()
    }
}

/// Inbound Megolm session for decrypting messages from a group member.
pub struct MegolmInboundSession {
    // TODO: vodozemac::megolm::InboundGroupSession
}

impl MegolmInboundSession {
    /// Create an inbound session from a session key.
    pub fn new(_session_key: &str) -> Result<Self> {
        Ok(Self {})
    }

    /// Decrypt a message.
    pub fn decrypt(&mut self, _ciphertext: &str) -> Result<String> {
        todo!()
    }
}
