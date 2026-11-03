//! Passkey relying-party configuration (C29 "RP and origins").

use crate::config::PublicOrigin;

/// The two Kaiku APKs (C29): the full app and Kaiku Einfach (v1.2).
pub const DEFAULT_ANDROID_APP_IDS: [&str; 2] = ["io.wolftown.kaiku", "io.wolftown.kaiku.simple"];

/// Colon hex SHA-256 of the official release signing certificate.
///
/// Public data; both APKs are signed with it. `None` until the maintainer
/// supplies it (LP Q1): official APKs then need `PASSKEY_ANDROID_CERT_SHA256`,
/// and without it the server has no Android passkey origin.
pub const OFFICIAL_RELEASE_CERT_SHA256: Option<&str> = None;

const IDS_VAR: &str = "PASSKEY_ANDROID_APP_IDS";
const CERTS_VAR: &str = "PASSKEY_ANDROID_CERT_SHA256";

/// The relying party: RP ID, web origin and the Android apps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasskeyConfig {
    /// `PublicOrigin.host` (already lower-case punycode; never a parent domain).
    pub rp_id: String,
    /// `PublicOrigin.origin` (`scheme://host[:port]`).
    pub web_origin: url::Url,
    /// Java package names of the Android apps (`assetlinks.json`).
    pub android_app_ids: Vec<String>,
    /// SHA-256 digests of their signing certificates.
    pub android_cert_sha256: Vec<[u8; 32]>,
}

/// One Android variable: unset/empty (the default), the literal `none`, or
/// an explicit comma list.
enum Setting<T> {
    Default,
    Off,
    List(Vec<T>),
}

fn setting<T>(
    raw: Option<&str>,
    parse: impl Fn(&str) -> anyhow::Result<T>,
) -> anyhow::Result<Setting<T>> {
    let Some(raw) = raw.map(str::trim).filter(|r| !r.is_empty()) else {
        return Ok(Setting::Default);
    };
    if raw.eq_ignore_ascii_case("none") {
        return Ok(Setting::Off);
    }
    let items = raw
        .split(',')
        .map(str::trim)
        .map(parse)
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(Setting::List(items))
}

