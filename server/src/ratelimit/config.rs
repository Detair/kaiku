//! Rate limiting configuration.

use std::collections::HashSet;

/// Configuration for the rate limiting system.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Whether rate limiting is enabled
    pub enabled: bool,
    /// Prefix for Redis keys (e.g., "canis:rl")
    pub redis_key_prefix: String,
    /// Whether to allow requests when Redis is unavailable
    pub fail_open: bool,
    /// Whether to trust X-Forwarded-For headers
    pub trust_proxy: bool,
    /// IP addresses that bypass rate limiting
    pub allowlist: HashSet<String>,
    /// Per-category rate limits
    pub limits: RateLimits,
}

/// Rate limits for each category.
#[derive(Debug, Clone)]
pub struct RateLimits {
    /// Login attempts
    pub auth_login: LimitConfig,
    /// Registration attempts
    pub auth_register: LimitConfig,
    /// Password reset requests
    pub auth_password_reset: LimitConfig,
    /// Other auth operations (token refresh, etc.)
    pub auth_other: LimitConfig,
    /// Write operations (create/update/delete)
    pub write: LimitConfig,
    /// Social operations (friend requests, invites)
    pub social: LimitConfig,
    /// Read operations (fetch data)
    pub read: LimitConfig,
    /// WebSocket connection attempts
    pub ws_connect: LimitConfig,
    /// WebSocket message rate
    pub ws_message: LimitConfig,
    /// Voice channel join attempts
    pub voice_join: LimitConfig,
    /// Search operations
    pub search: LimitConfig,
    /// Data governance operations (export, deletion)
    pub data_governance: LimitConfig,
    /// Failed authentication tracking
    pub failed_auth: FailedAuthConfig,
    /// Failed auth as `LimitConfig` (for consistency in `get_limit_config`)
    /// This is computed from `failed_auth` config.
    pub failed_auth_as_limit: LimitConfig,
}

/// Configuration for a single rate limit.
#[derive(Debug, Clone)]
pub struct LimitConfig {
    /// Maximum requests allowed in the window
    pub requests: u32,
    /// Window duration in seconds
    pub window_secs: u64,
}

/// Configuration for failed authentication tracking.
#[derive(Debug, Clone)]
pub struct FailedAuthConfig {
    /// Maximum failed attempts before blocking
    pub max_failures: u32,
    /// Duration to block in seconds after max failures
    pub block_duration_secs: u64,
    /// Window for counting failures in seconds
    pub window_secs: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            redis_key_prefix: "canis:rl".to_string(),
            // Security: fail_closed by default - reject requests when Redis unavailable
            // Set RATE_LIMIT_FAIL_OPEN=true only if availability > security for your use case
            fail_open: false,
            trust_proxy: false,
            allowlist: HashSet::new(),
            limits: RateLimits::default(),
        }
    }
}

impl Default for RateLimits {
    fn default() -> Self {
        let failed_auth = FailedAuthConfig {
            max_failures: 10,
            block_duration_secs: 900,
            window_secs: 300,
        };
        Self {
            auth_login: LimitConfig {
                requests: 3,
                window_secs: 60,
            },
            auth_register: LimitConfig {
                requests: 5,
                window_secs: 60,
            },
            auth_password_reset: LimitConfig {
                requests: 2,
                window_secs: 60,
            },
            auth_other: LimitConfig {
                requests: 20,
                window_secs: 60,
            },
            write: LimitConfig {
                requests: 30,
                window_secs: 60,
            },
            social: LimitConfig {
                requests: 20,
                window_secs: 60,
            },
            read: LimitConfig {
                requests: 200,
                window_secs: 60,
            },
            ws_connect: LimitConfig {
                requests: 10,
                window_secs: 60,
            },
            ws_message: LimitConfig {
                requests: 60,
                window_secs: 60,
            },
            voice_join: LimitConfig {
                requests: 5, // 5 joins per minute should be plenty for normal use
                window_secs: 60,
            },
            search: LimitConfig {
                requests: 15,
                window_secs: 60,
            },
            data_governance: LimitConfig {
                requests: 2,
                window_secs: 60,
            },
            failed_auth_as_limit: LimitConfig {
                requests: failed_auth.max_failures,
                window_secs: failed_auth.window_secs,
            },
            failed_auth,
        }
    }
}

