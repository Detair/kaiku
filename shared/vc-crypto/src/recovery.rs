//! Recovery Key for E2EE key backup
//!
//! Provides a user-friendly recovery key for backing up E2EE identity keys.
//! The recovery key is a 256-bit random value displayed as Base58 for easy
//! user storage and entry.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::{Argon2, Params};
use serde::{Deserialize, Serialize};
use zeroize::{ZeroizeOnDrop, Zeroizing};

use crate::{CryptoError, Result};

/// Magic bytes for backup verification
const BACKUP_MAGIC: &[u8] = b"CANIS_KEYS_V1";

/// Recovery key for backing up E2EE identity keys.
///
/// A 256-bit random value, displayed as Base58 for user storage.
/// The key is zeroized on drop to prevent memory leaks of sensitive data.
#[derive(Clone, ZeroizeOnDrop)]
pub struct RecoveryKey(pub(crate) [u8; 32]);

/// Derived backup encryption key.
///
/// This key is derived from a [`RecoveryKey`] using Argon2id and is used
/// to encrypt/decrypt the actual key backup data.
/// Automatically zeroizes on drop to prevent sensitive key material from
/// remaining in memory.
#[derive(ZeroizeOnDrop)]
pub struct BackupKey(pub(crate) [u8; 32]);

impl AsRef<[u8]> for BackupKey {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl RecoveryKey {
    /// Generate a new random recovery key using the system CSPRNG.
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        getrandom::getrandom(&mut bytes).expect("Failed to generate random bytes");
        Self(bytes)
    }

    /// Format for user display: groups of 4 Base58 characters separated by spaces.
    ///
    /// A 256-bit key encodes to approximately 43-44 Base58 characters,
    /// resulting in 11 groups (with the last group potentially shorter).
    ///
    /// Example output: `ABCD EFGH IJKL MNOP QRST UVWX YZ12 3456 7890 abc`
    pub fn to_formatted_string(&self) -> String {
        let encoded = bs58::encode(&self.0).into_string();
        encoded
            .chars()
            .collect::<Vec<_>>()
            .chunks(4)
            .map(|c| c.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Parse from formatted string (spaces and other whitespace ignored).
    ///
    /// # Errors
    ///
    /// Returns `CryptoError::InvalidKey` if the input is not valid Base58
    /// or does not decode to exactly 32 bytes.
    pub fn from_formatted_string(s: &str) -> Result<Self> {
        let cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        // Wrap in Zeroizing to ensure the intermediate Vec is zeroized on drop
        let bytes: Zeroizing<Vec<u8>> = Zeroizing::new(
            bs58::decode(&cleaned)
                .into_vec()
                .map_err(|e| CryptoError::InvalidKey(format!("Invalid recovery key: {e}")))?,
        );

        if bytes.len() != 32 {
            return Err(CryptoError::InvalidKey(format!(
                "Recovery key must be 32 bytes, got {}",
                bytes.len()
            )));
        }

        let mut arr = [0u8; 32];
        arr.copy_from_slice(bytes.as_slice());
        Ok(Self(arr))
    }

    /// Derive backup encryption key using Argon2id.
    ///
    /// Uses conservative parameters for key derivation:
    /// - Memory: 64 MiB (65536 KiB)
    /// - Iterations: 3
    /// - Parallelism: 1
    /// - Output: 32 bytes
    ///
    /// The salt should be randomly generated and stored alongside the backup.
    ///
    /// Returns a [`BackupKey`] which automatically zeroizes on drop.
    pub fn derive_backup_key(&self, salt: &[u8; 16]) -> BackupKey {
        let params = Params::new(65536, 3, 1, Some(32)).expect("Invalid Argon2 params");
        let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);

        let mut output = [0u8; 32];
        argon2
            .hash_password_into(&self.0, salt, &mut output)
            .expect("Argon2 hashing failed");
        BackupKey(output)
    }
}

/// Encrypted backup of identity keys.
///
/// Uses AES-256-GCM for authenticated encryption. The backup includes:
/// - A random salt for key derivation
/// - A random nonce for AES-GCM
/// - The encrypted data (with magic prefix for verification)
/// - A version number for future compatibility
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedBackup {
    /// Random salt for key derivation
    pub salt: [u8; 16],
    /// AES-GCM nonce
    pub nonce: [u8; 12],
    /// Encrypted data (includes magic prefix)
    pub ciphertext: Vec<u8>,
    /// Backup version for future compatibility
    pub version: u32,
}

impl EncryptedBackup {
    /// Create an encrypted backup from the provided data.
    ///
    /// The data is encrypted using AES-256-GCM with a key derived from the
    /// recovery key using Argon2id. A magic prefix is prepended to the plaintext
    /// before encryption to allow verification during decryption.
    ///
    /// # Panics
    ///
    /// Panics if the system CSPRNG fails to generate random bytes or if
    /// encryption fails (which should never happen with valid inputs).
    pub fn create(recovery_key: &RecoveryKey, data: &[u8]) -> Self {
        let mut salt = [0u8; 16];
        getrandom::getrandom(&mut salt).expect("Failed to generate salt");

        let mut nonce_bytes = [0u8; 12];
        getrandom::getrandom(&mut nonce_bytes).expect("Failed to generate nonce");

        let backup_key = recovery_key.derive_backup_key(&salt);
        let cipher = Aes256Gcm::new_from_slice(backup_key.as_ref()).expect("Invalid key length");
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Prepend magic bytes for verification
        let mut plaintext = BACKUP_MAGIC.to_vec();
        plaintext.extend_from_slice(data);

        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_slice())
            .expect("Encryption failed");

