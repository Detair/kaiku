//! SSRF Protection
//!
//! Blocks webhook deliveries to private/reserved network addresses.

use std::net::IpAddr;
use std::time::Duration;

/// Blocked hostname patterns (case-insensitive check performed by caller).
const BLOCKED_HOSTNAMES: &[&str] = &[
    "localhost",
    "localhost.localdomain",
    "ip6-localhost",
    "ip6-loopback",
];

/// Check if a hostname string points to a private or reserved address.
/// This performs a static check on the hostname — DNS resolution happens at delivery time.
pub fn is_blocked_host(host: &str) -> bool {
    let lower = host.to_lowercase();

    // Block known loopback hostnames
    if BLOCKED_HOSTNAMES.contains(&lower.as_str()) {
        return true;
    }

    // Block if it parses as a private/reserved IP directly
    if let Ok(ip) = host.parse::<IpAddr>() {
        return is_private_ip(&ip);
    }

    // Block IPv6 bracket notation (e.g., `[::1]`)
    let trimmed = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = trimmed.parse::<IpAddr>() {
        return is_private_ip(&ip);
    }

    false
}

/// Check if an IP address is private, loopback, link-local, or otherwise reserved.
pub fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()              // 127.0.0.0/8
                || v4.is_private()         // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
                || v4.is_link_local()      // 169.254.0.0/16
                || v4.is_broadcast()       // 255.255.255.255
                || v4.is_unspecified()     // 0.0.0.0
                || v4.octets()[0] == 0     // 0.0.0.0/8 ("this" network)
                || v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64 // 100.64.0.0/10 (CGN)
                || v4.octets()[0] == 198 && (v4.octets()[1] & 0xFE) == 18 // 198.18.0.0/15 (benchmark)
                || v4.octets()[0] == 192 && v4.octets()[1] == 0 && v4.octets()[2] == 0 // 192.0.0.0/24 (IETF)
                || v4.octets()[0] == 192 && v4.octets()[1] == 0 && v4.octets()[2] == 2 // 192.0.2.0/24 (TEST-NET-1)
                || v4.octets()[0] == 198 && v4.octets()[1] == 51 && v4.octets()[2] == 100 // 198.51.100.0/24 (TEST-NET-2)
                || v4.octets()[0] == 203 && v4.octets()[1] == 0 && v4.octets()[2] == 113 // 203.0.113.0/24 (TEST-NET-3)
                || v4.octets()[0] >= 224 // 224.0.0.0/4 (multicast) + 240.0.0.0/4 (reserved)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()              // ::1
                || v6.is_unspecified()     // ::
                || (v6.segments()[0] & 0xFE00) == 0xFC00 // fc00::/7 (ULA)
                || (v6.segments()[0] & 0xFFC0) == 0xFE80 // fe80::/10 (link-local)
                || (v6.segments()[0] & 0xFF00) == 0xFF00 // ff00::/8 (multicast)
                || (v6.segments()[0] == 0x2001 && v6.segments()[1] == 0x0db8) // 2001:db8::/32 (documentation)
                || is_v4_mapped_private(v6)
                || is_nat64_private(v6)
        }
    }
}

/// Check if an IPv6 address is a v4-mapped address (`::ffff:x.x.x.x`) pointing to a private IPv4.
fn is_v4_mapped_private(v6: &std::net::Ipv6Addr) -> bool {
    if let Some(v4) = v6.to_ipv4_mapped() {
        is_private_ip(&IpAddr::V4(v4))
    } else {
        false
    }
}

/// Check if an IPv6 address is in the NAT64 prefix (`64:ff9b::/96`) and embeds a private IPv4.
fn is_nat64_private(v6: &std::net::Ipv6Addr) -> bool {
    let seg = v6.segments();
    if seg[0] == 0x0064 && seg[1] == 0xff9b && seg[2..6].iter().all(|s| *s == 0) {
        let o = v6.octets();
        let v4 = std::net::Ipv4Addr::new(o[12], o[13], o[14], o[15]);
        is_private_ip(&IpAddr::V4(v4))
    } else {
        false
    }
}

/// Verified URL with pinned resolved addresses to prevent DNS rebinding.
pub struct VerifiedUrl {
    /// The original hostname from the URL.
    pub host: String,
    /// The first verified (non-private) socket address.
    pub addr: std::net::SocketAddr,
}

/// Why [`verify_resolved_ip`] rejected a URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SsrfError {
    /// The URL is malformed or its target is a private/reserved address.
    /// Retrying will not help.
    Blocked(String),
    /// The host could not be resolved right now (possibly transient).
    Resolution(String),
}