impl RateLimitConfig {
    /// Creates configuration from environment variables.
    ///
    /// Environment variables:
    /// - `RATE_LIMIT_ENABLED`: Enable/disable rate limiting (default: true)
    /// - `RATE_LIMIT_PREFIX`: Redis key prefix (default: "canis:rl")
    /// - `RATE_LIMIT_FAIL_OPEN`: Allow requests when Redis unavailable (default: false)
    /// - `RATE_LIMIT_TRUST_PROXY`: Trust X-Forwarded-For headers (default: false)
    /// - `RATE_LIMIT_ALLOWLIST`: Comma-separated IP allowlist
    /// - `RATE_LIMIT_AUTH_LOGIN`: Login limit as "`requests,window_secs`"
    /// - `RATE_LIMIT_AUTH_REGISTER`: Register limit as "`requests,window_secs`"
    /// - `RATE_LIMIT_AUTH_PASSWORD_RESET`: Password reset limit as "`requests,window_secs`"
    /// - `RATE_LIMIT_AUTH_OTHER`: Other auth limit as "`requests,window_secs`"
    /// - `RATE_LIMIT_WRITE`: Write limit as "`requests,window_secs`"
    /// - `RATE_LIMIT_SOCIAL`: Social limit as "`requests,window_secs`"
    /// - `RATE_LIMIT_READ`: Read limit as "`requests,window_secs`"
    /// - `RATE_LIMIT_WS_CONNECT`: WebSocket connect limit as "`requests,window_secs`"
    /// - `RATE_LIMIT_WS_MESSAGE`: WebSocket message limit as "`requests,window_secs`"
    /// - `RATE_LIMIT_SEARCH`: Search limit as "`requests,window_secs`"
    /// - `RATE_LIMIT_FAILED_AUTH`: Failed auth as "`max_failures,block_duration_secs,window_secs`"
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(val) = std::env::var("RATE_LIMIT_ENABLED") {
            config.enabled = val.parse().unwrap_or(true);
        }
        if let Ok(val) = std::env::var("RATE_LIMIT_PREFIX") {
            config.redis_key_prefix = val;
        }
        if let Ok(val) = std::env::var("RATE_LIMIT_FAIL_OPEN") {
            config.fail_open = val.parse().unwrap_or(false);
        }
        if let Ok(val) = std::env::var("RATE_LIMIT_TRUST_PROXY") {
            config.trust_proxy = val.parse().unwrap_or(false);
        }
        if let Ok(val) = std::env::var("RATE_LIMIT_ALLOWLIST") {
            config.allowlist = val.split(',').map(|s| s.trim().to_string()).collect();
        }

        // Parse per-category limits (format: "requests,window_secs")
        if let Ok(val) = std::env::var("RATE_LIMIT_AUTH_LOGIN") {
            if let Some(limit) = parse_limit_config(&val) {
                config.limits.auth_login = limit;
            }
        }
        if let Ok(val) = std::env::var("RATE_LIMIT_AUTH_REGISTER") {
            if let Some(limit) = parse_limit_config(&val) {
                config.limits.auth_register = limit;
            }
        }
        if let Ok(val) = std::env::var("RATE_LIMIT_AUTH_PASSWORD_RESET") {
            if let Some(limit) = parse_limit_config(&val) {
                config.limits.auth_password_reset = limit;
            }
        }
        if let Ok(val) = std::env::var("RATE_LIMIT_AUTH_OTHER") {
            if let Some(limit) = parse_limit_config(&val) {
                config.limits.auth_other = limit;
            }
        }
        if let Ok(val) = std::env::var("RATE_LIMIT_WRITE") {
            if let Some(limit) = parse_limit_config(&val) {
                config.limits.write = limit;
            }
        }
        if let Ok(val) = std::env::var("RATE_LIMIT_SOCIAL") {
            if let Some(limit) = parse_limit_config(&val) {
                config.limits.social = limit;
            }
        }
        if let Ok(val) = std::env::var("RATE_LIMIT_READ") {
            if let Some(limit) = parse_limit_config(&val) {
                config.limits.read = limit;
            }
        }
        if let Ok(val) = std::env::var("RATE_LIMIT_WS_CONNECT") {
            if let Some(limit) = parse_limit_config(&val) {
                config.limits.ws_connect = limit;
            }
        }
        if let Ok(val) = std::env::var("RATE_LIMIT_WS_MESSAGE") {
            if let Some(limit) = parse_limit_config(&val) {
                config.limits.ws_message = limit;
            }
        }
        if let Ok(val) = std::env::var("RATE_LIMIT_VOICE_JOIN") {
            if let Some(limit) = parse_limit_config(&val) {
                config.limits.voice_join = limit;
            }
        }
        if let Ok(val) = std::env::var("RATE_LIMIT_SEARCH") {
            if let Some(limit) = parse_limit_config(&val) {
                config.limits.search = limit;
            }
        }
        if let Ok(val) = std::env::var("RATE_LIMIT_FAILED_AUTH") {
            if let Some(limit) = parse_failed_auth_config(&val) {
                config.limits.failed_auth = limit;
            }
        }

