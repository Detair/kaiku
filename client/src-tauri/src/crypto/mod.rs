//! Client-side Cryptography
//!
//! Local storage and management of cryptographic keys for E2EE messaging.

pub mod store;

pub use store::{KeyStoreError, KeyStoreMetadata, LocalKeyStore, SessionKey};
