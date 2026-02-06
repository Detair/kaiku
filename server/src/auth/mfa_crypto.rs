//! MFA Secret Encryption
//!
//! Provides AES-256-GCM encryption for MFA secrets stored in the database.
//! This ensures that TOTP secrets are never stored in plaintext.

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use thiserror::Error;

/// Encryption errors.
#[derive(Error, Debug)]
pub enum CryptoError {
    #[error("Invalid encryption key length (expected 32 bytes, got {0})")]
    InvalidKeyLength(usize),

    #[error("Encryption failed: {0}")]
    EncryptionFailed(String),

    #[error("Decryption failed: {0}")]
    DecryptionFailed(String),

    #[error("Invalid encrypted data format")]
    InvalidFormat,

    #[error("Hex decoding failed: {0}")]
    HexError(#[from] hex::FromHexError),
}

pub type CryptoResult<T> = Result<T, CryptoError>;

/// Encrypt an MFA secret using AES-256-GCM.
///
/// # Arguments
/// * `secret` - The plaintext MFA secret (base32 TOTP secret)
/// * `key` - 32-byte encryption key
///
/// # Returns
/// Hex-encoded string containing: nonce(12 bytes) || ciphertext || tag(16 bytes)
///
/// # Example
/// ```ignore
/// let key = hex::decode("0123...").unwrap();
/// let encrypted = encrypt_mfa_secret("JBSWY3DPEHPK3PXP", &key)?;
/// ```
pub fn encrypt_mfa_secret(secret: &str, key: &[u8]) -> CryptoResult<String> {
    // Validate key length
    if key.len() != 32 {
        return Err(CryptoError::InvalidKeyLength(key.len()));
    }

    // Create cipher
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|e| CryptoError::EncryptionFailed(e.to_string()))?;

    // Generate random nonce (12 bytes for GCM)
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

    // Encrypt the secret
    let ciphertext = cipher
        .encrypt(&nonce, secret.as_bytes())
        .map_err(|e| CryptoError::EncryptionFailed(e.to_string()))?;

    // Combine: nonce || ciphertext (which includes the auth tag)
    let mut combined = nonce.to_vec();
    combined.extend_from_slice(&ciphertext);

    // Encode as hex for database storage
    Ok(hex::encode(combined))
}

/// Decrypt an MFA secret using AES-256-GCM.
///
/// # Arguments
/// * `encrypted` - Hex-encoded string containing: nonce(12 bytes) || ciphertext || tag(16 bytes)
/// * `key` - 32-byte encryption key (same as used for encryption)
///
/// # Returns
/// Plaintext MFA secret (base32 TOTP secret)
///
/// # Example
/// ```ignore
/// let key = hex::decode("0123...").unwrap();
/// let secret = decrypt_mfa_secret(&encrypted_hex, &key)?;
/// ```
pub fn decrypt_mfa_secret(encrypted: &str, key: &[u8]) -> CryptoResult<String> {
    // Validate key length
    if key.len() != 32 {
        return Err(CryptoError::InvalidKeyLength(key.len()));
    }

    // Decode hex
    let combined = hex::decode(encrypted)?;

    // Extract nonce (first 12 bytes)
    if combined.len() < 12 {
        return Err(CryptoError::InvalidFormat);
    }

    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);

    // Create cipher
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|e| CryptoError::DecryptionFailed(e.to_string()))?;

    // Decrypt
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| CryptoError::DecryptionFailed(e.to_string()))?;

    // Convert to string
    String::from_utf8(plaintext)
        .map_err(|e| CryptoError::DecryptionFailed(format!("Invalid UTF-8: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = [0u8; 32]; // Test key
        let secret = "JBSWY3DPEHPK3PXP";

        let encrypted = encrypt_mfa_secret(secret, &key).expect("encryption failed");
        let decrypted = decrypt_mfa_secret(&encrypted, &key).expect("decryption failed");

        assert_eq!(secret, decrypted);
    }

    #[test]
    fn test_different_keys_fail() {
        let key1 = [0u8; 32];
        let key2 = [1u8; 32];
        let secret = "JBSWY3DPEHPK3PXP";

        let encrypted = encrypt_mfa_secret(secret, &key1).expect("encryption failed");
        let result = decrypt_mfa_secret(&encrypted, &key2);

        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_key_length() {
        let short_key = [0u8; 16];
        let secret = "JBSWY3DPEHPK3PXP";

        let result = encrypt_mfa_secret(secret, &short_key);
        assert!(matches!(result, Err(CryptoError::InvalidKeyLength(16))));
    }

    #[test]
    fn test_invalid_encrypted_format() {
        let key = [0u8; 32];
        let invalid_hex = "00112233"; // Too short

        let result = decrypt_mfa_secret(invalid_hex, &key);
        assert!(result.is_err());
    }

    #[test]
    fn test_nonce_randomness() {
        let key = [0u8; 32];
        let secret = "JBSWY3DPEHPK3PXP";

        let encrypted1 = encrypt_mfa_secret(secret, &key).expect("encryption 1 failed");
        let encrypted2 = encrypt_mfa_secret(secret, &key).expect("encryption 2 failed");

        // Same plaintext + key should produce different ciphertext due to random nonce
        assert_ne!(encrypted1, encrypted2);

        // But both should decrypt to the same value
        let decrypted1 = decrypt_mfa_secret(&encrypted1, &key).expect("decryption 1 failed");
        let decrypted2 = decrypt_mfa_secret(&encrypted2, &key).expect("decryption 2 failed");
        assert_eq!(decrypted1, secret);
        assert_eq!(decrypted2, secret);
    }
}
