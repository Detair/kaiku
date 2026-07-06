//! Password Hashing with Argon2id

use std::sync::LazyLock;

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;

/// A dummy Argon2id hash used to equalize login timing.
///
/// When a login targets a non-existent user (or an account with no local
/// password), the handler still verifies the submitted password against this
/// hash so the request costs the same Argon2 work as a real check. Without it,
/// missing users return before any hashing and are distinguishable by response
/// time (username enumeration). Computed once via [`hash_password`], so its
/// cost parameters match every real hash exactly.
pub static DUMMY_PASSWORD_HASH: LazyLock<String> = LazyLock::new(|| {
    // The plaintext is irrelevant and never compared to a user input that could
    // match — only the hash's Argon2 cost parameters matter for timing.
    hash_password("kaiku-login-timing-equalizer")
        .expect("Argon2 dummy hash generation must not fail")
});

/// Hash a password using Argon2id.
pub fn hash_password(password: &str) -> Result<String, argon2::password_hash::Error> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    Ok(argon2
        .hash_password(password.as_bytes(), &salt)?
        .to_string())
}

/// Verify a password against a hash.
pub fn verify_password(password: &str, hash: &str) -> Result<bool, argon2::password_hash::Error> {
    let parsed_hash = PasswordHash::new(hash)?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_verifies() {
        let hash = hash_password("correct horse battery staple").unwrap();
        assert!(verify_password("correct horse battery staple", &hash).unwrap());
        assert!(!verify_password("wrong password", &hash).unwrap());
    }

    #[test]
    fn dummy_hash_is_valid_and_never_matches_trivially() {
        // Must be a well-formed Argon2 hash (so the login timing-equalizer path
        // returns Ok(false), not an error) and must not accept an empty guess.
        let dummy = DUMMY_PASSWORD_HASH.as_str();
        assert!(!verify_password("", dummy).unwrap());
        assert!(!verify_password("some-guess", dummy).unwrap());
    }
}
