//! Caps on concurrent WebSocket connections per user and per client IP.
//!
//! Every open socket holds a [`ConnectionPermit`] for its client IP (taken
//! before the upgrade) and one for its user (taken as soon as the user is
//! known). A permit gives its slot back when it is dropped, on every exit
//! path of the socket task.
//!
//! The counts live in this process: the server runs as a single node, like
//! the voice SFU.
//!
//! The tracker also carries the shutdown signal for open sockets: on SIGTERM
//! the server cancels [`WsConnectionTracker::shutdown_token`], every socket
//! closes with 1012 and runs its normal cleanup, and the server waits until
//! the IP slots (held by every socket, also before authentication) are free.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};

use axum::extract::ConnectInfo;
use axum::http::HeaderMap;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::ratelimit::{extract_client_ip, normalize_ip, TrustedProxies};

/// Default cap of concurrent sockets per user (`WS_MAX_CONNECTIONS_PER_USER`).
pub const DEFAULT_MAX_CONNECTIONS_PER_USER: usize = 10;

/// Default cap of concurrent sockets per client IP, IPv6 per /64
/// (`WS_MAX_CONNECTIONS_PER_IP`).
pub const DEFAULT_MAX_CONNECTIONS_PER_IP: usize = 30;

/// Error code (HTTP body and close reason) for a refused connection.
pub const TOO_MANY_CONNECTIONS: &str = "TOO_MANY_CONNECTIONS";

/// The two caps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectionLimits {
    pub per_user: usize,
    pub per_ip: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Slot {
    User(Uuid),
    Ip(String),
}

type Counts = Arc<Mutex<HashMap<Slot, usize>>>;

/// Counts open sockets per user and per client IP.
#[derive(Debug)]
pub struct WsConnectionTracker {
    limits: ConnectionLimits,
    trusted: TrustedProxies,
    counts: Counts,
    shutdown: CancellationToken,
}

/// One occupied slot; released when dropped.
#[derive(Debug)]
#[must_use = "the slot is released as soon as the permit is dropped"]
pub struct ConnectionPermit {
    counts: Counts,
    slot: Slot,
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        let mut counts = self.counts.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(n) = counts.get_mut(&self.slot) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                counts.remove(&self.slot);
            }
        }
    }
}

impl WsConnectionTracker {
    /// A tracker with these caps. `trusted` are the reverse proxies whose
    /// forwarded headers name the client (as for rate limiting).
    #[must_use]
    pub fn new(limits: ConnectionLimits, trusted: TrustedProxies) -> Self {
        Self {
            limits: ConnectionLimits {
                per_user: limits.per_user.max(1),
                per_ip: limits.per_ip.max(1),
            },
            trusted,
            counts: Arc::default(),
            shutdown: CancellationToken::new(),
        }
    }

