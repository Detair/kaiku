//! Pre-session secrets (B.2 "Data model", CC3).
//!
//! Transfer tokens, codes, setup tokens, claim, collection and request
//! secrets and registration tickets are 256-bit CSPRNG values (codes: six
//! digits), single use, and stored only as an HMAC under a server key held
//! outside the database ([`keyed_hash`]). They are never UUIDs, never logged
//! and always compared in constant time ([`ct_eq`]).

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use hmac::{Hmac, Mac};
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::Sha256;
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// A 256-bit secret from the operating system's CSPRNG.
///
/// `Debug` prints `[REDACTED]`; the bytes are zeroized on drop.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Secret256([u8; 32]);

impl Secret256 {
    /// A fresh secret from `OsRng`.
    #[must_use]
    pub fn random() -> Self {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        Self(bytes)
    }

    /// Parse the canonical 43-character b64u form; anything else is `None`.
    #[must_use]
    pub fn from_b64u(s: &str) -> Option<Self> {
        let mut decoded = b64u_decode(s)?;
        let out = <[u8; 32]>::try_from(decoded.as_slice()).ok().map(Self);
        decoded.zeroize();
        out
    }

    /// The canonical b64u form (43 characters, no padding).
    #[must_use]
    pub fn to_b64u(&self) -> String {
        b64u_encode(&self.0)
    }

    /// The raw bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for Secret256 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[REDACTED]")
    }
}

/// What a secret is for. Each purpose has its own HMAC domain, so a value of
/// one kind can never be presented as another. Append-only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecretPurpose {
    LoginToken,
    CollectionSecret,
    SetupToken,
    EnrollCode,
    ClaimSecret,
    RequestSecret,
    RegistrationTicket,
    /// The desktop passkey hand-off code (LP, C29 step 4).
    PasskeyHandoffCode,
}

impl SecretPurpose {
    /// The domain label inside the HMAC input.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::LoginToken => "login_token",
            Self::CollectionSecret => "collection_secret",
            Self::SetupToken => "setup_token",
            Self::EnrollCode => "enroll_code",
            Self::ClaimSecret => "claim_secret",
            Self::RequestSecret => "request_secret",
            Self::RegistrationTicket => "registration_ticket",
            Self::PasskeyHandoffCode => "passkey_handoff_code",
        }
    }
}

/// HMAC-SHA256(key, `"kaiku-secret-v1/" ‖ purpose label ‖ 0x00 ‖ secret`).
#[must_use]
pub fn keyed_hash(key: &[u8; 32], purpose: SecretPurpose, secret: &[u8]) -> [u8; 32] {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(b"kaiku-secret-v1/");
    mac.update(purpose.label().as_bytes());
    mac.update(&[0x00]);
    mac.update(secret);
    mac.finalize().into_bytes().into()
}

/// Constant-time equality for secrets and their hashes. Different lengths
/// compare unequal (the length itself is not secret).
#[must_use]
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
}

/// base64url without padding (C26 common rules).
#[must_use]
pub fn b64u_encode(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Strict base64url without padding: padding, the standard alphabet and
/// non-canonical trailing bits are refused.
#[must_use]
pub fn b64u_decode(s: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(s.as_bytes()).ok()
}

/// `n` uniformly distributed ASCII digits from the CSPRNG (rejection
/// sampling, so no digit is more likely than another).
#[must_use]
pub fn random_digits(n: usize) -> String {
    let mut out = String::with_capacity(n);
    let mut buf = [0u8; 16];
    while out.len() < n {
        OsRng.fill_bytes(&mut buf);
        for &b in &buf {
            // 250 = 25 * 10: bytes 250..=255 would bias the low digits.
            if b < 250 && out.len() < n {
                out.push(char::from(b'0' + b % 10));
            }
        }
    }
    buf.zeroize();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyed_hash_depends_on_purpose_and_key() {
        let k1 = [1u8; 32];
        let k2 = [2u8; 32];
        let a = keyed_hash(&k1, SecretPurpose::LoginToken, b"secret");
        assert_eq!(a, keyed_hash(&k1, SecretPurpose::LoginToken, b"secret"));
        assert_ne!(a, keyed_hash(&k1, SecretPurpose::SetupToken, b"secret"));
        assert_ne!(a, keyed_hash(&k2, SecretPurpose::LoginToken, b"secret"));
        assert_ne!(a, keyed_hash(&k1, SecretPurpose::LoginToken, b"secreT"));
    }

    #[test]
    fn secret_b64u_is_43_chars_and_round_trips() {
        let s = Secret256::random();
        let enc = s.to_b64u();
        assert_eq!(enc.len(), 43);
        let back = Secret256::from_b64u(&enc).expect("round trip");
        assert_eq!(back.as_bytes(), s.as_bytes());
        assert_ne!(Secret256::random().as_bytes(), s.as_bytes());
        assert_eq!(format!("{s:?}"), "[REDACTED]");
        assert!(Secret256::from_b64u(&enc[..42]).is_none());
    }

    #[test]
    fn b64u_rejects_padding_and_noncanonical() {
        assert_eq!(b64u_decode("AQI").as_deref(), Some(&[1u8, 2][..]));
        assert!(b64u_decode("AQI=").is_none());
        // 'AQJ' decodes to the same bytes with non-zero trailing bits.
        assert!(b64u_decode("AQJ").is_none());
        assert!(b64u_decode("AQ+/").is_none());
        assert_eq!(b64u_encode(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn ct_eq_handles_length_mismatch() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"abcd"));
        assert!(!ct_eq(b"", b"a"));
    }

    #[test]
    fn random_digits_are_six_ascii_digits() {
        for _ in 0..200 {
            let d = random_digits(6);
            assert_eq!(d.len(), 6);
            assert!(d.bytes().all(|b| b.is_ascii_digit()), "{d}");
        }
    }
}
