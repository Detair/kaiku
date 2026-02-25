//! Core rate limiter service using Redis.

use std::sync::Arc;

use fred::prelude::*;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::ratelimit::{
    LimitConfig, RateLimitCategory, RateLimitConfig, RateLimitError, RateLimitResult,
    SCRIPT_ALLOWED,
};

/// Embedded Lua script for atomic rate limit check and increment.
const RATE_LIMIT_SCRIPT: &str = include_str!("rate_limit.lua");

/// Embedded Lua script for atomic failed auth tracking and blocking.
const FAILED_AUTH_SCRIPT: &str = include_str!("failed_auth.lua");

/// Script SHAs for Lua scripts loaded in Redis.
#[derive(Clone, Default)]
struct ScriptShas {
    rate_limit: String,
    failed_auth: String,
}

/// Core rate limiter service backed by Redis.
///
/// Provides atomic rate limit checks using Lua scripts for correctness,
/// IP blocking for failed authentication, and allowlist bypassing.
#[derive(Clone)]
pub struct RateLimiter {
    redis: Client,
    config: Arc<RateLimitConfig>,
    scripts: Arc<RwLock<ScriptShas>>,
}

impl RateLimiter {
    /// Creates a new rate limiter instance.
    ///
    /// Call `init()` after creation to load the Lua scripts into Redis.
    pub fn new(redis: Client, config: RateLimitConfig) -> Self {
        Self {
            redis,
            config: Arc::new(config),
            scripts: Arc::new(RwLock::new(ScriptShas::default())),
        }
    }

    /// Initializes the rate limiter by loading Lua scripts into Redis.
    ///
    /// Must be called before using `check()` or `record_failed_auth()`.
    pub async fn init(&mut self) -> Result<(), Error> {
        self.load_scripts().await
    }

    /// Loads or reloads Lua scripts into Redis.
    ///
    /// Called during init and when NOSCRIPT errors are encountered.
    async fn load_scripts(&self) -> Result<(), Error> {
        let rate_limit_sha: String = self.redis.script_load(RATE_LIMIT_SCRIPT).await?;
        let failed_auth_sha: String = self.redis.script_load(FAILED_AUTH_SCRIPT).await?;

        info!(
            rate_limit_sha = %rate_limit_sha,
            failed_auth_sha = %failed_auth_sha,
            "Lua scripts loaded into Redis"
        );

        let mut scripts = self.scripts.write().await;
        scripts.rate_limit = rate_limit_sha;
        scripts.failed_auth = failed_auth_sha;
        Ok(())
    }

    /// Checks if an error is a NOSCRIPT error (script not found in Redis).
    fn is_noscript_error(error: &Error) -> bool {
        error.to_string().contains("NOSCRIPT")
    }

    /// Checks and increments the rate limit for a given category and identifier.
    ///
    /// Returns `Ok(RateLimitResult)` with `allowed: true` if the request is permitted,
    /// or `allowed: false` with retry information if the limit is exceeded.
    ///
    /// # Arguments
    /// * `category` - The rate limit category (e.g., `AuthLogin`, `Read`, `Write`)
    /// * `identifier` - The client identifier (typically normalized IP address)
    ///
    /// # Errors
    /// Returns `RateLimitError::RedisUnavailable` if Redis is unreachable.
    #[tracing::instrument(skip(self), fields(category = %category.as_str()))]
    pub async fn check(
        &self,
        category: RateLimitCategory,
        identifier: &str,
    ) -> Result<RateLimitResult, RateLimitError> {
        // Skip rate limiting if disabled
        if !self.config.enabled {
            return Ok(RateLimitResult {
                allowed: true,
                limit: 0,
                remaining: 0,
                reset_at: 0,
                retry_after: 0,
            });
        }

        // Skip rate limiting for allowlisted IPs
        if self.is_allowed_by_config(identifier) {
            debug!(ip = %identifier, "IP in allowlist, bypassing rate limit");
            return Ok(RateLimitResult {
                allowed: true,
                limit: 0,
                remaining: 0,
                reset_at: 0,
                retry_after: 0,
            });
        }

        let limit_config = self.get_limit_config(category);
        let key = self.build_key(category.as_str(), identifier);

        // Execute Lua script atomically with NOSCRIPT retry
        let result = self.execute_rate_limit_script(&key, limit_config).await?;

        let count = result[0] as u32;
        let allowed = result[1] == SCRIPT_ALLOWED;
        let ttl = result[2].max(0) as u64;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(RateLimitResult {
            allowed,
            limit: limit_config.requests,
            remaining: if allowed {
                limit_config.requests.saturating_sub(count)
            } else {
                0
            },
            reset_at: now + ttl,
            retry_after: if allowed { 0 } else { ttl },
        })
    }

