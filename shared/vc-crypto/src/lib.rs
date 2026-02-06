//! `VoiceChat` E2EE Cryptography
//!
//! End-to-end encryption using vodozemac (Olm/Megolm).
//!
//! - **Olm**: Double Ratchet for 1:1 encrypted sessions (DMs)
//! - **Megolm**: Efficient group encryption for channels

pub mod error;
pub mod megolm;
pub mod olm;
pub mod recovery;

pub use error::{CryptoError, Result};
pub use recovery::{EncryptedBackup, RecoveryKey};

/// Re-export vodozemac types that are commonly needed.
pub mod types {
    pub use vodozemac::{Curve25519PublicKey, Ed25519PublicKey, Ed25519Signature, KeyId};
}
