//! The v1.0 key-attestation minimum (spec B.1, DC9).
//!
//! The chain is stored **unverified** (a later verifier checks it against
//! Google's roots); here the server only reads the leaf's Android key
//! attestation extension (`KeyDescription`, OID 1.3.6.1.4.1.11129.2.1.17)
//! and refuses a key whose reported properties fall below B.1: security
//! level below TEE, no user authentication, a timeout other than per-use or
//! ≤ 10 s, or an attestation challenge other than the enrolment nonce. That
//! catches app bugs, not a modified app.

use super::UvClass;
use crate::approvals::secrets::ct_eq;

/// The Android key attestation extension.
const KEY_DESCRIPTION_OID: &str = "1.3.6.1.4.1.11129.2.1.17";
/// `AuthorizationList` tags (context-specific, explicit).
const TAG_NO_AUTH_REQUIRED: u32 = 503;
const TAG_USER_AUTH_TYPE: u32 = 504;
const TAG_AUTH_TIMEOUT: u32 = 505;
/// Longest authentication window B.1 accepts (API 26–29 PIN-only phones).
const MAX_WINDOW_SECS: u64 = 10;
/// At most this many certificates are read from a chain.
const MAX_CHAIN: usize = 8;

/// What the leaf reported (stored as risk information).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ReportedProperties {
    /// 0 software, 1 TEE, 2 `StrongBox`.
    pub security_level: u8,
    pub user_auth: bool,
    pub auth_timeout: Option<u32>,
    pub challenge_ok: bool,
}

/// Why a claim is refused (400 `DEVICE_UNSUITABLE {reason}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unsuitable {
    /// No chain, no leaf extension, or an unreadable one.
    AttestationMissing,
    /// The reported properties fall below B.1.
    AttestationProperties,
}

impl Unsuitable {
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::AttestationMissing => "attestation_missing",
            Self::AttestationProperties => "attestation_properties",
        }
    }
}

/// Check the leaf of `chain` (DER, leaf first) against B.1 for `uv_class`.
pub fn check(
    chain: &[Vec<u8>],
    nonce: &[u8; 32],
    uv_class: UvClass,
) -> Result<ReportedProperties, Unsuitable> {
    let leaf = chain.first().ok_or(Unsuitable::AttestationMissing)?;
    if chain.len() > MAX_CHAIN {
        return Err(Unsuitable::AttestationMissing);
    }
    let (_, cert) =
        x509_parser::parse_x509_certificate(leaf).map_err(|_| Unsuitable::AttestationMissing)?;
    let ext = cert
        .extensions()
        .iter()
        .find(|e| e.oid.to_id_string() == KEY_DESCRIPTION_OID)
        .ok_or(Unsuitable::AttestationMissing)?;
    let desc = parse_key_description(ext.value).ok_or(Unsuitable::AttestationMissing)?;
    let props = ReportedProperties {
        security_level: desc.security_level,
        user_auth: desc.user_auth,
        auth_timeout: desc.auth_timeout.and_then(|t| u32::try_from(t).ok()),
        challenge_ok: ct_eq(&desc.challenge, nonce),
    };
    let timeout_ok = match (uv_class, desc.auth_timeout) {
        (UvClass::PerUse, None | Some(0)) => true,
        (UvClass::Window10s, Some(t)) => (1..=MAX_WINDOW_SECS).contains(&t),
        _ => false,
    };
    if props.security_level < 1 || !props.user_auth || !timeout_ok || !props.challenge_ok {
        return Err(Unsuitable::AttestationProperties);
    }
    Ok(props)
}

struct KeyDescription {
    security_level: u8,
    challenge: Vec<u8>,
    user_auth: bool,
    auth_timeout: Option<u64>,
}

/// One DER TLV: (class+constructed bits of the first byte, tag number, content).
struct Tlv<'a> {
    first: u8,
    tag: u32,
    content: &'a [u8],
}

