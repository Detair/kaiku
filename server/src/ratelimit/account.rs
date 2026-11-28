//! Per-account throttling of passwords and TOTP codes (B.5).
//!
//! A counter of consecutive failures per account name (passwords; unknown
//! usernames count alike, DC15) or per account (TOTP), in Valkey under
//! `ratelimit:acct:{kind}:{key}`. From the 5th failure on, further tries wait
//! `30 s · 2^(n−5)` (at most 1 h); a success resets the counter. Every
//! check reserves its try first ([`AccountLimiter::reserve`]): the try is
//! counted before it is checked, so parallel tries never share a count.
//! Strong sign-ins (passkey, key device, QR) never consult it.

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
/// A throttle claim whose effects neither committed nor gave it back (the
/// claimant died) is taken over after this long (Codex r6).
pub const THROTTLE_CLAIM_SECS: i64 = 300;
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// The consecutive count.
    pub count: u32,
    /// The counter's epoch (a success starts a new one).
    pub epoch: i64,
    /// This failure claimed the epoch's throttle (count ≥ [`THROTTLE_AT`]).
    pub throttle: bool,
    /// The claim token (`p:{epoch}:{claimed_at}`), for
    /// [`AccountLimiter::release_throttle`] / [`AccountLimiter::confirm_throttle`].
    claim: Option<String>,
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
    /// [`THROTTLE_AT`] in this epoch: the 100th applies once, and a count
    /// past it still throttles if the 100th's effects were lost (Flash r5).
    /// The claim `thr` is `p:{epoch}:{claimed_at}` while its effects run and
    /// `d` once they committed; a pending claim older than
    /// [`THROTTLE_CLAIM_SECS`] is taken over (its claimant died, Codex r6).
    pub async fn count_failure(&self, key: &CounterKey) -> Result<Failure, RateLimitError> {
        // An ungated count is never refused.
        self.count(key, false)
            .await?
            .map_err(|_| RateLimitError::RedisUnavailable)
    }

    /// Reserve one try before it is checked: refused while the delay runs
    /// (nothing counted), else counted as a failure at once, in the same
    /// script as [`Self::count_failure`]. Parallel tries therefore never see
    /// the same count, and at most [`FREE_FAILURES`] of them pass before
    /// the delay starts. The caller then settles the try: a success resets
    /// the counter ([`Self::reset_epoch`]), a failure applies the effects of
    /// the returned count, and a try that was never checked is given back
    /// ([`Self::give_back`]).
    pub async fn reserve(
        &self,
        key: &CounterKey,
    ) -> Result<Result<Failure, Delay>, RateLimitError> {
        self.count(key, true).await
    }

    async fn count(
        &self,
        key: &CounterKey,
        gated: bool,
    ) -> Result<Result<Failure, Delay>, RateLimitError> {
        use fred::interfaces::LuaInterface;
        let (n, epoch, claim): (i64, i64, String) = self
            .redis
            .eval(
                "local now = tonumber(ARGV[4])
                 if ARGV[8] == '1' then
                   local nxt = tonumber(redis.call('HGET', KEYS[1], 'next') or '0')
                   if nxt and nxt > now then
                     return {-1, nxt - now, ''}
                   end
                 end
                 local c = redis.call('HINCRBY', KEYS[1], 'count', 1)
                 local e = tonumber(redis.call('HGET', KEYS[1], 'epoch') or '0')
                 local free = tonumber(ARGV[1])
                 if c >= free then
                   local secs = tonumber(ARGV[3])
                   if c - free < 7 then
                     secs = math.min(tonumber(ARGV[2]) * 2 ^ (c - free), secs)
                   end
                   redis.call('HSET', KEYS[1], 'next', string.format('%d', now + secs))
                 end
                 local claim = ''
                 if c >= tonumber(ARGV[6]) then
                   local thr = redis.call('HGET', KEYS[1], 'thr')
                   local take = not thr
                   if thr then
                     local at = string.match(thr, '^p:%-?%d+:(%d+)$')
                     if at and now - tonumber(at) > tonumber(ARGV[7]) then
                       take = true
                     end
                   end
                   if take then
                     claim = string.format('p:%d:%d', e, now)
                     redis.call('HSET', KEYS[1], 'thr', claim)
                   end
                 end
                 redis.call('EXPIRE', KEYS[1], ARGV[5])
                 return {c, e, claim}",
                vec![key.redis_key()],
                vec![
                    FREE_FAILURES.to_string(),
                    FIRST_DELAY_SECS.to_string(),
                    MAX_DELAY_SECS.to_string(),
                    Utc::now().timestamp().to_string(),
                    COUNTER_TTL_SECS.to_string(),
                    THROTTLE_AT.to_string(),
                    THROTTLE_CLAIM_SECS.to_string(),
                    if gated { "1" } else { "0" }.to_string(),
                ],
            )
            .await
            .map_err(|_| RateLimitError::RedisUnavailable)?;
        if n < 0 {
            return Ok(Err(Delay {
                retry_after: u64::try_from(epoch).unwrap_or(1).max(1),
            }));
        }
        Ok(Ok(Failure {
            count: u32::try_from(n).unwrap_or(u32::MAX),
            epoch,
            throttle: !claim.is_empty(),
            claim: (!claim.is_empty()).then_some(claim),
        }))
    }

    /// Give back a reserved try whose check failed with a server error.
    ///
    /// The try's own throttle claim is always released (the next counted
    /// failure claims it). The count goes back only while no other try or
    /// success has touched the counter since (same epoch, same count), so a
    /// try is never taken back from someone else; otherwise the try stays
    /// counted. A delay the try started stays unless the count falls below
    /// [`FREE_FAILURES`].
    pub async fn give_back(
        &self,
        key: &CounterKey,
        reserved: &Failure,
    ) -> Result<(), RateLimitError> {
        use fred::interfaces::LuaInterface;
        let _: i64 = self
            .redis
            .eval(
                "if ARGV[4] ~= '' and redis.call('HGET', KEYS[1], 'thr') == ARGV[4] then
                   redis.call('HDEL', KEYS[1], 'thr')
                 end
                 local e = tonumber(redis.call('HGET', KEYS[1], 'epoch') or '0')
                 local c = tonumber(redis.call('HGET', KEYS[1], 'count') or '0')
                 if e ~= tonumber(ARGV[1]) or c ~= tonumber(ARGV[2]) then
                   return 0
                 end
                 c = redis.call('HINCRBY', KEYS[1], 'count', -1)
                 if c < tonumber(ARGV[3]) then
                   redis.call('HDEL', KEYS[1], 'next')
                 end
                 return 1",
                vec![key.redis_key()],
                vec![
                    reserved.epoch.to_string(),
                    reserved.count.to_string(),
                    FREE_FAILURES.to_string(),
                    reserved.claim.clone().unwrap_or_default(),
                ],
            )
            .await
            .map_err(|_| RateLimitError::RedisUnavailable)?;
        Ok(())
    }

    /// Give back `failure`'s throttle claim when its effects did not commit,
    /// so the next failure applies them: only while the stored claim is
    /// still that one (never a later epoch's or a takeover's; Codex r6).
    pub async fn release_throttle(
        &self,
        key: &CounterKey,
        failure: &Failure,
    ) -> Result<(), RateLimitError> {
        self.settle_claim(key, failure, false).await
    }

    /// Mark `failure`'s throttle claim done after its effects committed (it
    /// is never taken over in this epoch).
    pub async fn confirm_throttle(
        &self,
        key: &CounterKey,
        failure: &Failure,
    ) -> Result<(), RateLimitError> {
        self.settle_claim(key, failure, true).await
    }

    async fn settle_claim(
        &self,
        key: &CounterKey,
        failure: &Failure,
        done: bool,
    ) -> Result<(), RateLimitError> {
        use fred::interfaces::LuaInterface;
        let Some(claim) = &failure.claim else {
            return Ok(());
        };
        let _: i64 = self
            .redis
            .eval(
                "if redis.call('HGET', KEYS[1], 'thr') == ARGV[1] then
                   if ARGV[2] == '1' then
                     redis.call('HSET', KEYS[1], 'thr', 'd')
                   else
                     redis.call('HDEL', KEYS[1], 'thr')
                   end
                   return 1
                 end
                 return 0",
                vec![key.redis_key()],
                vec![claim.clone(), if done { "1" } else { "0" }.to_string()],
            )
            .await
            .map_err(|_| RateLimitError::RedisUnavailable)?;
        Ok(())
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

    /// Valkey from `REDIS_URL` (the test default otherwise).
    async fn valkey() -> fred::clients::Client {
        use fred::interfaces::ClientLike;
        let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".into());
        let client = fred::clients::Client::new(
            fred::types::config::Config::from_url(&url).expect("REDIS_URL"),
            None,
            None,
            None,
        );
        client.connect();
        client.wait_for_connect().await.expect("connect Valkey");
        client
    }

    /// A counter no other test touches.
    fn fresh_key() -> CounterKey {
        CounterKey::totp(Uuid::new_v4(), 1)
    }

    async fn reserve_ok(limiter: &AccountLimiter<'_>, key: &CounterKey) -> Failure {
        limiter
            .reserve(key)
            .await
            .expect("Valkey")
            .expect("not delayed")
    }

    async fn next_of(redis: &fred::clients::Client, key: &CounterKey) -> Option<i64> {
        redis.hget(key.redis_key(), "next").await.expect("Valkey")
    }

    async fn forget(redis: &fred::clients::Client, key: &CounterKey) {
        use fred::interfaces::KeysInterface;
        let _: i64 = redis.del(key.redis_key()).await.expect("Valkey");
    }

    #[tokio::test]
    async fn reserve_refuses_after_free_failures() {
        let redis = valkey().await;
        let limiter = AccountLimiter::new(&redis);
        let key = fresh_key();
        for n in 1..=FREE_FAILURES {
            assert_eq!(reserve_ok(&limiter, &key).await.count, n);
        }
        let refused = limiter.reserve(&key).await.expect("Valkey");
        let delay = refused.expect_err("the 6th try waits");
        assert!(
            (1..=FIRST_DELAY_SECS).contains(&delay.retry_after),
            "{delay:?}"
        );
        // A refused try is not counted.
        assert_eq!(
            limiter.read(&key).await.unwrap().0,
            i64::from(FREE_FAILURES)
        );
        forget(&redis, &key).await;
    }

    #[tokio::test]
    async fn parallel_reservations_pass_at_most_free_failures() {
        let redis = valkey().await;
        let key = fresh_key();
        let tries = (0..200)
            .map(|_| {
                let redis = redis.clone();
                let key = key.clone();
                tokio::spawn(async move { AccountLimiter::new(&redis).reserve(&key).await })
            })
            .collect::<Vec<_>>();
        let mut passed = 0;
        for t in tries {
            if t.await.unwrap().expect("Valkey").is_ok() {
                passed += 1;
            }
        }
        assert_eq!(passed, FREE_FAILURES);
        forget(&redis, &key).await;
    }

    #[tokio::test]
    async fn success_resets_epoch() {
        let redis = valkey().await;
        let limiter = AccountLimiter::new(&redis);
        let key = fresh_key();
        let mut last = reserve_ok(&limiter, &key).await;
        for _ in 1..FREE_FAILURES {
            last = reserve_ok(&limiter, &key).await;
        }
        assert!(limiter.reserve(&key).await.unwrap().is_err());
        limiter.reset_epoch(&key).await.unwrap();
        let after = reserve_ok(&limiter, &key).await;
        assert_eq!((after.count, after.epoch), (1, last.epoch + 1));
        assert_eq!(next_of(&redis, &key).await, None);
        forget(&redis, &key).await;
    }

    #[tokio::test]
    async fn give_back_restores_count_and_next() {
        let redis = valkey().await;
        let limiter = AccountLimiter::new(&redis);
        let key = fresh_key();
        for _ in 1..FREE_FAILURES {
            reserve_ok(&limiter, &key).await;
        }
        assert_eq!(next_of(&redis, &key).await, None);
        // The try that starts the delay, never checked.
        let fifth = reserve_ok(&limiter, &key).await;
        assert!(next_of(&redis, &key).await.is_some());
        limiter.give_back(&key, &fifth).await.unwrap();
        assert_eq!(limiter.read(&key).await.unwrap().0, 4);
        assert_eq!(next_of(&redis, &key).await, None, "no delay below five");
        // The next try passes again.
        let again = reserve_ok(&limiter, &key).await;
        assert_eq!(again.count, FREE_FAILURES);
        // A reservation of an older epoch gives nothing back.
        limiter.reset_epoch(&key).await.unwrap();
        let first = reserve_ok(&limiter, &key).await;
        limiter.give_back(&key, &again).await.unwrap();
        assert_eq!(limiter.read(&key).await.unwrap().0, 1);
        // Nor one a later try has counted past (that try stays counted).
        let second = reserve_ok(&limiter, &key).await;
        limiter.give_back(&key, &first).await.unwrap();
        assert_eq!(limiter.read(&key).await.unwrap().0, i64::from(second.count));
        forget(&redis, &key).await;
    }

    #[tokio::test]
    async fn give_back_releases_its_claim_when_the_count_moved_on() {
        let redis = valkey().await;
        let limiter = AccountLimiter::new(&redis);
        let key = fresh_key();
        let clear_delay = || {
            let redis = redis.clone();
            let key = key.clone();
            async move {
                let _: i64 = redis.hdel(key.redis_key(), "next").await.expect("Valkey");
            }
        };
        let _: i64 = redis
            .hset(key.redis_key(), ("count", i64::from(THROTTLE_AT) - 1))
            .await
            .expect("Valkey");
        let claimant = reserve_ok(&limiter, &key).await;
        assert!(claimant.throttle);
        clear_delay().await;
        let later = reserve_ok(&limiter, &key).await;
        assert!(!later.throttle, "the claim is taken");
        // The claimant's check never ran; another try counted since, so
        // its try stays counted, but its claim is free again.
        limiter.give_back(&key, &claimant).await.unwrap();
        assert_eq!(limiter.read(&key).await.unwrap().0, i64::from(later.count));
        clear_delay().await;
        assert!(
            reserve_ok(&limiter, &key).await.throttle,
            "the next counted failure claims the throttle"
        );
        forget(&redis, &key).await;
    }

    #[tokio::test]
    async fn throttle_claim_once_per_epoch() {
        let redis = valkey().await;
        let limiter = AccountLimiter::new(&redis);
        let key = fresh_key();
        let set_count = |n: i64| {
            let redis = redis.clone();
            let key = key.clone();
            async move {
                let _: i64 = redis
                    .hset(key.redis_key(), ("count", n))
                    .await
                    .expect("Valkey");
                let _: i64 = redis.hdel(key.redis_key(), "next").await.expect("Valkey");
            }
        };
        set_count(i64::from(THROTTLE_AT) - 1).await;
        let first = reserve_ok(&limiter, &key).await;
        assert!(first.throttle, "the 100th claims the throttle");
        set_count(i64::from(THROTTLE_AT)).await;
        assert!(!reserve_ok(&limiter, &key).await.throttle, "once per epoch");
        // Given back (its effects failed): the next try claims it.
        limiter.release_throttle(&key, &first).await.unwrap();
        set_count(i64::from(THROTTLE_AT) + 1).await;
        let retaken = reserve_ok(&limiter, &key).await;
        assert!(retaken.throttle);
        limiter.confirm_throttle(&key, &retaken).await.unwrap();
        set_count(i64::from(THROTTLE_AT) + 2).await;
        assert!(!reserve_ok(&limiter, &key).await.throttle);
        // A new epoch claims again.
        limiter.reset_epoch(&key).await.unwrap();
        set_count(i64::from(THROTTLE_AT) - 1).await;
        assert!(reserve_ok(&limiter, &key).await.throttle);
        forget(&redis, &key).await;
    }
}
