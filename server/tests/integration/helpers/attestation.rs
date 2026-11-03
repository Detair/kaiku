//! Android key-attestation fixtures: a leaf certificate (rcgen) with the
//! `KeyDescription` extension (OID 1.3.6.1.4.1.11129.2.1.17) built from the
//! properties a test wants. Generated at test time; nothing is stored.

/// What the fake leaf reports.
#[derive(Clone)]
pub struct Reported {
    /// 0 software, 1 TEE, 2 StrongBox.
    pub level: u8,
    pub user_auth: bool,
    pub timeout: Option<u8>,
    pub challenge: Vec<u8>,
}

impl Reported {
    /// A TEE key with per-use user authentication for `challenge`.
    pub fn good(challenge: &[u8]) -> Self {
        Self {
            level: 1,
            user_auth: true,
            timeout: None,
            challenge: challenge.to_vec(),
        }
    }
}

fn tlv(tag: &[u8], content: &[u8]) -> Vec<u8> {
    let mut out = tag.to_vec();
    let len = content.len();
    if len < 128 {
        out.push(u8::try_from(len).unwrap());
    } else {
        out.push(0x82);
        out.extend_from_slice(&u16::try_from(len).unwrap().to_be_bytes());
    }
    out.extend_from_slice(content);
    out
}

/// `[tag] EXPLICIT inner` with a high tag number (≥ 31).
fn explicit(tag: u32, inner: &[u8]) -> Vec<u8> {
    let t = [
        0xbf,
        0x80 | u8::try_from(tag >> 7).unwrap(),
        u8::try_from(tag & 0x7f).unwrap(),
    ];
    tlv(&t, inner)
}

/// DER `KeyDescription`.
pub fn key_description(r: &Reported) -> Vec<u8> {
    let mut hw = Vec::new();
    if r.user_auth {
        hw.extend(explicit(504, &tlv(&[0x02], &[2])));
    } else {
        hw.extend(explicit(503, &tlv(&[0x05], &[])));
    }
    if let Some(t) = r.timeout {
        hw.extend(explicit(505, &tlv(&[0x02], &[t])));
    }
    let mut seq = Vec::new();
    seq.extend(tlv(&[0x02], &[100]));
    seq.extend(tlv(&[0x0a], &[r.level]));
    seq.extend(tlv(&[0x02], &[100]));
    seq.extend(tlv(&[0x0a], &[r.level]));
    seq.extend(tlv(&[0x04], &r.challenge));
    seq.extend(tlv(&[0x04], &[]));
    seq.extend(tlv(&[0x30], &[]));
    seq.extend(tlv(&[0x30], &hw));
    tlv(&[0x30], &seq)
}

/// A one-certificate chain whose leaf reports `r` (DER).
pub fn chain(r: &Reported) -> Vec<Vec<u8>> {
    let mut params = rcgen::CertificateParams::new(vec!["android-keystore".to_string()]).unwrap();
    params
        .custom_extensions
        .push(rcgen::CustomExtension::from_oid_content(
            &[1, 3, 6, 1, 4, 1, 11129, 2, 1, 17],
            key_description(r),
        ));
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = params.self_signed(&key).unwrap();
    vec![cert.der().to_vec()]
}

/// A leaf without the attestation extension.
pub fn chain_without_extension() -> Vec<Vec<u8>> {
    let params = rcgen::CertificateParams::new(vec!["plain".to_string()]).unwrap();
    let key = rcgen::KeyPair::generate().unwrap();
    vec![params.self_signed(&key).unwrap().der().to_vec()]
}
