//! The v1.0 key-attestation minimum (spec B.1, DC9).
//!
//! The chain is stored **unverified** (a later verifier checks it against
//! Google's roots); here the server only reads the leaf's Android key
//! attestation extension (`KeyDescription`, OID 1.3.6.1.4.1.11129.2.1.17)
//! and refuses a key whose reported properties fall below B.1: security
//! level below TEE, no user authentication, a timeout other than per-use or
//! ≤ 10 s, or an attestation challenge other than the enrolment nonce. That
//! catches app bugs, not a modified app. User authentication means a
//! hardware-enforced `userAuthType` other than none and no `noAuthRequired`;
//! an unreadable `authTimeout` makes the extension unreadable.

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
    let no_auth = hw
        .iter()
        .chain(sw.iter())
        .any(|(t, _)| *t == TAG_NO_AUTH_REQUIRED);
    // Only the TEE enforces user authentication: `userAuthType` must be
    // hardware-enforced and name an authenticator (0 is `HW_AUTH_NONE`).
    let hw_auth_type = hw
        .iter()
        .any(|(t, v)| *t == TAG_USER_AUTH_TYPE && uint(v).is_some_and(|v| v > 0));
    // Every reported `authTimeout` must be readable (an unreadable one is
    // never "absent", which would pass as per-use); the longest window decides.
    let mut timeout: Option<u64> = None;
    for (_, v) in hw
        .iter()
        .chain(sw.iter())
        .filter(|(t, _)| *t == TAG_AUTH_TIMEOUT)
    {
        let t = uint(v)?;
        timeout = Some(timeout.map_or(t, |m| m.max(t)));
    }
    Some(KeyDescription {
        security_level: attestation_level.min(keymint_level),
        challenge,
        user_auth: !no_auth && hw_auth_type,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tlv(tag: &[u8], content: &[u8]) -> Vec<u8> {
        let mut out = tag.to_vec();
        out.push(u8::try_from(content.len()).unwrap());
        out.extend_from_slice(content);
        out
    }

    /// `[tag] EXPLICIT inner` with a high tag number.
    fn explicit(tag: u32, inner: &[u8]) -> Vec<u8> {
        let t = [
            0xbf,
            0x80 | u8::try_from(tag >> 7).unwrap(),
            u8::try_from(tag & 0x7f).unwrap(),
        ];
        tlv(&t, inner)
    }

    fn int(v: &[u8]) -> Vec<u8> {
        tlv(&[0x02], v)
    }

    /// A TEE `KeyDescription` with the given authorisation lists.
    fn description(sw: &[u8], hw: &[u8]) -> Vec<u8> {
        let mut seq = Vec::new();
        seq.extend(int(&[100]));
        seq.extend(tlv(&[0x0a], &[1]));
        seq.extend(int(&[100]));
        seq.extend(tlv(&[0x0a], &[1]));
        seq.extend(tlv(&[0x04], &[7; 32]));
        seq.extend(tlv(&[0x04], &[]));
        seq.extend(tlv(&[0x30], sw));
        seq.extend(tlv(&[0x30], hw));
        tlv(&[0x30], &seq)
    }

    #[test]
    fn hardware_user_auth_is_user_auth() {
        let d = parse_key_description(&description(&[], &explicit(504, &int(&[2])))).unwrap();
        assert!(d.user_auth);
        assert_eq!(d.auth_timeout, None);
    }

    /// Review round 3 (Argon coverage, part 1 M2): only the TEE enforces
    /// user authentication, so a `userAuthType` in `softwareEnforced` is none.
    #[test]
    fn software_enforced_user_auth_is_no_user_auth() {
        let d = parse_key_description(&description(&explicit(504, &int(&[2])), &[])).unwrap();
        assert!(!d.user_auth);
    }

    /// `userAuthType = 0` is `HW_AUTH_NONE`.
    #[test]
    fn user_auth_type_none_is_no_user_auth() {
        let d = parse_key_description(&description(&[], &explicit(504, &int(&[0])))).unwrap();
        assert!(!d.user_auth);
        let empty = parse_key_description(&description(&[], &explicit(504, &[]))).unwrap();
        assert!(!empty.user_auth);
    }

    #[test]
    fn no_auth_required_anywhere_is_no_user_auth() {
        let mut hw = explicit(504, &int(&[2]));
        let sw = explicit(503, &tlv(&[0x05], &[]));
        assert!(
            !parse_key_description(&description(&sw, &hw))
                .unwrap()
                .user_auth
        );
        hw.extend(explicit(503, &tlv(&[0x05], &[])));
        assert!(
            !parse_key_description(&description(&[], &hw))
                .unwrap()
                .user_auth
        );
    }

    /// A present but unreadable `authTimeout` is never read as "absent"
    /// (which would pass as per-use).
    #[test]
    fn malformed_auth_timeout_is_unreadable() {
        let mut hw = explicit(504, &int(&[2]));
        hw.extend(explicit(505, &int(&[0x80])));
        assert!(parse_key_description(&description(&[], &hw)).is_none());
        let mut hw = explicit(504, &int(&[2]));
        hw.extend(explicit(505, &[]));
        assert!(parse_key_description(&description(&[], &hw)).is_none());
    }

    /// Every reported `authTimeout` counts; the longest window decides.
    #[test]
    fn longest_auth_timeout_decides() {
        let mut hw = explicit(504, &int(&[2]));
        hw.extend(explicit(505, &int(&[0])));
        let sw = explicit(505, &int(&[30]));
        let d = parse_key_description(&description(&sw, &hw)).unwrap();
        assert_eq!(d.auth_timeout, Some(30));
    }
}