    /// Executes the rate limit Lua script with NOSCRIPT retry.
    async fn execute_rate_limit_script(
        &self,
        key: &str,
        limit_config: &LimitConfig,
    ) -> Result<Vec<i64>, RateLimitError> {
        let scripts = self.scripts.read().await;
        let sha = scripts.rate_limit.clone();
        drop(scripts);

        let result: Result<Vec<i64>, _> = self
            .redis
            .evalsha(
                &sha,
                vec![key],
                vec![
                    limit_config.window_secs.to_string(),
                    limit_config.requests.to_string(),
                ],
            )
            .await;

        match result {
            Ok(r) => Ok(r),
            Err(e) if Self::is_noscript_error(&e) => {
                warn!("NOSCRIPT error, reloading Lua scripts");
                self.load_scripts().await.map_err(|e| {
                    warn!(error = %e, "Failed to reload scripts");
                    RateLimitError::RedisUnavailable
                })?;

                // Retry with new SHA
                let scripts = self.scripts.read().await;
                let new_sha = scripts.rate_limit.clone();
                drop(scripts);

                self.redis
                    .evalsha(
                        &new_sha,
                        vec![key],
                        vec![
                            limit_config.window_secs.to_string(),
                            limit_config.requests.to_string(),
                        ],
                    )
                    .await
                    .map_err(|e| {
                        warn!(error = %e, "Redis rate limit check failed after reload");
                        RateLimitError::RedisUnavailable
                    })
            }
            Err(e) => {
                warn!(error = %e, "Redis rate limit check failed");
                Err(RateLimitError::RedisUnavailable)
            }
        }
    }

    /// Checks if the identifier is in the allowlist configuration.
    pub fn is_allowed_by_config(&self, identifier: &str) -> bool {
        self.config.allowlist.contains(identifier)
    }

    /// Records a failed authentication attempt for the given IP.
    ///
    /// Atomically increments the failure counter and blocks the IP if the threshold is exceeded.
    /// Uses a Lua script to ensure atomicity of the increment, expiry, and block operations.
    ///
    /// # Returns
    /// * `Ok(true)` if the IP is now blocked after this failure
    /// * `Ok(false)` if the IP is not yet blocked
    ///
    /// # Errors
    /// Returns `RateLimitError::RedisUnavailable` if Redis is unreachable.
    #[tracing::instrument(skip(self))]
    pub async fn record_failed_auth(&self, ip: &str) -> Result<bool, RateLimitError> {
        if !self.config.enabled {
            return Ok(false);
        }

        if self.is_allowed_by_config(ip) {
            return Ok(false);
        }

        let failed_key = self.build_key("failed_auth", ip);
        let block_key = self.build_key("blocked", ip);
        let config = &self.config.limits.failed_auth;

        // Execute atomic Lua script with NOSCRIPT retry
        let result = self
            .execute_failed_auth_script(&failed_key, &block_key, config)
            .await?;

        let count = result[0];
        let is_blocked = result[1] == SCRIPT_ALLOWED;
        let is_newly_blocked = result[2] == SCRIPT_ALLOWED;

        if is_newly_blocked {
            warn!(
                ip = %ip,
                failures = count,
                block_duration = config.block_duration_secs,
                "IP blocked due to repeated auth failures"
            );
        } else {
            debug!(
                ip = %ip,
                failures = count,
                max_failures = config.max_failures,
                is_blocked = is_blocked,
                "Auth failure recorded"
            );
        }

        Ok(is_blocked)
    }

    /// Executes the failed auth Lua script with NOSCRIPT retry.
    async fn execute_failed_auth_script(
        &self,
        failed_key: &str,
        block_key: &str,
        config: &crate::ratelimit::FailedAuthConfig,
    ) -> Result<Vec<i64>, RateLimitError> {
        let scripts = self.scripts.read().await;
        let sha = scripts.failed_auth.clone();
        drop(scripts);

        let args = vec![
            config.window_secs.to_string(),
            config.max_failures.to_string(),
            config.block_duration_secs.to_string(),
        ];

        let result: Result<Vec<i64>, _> = self
            .redis
            .evalsha(&sha, vec![failed_key, block_key], args.clone())
            .await;

        match result {
            Ok(r) => Ok(r),
            Err(e) if Self::is_noscript_error(&e) => {
                warn!("NOSCRIPT error in failed_auth, reloading Lua scripts");
                self.load_scripts().await.map_err(|e| {
                    warn!(error = %e, "Failed to reload scripts");
                    RateLimitError::RedisUnavailable
                })?;

                // Retry with new SHA
                let scripts = self.scripts.read().await;
                let new_sha = scripts.failed_auth.clone();
                drop(scripts);

                self.redis
                    .evalsha(&new_sha, vec![failed_key, block_key], args)
                    .await
                    .map_err(|e| {
                        warn!(error = %e, "Failed auth script failed after reload");
                        RateLimitError::RedisUnavailable
                    })
            }
            Err(e) => {
                warn!(error = %e, "Failed to execute failed auth script");
                Err(RateLimitError::RedisUnavailable)
            }
        }
    }

