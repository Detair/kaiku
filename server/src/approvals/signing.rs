//! The `kaiku-sig-v1` signed message format (spec B.2, contract C26).
//!
//! ```text
//! M            = "kaiku-sig-v1" ‖ kind ‖ challenge_id ‖ nonce[32] ‖ i64(expires_at) ‖ lp(origin)
//!                ‖ user_id ‖ device_id ‖ context_hash ‖ decision ‖ lp(response)
//! context_hash = SHA-256("kaiku-sig-context-v1" ‖ kind ‖ C)
//! ```
//!
//! Any change to these bytes needs a new magic string. Signatures are P-256
//! ECDSA over SHA-256(M), ASN.1 DER; high and low `s` are both accepted
//! (Android Keystore produces both). This module only verifies: the server
//! never holds a device key.

use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use p256::pkcs8::spki::der::Decode;
use p256::pkcs8::spki::{ObjectIdentifier, SubjectPublicKeyInfoRef};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Magic of every signed message (12 ASCII bytes, no terminator).
pub const MAGIC: &[u8; 12] = b"kaiku-sig-v1";
/// Magic of every context hash (20 ASCII bytes, no terminator).
pub const CONTEXT_MAGIC: &[u8; 20] = b"kaiku-sig-context-v1";

/// `id-ecPublicKey` (RFC 5480).
const ID_EC_PUBLIC_KEY: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.2.1");
/// `secp256r1` / `prime256v1` (RFC 5480).
const SECP256R1: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.3.1.7");

/// Challenge kinds (C26 kind bytes). 0x10–0x1F are reserved for v1.1 agent
/// kinds; every other byte is unknown and refused. Append-only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Kind {
    Login = 1,
    Enroll = 2,
    DeviceLogin = 3,
    DeviceStepup = 4,
    EnrollPop = 5,
}

impl Kind {
    /// The kind for a byte, `None` for every byte v1.0 does not know.
    #[must_use]
    pub const fn from_byte(b: u8) -> Option<Self> {
        match b {
            1 => Some(Self::Login),
            2 => Some(Self::Enroll),
            3 => Some(Self::DeviceLogin),
            4 => Some(Self::DeviceStepup),
            5 => Some(Self::EnrollPop),
            _ => None,
        }
    }

    /// The JSON / database form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Login => "login",
            Self::Enroll => "enroll",
            Self::DeviceLogin => "device_login",
            Self::DeviceStepup => "device_stepup",
            Self::EnrollPop => "enroll_pop",
        }
    }

    /// Parse the JSON / database form.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "login" => Some(Self::Login),
            "enroll" => Some(Self::Enroll),
            "device_login" => Some(Self::DeviceLogin),
            "device_stepup" => Some(Self::DeviceStepup),
            "enroll_pop" => Some(Self::EnrollPop),
            _ => None,
        }
    }

    /// `login` and `enroll` are approvals (a transfer, deny/report); the
    /// others are proofs (approve only).
    #[must_use]
    pub const fn is_approval(self) -> bool {
        matches!(self, Self::Login | Self::Enroll)
    }
}

/// The decision byte. `Deny` is reserved: v1.0 never signs a deny.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Decision {
    Approve = 1,
    Deny = 2,
    Report = 3,
}

/// Everything in M before the decision.
#[derive(Debug, Clone)]
pub struct SigningInput<'a> {
    pub kind: Kind,
    pub challenge_id: Uuid,
    pub nonce: [u8; 32],
    pub expires_at: i64,
    pub origin: &'a str,
    pub user_id: Uuid,
    pub device_id: Uuid,
    pub context_hash: [u8; 32],
}

/// Why a key or signature was refused.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SignatureError {
    /// Not a P-256 `SubjectPublicKeyInfo` with an uncompressed point.
    #[error("public key is not an uncompressed P-256 key")]
    BadKey,
    /// Not an ASN.1 DER ECDSA signature.
    #[error("signature is not DER")]
    BadSignature,
    /// Well-formed, but does not verify over the message.
    #[error("signature does not verify")]
    Invalid,
}

/// `lp(x)` = u32 big-endian byte length ‖ x.
///
/// # Panics
///
/// If `bytes` is longer than `u32::MAX` (never for the bounded inputs here).
pub fn lp(out: &mut Vec<u8>, bytes: &[u8]) {
    let len = u32::try_from(bytes.len()).expect("lp input fits u32");
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(bytes);
}

/// `SHA-256("kaiku-sig-context-v1" ‖ kind ‖ C)`.
#[must_use]
pub fn context_hash(kind: Kind, canonical: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(CONTEXT_MAGIC);
    h.update([kind as u8]);
    h.update(canonical);
    h.finalize().into()
}

/// M up to and including `context_hash` (the `signing_prefix` the device
/// receives; it appends `decision ‖ lp(response)`).
#[must_use]
pub fn signing_prefix(input: &SigningInput<'_>) -> Vec<u8> {
    let mut out = Vec::with_capacity(137 + input.origin.len());
    out.extend_from_slice(MAGIC);
    out.push(input.kind as u8);
    out.extend_from_slice(input.challenge_id.as_bytes());
    out.extend_from_slice(&input.nonce);
    out.extend_from_slice(&input.expires_at.to_be_bytes());
    lp(&mut out, input.origin.as_bytes());
    out.extend_from_slice(input.user_id.as_bytes());
    out.extend_from_slice(input.device_id.as_bytes());
    out.extend_from_slice(&input.context_hash);
    out
}

/// The full M: `prefix ‖ decision ‖ lp(response)`.
#[must_use]
pub fn message(prefix: &[u8], decision: Decision, response: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(prefix.len() + 5 + response.len());
    out.extend_from_slice(prefix);
    out.push(decision as u8);
    lp(&mut out, response);
    out
}

/// Parse a P-256 `SubjectPublicKeyInfo` DER with an uncompressed point
/// (Android `getEncoded()`). Compressed points, other curves and trailing
/// bytes are refused.
pub fn parse_public_key(spki_der: &[u8]) -> Result<VerifyingKey, SignatureError> {
    let spki = SubjectPublicKeyInfoRef::from_der(spki_der).map_err(|_| SignatureError::BadKey)?;
    if spki.algorithm.oid != ID_EC_PUBLIC_KEY {
        return Err(SignatureError::BadKey);
    }
    let curve: ObjectIdentifier = spki
        .algorithm
        .parameters_oid()
        .map_err(|_| SignatureError::BadKey)?;
    if curve != SECP256R1 {
        return Err(SignatureError::BadKey);
    }
    let point = spki
        .subject_public_key
        .as_bytes()
        .ok_or(SignatureError::BadKey)?;
    if point.len() != 65 || point[0] != 0x04 {
        return Err(SignatureError::BadKey);
    }
    VerifyingKey::from_sec1_bytes(point).map_err(|_| SignatureError::BadKey)
}

/// Verify a DER ECDSA signature over SHA-256(`message`). A high-`s`
/// signature is normalised first, so both forms verify.
pub fn verify(key: &VerifyingKey, message: &[u8], der_sig: &[u8]) -> Result<(), SignatureError> {
    let sig = Signature::from_der(der_sig).map_err(|_| SignatureError::BadSignature)?;
    let sig = sig.normalize_s().unwrap_or(sig);
    key.verify(message, &sig)
        .map_err(|_| SignatureError::Invalid)
}