/// A Java package name: at least two dot-separated segments of
/// `[A-Za-z][A-Za-z0-9_]*`.
fn parse_app_id(raw: &str) -> anyhow::Result<String> {
    let segments: Vec<&str> = raw.split('.').collect();
    let ok = segments.len() >= 2
        && segments.iter().all(|s| {
            let mut chars = s.chars();
            chars.next().is_some_and(|c| c.is_ascii_alphabetic())
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
    if !ok {
        anyhow::bail!("{IDS_VAR} must be a comma list of Android package names (io.example.app)");
    }
    Ok(raw.to_string())
}

impl PasskeyConfig {
    /// Build from `PUBLIC_URL` (parsed by L1a) and the two Android
    /// variables. Ids without certificates, or certificates without ids,
    /// refuse startup; error messages name the variable and the rule and never
    /// echo a value.
    pub fn new(
        public: &PublicOrigin,
        app_ids: Option<&str>,
        certs: Option<&str>,
    ) -> anyhow::Result<Self> {
        let web_origin = url::Url::parse(&public.origin)
            .map_err(|_| anyhow::anyhow!("PUBLIC_URL must be an absolute https:// URL"))?;
        let ids = setting(app_ids, parse_app_id)?;
        let certs = setting(certs, parse_cert_fingerprint)?;
        let official = || -> anyhow::Result<Vec<[u8; 32]>> {
            OFFICIAL_RELEASE_CERT_SHA256
                .iter()
                .map(|raw| parse_cert_fingerprint(raw))
                .collect()
        };
        let default_ids = || {
            DEFAULT_ANDROID_APP_IDS
                .iter()
                .map(|s| (*s).to_string())
                .collect()
        };
        let (android_app_ids, android_cert_sha256): (Vec<String>, Vec<[u8; 32]>) = match (
            ids, certs,
        ) {
            (Setting::Off, Setting::Off | Setting::Default) | (Setting::Default, Setting::Off) => {
                (Vec::new(), Vec::new())
            }
            (Setting::Off, Setting::List(_)) => anyhow::bail!(
                "{CERTS_VAR} is set but {IDS_VAR}=none; set the app ids or unset the certificates"
            ),
            (Setting::List(_), Setting::Off) => anyhow::bail!(
                "{IDS_VAR} is set but {CERTS_VAR}=none; set the signing certificates of those apps"
            ),
            (Setting::Default, Setting::Default) => {
                let certs = official()?;
                if certs.is_empty() {
                    (Vec::new(), Vec::new())
                } else {
                    (default_ids(), certs)
                }
            }
            (Setting::List(ids), Setting::Default) => {
                let certs = official()?;
                if certs.is_empty() {
                    anyhow::bail!(
                            "{IDS_VAR} is set but {CERTS_VAR} is not; set the signing certificates of those apps"
                        );
                }
                (ids, certs)
            }
            (Setting::Default, Setting::List(certs)) => (default_ids(), certs),
            (Setting::List(ids), Setting::List(certs)) => (ids, certs),
        };
        Ok(Self {
            rp_id: public.host.clone(),
            web_origin,
            android_app_ids,
            android_cert_sha256,
        })
    }

    /// One `android:apk-key-hash:` origin per certificate.
    #[must_use]
    pub fn android_origins(&self) -> Vec<String> {
        self.android_cert_sha256
            .iter()
            .map(apk_key_hash_origin)
            .collect()
    }
}

/// A certificate SHA-256 in hex, with or without colons, any case
/// (`apksigner` prints lower case without colons, `keytool` upper case with).
pub fn parse_cert_fingerprint(raw: &str) -> anyhow::Result<[u8; 32]> {
    let raw = raw.trim();
    let hex_str: String = if raw.contains(':') {
        let parts: Vec<&str> = raw.split(':').collect();
        if parts.len() != 32 || parts.iter().any(|p| p.len() != 2) {
            anyhow::bail!("{CERTS_VAR} entries must be 32-byte SHA-256 digests in hex");
        }
        parts.concat()
    } else {
        raw.to_string()
    };
    let bytes = hex::decode(&hex_str).map_err(|_| {
        anyhow::anyhow!("{CERTS_VAR} entries must be 32-byte SHA-256 digests in hex")
    })?;
    <[u8; 32]>::try_from(bytes.as_slice())
        .map_err(|_| anyhow::anyhow!("{CERTS_VAR} entries must be 32-byte SHA-256 digests in hex"))
}

/// `assetlinks.json` form: upper-case colon-separated hex.
#[must_use]
pub fn assetlinks_fingerprint(fp: &[u8; 32]) -> String {
    fp.iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// `clientDataJSON` form: `android:apk-key-hash:` + unpadded base64url.
#[must_use]
pub fn apk_key_hash_origin(fp: &[u8; 32]) -> String {
    format!(
        "android:apk-key-hash:{}",
        crate::approvals::secrets::b64u_encode(fp)
    )
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::*;

    fn public(raw: &str) -> PublicOrigin {
        PublicOrigin::parse(raw).expect("valid PUBLIC_URL")
    }

    const TEST_CERT: &str =
        "18:6F:F0:75:BF:FB:D4:76:5E:1C:5C:D9:E1:22:C4:09:44:58:A3:58:BC:E5:34:F5:3E:49:D7:B2:C3:81:AB:C0";

    #[test]
    fn rp_id_is_the_host() {
        let c = PasskeyConfig::new(&public("https://Chat.Example.com/"), None, None).unwrap();
        assert_eq!(c.rp_id, "chat.example.com");
        assert_eq!(c.web_origin.as_str(), "https://chat.example.com/");
        assert_eq!(
            c.web_origin.origin().ascii_serialization(),
            "https://chat.example.com"
        );
    }

    #[test]
    fn port_kept_in_origin_not_in_rp_id() {
        let c = PasskeyConfig::new(&public("https://chat.example.com:8443"), None, None).unwrap();
        assert_eq!(c.rp_id, "chat.example.com");
        assert_eq!(
            c.web_origin.origin().ascii_serialization(),
            "https://chat.example.com:8443"
        );
    }

    #[test]
    fn idn_rp_id_is_punycode() {
        let c = PasskeyConfig::new(&public("https://bücher.example"), None, None).unwrap();
        assert_eq!(c.rp_id, "xn--bcher-kva.example");
    }

    #[test]
    fn android_defaults() {
        let c = PasskeyConfig::new(&public("https://chat.example.com"), None, None).unwrap();
        let official: Vec<[u8; 32]> = OFFICIAL_RELEASE_CERT_SHA256
            .iter()
            .map(|raw| parse_cert_fingerprint(raw).unwrap())
            .collect();
        assert_eq!(c.android_cert_sha256, official);
        if official.is_empty() {
            // No official certificate compiled in yet (Q1): Android is off.
            assert!(c.android_app_ids.is_empty());
        } else {
            assert_eq!(c.android_app_ids, DEFAULT_ANDROID_APP_IDS.to_vec());
        }
        // With an operator certificate, the default app ids apply.
        let c = PasskeyConfig::new(
            &public("https://chat.example.com"),
            Some(""),
            Some(TEST_CERT),
        )
        .unwrap();
        assert_eq!(c.android_app_ids, DEFAULT_ANDROID_APP_IDS.to_vec());
        assert_eq!(c.android_cert_sha256.len(), 1);
    }

    #[test]
    fn android_none_disables() {
        let p = public("https://chat.example.com");
        for (ids, certs) in [
            (Some("none"), None),
            (None, Some("none")),
            (Some("none"), Some("none")),
            (Some("NONE"), Some("")),
        ] {
            let c = PasskeyConfig::new(&p, ids, certs).unwrap();
            assert!(c.android_app_ids.is_empty(), "{ids:?} {certs:?}");
            assert!(c.android_cert_sha256.is_empty(), "{ids:?} {certs:?}");
            assert!(c.android_origins().is_empty());
        }
    }

    #[test]
    fn android_ids_and_certs_must_pair() {
        let p = public("https://chat.example.com");
        // Ids set, certificates off (or the reverse) → refused.
        let err = PasskeyConfig::new(&p, Some("io.example.app"), Some("none")).unwrap_err();
        assert!(
            err.to_string().contains("PASSKEY_ANDROID_CERT_SHA256"),
            "{err}"
        );
        let err = PasskeyConfig::new(&p, Some("none"), Some(TEST_CERT)).unwrap_err();
        assert!(err.to_string().contains("PASSKEY_ANDROID_APP_IDS"), "{err}");
        // Explicit ids without any certificate (none compiled in) → refused.
        if OFFICIAL_RELEASE_CERT_SHA256.is_none() {
            let err = PasskeyConfig::new(&p, Some("io.example.app"), None).unwrap_err();
            assert!(
                err.to_string().contains("PASSKEY_ANDROID_CERT_SHA256"),
                "{err}"
            );
        }
        // Bad package names are refused without echoing the value.
        let err =
            PasskeyConfig::new(&p, Some("io.example.app,bad name"), Some(TEST_CERT)).unwrap_err();
        assert!(err.to_string().contains("PASSKEY_ANDROID_APP_IDS"), "{err}");
        assert!(!err.to_string().contains("bad name"), "{err}");
        let c = PasskeyConfig::new(
            &p,
            Some(" io.example.app , io.example.app2 "),
            Some(&format!("{TEST_CERT},{}", "00".repeat(32))),
        )
        .unwrap();
        assert_eq!(c.android_app_ids, vec!["io.example.app", "io.example.app2"]);
        assert_eq!(c.android_cert_sha256.len(), 2);
    }

    #[test]
    fn fingerprint_encodings_match_c29_vectors() {
        let mut seq = [0u8; 32];
        for (i, b) in seq.iter_mut().enumerate() {
            *b = u8::try_from(i).unwrap();
        }
        assert_eq!(
            assetlinks_fingerprint(&seq),
            "00:01:02:03:04:05:06:07:08:09:0A:0B:0C:0D:0E:0F:10:11:12:13:14:15:16:17:18:19:1A:1B:1C:1D:1E:1F"
        );
        assert_eq!(
            apk_key_hash_origin(&seq),
            "android:apk-key-hash:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8"
        );
        let test: [u8; 32] = Sha256::digest(b"kaiku-test-cert").into();
        assert_eq!(assetlinks_fingerprint(&test), TEST_CERT);
        assert_eq!(
            apk_key_hash_origin(&test),
            "android:apk-key-hash:GG_wdb_71HZeHFzZ4SLECURYo1i85TT1PknXssOBq8A"
        );
    }

    #[test]
    fn fingerprint_parse_accepts_apksigner_and_keytool_forms() {
        let test: [u8; 32] = Sha256::digest(b"kaiku-test-cert").into();
        // keytool: upper case with colons; apksigner: lower case without.
        assert_eq!(parse_cert_fingerprint(TEST_CERT).unwrap(), test);
        assert_eq!(
            parse_cert_fingerprint(&TEST_CERT.replace(':', "").to_lowercase()).unwrap(),
            test
        );
        assert_eq!(
            parse_cert_fingerprint(&format!(" {} ", TEST_CERT.to_lowercase())).unwrap(),
            test
        );
    }

    #[test]
    fn fingerprint_parse_rejects_wrong_length() {
        for bad in [
            "",
            "00",
            &"00".repeat(31),
            &"00".repeat(33),
            &format!("{TEST_CERT}:"),
            "zz".repeat(32).as_str(),
            "0:00:00",
        ] {
            let err = parse_cert_fingerprint(bad).unwrap_err();
            assert!(
                err.to_string().contains("PASSKEY_ANDROID_CERT_SHA256"),
                "{bad}"
            );
        }
    }
}
