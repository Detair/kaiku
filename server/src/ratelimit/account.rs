//! Per-account throttling of passwords and TOTP codes (B.5).
//!
//! A counter of consecutive failures per account name (passwords; unknown
//! usernames count alike, DC15) or per account (TOTP), in Valkey under
//! `ratelimit:acct:{kind}:{key}`. From the 5th failure on, further tries wait
//! `30 s · 2^(n−5)` (at most 1 h); a success resets the counter. Strong
//! sign-ins (passkey, key device, QR) never consult it.

use std::time::Duration;

use chrono::Utc;
use fred::interfaces::{HashesInterface, KeysInterface};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::error::RateLimitError;

/// Failures before the first delay.
pub const FREE_FAILURES: u32 = 5;
/// The first delay.
pub const FIRST_DELAY_SECS: u64 = 30;
/// The longest delay.
pub const MAX_DELAY_SECS: u64 = 3600;
/// A `guessing` notice every this many failures (coalesced).
pub const NOTICE_EVERY: u32 = 20;
/// Failures that throttle the factor (B.1 `throttled`).
pub const THROTTLE_AT: u32 = 100;
/// How long a counter lives without failures.
const COUNTER_TTL_SECS: i64 = 24 * 3600;

/// Whose counter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CounterKey {
    /// `sha256(lower(username))` hex.
    Password(String),
    Totp(Uuid),
}

impl CounterKey {
    /// The password counter of a typed username.
    #[must_use]
    pub fn password(username: &str) -> Self {
        Self::Password(hex::encode(Sha256::digest(
            username.trim().to_lowercase().as_bytes(),
        )))
    }

    fn redis_key(&self) -> String {
        match self {
            Self::Password(h) => format!("ratelimit:acct:password:{h}"),
            Self::Totp(u) => format!("ratelimit:acct:totp:{u}"),
        }
    }
}

/// The delay before the next try after `consecutive` failures.
#[must_use]
pub fn delay_after(consecutive: u32) -> Option<Duration> {
    if consecutive < FREE_FAILURES {
        return None;
    }
    let exp = consecutive - FREE_FAILURES;
    let secs = if exp >= 7 {
        MAX_DELAY_SECS
    } else {
        (FIRST_DELAY_SECS << exp).min(MAX_DELAY_SECS)
    };
    Some(Duration::from_secs(secs))
}

/// A refused try: wait `retry_after` seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Delay {
    pub retry_after: u64,
}

/// The per-account limiter.
pub struct AccountLimiter<'a> {
    redis: &'a fred::clients::Client,
}

impl<'a> AccountLimiter<'a> {
    #[must_use]
    pub const fn new(redis: &'a fred::clients::Client) -> Self {
        Self { redis }
    }

    /// `Err(Delay)` while the counter's delay runs (429 `TOO_MANY_ATTEMPTS`
    /// with `Retry-After`).
    pub async fn check(&self, key: &CounterKey) -> Result<Result<(), Delay>, RateLimitError> {
        let next: Option<i64> = self
            .redis
            .hget(key.redis_key(), "next")
            .await
            .map_err(|_| RateLimitError::RedisUnavailable)?;
        let now = Utc::now().timestamp();
        Ok(match next {
            Some(t) if t > now => Err(Delay {
                retry_after: u64::try_from(t - now).unwrap_or(1),
            }),
            _ => Ok(()),
        })
    }

    /// Count a failure; returns the consecutive count.
    pub async fn record_failure(&self, key: &CounterKey) -> Result<u32, RateLimitError> {
        let k = key.redis_key();
        let n: i64 = self
            .redis
            .hincrby(&k, "count", 1)
            .await
            .map_err(|_| RateLimitError::RedisUnavailable)?;
        let n = u32::try_from(n).unwrap_or(u32::MAX);
        if let Some(d) = delay_after(n) {
            let next = Utc::now().timestamp() + i64::try_from(d.as_secs()).unwrap_or(0);
            self.redis
                .hset::<(), _, _>(&k, ("next", next))
                .await
                .map_err(|_| RateLimitError::RedisUnavailable)?;
        }
        self.redis
            .expire::<(), _>(&k, COUNTER_TTL_SECS, None)
            .await
            .map_err(|_| RateLimitError::RedisUnavailable)?;
        Ok(n)
    }

    /// A success resets the counter.
    pub async fn record_success(&self, key: &CounterKey) -> Result<(), RateLimitError> {
        self.redis
            .del::<(), _>(key.redis_key())
            .await
            .map_err(|_| RateLimitError::RedisUnavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_schedule() {
        assert_eq!(delay_after(4), None);
        assert_eq!(delay_after(5), Some(Duration::from_secs(30)));
        assert_eq!(delay_after(6), Some(Duration::from_secs(60)));
        assert_eq!(delay_after(11), Some(Duration::from_secs(1920)));
        assert_eq!(delay_after(12), Some(Duration::from_secs(3600)));
        assert_eq!(delay_after(1000), Some(Duration::from_secs(3600)));
    }

    #[test]
    fn password_key_ignores_case() {
        assert_eq!(CounterKey::password("Oma"), CounterKey::password("oma "));
    }
}
