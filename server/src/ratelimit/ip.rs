//! IP extraction and normalization for rate limiting.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::extract::ConnectInfo;
use axum::http::HeaderMap;
use ipnet::IpNet;

use crate::ratelimit::constants::IPV6_PREFIX_SEGMENTS;

/// Set of proxy networks whose forwarded headers are trusted.
///
/// Empty by default: no proxy is trusted and forwarded headers are ignored.
#[derive(Clone, Debug, Default)]
pub struct TrustedProxies(Vec<IpNet>);

impl TrustedProxies {
    /// Parses a comma-separated list of CIDRs or bare IPs (bare IPs become
    /// /32 or /128). An empty list trusts nothing.
    pub fn parse(list: &str) -> Result<Self, String> {
        let mut nets = Vec::new();
        for entry in list.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            nets.push(Self::parse_entry(entry)?);
        }
        Ok(Self(nets))
    }

    /// Parses a single CIDR or bare IP.
    pub(crate) fn parse_entry(entry: &str) -> Result<IpNet, String> {
        if let Ok(net) = entry.parse::<IpNet>() {
            return Ok(net);
        }
        entry
            .parse::<IpAddr>()
            .map(IpNet::from)
            .map_err(|_| format!("invalid proxy address or CIDR: {entry:?}"))
    }

    /// Builds a set from already-parsed networks.
    pub(crate) const fn from_nets(nets: Vec<IpNet>) -> Self {
        Self(nets)
    }

    /// Returns true if no proxy is trusted.
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns true if `ip` is inside any trusted network.
    pub fn contains(&self, ip: IpAddr) -> bool {
        let ip = ip.to_canonical();
        self.0.iter().any(|net| net.contains(&ip))
    }
}

