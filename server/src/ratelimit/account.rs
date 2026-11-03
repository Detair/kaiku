//! Per-account throttling of passwords and TOTP codes (B.5).
//!
//! A counter of consecutive failures per account name (passwords; unknown
//! usernames count alike, DC15) or per account (TOTP), in Valkey under
//! `ratelimit:acct:{kind}:{key}`. From the 5th failure on, further tries wait
//! `30 s · 2^(n−5)` (at most 1 h); a success resets the counter. Strong
//! sign-ins (passkey, key device, QR) never consult it.

use std::time::Duration;

use chrono::Utc;
use fred::interfaces::HashesInterface;
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
    /// Per account and TOTP generation: a renewal starts a fresh counter,
    /// and failures against an old secret never touch the new one.
    Totp(Uuid, i64),
}

impl CounterKey {
    /// The password counter of a typed username.
    #[must_use]
    pub fn password(username: &str) -> Self {
        Self::Password(hex::encode(Sha256::digest(
            username.trim().to_lowercase().as_bytes(),
        )))
    }

    /// The TOTP counter of `user`'s generation `generation`.
    #[must_use]
    pub const fn totp(user: Uuid, generation: i64) -> Self {
        Self::Totp(user, generation)
    }

    /// The Valkey key.
    #[must_use]
    pub fn redis_key(&self) -> String {
        match self {
            Self::Password(h) => format!("ratelimit:acct:password:{h}"),
            Self::Totp(u, g) => format!("ratelimit:acct:totp:{u}:{g}"),
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

/// One counted failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Failure {
    /// The consecutive count.
    pub count: u32,
    /// The counter's epoch (a success starts a new one).
    pub epoch: i64,
    /// This failure claimed the epoch's throttle (count ≥ [`THROTTLE_AT`]).
    pub throttle: bool,
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
        Ok(self.count_failure(key).await?.count)
    }

    /// Count a failure atomically with the counter's epoch (a success bumps
    /// it): `(consecutive count, epoch)`.
    pub async fn record_failure_epoch(
        &self,
        key: &CounterKey,
    ) -> Result<(u32, i64), RateLimitError> {
        let f = self.count_failure(key).await?;
        Ok((f.count, f.epoch))
    }

    /// Count a failure. The count, the epoch, the delay (`next`), the
    /// expiry and the throttle claim are one script, so a success never
    /// interleaves with a half-recorded failure and leaves a stale delay
    /// (Codex r4 #3). `throttle` is set for the first failure at or past
    /// [`THROTTLE_AT`] in this epoch (`HSETNX thr`): the 100th applies once,
    /// and a count past it still throttles if the 100th's effects were lost
    /// (Flash r5); [`Self::release_throttle`] gives the claim back when the
    /// effects fail.
    pub async fn count_failure(&self, key: &CounterKey) -> Result<Failure, RateLimitError> {
        use fred::interfaces::LuaInterface;
        let (n, epoch, first): (i64, i64, i64) = self
            .redis
            .eval(
                "local c = redis.call('HINCRBY', KEYS[1], 'count', 1)
                 local e = tonumber(redis.call('HGET', KEYS[1], 'epoch') or '0')
                 local free = tonumber(ARGV[1])
                 if c >= free then
                   local secs = tonumber(ARGV[3])
                   if c - free < 7 then
                     secs = math.min(tonumber(ARGV[2]) * 2 ^ (c - free), secs)
                   end
                   redis.call('HSET', KEYS[1], 'next', string.format('%d', tonumber(ARGV[4]) + secs))
                 end
                 local first = 0
                 if c >= tonumber(ARGV[6]) and redis.call('HSETNX', KEYS[1], 'thr', 1) == 1 then
                   first = 1
                 end
                 redis.call('EXPIRE', KEYS[1], ARGV[5])
                 return {c, e, first}",
                vec![key.redis_key()],
                vec![
                    FREE_FAILURES.to_string(),
                    FIRST_DELAY_SECS.to_string(),
                    MAX_DELAY_SECS.to_string(),
                    Utc::now().timestamp().to_string(),
                    COUNTER_TTL_SECS.to_string(),
                    THROTTLE_AT.to_string(),
                ],
            )
            .await
            .map_err(|_| RateLimitError::RedisUnavailable)?;
        Ok(Failure {
            count: u32::try_from(n).unwrap_or(u32::MAX),
            epoch,
            throttle: first == 1,
        })
    }

    /// Give back a throttle claim whose effects did not commit, so the next
    /// failure applies them.
    pub async fn release_throttle(&self, key: &CounterKey) -> Result<(), RateLimitError> {
        self.redis
            .hdel::<(), _, _>(key.redis_key(), "thr")
            .await
            .map_err(|_| RateLimitError::RedisUnavailable)
    }

    /// A success starts a new epoch: the count and the delay go, and
    /// effects scheduled in the old epoch no longer apply.
    pub async fn reset_epoch(&self, key: &CounterKey) -> Result<(), RateLimitError> {
        use fred::interfaces::LuaInterface;
        let _: i64 = self
            .redis
            .eval(
                "redis.call('HINCRBY', KEYS[1], 'epoch', 1)
                 redis.call('HSET', KEYS[1], 'count', 0)
                 redis.call('HDEL', KEYS[1], 'next', 'thr')
                 redis.call('EXPIRE', KEYS[1], ARGV[1])
                 return 1",
                vec![key.redis_key()],
                vec![COUNTER_TTL_SECS.to_string()],
            )
            .await
            .map_err(|_| RateLimitError::RedisUnavailable)?;
        Ok(())
    }

    /// `(count, epoch)` of a counter now.
    pub async fn read(&self, key: &CounterKey) -> Result<(i64, i64), RateLimitError> {
        let (c, e): (Option<i64>, Option<i64>) = self
            .redis
            .hmget(key.redis_key(), vec!["count", "epoch"])
            .await
            .map_err(|_| RateLimitError::RedisUnavailable)?;
        Ok((c.unwrap_or(0), e.unwrap_or(0)))
    }

    /// A success resets the counter: a new epoch, as for TOTP (Flash r5),
    /// so the epoch never restarts at 0.
    pub async fn record_success(&self, key: &CounterKey) -> Result<(), RateLimitError> {
        self.reset_epoch(key).await
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
