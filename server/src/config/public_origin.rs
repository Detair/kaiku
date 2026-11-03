//! `PUBLIC_URL` → the signature origin and RP host (B.1, C26).
//!
//! `PUBLIC_URL` is mandatory: device signatures (C26) bind the origin and
//! passkeys (C29) the host. The server refuses to start without a valid one.

use anyhow::{bail, Result};
use hkdf::Hkdf;
use sha2::Sha256;

/// The public origin of this server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicOrigin {
    /// ASCII serialisation of the origin: `https://chat.example.com`
    /// (lowercase, punycode, no trailing slash, a non-default port kept).
    pub origin: String,
    /// The host alone (`chat.example.com`), for the passkey RP ID.
    pub host: String,
}

impl PublicOrigin {
    /// Parse `PUBLIC_URL`. One trailing `/` is ignored; anything beyond the
    /// origin (path, query, fragment, userinfo), an IP literal, or `http` for
    /// a host other than `localhost` is refused. Errors name `PUBLIC_URL`
    /// and the rule and never echo the value.
    pub fn parse(raw: &str) -> Result<Self> {
        let stripped = raw.strip_suffix('/').unwrap_or(raw);
        if stripped.is_empty() {
            bail!("PUBLIC_URL must be set to the public https:// address of this server");
        }
        let Ok(url) = url::Url::parse(stripped) else {
            bail!("PUBLIC_URL must be an absolute URL such as https://chat.example.com");
        };
        let host = match url.host() {
            Some(url::Host::Domain(d)) => d.to_string(),
            Some(url::Host::Ipv4(_) | url::Host::Ipv6(_)) => {
                bail!("PUBLIC_URL must name a host, not an IP address")
            }
            None => bail!("PUBLIC_URL must name a host"),
        };
        match url.scheme() {
            "https" => {}
            "http" if host == "localhost" => {}
            "http" => bail!("PUBLIC_URL must use https:// (http:// only for localhost)"),
            _ => bail!("PUBLIC_URL must use https://"),
        }
        if !url.username().is_empty() || url.password().is_some() {
            bail!("PUBLIC_URL must not contain user information");
        }
        if stripped.ends_with('/') || url.path() != "/" {
            bail!("PUBLIC_URL must not contain a path");
        }
        if url.query().is_some() || url.fragment().is_some() {
            bail!("PUBLIC_URL must not contain a query or fragment");
        }
        Ok(Self {
            origin: url.origin().ascii_serialization(),
            host,
        })
    }
}

/// `PUBLIC_URL` from the environment; unset or empty refuses startup.
pub fn parse_public_url(raw: Option<&str>) -> Result<PublicOrigin> {
    match raw {
        Some(raw) if !raw.trim().is_empty() => PublicOrigin::parse(raw),
        _ => bail!(
            "PUBLIC_URL must be set to the public https:// address of this server \
             (sign-in confirmations and passkeys are bound to it)"
        ),
    }
}

/// HKDF info for the derived pre-session HMAC key (DC1).
const AUTH_HMAC_INFO: &[u8] = b"kaiku-auth-hmac-v1";

/// The server key for [`crate::approvals::secrets::keyed_hash`] (DC1).
///
/// `AUTH_HMAC_KEY` (64 hex characters) if set, else HKDF-SHA256 over the
/// JWT signing key with info `kaiku-auth-hmac-v1`. Anything else refuses
/// startup, naming the variable only.
pub fn derive_auth_hmac_key(raw: Option<&str>, jwt_private_key: &str) -> Result<[u8; 32]> {
    if let Some(raw) = raw {
        let mut key = [0u8; 32];
        if raw.len() != 64 || hex::decode_to_slice(raw, &mut key).is_err() {
            bail!("AUTH_HMAC_KEY must be 64 hexadecimal characters (32 bytes) or unset");
        }
        return Ok(key);
    }
    let mut key = [0u8; 32];
    Hkdf::<Sha256>::new(None, jwt_private_key.as_bytes())
        .expand(AUTH_HMAC_INFO, &mut key)
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_url_is_required() {
        assert!(parse_public_url(None).is_err());
        assert!(parse_public_url(Some("")).is_err());
        let err = parse_public_url(None).unwrap_err().to_string();
        assert!(err.contains("PUBLIC_URL"), "{err}");
    }

    #[test]
    fn origin_is_lowercase_without_slash() {
        let o = PublicOrigin::parse("https://Chat.Example.com/").unwrap();
        assert_eq!(o.origin, "https://chat.example.com");
        assert_eq!(o.host, "chat.example.com");
        let o = PublicOrigin::parse("https://chat.example.com:443").unwrap();
        assert_eq!(o.origin, "https://chat.example.com");
    }

    #[test]
    fn non_default_port_kept() {
        let o = PublicOrigin::parse("https://chat.example.com:8443").unwrap();
        assert_eq!(o.origin, "https://chat.example.com:8443");
        assert_eq!(o.host, "chat.example.com");
    }

    #[test]
    fn http_only_for_localhost() {
        assert!(PublicOrigin::parse("http://chat.example.com").is_err());
        let o = PublicOrigin::parse("http://localhost:8080").unwrap();
        assert_eq!(o.origin, "http://localhost:8080");
        assert!(PublicOrigin::parse("ftp://chat.example.com").is_err());
        assert!(PublicOrigin::parse("chat.example.com").is_err());
    }

    #[test]
    fn ip_hosts_refused() {
        for raw in [
            "https://192.0.2.1",
            "https://[2001:db8::1]",
            "http://127.0.0.1:8080",
        ] {
            assert!(PublicOrigin::parse(raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn path_query_fragment_userinfo_refused() {
        for raw in [
            "https://chat.example.com/app",
            "https://chat.example.com//",
            "https://chat.example.com/?a=b",
            "https://chat.example.com/#x",
            "https://user@chat.example.com",
            "https://user:pw@chat.example.com",
        ] {
            let err = PublicOrigin::parse(raw).unwrap_err().to_string();
            assert!(err.contains("PUBLIC_URL"), "{err}");
            assert!(!err.contains("pw"), "error must not echo the value: {err}");
        }
    }

    #[test]
    fn idn_host_is_punycode() {
        let o = PublicOrigin::parse("https://bücher.example").unwrap();
        assert_eq!(o.host, "xn--bcher-kva.example");
        assert_eq!(o.origin, "https://xn--bcher-kva.example");
    }

    #[test]
    fn hmac_key_derived_when_unset_and_stable() {
        let a = derive_auth_hmac_key(None, "jwt-key-1").unwrap();
        assert_eq!(a, derive_auth_hmac_key(None, "jwt-key-1").unwrap());
        assert_ne!(a, derive_auth_hmac_key(None, "jwt-key-2").unwrap());
        assert_ne!(a, [0u8; 32]);
    }

    #[test]
    fn explicit_hmac_key_must_be_64_hex() {
        let hex64 = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
        let k = derive_auth_hmac_key(Some(hex64), "jwt").unwrap();
        assert_eq!(k[0], 0x00);
        assert_eq!(k[1], 0x11);
        assert_eq!(k[31], 0xff);
        for bad in [
            "",
            "abc",
            &hex64[..62],
            "zz112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
        ] {
            let err = derive_auth_hmac_key(Some(bad), "jwt")
                .unwrap_err()
                .to_string();
            assert!(err.contains("AUTH_HMAC_KEY"), "{err}");
        }
    }
}