impl std::fmt::Display for SsrfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Blocked(m) | Self::Resolution(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for SsrfError {}

/// Resolve a URL's hostname and verify the resolved IP is not private/reserved.
/// Returns the verified host and a pinned socket address to use for delivery,
/// preventing DNS rebinding (TOCTOU) attacks.
pub async fn verify_resolved_ip(url: &str) -> Result<VerifiedUrl, SsrfError> {
    let parsed =
        reqwest::Url::parse(url).map_err(|e| SsrfError::Blocked(format!("Invalid URL: {e}")))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| SsrfError::Blocked("URL has no host".to_string()))?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();

    let port = parsed.port_or_known_default().unwrap_or(443);

    // C4: Always validate raw IPs at delivery time (defense-in-depth against DNS rebinding)
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_private_ip(&ip) {
            return Err(SsrfError::Blocked(format!(
                "URL contains private IP address: {ip}"
            )));
        }
        return Ok(VerifiedUrl {
            host: host.clone(),
            addr: std::net::SocketAddr::new(ip, port),
        });
    }

    let addr_str = format!("{host}:{port}");

    let addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host(&addr_str)
        .await
        .map_err(|e| SsrfError::Resolution(format!("DNS resolution failed for {host}: {e}")))?
        .collect();

    if addrs.is_empty() {
        return Err(SsrfError::Resolution(format!(
            "DNS resolution returned no addresses for {host}"
        )));
    }

    for addr in &addrs {
        if is_private_ip(&addr.ip()) {
            return Err(SsrfError::Blocked(format!(
                "DNS for {host} resolved to private address {}",
                addr.ip()
            )));
        }
    }

    Ok(VerifiedUrl {
        host,
        addr: addrs[0],
    })
}

/// Build the only HTTP client allowed to fetch a user-controlled URL: it
/// connects to the address that passed validation, never follows redirects
/// and ignores proxy environment variables.
pub fn pinned_client(
    verified: &VerifiedUrl,
    timeout: Duration,
) -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .resolve(&verified.host, verified.addr)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .timeout(timeout)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_localhost() {
        assert!(is_blocked_host("localhost"));
        assert!(is_blocked_host("LOCALHOST"));
        assert!(is_blocked_host("Localhost"));
    }

    #[test]
    fn blocks_loopback_ipv4() {
        assert!(is_blocked_host("127.0.0.1"));
        assert!(is_blocked_host("127.0.0.2"));
    }

    #[test]
    fn blocks_private_ipv4() {
        assert!(is_blocked_host("10.0.0.1"));
        assert!(is_blocked_host("172.16.0.1"));
        assert!(is_blocked_host("192.168.1.1"));
    }

    #[test]
    fn blocks_link_local() {
        assert!(is_blocked_host("169.254.1.1"));
    }

    #[test]
    fn blocks_ipv6_loopback() {
        assert!(is_blocked_host("::1"));
        assert!(is_blocked_host("[::1]"));
    }

    #[test]
    fn blocks_cloud_metadata() {
        assert!(is_blocked_host("169.254.169.254"));
    }

    #[test]
    fn allows_public_ip() {
        assert!(!is_blocked_host("8.8.8.8"));
        assert!(!is_blocked_host("1.1.1.1"));
    }

    #[test]
    fn allows_public_hostname() {
        assert!(!is_blocked_host("example.com"));
        assert!(!is_blocked_host("api.discord.com"));
    }

    #[test]
    fn blocks_cgn_range() {
        assert!(is_blocked_host("100.64.0.1"));
        assert!(is_blocked_host("100.127.255.254"));
    }

    #[test]
    fn blocks_zero_network_and_nat64_private() {
        assert!(is_private_ip(&"0.1.2.3".parse().unwrap()));
        assert!(is_private_ip(&"64:ff9b::a00:1".parse().unwrap())); // 10.0.0.1
        assert!(!is_private_ip(&"64:ff9b::808:808".parse().unwrap())); // 8.8.8.8
    }

    #[test]
    fn blocks_ipv6_multicast_and_documentation() {
        assert!(is_private_ip(&"ff02::1".parse().unwrap()));
        assert!(is_private_ip(&"ff00::".parse().unwrap()));
        assert!(is_private_ip(&"2001:db8::1".parse().unwrap()));
        assert!(is_private_ip(&"2001:db8:ffff::1".parse().unwrap()));
        // Neighbouring public space stays reachable.
        assert!(!is_private_ip(&"2001:4860:4860::8888".parse().unwrap()));
        assert!(!is_private_ip(&"2001:db9::1".parse().unwrap()));
    }

    #[tokio::test]
    async fn verified_url_host_has_no_ipv6_brackets() {
        let v = verify_resolved_ip("https://[2606:4700::1111]/x")
            .await
            .unwrap();
        assert_eq!(v.host, "2606:4700::1111");
        assert!(verify_resolved_ip("https://[::1]/x").await.is_err());
    }

    #[tokio::test]
    async fn pinned_client_does_not_follow_redirects() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        use axum::http::header;
        use axum::response::IntoResponse;
        use axum::routing::get;
        use axum::Router;

        let b_hits = Arc::new(AtomicUsize::new(0));
        let hits = b_hits.clone();
        let app_b = Router::new().route(
            "/hit",
            get(move || {
                let hits = hits.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    "ok"
                }
            }),
        );
        let lb = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let pb = lb.local_addr().unwrap().port();
        tokio::spawn(async move { axum::serve(lb, app_b).await.unwrap() });

        let app_a = Router::new().route(
            "/",
            get(move || async move {
                (
                    axum::http::StatusCode::FOUND,
                    [(header::LOCATION, format!("http://127.0.0.1:{pb}/hit"))],
                )
                    .into_response()
            }),
        );
        let la = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let a_addr = la.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(la, app_a).await.unwrap() });

        let v = VerifiedUrl {
            host: "hook.test".into(),
            addr: a_addr,
        };
        let resp = pinned_client(&v, Duration::from_secs(2))
            .unwrap()
            .get(format!("http://hook.test:{}/", a_addr.port()))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 302);
        assert_eq!(b_hits.load(Ordering::SeqCst), 0);
    }
}