/// Read one TLV from `input`; returns it and the rest. Bounded: long-form
/// lengths up to 4 bytes, tags up to 4 continuation bytes.
fn read_tlv(input: &[u8]) -> Option<(Tlv<'_>, &[u8])> {
    let (&first, mut rest) = input.split_first()?;
    let mut tag = u32::from(first & 0x1f);
    if tag == 0x1f {
        tag = 0;
        for i in 0..4 {
            let (&b, r) = rest.split_first()?;
            rest = r;
            tag = (tag << 7) | u32::from(b & 0x7f);
            if b & 0x80 == 0 {
                break;
            }
            if i == 3 {
                return None;
            }
        }
    }
    let (&len0, mut rest) = rest.split_first()?;
    let len = if len0 & 0x80 == 0 {
        usize::from(len0)
    } else {
        let n = usize::from(len0 & 0x7f);
        if n == 0 || n > 4 || rest.len() < n {
            return None;
        }
        let mut l = 0usize;
        for &b in &rest[..n] {
            l = (l << 8) | usize::from(b);
        }
        rest = &rest[n..];
        l
    };
    if rest.len() < len {
        return None;
    }
    Some((
        Tlv {
            first,
            tag,
            content: &rest[..len],
        },
        &rest[len..],
    ))
}

/// The elements of a SEQUENCE's content.
fn elements(mut content: &[u8]) -> Option<Vec<Tlv<'_>>> {
    let mut out = Vec::new();
    while !content.is_empty() {
        let (t, rest) = read_tlv(content)?;
        out.push(t);
        content = rest;
        if out.len() > 128 {
            return None;
        }
    }
    Some(out)
}

fn uint(content: &[u8]) -> Option<u64> {
    if content.is_empty() || content.len() > 9 || content[0] & 0x80 != 0 {
        return None;
    }
    let mut v = 0u64;
    for &b in content {
        v = v.checked_mul(256)? + u64::from(b);
    }
    Some(v)
}

/// The tags present in an `AuthorizationList` (with their inner value).
fn auth_list(content: &[u8]) -> Option<Vec<(u32, &[u8])>> {
    let mut out = Vec::new();
    for t in elements(content)? {
        // Context-specific, constructed, explicit: the inner TLV is the value.
        if t.first & 0xe0 != 0xa0 {
            continue;
        }
        let inner = read_tlv(t.content).map_or(&[][..], |(i, _)| i.content);
        out.push((t.tag, inner));
    }
    Some(out)
}

/// Parse `KeyDescription` (the extension's OCTET STRING content).
fn parse_key_description(der: &[u8]) -> Option<KeyDescription> {
    let (seq, _) = read_tlv(der)?;
    if seq.first != 0x30 {
        return None;
    }
    let f = elements(seq.content)?;
    // attestationVersion, attestationSecurityLevel, keyMintVersion,
    // keyMintSecurityLevel, attestationChallenge, uniqueId, softwareEnforced,
    // hardwareEnforced.
    if f.len() < 8 {
        return None;
    }
    let attestation_level = u8::try_from(uint(f[1].content)?).ok()?;
    let keymint_level = u8::try_from(uint(f[3].content)?).ok()?;
    let challenge = f[4].content.to_vec();
    let sw = auth_list(f[6].content)?;
    let hw = auth_list(f[7].content)?;
    let has = |tag: u32| hw.iter().chain(sw.iter()).any(|(t, _)| *t == tag);
    let timeout = hw
        .iter()
        .chain(sw.iter())
        .find(|(t, _)| *t == TAG_AUTH_TIMEOUT)
        .and_then(|(_, v)| uint(v));
    Some(KeyDescription {
        security_level: attestation_level.min(keymint_level),
        challenge,
        user_auth: !has(TAG_NO_AUTH_REQUIRED) && has(TAG_USER_AUTH_TYPE),
        auth_timeout: timeout,
    })
}

/// Bound and clean a self-reported device label once at intake (Review Focus 5):
///
/// control characters and bidi overrides (U+202A–U+202E, U+2066–U+2069) removed, then at most
/// 64 characters (a character boundary). Afterwards it is stored, shown and signed exactly as
/// returned.
#[must_use]
pub fn clean_label(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(64)
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "Android phone".to_string()
    } else {
        trimmed.to_string()
    }
}
