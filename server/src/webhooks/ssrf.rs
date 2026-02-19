//! SSRF Protection
//!
//! Blocks webhook deliveries to private/reserved network addresses.

use std::net::IpAddr;

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
                || is_v4_mapped_private(v6)
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

/// Resolve a URL's hostname and verify the resolved IP is not private/reserved.
/// Returns `Ok(())` if safe, `Err(message)` if blocked.
pub async fn verify_resolved_ip(url: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("Invalid URL: {e}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| "URL has no host".to_string())?;

    // C4: Always validate raw IPs at delivery time (defense-in-depth against DNS rebinding)
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_private_ip(&ip) {
            return Err(format!("URL contains private IP address: {ip}"));
        }
        return Ok(());
    }

    let port = parsed.port_or_known_default().unwrap_or(443);
    let addr_str = format!("{host}:{port}");

    let addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host(&addr_str)
        .await
        .map_err(|e| format!("DNS resolution failed for {host}: {e}"))?
        .collect();

    if addrs.is_empty() {
        return Err(format!("DNS resolution returned no addresses for {host}"));
    }

    for addr in &addrs {
        if is_private_ip(&addr.ip()) {
            return Err(format!(
                "DNS for {host} resolved to private address {}",
                addr.ip()
            ));
        }
    }

    Ok(())
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
}
