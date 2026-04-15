//! Rate limiting for voice operations.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use tokio::sync::RwLock;
// Use `tokio::time::Instant` (not `std::time::Instant`) for the token bucket
// so integration tests with `#[tokio::test(start_paused = true)]` can use
// virtual time deterministically.
use tokio::time::Instant as TokioInstant;
use tracing::debug;
use uuid::Uuid;

use super::error::VoiceError;

/// Rate limiter for voice stats operations.
pub struct VoiceStatsLimiter {
    /// Map of `user_id` to last stats reporting time.
    last_stats: Arc<RwLock<HashMap<Uuid, Instant>>>,
    /// Minimum time between stats reports.
    min_stats_interval: Duration,
    /// Cleanup interval for periodic cleanup task.
    cleanup_interval: Duration,
}

impl VoiceStatsLimiter {
    /// Create a new rate limiter.
    pub fn new(min_stats_interval: Duration) -> Self {
        Self {
            last_stats: Arc::new(RwLock::new(HashMap::new())),
            min_stats_interval,
            cleanup_interval: Duration::from_mins(1), // Default cleanup every 60 seconds
        }
    }

    /// Create a rate limiter with default settings.
    /// - 1 stats report per second
    /// - Cleanup every 60 seconds
    pub fn default() -> Self {
        Self::new(Duration::from_secs(1))
    }

    /// Start a background task that periodically cleans up stale entries.
    /// Returns a handle to the spawned task.
    pub fn start_cleanup_task(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let limiter = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(limiter.cleanup_interval);
            loop {
                interval.tick().await;
                let before = limiter.last_stats.read().await.len();
                limiter.cleanup().await;
                let after = limiter.last_stats.read().await.len();
                if before > after {
                    debug!(
                        removed = before - after,
                        remaining = after,
                        "VoiceStatsLimiter cleanup complete"
                    );
                }
            }
        })
    }

    /// Check if a user can report voice stats (rate limit check).
    pub async fn check_stats(&self, user_id: Uuid) -> Result<(), VoiceError> {
        let mut map = self.last_stats.write().await;

        if let Some(last) = map.get(&user_id) {
            let elapsed = last.elapsed();
            if elapsed < self.min_stats_interval {
                // Still within rate limit window
                return Err(VoiceError::RateLimited("voice_stats"));
            }
        }

        // Update last stats time
        map.insert(user_id, Instant::now());
        Ok(())
    }

    /// Cleanup old entries (call periodically to prevent memory leak).
    pub async fn cleanup(&self) {
        let now = Instant::now();

        // Cleanup stats
        let stats_threshold = self.min_stats_interval * 10;
        let mut stats_map = self.last_stats.write().await;
        stats_map.retain(|_, last| now.duration_since(*last) < stats_threshold);
    }
}

/// Event class key for per-peer voice signaling rate limiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventClass {
    /// ICE candidates for the publisher `PeerConnection`.
    IceCandidatePublisher,
    /// ICE candidates for the subscriber `PeerConnection`.
    IceCandidateSubscriber,
    /// Covers `VoicePublisherOffer` and `VoiceSubscriberAnswer`.
    PublisherOffer,
    /// Covers `VoiceScreenShareStart` and `VoiceScreenShareStop`.
    ScreenShareToggle,
    /// Covers Mute / Unmute / `WebcamStart` / `WebcamStop`.
    MuteOrWebcamToggle,
    /// Simulcast layer preference updates.
    SetLayerPreference,
}

impl EventClass {
    /// Stable label for observability logs.
    #[must_use]
    pub const fn as_scope(self) -> &'static str {
        match self {
            Self::IceCandidatePublisher => "ice_candidate_publisher",
            Self::IceCandidateSubscriber => "ice_candidate_subscriber",
            Self::PublisherOffer => "publisher_offer",
            Self::ScreenShareToggle => "screen_share_toggle",
            Self::MuteOrWebcamToggle => "mute_or_webcam_toggle",
            Self::SetLayerPreference => "set_layer_preference",
        }
    }
}

/// Lazy-refilling token bucket backed by an atomic counter.
///
/// Refills are computed on demand from elapsed time, so no background task is
/// required. Tokens are capped at `capacity`.
#[derive(Debug)]
pub struct TokenBucket {
    capacity: u32,
    tokens: AtomicU32,
    refill_per_sec: u32,
    last_refill: std::sync::Mutex<TokioInstant>,
}

impl TokenBucket {
    /// Create a new token bucket, starting full at `capacity`.
    #[must_use]
    pub fn new(capacity: u32, refill_per_sec: u32) -> Self {
        Self {
            capacity,
            tokens: AtomicU32::new(capacity),
            refill_per_sec,
            last_refill: std::sync::Mutex::new(TokioInstant::now()),
        }
    }

