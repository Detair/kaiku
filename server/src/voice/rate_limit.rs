//! Rate limiting for voice operations.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;
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
            cleanup_interval: Duration::from_secs(60), // Default cleanup every 60 seconds
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
                return Err(VoiceError::RateLimited);
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
}