/// Extract client IP from connection info and, for trusted proxies, headers.
///
/// If the TCP peer is not in `trusted`, forwarded headers are ignored and the
/// peer IP is returned. Otherwise all `X-Forwarded-For` values are walked
/// right-to-left, skipping trusted proxies; the first untrusted parseable
/// entry is the client. Without a usable `X-Forwarded-For`, `X-Real-IP` is
/// used, then the peer IP. Falls back to 127.0.0.1 without connection info.
pub fn extract_client_ip(
    headers: &HeaderMap,
    connect_info: Option<&ConnectInfo<SocketAddr>>,
    trusted: &TrustedProxies,
) -> IpAddr {
    let peer = connect_info
        .map(|c| c.0.ip())
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    if connect_info.is_none() || !trusted.contains(peer) {
        return peer;
    }

    // Right-to-left walk; `None` marks an unparseable entry (or a header
    // value that is not valid text), which ends the walk.
    let entries: Vec<Option<IpAddr>> = headers
        .get_all("X-Forwarded-For")
        .iter()
        .flat_map(|v| match v.to_str() {
            Ok(s) => s.split(',').map(|e| e.trim().parse().ok()).collect(),
            Err(_) => vec![None],
        })
        .collect();
    for entry in entries.iter().rev() {
        match entry {
            Some(ip) if trusted.contains(*ip) => {}
            Some(ip) => return *ip,
            None => break,
        }
    }

    if let Some(real_ip) = headers.get("X-Real-IP") {
        if let Some(ip) = real_ip.to_str().ok().and_then(|s| s.trim().parse().ok()) {
            return ip;
        }
    }
    peer
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

    fn peer(ip: &str) -> ConnectInfo<SocketAddr> {
        ConnectInfo(SocketAddr::new(ip.parse().unwrap(), 12345))
    }

    fn hdrs(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.append(*k, v.parse().unwrap());
        }
        h
    }

    #[test]
    fn untrusted_peer_headers_ignored() {
        let headers = hdrs(&[("X-Forwarded-For", "1.2.3.4"), ("X-Real-IP", "5.6.7.8")]);
        let ip = extract_client_ip(
            &headers,
            Some(&peer("203.0.113.9")),
            &TrustedProxies::default(),
        );
        assert_eq!(ip, "203.0.113.9".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn trusted_peer_uses_rightmost_untrusted() {
        let trusted = TrustedProxies::parse("10.42.0.0/16").unwrap();
        let headers = hdrs(&[("X-Forwarded-For", "6.6.6.6, 198.51.100.4, 10.42.1.2")]);
        let ip = extract_client_ip(&headers, Some(&peer("10.42.0.7")), &trusted);
        assert_eq!(ip, "198.51.100.4".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn trusted_peer_walks_multiple_xff_header_values() {
        let trusted = TrustedProxies::parse("10.42.0.0/16").unwrap();
        let headers = hdrs(&[
            ("X-Forwarded-For", "6.6.6.6, 198.51.100.4"),
            ("X-Forwarded-For", "10.42.1.2"),
        ]);
        let ip = extract_client_ip(&headers, Some(&peer("10.42.0.7")), &trusted);
        assert_eq!(ip, "198.51.100.4".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn trusted_peer_all_trusted_falls_back_to_peer() {
        let trusted = TrustedProxies::parse("10.42.0.0/16").unwrap();
        let headers = hdrs(&[("X-Forwarded-For", "10.42.3.3, 10.42.1.2")]);
        let ip = extract_client_ip(&headers, Some(&peer("10.42.0.7")), &trusted);
        assert_eq!(ip, "10.42.0.7".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn trusted_peer_real_ip_when_no_xff() {
        let trusted = TrustedProxies::parse("10.42.0.0/16").unwrap();
        let headers = hdrs(&[("X-Real-IP", "198.51.100.25")]);
        let ip = extract_client_ip(&headers, Some(&peer("10.42.0.7")), &trusted);
        assert_eq!(ip, "198.51.100.25".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn trusted_peer_invalid_xff_falls_back_to_peer() {
        let trusted = TrustedProxies::parse("10.42.0.0/16").unwrap();
        let headers = hdrs(&[("X-Forwarded-For", "not-an-ip")]);
        let ip = extract_client_ip(&headers, Some(&peer("10.42.0.7")), &trusted);
        assert_eq!(ip, "10.42.0.7".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn trusted_peer_unparseable_rightmost_entry_stops_walk() {
        let trusted = TrustedProxies::parse("10.42.0.0/16").unwrap();
        for xff in [
            "6.6.6.6, 1.2.3.4:5678",
            "6.6.6.6, garbage",
            "6.6.6.6, [2001:db8::1]:80",
        ] {
            let headers = hdrs(&[("X-Forwarded-For", xff)]);
            let ip = extract_client_ip(&headers, Some(&peer("10.42.0.7")), &trusted);
            assert_eq!(ip, "10.42.0.7".parse::<IpAddr>().unwrap(), "{xff}");
        }
        let headers = hdrs(&[
            ("X-Forwarded-For", "6.6.6.6, 1.2.3.4:5678"),
            ("X-Real-IP", "198.51.100.25"),
        ]);
        let ip = extract_client_ip(&headers, Some(&peer("10.42.0.7")), &trusted);
        assert_eq!(ip, "198.51.100.25".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn no_connect_info_is_localhost() {
        let ip = extract_client_ip(&HeaderMap::new(), None, &TrustedProxies::default());
        assert_eq!(ip, IpAddr::V4(Ipv4Addr::LOCALHOST));
    }

    #[test]
    fn parse_accepts_ips_and_cidrs_and_rejects_garbage() {
        let t = TrustedProxies::parse("10.0.0.0/8, 192.0.2.1, ::1").unwrap();
        assert!(t.contains("10.9.9.9".parse().unwrap()));
        assert!(t.contains("192.0.2.1".parse().unwrap()));
        assert!(!t.contains("192.0.2.2".parse().unwrap()));
        assert!(t.contains("::1".parse().unwrap()));
        assert!(TrustedProxies::parse("nope").is_err());
        assert!(TrustedProxies::parse("").unwrap().is_empty());
    }
}