    /// Attempt to acquire one token. Performs a lazy refill before trying.
    ///
    /// Returns `true` if a token was consumed, `false` if the bucket was empty.
    pub fn try_acquire(&self) -> bool {
        let now = TokioInstant::now();
        {
            let mut last = self.last_refill.lock().unwrap();
            let elapsed_ms = now.duration_since(*last).as_millis() as u64;
            if elapsed_ms > 0 {
                let tokens_to_add = ((elapsed_ms * u64::from(self.refill_per_sec)) / 1000) as u32;
                if tokens_to_add > 0 {
                    let current = self.tokens.load(Ordering::Relaxed);
                    let new_value = current.saturating_add(tokens_to_add).min(self.capacity);
                    self.tokens.store(new_value, Ordering::Release);
                    *last = now;
                }
            }
        }

        let mut current = self.tokens.load(Ordering::Acquire);
        loop {
            if current == 0 {
                return false;
            }
            match self.tokens.compare_exchange_weak(
                current,
                current - 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return true,
                Err(actual) => current = actual,
            }
        }
    }
}

/// Per-peer, per-event-class token bucket rate limiter for voice signaling.
///
/// Buckets are created lazily on first access for a `(peer_id, event_class)`
/// pair and removed when the peer disconnects via [`Self::forget_peer`].
#[derive(Debug, Default)]
pub struct VoiceRateLimiter {
    buckets: DashMap<(Uuid, EventClass), TokenBucket>,
}

impl VoiceRateLimiter {
    /// Create a new empty rate limiter.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Try to acquire a token for `(peer_id, class)`.
    ///
    /// Returns `true` if the event is allowed, `false` if it should be dropped.
    /// Creates the bucket on first use with the class's default configuration.
    pub fn try_acquire(&self, peer_id: Uuid, class: EventClass) -> bool {
        let bucket = self
            .buckets
            .entry((peer_id, class))
            .or_insert_with(|| Self::bucket_for_class(class));
        bucket.try_acquire()
    }

    /// Remove all buckets for a peer. Call on voice disconnect to avoid leaks.
    pub fn forget_peer(&self, peer_id: Uuid) {
        self.buckets.retain(|(pid, _), _| *pid != peer_id);
    }