    /// Checks if the given IP address is currently blocked.
    ///
    /// # Returns
    /// * `Ok(true)` if the IP is blocked
    /// * `Ok(false)` if the IP is not blocked
    ///
    /// # Errors
    /// Returns `RateLimitError::RedisUnavailable` if Redis is unreachable.
    #[tracing::instrument(skip(self))]
    pub async fn is_blocked(&self, ip: &str) -> Result<bool, RateLimitError> {
        if !self.config.enabled {
            return Ok(false);
        }

        if self.is_allowed_by_config(ip) {
            return Ok(false);
        }

        let key = self.build_key("blocked", ip);
        let exists: bool = self.redis.exists(&key).await.map_err(|e| {
            warn!(error = %e, "Failed to check IP block status");
            RateLimitError::RedisUnavailable
        })?;

        if exists {
            debug!(ip = %ip, "IP is blocked");
        }

        Ok(exists)
    }

    /// Returns the remaining block time in seconds for a blocked IP.
    ///
    /// Returns `None` if the IP is not blocked or Redis is unavailable.
    #[tracing::instrument(skip(self))]
    pub async fn get_block_ttl(&self, ip: &str) -> Option<u64> {
        if !self.config.enabled {
            return None;
        }

        let key = self.build_key("blocked", ip);
        let ttl: i64 = self.redis.ttl(&key).await.ok()?;

        if ttl > 0 {
            Some(ttl as u64)
        } else {
            None
        }
    }

    /// Clears the failed auth counter and block for an IP.
    ///
    /// Useful when a user successfully authenticates to reset their failure count.
    #[tracing::instrument(skip(self))]
    pub async fn clear_failed_auth(&self, ip: &str) -> Result<(), RateLimitError> {
        let failed_key = self.build_key("failed_auth", ip);
        let block_key = self.build_key("blocked", ip);

        // Delete both keys (ignoring errors for individual deletes)
        let _ = self.redis.del::<(), _>(&failed_key).await;
        let _ = self.redis.del::<(), _>(&block_key).await;

        debug!(ip = %ip, "Cleared failed auth state");
        Ok(())
    }

    /// Returns the configuration for this rate limiter.
    pub fn config(&self) -> &RateLimitConfig {
        &self.config
    }

    /// Builds a Redis key with the configured prefix.
    fn build_key(&self, category: &str, identifier: &str) -> String {
        format!(
            "{}:{}:{}",
            self.config.redis_key_prefix, category, identifier
        )
    }

    /// Returns the limit configuration for a given category.
    fn get_limit_config(&self, category: RateLimitCategory) -> &LimitConfig {
        match category {
            RateLimitCategory::AuthLogin => &self.config.limits.auth_login,
            RateLimitCategory::AuthRegister => &self.config.limits.auth_register,
            RateLimitCategory::AuthPasswordReset => &self.config.limits.auth_password_reset,
            RateLimitCategory::AuthOther => &self.config.limits.auth_other,
            RateLimitCategory::Write => &self.config.limits.write,
            RateLimitCategory::Social => &self.config.limits.social,
            RateLimitCategory::Read => &self.config.limits.read,
            RateLimitCategory::WsConnect => &self.config.limits.ws_connect,
            RateLimitCategory::WsMessage => &self.config.limits.ws_message,
            RateLimitCategory::VoiceJoin => &self.config.limits.voice_join,
            RateLimitCategory::Search => &self.config.limits.search,
            RateLimitCategory::DataGovernance => &self.config.limits.data_governance,
            RateLimitCategory::FailedAuth => {
                // FailedAuth uses max_failures as requests and window_secs from failed_auth config.
                // Note: This category should not be used with check() - use record_failed_auth()
                // instead. This exists for consistency in the type system.
                &self.config.limits.failed_auth_as_limit
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn mock_config() -> RateLimitConfig {
        RateLimitConfig {
            enabled: true,
            redis_key_prefix: "test:rl".to_string(),
            fail_open: true,
            trust_proxy: false,
            allowlist: HashSet::from(["127.0.0.1".to_string()]),
            ..Default::default()
        }
    }

    fn create_mock_limiter(config: RateLimitConfig) -> RateLimiter {
        RateLimiter {
            redis: create_mock_client(),
            config: Arc::new(config),
            scripts: Arc::new(RwLock::new(ScriptShas::default())),
        }
    }

    #[test]
    fn test_build_key() {
        let limiter = create_mock_limiter(mock_config());

        let key = limiter.build_key("auth_login", "192.168.1.1");
        assert_eq!(key, "test:rl:auth_login:192.168.1.1");
    }

    #[test]
    fn test_is_allowed_by_config() {
        let limiter = create_mock_limiter(mock_config());

        assert!(limiter.is_allowed_by_config("127.0.0.1"));
        assert!(!limiter.is_allowed_by_config("192.168.1.1"));
    }

    #[test]
    fn test_get_limit_config() {
        let limiter = create_mock_limiter(mock_config());

        let auth_login = limiter.get_limit_config(RateLimitCategory::AuthLogin);
        assert_eq!(auth_login.requests, 3);
        assert_eq!(auth_login.window_secs, 60);

        let read = limiter.get_limit_config(RateLimitCategory::Read);
        assert_eq!(read.requests, 200);
    }

    /// Helper to create a mock Redis client for tests that don't need actual Redis.
    fn create_mock_client() -> Client {
        let config = Config::from_url("redis://localhost:6379").unwrap();
        Client::new(config, None, None, None)
    }
}
