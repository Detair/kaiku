//! Rate limiting for voice operations.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use uuid::Uuid;

use super::error::VoiceError;

/// Rate limiter for voice stats operations.
pub struct VoiceStatsLimiter {
    /// Map of `user_id` to last stats reporting time.
    last_stats: Arc<RwLock<HashMap<Uuid, Instant>>>,
    /// Minimum time between stats reports.
    min_stats_interval: Duration,
}

impl VoiceStatsLimiter {
    /// Create a new rate limiter.
    pub fn new(min_stats_interval: Duration) -> Self {
        Self {
            last_stats: Arc::new(RwLock::new(HashMap::new())),
            min_stats_interval,
        }
    }

    /// Create a rate limiter with default settings.
    /// - 1 stats report per second
    pub fn default() -> Self {
        Self::new(Duration::from_secs(1))
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
    async fn test_rate_limiter_allows_first_join() {
        let limiter = VoiceRateLimiter::new(Duration::from_millis(100), Duration::from_millis(100));
        let user_id = Uuid::new_v4();

        // First join should succeed
        assert!(limiter.check_join(user_id).await.is_ok());
    }

    #[tokio::test]
    async fn test_rate_limiter_blocks_rapid_joins() {
        let limiter = VoiceRateLimiter::new(Duration::from_millis(100), Duration::from_millis(100));
        let user_id = Uuid::new_v4();

        // First join succeeds
        assert!(limiter.check_join(user_id).await.is_ok());

        // Immediate second join should fail
        assert!(limiter.check_join(user_id).await.is_err());
    }

    #[tokio::test]
    async fn test_rate_limiter_allows_after_interval() {
        let limiter = VoiceRateLimiter::new(Duration::from_millis(50), Duration::from_millis(50));
        let user_id = Uuid::new_v4();

        // First join succeeds
        assert!(limiter.check_join(user_id).await.is_ok());

        // Wait for rate limit to expire
        tokio::time::sleep(Duration::from_millis(60)).await;

        // Second join should now succeed
        assert!(limiter.check_join(user_id).await.is_ok());
    }

    #[tokio::test]
    async fn test_rate_limiter_stats() {
        let limiter = VoiceRateLimiter::new(Duration::from_millis(100), Duration::from_millis(100));
        let user_id = Uuid::new_v4();

        // First stats succeeds
        assert!(limiter.check_stats(user_id).await.is_ok());

        // Immediate second stats should fail
        assert!(limiter.check_stats(user_id).await.is_err());

        // Wait for rate limit to expire
        tokio::time::sleep(Duration::from_millis(110)).await;

        // Third stats should now succeed
        assert!(limiter.check_stats(user_id).await.is_ok());
    }

    #[tokio::test]
    async fn test_rate_limiter_independent_users() {
        let limiter = VoiceRateLimiter::new(Duration::from_millis(100), Duration::from_millis(100));
        let user1 = Uuid::new_v4();
        let user2 = Uuid::new_v4();

        // Both users should be able to join
        assert!(limiter.check_join(user1).await.is_ok());
        assert!(limiter.check_join(user2).await.is_ok());
    }

    #[tokio::test]
    async fn test_cleanup_removes_old_entries() {
        let limiter = VoiceRateLimiter::new(Duration::from_millis(10), Duration::from_millis(10));
        let user_id = Uuid::new_v4();

        // Join and verify entry exists
        limiter.check_join(user_id).await.ok();
        limiter.check_stats(user_id).await.ok();
        assert_eq!(limiter.last_join.read().await.len(), 1);
        assert_eq!(limiter.last_stats.read().await.len(), 1);

        // Wait for cleanup threshold
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Cleanup should remove old entries
        limiter.cleanup().await;
        assert_eq!(limiter.last_join.read().await.len(), 0);
        assert_eq!(limiter.last_stats.read().await.len(), 0);
    }
}