    /// Bucket configuration per event class.
    ///
    /// See `docs/superpowers/specs/2026-04-15-voice-screenshare-fixes-design.md`
    /// Fix 2 for the design rationale behind these limits.
    fn bucket_for_class(class: EventClass) -> TokenBucket {
        match class {
            // ICE trickle is bursty per PC; tolerate flurries but cap sustained rate.
            EventClass::IceCandidatePublisher | EventClass::IceCandidateSubscriber => {
                TokenBucket::new(200, 40)
            }
            // Renegotiation should be rare — protect against offer storms.
            EventClass::PublisherOffer => TokenBucket::new(5, 1),
            // Integer approximation of "1 per 5 seconds" for screen share toggles.
            EventClass::ScreenShareToggle => TokenBucket::new(5, 1),
            EventClass::MuteOrWebcamToggle => TokenBucket::new(10, 2),
            EventClass::SetLayerPreference => TokenBucket::new(20, 5),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter_allows_first_stats() {
        let limiter = VoiceStatsLimiter::new(Duration::from_millis(100));
        let user_id = Uuid::new_v4();

        // First stats should succeed
        assert!(limiter.check_stats(user_id).await.is_ok());
    }

    #[tokio::test]
    async fn test_rate_limiter_blocks_rapid_stats() {
        let limiter = VoiceStatsLimiter::new(Duration::from_millis(100));
        let user_id = Uuid::new_v4();

        // First stats succeeds
        assert!(limiter.check_stats(user_id).await.is_ok());

        // Immediate second stats should fail
        assert!(limiter.check_stats(user_id).await.is_err());
    }

    #[tokio::test]
    async fn test_rate_limiter_allows_after_interval() {
        let limiter = VoiceStatsLimiter::new(Duration::from_millis(50));
        let user_id = Uuid::new_v4();

        // First stats succeeds
        assert!(limiter.check_stats(user_id).await.is_ok());

        // Wait for rate limit to expire
        tokio::time::sleep(Duration::from_millis(60)).await;

        // Second stats should now succeed
        assert!(limiter.check_stats(user_id).await.is_ok());
    }

    #[tokio::test]
    async fn test_rate_limiter_independent_users() {
        let limiter = VoiceStatsLimiter::new(Duration::from_millis(100));
        let user1 = Uuid::new_v4();
        let user2 = Uuid::new_v4();

        // Both users should be able to report stats
        assert!(limiter.check_stats(user1).await.is_ok());
        assert!(limiter.check_stats(user2).await.is_ok());
    }

    #[tokio::test]
    async fn test_cleanup_removes_old_entries() {
        let limiter = VoiceStatsLimiter::new(Duration::from_millis(10));
        let user_id = Uuid::new_v4();

        // Report stats and verify entry exists
        limiter.check_stats(user_id).await.ok();
        assert_eq!(limiter.last_stats.read().await.len(), 1);

        // Wait for cleanup threshold (10x the interval)
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Cleanup should remove old entries
        limiter.cleanup().await;
        assert_eq!(limiter.last_stats.read().await.len(), 0);
    }

    // ----- TokenBucket / VoiceRateLimiter tests -----

    #[tokio::test(start_paused = true)]
    async fn token_bucket_allows_initial_capacity_then_blocks() {
        let bucket = TokenBucket::new(3, 1);

        assert!(bucket.try_acquire());
        assert!(bucket.try_acquire());
        assert!(bucket.try_acquire());
        // Capacity exhausted, and no time has passed under paused time.
        assert!(!bucket.try_acquire());
    }

    #[tokio::test(start_paused = true)]
    async fn token_bucket_refills_after_one_second() {
        let bucket = TokenBucket::new(2, 5);

        assert!(bucket.try_acquire());
        assert!(bucket.try_acquire());
        assert!(!bucket.try_acquire());

        // Advance virtual time by one second — 5 tokens should be generated,
        // but the bucket caps at capacity (2).
        tokio::time::advance(Duration::from_secs(1)).await;

        assert!(bucket.try_acquire());
        assert!(bucket.try_acquire());
        assert!(!bucket.try_acquire());
    }

    #[tokio::test(start_paused = true)]
    async fn token_bucket_partial_refill_from_elapsed_time() {
        // 10 tokens/sec → one token per 100ms.
        let bucket = TokenBucket::new(10, 10);

        // Drain the bucket.
        for _ in 0..10 {
            assert!(bucket.try_acquire());
        }
        assert!(!bucket.try_acquire());

        // Advance 300ms — expect 3 tokens added.
        tokio::time::advance(Duration::from_millis(300)).await;

        assert!(bucket.try_acquire());
        assert!(bucket.try_acquire());
        assert!(bucket.try_acquire());
        assert!(!bucket.try_acquire());
    }

    #[tokio::test(start_paused = true)]
    async fn token_bucket_does_not_over_deliver_under_sequential_load() {
        let bucket = TokenBucket::new(50, 10);

        // Under paused time no refill happens; we should get exactly `capacity`
        // successful acquires across many attempts.
        let mut allowed = 0;
        for _ in 0..200 {
            if bucket.try_acquire() {
                allowed += 1;
            }
        }
        assert_eq!(allowed, 50);
    }

    #[tokio::test(start_paused = true)]
    async fn voice_rate_limiter_separates_peers_and_classes() {
        let limiter = VoiceRateLimiter::new();
        let peer_a = Uuid::new_v4();
        let peer_b = Uuid::new_v4();

        // publisher_offer: capacity 5.
        for _ in 0..5 {
            assert!(limiter.try_acquire(peer_a, EventClass::PublisherOffer));
        }
        assert!(!limiter.try_acquire(peer_a, EventClass::PublisherOffer));

        // Different peer has its own fresh bucket.
        assert!(limiter.try_acquire(peer_b, EventClass::PublisherOffer));

        // Different class on the same peer has its own fresh bucket.
        assert!(limiter.try_acquire(peer_a, EventClass::MuteOrWebcamToggle));
    }

    #[tokio::test(start_paused = true)]
    async fn voice_rate_limiter_forget_peer_frees_buckets() {
        let limiter = VoiceRateLimiter::new();
        let peer = Uuid::new_v4();

        // Drain publisher_offer capacity.
        for _ in 0..5 {
            assert!(limiter.try_acquire(peer, EventClass::PublisherOffer));
        }
        assert!(!limiter.try_acquire(peer, EventClass::PublisherOffer));

        limiter.forget_peer(peer);

        // Bucket is recreated at full capacity after forget.
        assert!(limiter.try_acquire(peer, EventClass::PublisherOffer));
    }

    #[test]
    fn event_class_scope_labels_are_stable() {
        assert_eq!(
            EventClass::IceCandidatePublisher.as_scope(),
            "ice_candidate_publisher"
        );
        assert_eq!(
            EventClass::IceCandidateSubscriber.as_scope(),
            "ice_candidate_subscriber"
        );
        assert_eq!(EventClass::PublisherOffer.as_scope(), "publisher_offer");
        assert_eq!(
            EventClass::ScreenShareToggle.as_scope(),
            "screen_share_toggle"
        );
        assert_eq!(
            EventClass::MuteOrWebcamToggle.as_scope(),
            "mute_or_webcam_toggle"
        );
        assert_eq!(
            EventClass::SetLayerPreference.as_scope(),
            "set_layer_preference"
        );
    }
}
