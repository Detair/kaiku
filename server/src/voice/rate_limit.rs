//! Rate limiting for voice operations.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use uuid::Uuid;

use super::error::VoiceError;

/// Rate limiter for voice operations.
pub struct VoiceRateLimiter {
    /// Map of user_id to last join time.
    last_join: Arc<RwLock<HashMap<Uuid, Instant>>>,
    /// Minimum time between join requests.
    min_interval: Duration,
}

impl VoiceRateLimiter {
    /// Create a new rate limiter.
    pub fn new(min_interval: Duration) -> Self {
        Self {
            last_join: Arc::new(RwLock::new(HashMap::new())),
            min_interval,
        }
    }

    /// Create a rate limiter with default settings (1 join per second).
    pub fn default() -> Self {
        Self::new(Duration::from_secs(1))
    }

    /// Check if a user can join voice (rate limit check).
    pub async fn check_join(&self, user_id: Uuid) -> Result<(), VoiceError> {
        let mut map = self.last_join.write().await;

        if let Some(last) = map.get(&user_id) {
            let elapsed = last.elapsed();
            if elapsed < self.min_interval {
                // Still within rate limit window
                return Err(VoiceError::RateLimited);
            }
        }

        // Update last join time
        map.insert(user_id, Instant::now());
        Ok(())
    }

    /// Cleanup old entries (call periodically to prevent memory leak).
    #[allow(dead_code)]
    pub async fn cleanup(&self) {
        let cleanup_threshold = self.min_interval * 10; // Keep 10x the interval
        let mut map = self.last_join.write().await;

        map.retain(|_, last| last.elapsed() < cleanup_threshold);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter_allows_first_join() {
        let limiter = VoiceRateLimiter::new(Duration::from_millis(100));
        let user_id = Uuid::new_v4();

        // First join should succeed
        assert!(limiter.check_join(user_id).await.is_ok());
    }

    #[tokio::test]
    async fn test_rate_limiter_blocks_rapid_joins() {
        let limiter = VoiceRateLimiter::new(Duration::from_millis(100));
        let user_id = Uuid::new_v4();

        // First join succeeds
        assert!(limiter.check_join(user_id).await.is_ok());

        // Immediate second join should fail
        assert!(limiter.check_join(user_id).await.is_err());
    }

    #[tokio::test]
    async fn test_rate_limiter_allows_after_interval() {
        let limiter = VoiceRateLimiter::new(Duration::from_millis(50));
        let user_id = Uuid::new_v4();

        // First join succeeds
        assert!(limiter.check_join(user_id).await.is_ok());

        // Wait for rate limit to expire
        tokio::time::sleep(Duration::from_millis(60)).await;

        // Second join should now succeed
        assert!(limiter.check_join(user_id).await.is_ok());
    }

    #[tokio::test]
    async fn test_rate_limiter_independent_users() {
        let limiter = VoiceRateLimiter::new(Duration::from_millis(100));
        let user1 = Uuid::new_v4();
        let user2 = Uuid::new_v4();

        // Both users should be able to join
        assert!(limiter.check_join(user1).await.is_ok());
        assert!(limiter.check_join(user2).await.is_ok());
    }

    #[tokio::test]
    async fn test_cleanup_removes_old_entries() {
        let limiter = VoiceRateLimiter::new(Duration::from_millis(10));
        let user_id = Uuid::new_v4();

        // Join and verify entry exists
        limiter.check_join(user_id).await.ok();
        assert_eq!(limiter.last_join.read().await.len(), 1);

        // Wait for cleanup threshold
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Cleanup should remove old entries
        limiter.cleanup().await;
        assert_eq!(limiter.last_join.read().await.len(), 0);
    }
}