    /// Cancelled when the server shuts down; every socket watches it.
    #[must_use]
    pub fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.clone()
    }

    /// Tell every open socket to close (1012) and clean up.
    pub fn begin_shutdown(&self) {
        self.shutdown.cancel();
    }

    /// Open sockets (every socket holds an IP slot, also before it has
    /// authenticated).
    #[must_use]
    pub fn open_sockets(&self) -> usize {
        self.counts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(slot, _)| matches!(slot, Slot::Ip(_)))
            .map(|(_, n)| *n)
            .sum()
    }

    /// Wait until every socket has finished (or `bound` passed). Returns the
    /// number still open.
    pub async fn wait_closed(&self, bound: std::time::Duration) -> usize {
        let deadline = tokio::time::Instant::now() + bound;
        loop {
            let open = self.open_sockets();
            if open == 0 || tokio::time::Instant::now() >= deadline {
                return open;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    /// Whether the per-IP cap could be counting a reverse proxy instead of
    /// the clients: no trusted proxies are configured and the server listens
    /// on more than loopback. Logged once at startup (see `main`).
    #[must_use]
    pub fn proxy_warning_applies(&self, bind_address: &str) -> bool {
        if !self.trusted.is_empty() {
            return false;
        }
        let host = bind_address
            .rsplit_once(':')
            .map_or(bind_address, |(host, _)| host)
            .trim_start_matches('[')
            .trim_end_matches(']');
        !host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
            && host != "localhost"
    }

    /// The caps in force.
    #[must_use]
    pub const fn limits(&self) -> ConnectionLimits {
        self.limits
    }

    /// The client's IP bucket, derived exactly like the rate limiter does it
    /// (trusted proxies honoured, IPv6 per /64).
    #[must_use]
    pub fn client_ip_key(
        &self,
        headers: &HeaderMap,
        connect_info: Option<&ConnectInfo<SocketAddr>>,
    ) -> String {
        normalize_ip(extract_client_ip(headers, connect_info, &self.trusted))
    }

    /// Take a slot of the client IP bucket `ip_key`, or `None` at the cap.
    pub fn try_acquire_ip(&self, ip_key: String) -> Option<ConnectionPermit> {
        self.try_acquire(Slot::Ip(ip_key), self.limits.per_ip)
    }

    /// Take a slot of `user_id`, or `None` at the cap.
    pub fn try_acquire_user(&self, user_id: Uuid) -> Option<ConnectionPermit> {
        self.try_acquire(Slot::User(user_id), self.limits.per_user)
    }

    /// Open sockets counted for `user_id`.
    #[must_use]
    pub fn user_connections(&self, user_id: Uuid) -> usize {
        self.count(&Slot::User(user_id))
    }

    /// Open sockets counted for the client IP bucket `ip_key`.
    #[must_use]
    pub fn ip_connections(&self, ip_key: &str) -> usize {
        self.count(&Slot::Ip(ip_key.to_string()))
    }

    fn count(&self, slot: &Slot) -> usize {
        self.counts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(slot)
            .copied()
            .unwrap_or(0)
    }

    fn try_acquire(&self, slot: Slot, cap: usize) -> Option<ConnectionPermit> {
        let mut counts = self.counts.lock().unwrap_or_else(PoisonError::into_inner);
        let n = counts.entry(slot.clone()).or_insert(0);
        if *n >= cap {
            if *n == 0 {
                counts.remove(&slot);
            }
            return None;
        }
        *n += 1;
        drop(counts);
        Some(ConnectionPermit {
            counts: self.counts.clone(),
            slot,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv6Addr};

    use super::*;

    fn tracker(per_user: usize, per_ip: usize) -> WsConnectionTracker {
        WsConnectionTracker::new(
            ConnectionLimits { per_user, per_ip },
            TrustedProxies::default(),
        )
    }

    #[test]
    fn user_cap_refuses_and_drop_releases() {
        let t = tracker(2, 30);
        let user = Uuid::new_v4();
        let a = t.try_acquire_user(user).expect("first");
        let _b = t.try_acquire_user(user).expect("second");
        assert!(t.try_acquire_user(user).is_none(), "third must be refused");
        // Other users are not affected.
        assert!(t.try_acquire_user(Uuid::new_v4()).is_some());
        drop(a);
        assert_eq!(t.user_connections(user), 1);
        assert!(t.try_acquire_user(user).is_some(), "slot freed by drop");
    }

    #[test]
    fn released_slots_leave_no_entry() {
        let t = tracker(1, 1);
        let user = Uuid::new_v4();
        drop(t.try_acquire_user(user).expect("permit"));
        drop(t.try_acquire_ip("192.0.2.1".into()).expect("permit"));
        assert!(t.counts.lock().unwrap().is_empty());
    }

    #[test]
    fn ipv6_addresses_of_one_64_share_a_bucket() {
        let t = tracker(10, 1);
        let headers = HeaderMap::new();
        let a = ConnectInfo(SocketAddr::new(
            IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 1, 2, 0, 0, 0, 1)),
            1000,
        ));
        let b = ConnectInfo(SocketAddr::new(
            IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 1, 2, 0xffff, 0, 0, 9)),
            1001,
        ));
        let other = ConnectInfo(SocketAddr::new(
            IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 1, 3, 0, 0, 0, 1)),
            1002,
        ));
        let _held = t
            .try_acquire_ip(t.client_ip_key(&headers, Some(&a)))
            .expect("first");
        assert!(t
            .try_acquire_ip(t.client_ip_key(&headers, Some(&b)))
            .is_none());
        assert!(t
            .try_acquire_ip(t.client_ip_key(&headers, Some(&other)))
            .is_some());
    }

    #[test]
    fn forwarded_header_counts_only_from_trusted_proxy() {
        let t = WsConnectionTracker::new(
            ConnectionLimits {
                per_user: 10,
                per_ip: 30,
            },
            TrustedProxies::parse("10.0.0.0/8").unwrap(),
        );
        let mut headers = HeaderMap::new();
        headers.insert("X-Forwarded-For", "203.0.113.7".parse().unwrap());
        let proxy = ConnectInfo(SocketAddr::from(([10, 0, 0, 5], 4000)));
        let direct = ConnectInfo(SocketAddr::from(([198, 51, 100, 1], 4000)));
        assert_eq!(t.client_ip_key(&headers, Some(&proxy)), "203.0.113.7");
        assert_eq!(t.client_ip_key(&headers, Some(&direct)), "198.51.100.1");
    }
}
