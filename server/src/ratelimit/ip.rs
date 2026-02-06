//! IP extraction and normalization for rate limiting.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::extract::ConnectInfo;
use axum::http::HeaderMap;

use crate::ratelimit::constants::IPV6_PREFIX_SEGMENTS;

/// Extract client IP from request headers or connection info.
///
/// When `trust_proxy` is true, checks X-Forwarded-For and X-Real-IP headers.
/// Falls back to direct connection IP, or 127.0.0.1 if unavailable.
pub fn extract_client_ip(
    headers: &HeaderMap,
    connect_info: Option<&ConnectInfo<SocketAddr>>,
    trust_proxy: bool,
) -> IpAddr {
    if trust_proxy {
        if let Some(forwarded) = headers.get("X-Forwarded-For") {
            if let Ok(s) = forwarded.to_str() {
                if let Some(first_ip) = s.split(',').next() {
                    if let Ok(ip) = first_ip.trim().parse() {
                        return ip;
                    }
                }
            }
        }
        if let Some(real_ip) = headers.get("X-Real-IP") {
            if let Ok(s) = real_ip.to_str() {
                if let Ok(ip) = s.trim().parse() {
                    return ip;
                }
            }
        }
    }
    connect_info
        .map(|c| c.0.ip())
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST))
}

/// Normalize IP address for rate limiting.
///
/// IPv4 addresses are kept as-is.
/// IPv6 addresses are normalized to /64 prefix to prevent circumvention
/// by using multiple addresses within the same allocation.
pub fn normalize_ip(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => {
            let seg = v6.segments();
            let prefix: Vec<String> = (0..IPV6_PREFIX_SEGMENTS)
                .map(|i| format!("{:x}", seg[i]))
                .collect();
            format!("{}::/64", prefix.join(":"))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv6Addr;

    use super::*;

    #[test]
    fn test_normalize_ipv4() {
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100));
        assert_eq!(normalize_ip(ip), "192.168.1.100");
    }

    #[test]
    fn test_normalize_ipv6() {
        let ip = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0x85a3, 0x1234, 0, 0, 0, 1));
        assert_eq!(normalize_ip(ip), "2001:db8:85a3:1234::/64");
    }

    #[test]
    fn test_extract_client_ip_no_proxy() {
        let headers = HeaderMap::new();
        let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 12345);
        let connect_info = ConnectInfo(socket);

        let ip = extract_client_ip(&headers, Some(&connect_info), false);
        assert_eq!(ip, IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)));
    }

    #[test]
    fn test_extract_client_ip_with_forwarded_for() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Forwarded-For",
            "203.0.113.50, 70.41.3.18".parse().unwrap(),
        );
        let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 12345);
        let connect_info = ConnectInfo(socket);

        // With trust_proxy = true, should use X-Forwarded-For
        let ip = extract_client_ip(&headers, Some(&connect_info), true);
        assert_eq!(ip, IpAddr::V4(Ipv4Addr::new(203, 0, 113, 50)));

        // With trust_proxy = false, should use connect_info
        let ip = extract_client_ip(&headers, Some(&connect_info), false);
        assert_eq!(ip, IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)));
    }

    #[test]
    fn test_extract_client_ip_with_real_ip() {
        let mut headers = HeaderMap::new();
        headers.insert("X-Real-IP", "198.51.100.25".parse().unwrap());
        let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 12345);
        let connect_info = ConnectInfo(socket);

        let ip = extract_client_ip(&headers, Some(&connect_info), true);
        assert_eq!(ip, IpAddr::V4(Ipv4Addr::new(198, 51, 100, 25)));
    }

    #[test]
    fn test_extract_client_ip_forwarded_for_takes_precedence() {
        let mut headers = HeaderMap::new();
        headers.insert("X-Forwarded-For", "203.0.113.50".parse().unwrap());
        headers.insert("X-Real-IP", "198.51.100.25".parse().unwrap());
        let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 12345);
        let connect_info = ConnectInfo(socket);

        let ip = extract_client_ip(&headers, Some(&connect_info), true);
        assert_eq!(ip, IpAddr::V4(Ipv4Addr::new(203, 0, 113, 50)));
    }

    #[test]
    fn test_extract_client_ip_fallback_to_localhost() {
        let headers = HeaderMap::new();
        let ip = extract_client_ip(&headers, None, false);
        assert_eq!(ip, IpAddr::V4(Ipv4Addr::LOCALHOST));
    }

    #[test]
    fn test_extract_client_ip_invalid_header() {
        let mut headers = HeaderMap::new();
        headers.insert("X-Forwarded-For", "not-an-ip".parse().unwrap());
        let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 12345);
        let connect_info = ConnectInfo(socket);

        // Should fall back to connect_info when header is invalid
        let ip = extract_client_ip(&headers, Some(&connect_info), true);
        assert_eq!(ip, IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)));
    }
}