        // Keep failed_auth_as_limit in sync with failed_auth
        config.limits.failed_auth_as_limit = LimitConfig {
            requests: config.limits.failed_auth.max_failures,
            window_secs: config.limits.failed_auth.window_secs,
        };

        config
    }
}

/// Parses a limit config from "`requests,window_secs`" format.
fn parse_limit_config(val: &str) -> Option<LimitConfig> {
    let parts: Vec<&str> = val.split(',').collect();
    if parts.len() == 2 {
        let requests = parts[0].trim().parse().ok()?;
        let window_secs = parts[1].trim().parse().ok()?;
        Some(LimitConfig {
            requests,
            window_secs,
        })
    } else {
        None
    }
}

/// Parses a failed auth config from "`max_failures,block_duration_secs,window_secs`" format.
fn parse_failed_auth_config(val: &str) -> Option<FailedAuthConfig> {
    let parts: Vec<&str> = val.split(',').collect();
    if parts.len() == 3 {
        let max_failures = parts[0].trim().parse().ok()?;
        let block_duration_secs = parts[1].trim().parse().ok()?;
        let window_secs = parts[2].trim().parse().ok()?;
        Some(FailedAuthConfig {
            max_failures,
            block_duration_secs,
            window_secs,
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = RateLimitConfig::default();
        assert!(config.enabled);
        assert_eq!(config.redis_key_prefix, "canis:rl");
        assert!(!config.fail_open);
        assert!(!config.trust_proxy);
        assert!(config.allowlist.is_empty());
    }

    #[test]
    fn test_default_limits() {
        let limits = RateLimits::default();
        assert_eq!(limits.auth_login.requests, 3);
        assert_eq!(limits.auth_login.window_secs, 60);
        assert_eq!(limits.read.requests, 200);
        assert_eq!(limits.failed_auth.max_failures, 10);
        assert_eq!(limits.failed_auth.block_duration_secs, 900);
    }

    #[test]
    fn test_parse_limit_config() {
        assert!(parse_limit_config("10,60").is_some());
        let limit = parse_limit_config("10,60").unwrap();
        assert_eq!(limit.requests, 10);
        assert_eq!(limit.window_secs, 60);

        // With whitespace
        let limit = parse_limit_config(" 20 , 120 ").unwrap();
        assert_eq!(limit.requests, 20);
        assert_eq!(limit.window_secs, 120);

        // Invalid formats
        assert!(parse_limit_config("10").is_none());
        assert!(parse_limit_config("10,60,extra").is_none());
        assert!(parse_limit_config("abc,60").is_none());
    }

    #[test]
    fn test_parse_failed_auth_config() {
        let config = parse_failed_auth_config("5,300,120").unwrap();
        assert_eq!(config.max_failures, 5);
        assert_eq!(config.block_duration_secs, 300);
        assert_eq!(config.window_secs, 120);

        // Invalid formats
        assert!(parse_failed_auth_config("5,300").is_none());
        assert!(parse_failed_auth_config("abc,300,120").is_none());
    }
}