        Self {
            salt,
            nonce: nonce_bytes,
            ciphertext,
            version: 1,
        }
    }

    /// Decrypt the backup using the provided recovery key.
    ///
    /// # Errors
    ///
    /// Returns `CryptoError::InvalidKey` if the backup key cannot be created.
    /// Returns `CryptoError::DecryptionFailed` if:
    /// - The recovery key is wrong (AES-GCM authentication fails)
    /// - The decrypted data doesn't have the expected magic prefix
    pub fn decrypt(&self, recovery_key: &RecoveryKey) -> Result<Vec<u8>> {
        let backup_key = recovery_key.derive_backup_key(&self.salt);
        let cipher = Aes256Gcm::new_from_slice(backup_key.as_ref())
            .map_err(|_| CryptoError::InvalidKey("Invalid backup key".into()))?;
        let nonce = Nonce::from_slice(&self.nonce);

        let plaintext = cipher
            .decrypt(nonce, self.ciphertext.as_slice())
            .map_err(|_| {
                CryptoError::DecryptionFailed(
                    "Backup decryption failed - wrong recovery key?".into(),
                )
            })?;

        // Verify magic bytes
        if plaintext.len() < BACKUP_MAGIC.len() || &plaintext[..BACKUP_MAGIC.len()] != BACKUP_MAGIC
        {
            return Err(CryptoError::DecryptionFailed(
                "Invalid backup format".into(),
            ));
        }

        Ok(plaintext[BACKUP_MAGIC.len()..].to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recovery_key_generation() {
        let key = RecoveryKey::generate();
        let formatted = key.to_formatted_string();

        // 32 bytes encodes to ~43-44 Base58 chars, giving 11 groups
        // (10 full groups of 4 + 1 partial group of 3-4)
        let groups: Vec<&str> = formatted.split_whitespace().collect();
        assert!(groups.len() >= 10 && groups.len() <= 12);
        // All groups except possibly the last should have 4 chars
        for (i, group) in groups.iter().enumerate() {
            if i < groups.len() - 1 {
                assert_eq!(group.len(), 4);
            } else {
                assert!(group.len() >= 1 && group.len() <= 4);
            }
        }
    }

    #[test]
    fn test_recovery_key_parsing() {
        let key = RecoveryKey::generate();
        let formatted = key.to_formatted_string();

        let parsed = RecoveryKey::from_formatted_string(&formatted).unwrap();
        assert_eq!(key.0, parsed.0);
    }

    #[test]
    fn test_recovery_key_parsing_with_extra_whitespace() {
        let key = RecoveryKey::generate();
        let formatted = key.to_formatted_string();

        // Add extra whitespace
        let with_extra_whitespace = format!("  {}  ", formatted.replace(' ', "   "));
        let parsed = RecoveryKey::from_formatted_string(&with_extra_whitespace).unwrap();
        assert_eq!(key.0, parsed.0);
    }

    #[test]
    fn test_recovery_key_invalid_base58() {
        let result = RecoveryKey::from_formatted_string("0OIl"); // Invalid Base58 chars
        assert!(result.is_err());
        match result {
            Err(CryptoError::InvalidKey(msg)) => {
                assert!(msg.contains("Invalid recovery key"));
            }
            _ => panic!("Expected InvalidKey error"),
        }
    }

    #[test]
    fn test_recovery_key_wrong_length() {
        // Too short - decodes to fewer than 32 bytes
        let result = RecoveryKey::from_formatted_string("ABCD EFGH IJKL");
        assert!(result.is_err());
        match result {
            Err(CryptoError::InvalidKey(msg)) => {
                // Should indicate wrong length
                assert!(
                    msg.contains("must be 32 bytes") || msg.contains("Invalid recovery key"),
                    "Unexpected error message: {}",
                    msg
                );
            }
            _ => panic!("Expected InvalidKey error"),
        }
    }

    #[test]
    fn test_derive_backup_key() {
        let recovery_key = RecoveryKey::generate();
        let salt = [0u8; 16];

        let backup_key = recovery_key.derive_backup_key(&salt);
        assert_eq!(backup_key.as_ref().len(), 32);

        // Same inputs should produce same output (deterministic)
        let backup_key2 = recovery_key.derive_backup_key(&salt);
        assert_eq!(backup_key.as_ref(), backup_key2.as_ref());
    }

    #[test]
    fn test_derive_backup_key_different_salts() {
        let recovery_key = RecoveryKey::generate();
        let salt1 = [0u8; 16];
        let salt2 = [1u8; 16];

        let backup_key1 = recovery_key.derive_backup_key(&salt1);
        let backup_key2 = recovery_key.derive_backup_key(&salt2);

        // Different salts should produce different keys
        assert_ne!(backup_key1.as_ref(), backup_key2.as_ref());
    }

    #[test]
    fn test_derive_backup_key_different_recovery_keys() {
        let recovery_key1 = RecoveryKey::generate();
        let recovery_key2 = RecoveryKey::generate();
        let salt = [0u8; 16];

        let backup_key1 = recovery_key1.derive_backup_key(&salt);
        let backup_key2 = recovery_key2.derive_backup_key(&salt);

        // Different recovery keys should produce different backup keys
        assert_ne!(backup_key1.as_ref(), backup_key2.as_ref());
    }

    #[test]
    fn test_backup_encrypt_decrypt() {
        let recovery_key = RecoveryKey::generate();
        let original_data = b"secret identity keys";

        let backup = EncryptedBackup::create(&recovery_key, original_data);

        let decrypted = backup.decrypt(&recovery_key).unwrap();
        assert_eq!(decrypted, original_data);
    }

    #[test]
    fn test_backup_wrong_key_fails() {
        let recovery_key = RecoveryKey::generate();
        let wrong_key = RecoveryKey::generate();
        let data = b"secret";

        let backup = EncryptedBackup::create(&recovery_key, data);
        let result = backup.decrypt(&wrong_key);

        assert!(result.is_err());
        match result {
            Err(CryptoError::DecryptionFailed(msg)) => {
                assert!(msg.contains("wrong recovery key"));
            }
            _ => panic!("Expected DecryptionFailed error"),
        }
    }

    #[test]
    fn test_backup_serialization() {
        let recovery_key = RecoveryKey::generate();
        let data = b"secret";

        let backup = EncryptedBackup::create(&recovery_key, data);
        let json = serde_json::to_string(&backup).unwrap();
        let restored: EncryptedBackup = serde_json::from_str(&json).unwrap();

        let decrypted = restored.decrypt(&recovery_key).unwrap();
        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_backup_version() {
        let recovery_key = RecoveryKey::generate();
        let backup = EncryptedBackup::create(&recovery_key, b"data");

        assert_eq!(backup.version, 1);
    }

    #[test]
    fn test_backup_unique_salts_and_nonces() {
        let recovery_key = RecoveryKey::generate();
        let data = b"same data";

        let backup1 = EncryptedBackup::create(&recovery_key, data);
        let backup2 = EncryptedBackup::create(&recovery_key, data);

        // Each backup should have unique salt and nonce
        assert_ne!(backup1.salt, backup2.salt);
        assert_ne!(backup1.nonce, backup2.nonce);
        // And therefore different ciphertext
        assert_ne!(backup1.ciphertext, backup2.ciphertext);
    }
}
