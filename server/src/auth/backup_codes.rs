//! MFA Backup Code Generation
//!
//! Provides generation of one-time-use recovery codes for MFA-enabled accounts.
//! Each code is a random 8-character alphanumeric string, hashed with Argon2id
//! before database storage.

use rand::distributions::Alphanumeric;
use rand::Rng;

use super::password::{hash_password, verify_password};

/// Number of backup codes to generate per user.
pub const BACKUP_CODE_COUNT: usize = 10;

/// Length of each backup code (characters).
const BACKUP_CODE_LENGTH: usize = 8;

/// Generate `BACKUP_CODE_COUNT` random alphanumeric backup codes.
///
/// Returns the plaintext codes (shown to the user exactly once) and their
/// Argon2id hashes (stored in the database).
///
/// # Errors
///
/// Returns an error if any code cannot be hashed (e.g., Argon2 failure).
pub fn generate_backup_codes()
-> Result<(Vec<String>, Vec<String>), argon2::password_hash::Error> {
    let mut rng = rand::thread_rng();
    let mut plaintext = Vec::with_capacity(BACKUP_CODE_COUNT);
    let mut hashes = Vec::with_capacity(BACKUP_CODE_COUNT);

    for _ in 0..BACKUP_CODE_COUNT {
        let code: String = (&mut rng)
            .sample_iter(&Alphanumeric)
            .take(BACKUP_CODE_LENGTH)
            .map(char::from)
            .collect();

        let hash = hash_password(&code)?;
        plaintext.push(code);
        hashes.push(hash);
    }

    Ok((plaintext, hashes))
}

/// Verify a plaintext backup code against a list of hashed codes.
///
/// Returns the index of the matching code if found, `None` otherwise.
/// Uses constant-time comparison via Argon2id to resist timing attacks.
pub fn find_matching_backup_code(plaintext: &str, hashes: &[String]) -> Option<usize> {
    for (i, hash) in hashes.iter().enumerate() {
        if verify_password(plaintext, hash).unwrap_or(false) {
            return Some(i);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_backup_codes_count() {
        let (codes, hashes) = generate_backup_codes().expect("generation failed");
        assert_eq!(codes.len(), BACKUP_CODE_COUNT);
        assert_eq!(hashes.len(), BACKUP_CODE_COUNT);
    }

    #[test]
    fn test_codes_are_alphanumeric() {
        let (codes, _) = generate_backup_codes().expect("generation failed");
        for code in &codes {
            assert_eq!(code.len(), BACKUP_CODE_LENGTH);
            assert!(code.chars().all(char::is_alphanumeric), "code '{code}' contains non-alphanumeric characters");
        }
    }

    #[test]
    fn test_codes_are_unique() {
        let (codes, _) = generate_backup_codes().expect("generation failed");
        let mut seen = std::collections::HashSet::new();
        for code in &codes {
            assert!(seen.insert(code.clone()), "duplicate code generated: {code}");
        }
    }

    #[test]
    fn test_find_matching_backup_code_found() {
        let (codes, hashes) = generate_backup_codes().expect("generation failed");
        // The 3rd code (index 2) should be found
        let idx = find_matching_backup_code(&codes[2], &hashes);
        assert_eq!(idx, Some(2));
    }

    #[test]
    fn test_find_matching_backup_code_not_found() {
        let (_, hashes) = generate_backup_codes().expect("generation failed");
        let idx = find_matching_backup_code("XXXXXXXX", &hashes);
        assert!(idx.is_none());
    }

    #[test]
    fn test_all_codes_verify() {
        let (codes, hashes) = generate_backup_codes().expect("generation failed");
        for (i, code) in codes.iter().enumerate() {
            let idx = find_matching_backup_code(code, &hashes);
            assert_eq!(idx, Some(i), "code at index {i} did not verify");
        }
    }
}
