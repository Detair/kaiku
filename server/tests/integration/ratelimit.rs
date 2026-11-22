//! Integration tests for the rate limiting system.
//!
//! These tests require a running Redis instance at `redis://localhost:6379`.
//! Run with: `cargo test ratelimit --ignored -- --nocapture`

use std::collections::HashSet;

use vc_server::config::Config;
use vc_server::db;
use vc_server::ratelimit::{
    FailedAuthConfig, LimitConfig, RateLimitCategory, RateLimitConfig, RateLimiter, RateLimits,
};

/// Helper to create a test Redis client connected to localhost.
async fn create_test_redis() -> fred::clients::Client {
    let config = Config::default_for_test();
    db::create_redis_client(&config.redis_url)
        .await
        .expect("Failed to connect to Redis")
}

/// Helper to create a rate limiter with test-specific configuration.
///
/// Uses a unique key prefix to avoid conflicts between test runs.
async fn create_test_limiter(redis: fred::clients::Client) -> RateLimiter {
    let config = RateLimitConfig {
        enabled: true,
        redis_key_prefix: format!("test:rl:{}", uuid::Uuid::new_v4()),
        fail_open: false,
        trusted_proxies: vc_server::ratelimit::TrustedProxies::default(),
        allowlist: HashSet::new(),
        limits: RateLimits {
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
                requests: 10,
                window_secs: 60,
            },
            approval_login: LimitConfig {
                requests: 30,
                window_secs: 60,
            },
            auth_flow: LimitConfig {
                requests: 60,
                window_secs: 60,
            },
            auth_passkey: LimitConfig {
                requests: 30,
                window_secs: 60,
            },
            write: LimitConfig {
                requests: 10,
                window_secs: 60,
            },
            social: LimitConfig {
                requests: 10,
                window_secs: 60,
            },
            read: LimitConfig {
                requests: 20,
                window_secs: 60,
            },
            ws_connect: LimitConfig {
                requests: 5,
                window_secs: 60,
            },
            ws_message: LimitConfig {
                requests: 30,
                window_secs: 60,
            },
            voice_join: LimitConfig {
                requests: 5,
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
            webhook_execute: LimitConfig {
                requests: 5,
                window_secs: 2,
            },
            failed_auth: FailedAuthConfig {
                max_failures: 3,
                block_duration_secs: 60,
                window_secs: 300,
            },
            failed_auth_as_limit: LimitConfig {
                requests: 3,
                window_secs: 300,
            },
        },
    };
    let mut limiter = RateLimiter::new(redis, config);
    limiter.init().await.expect("Failed to initialize limiter");
    limiter
}

/// Test that requests under the rate limit are allowed.
#[tokio::test]
#[ignore] // Requires Redis
async fn test_under_limit_allows_requests() {
    let redis = create_test_redis().await;
    let limiter = create_test_limiter(redis).await;

    // Unique identifier for this test to avoid conflicts
    let identifier = format!("test-ip-under-{}", uuid::Uuid::new_v4());

    // First request should be allowed with 2 remaining (3 - 1 = 2)
    let result = limiter
        .check(RateLimitCategory::AuthLogin, &identifier)
        .await
        .expect("Rate limit check failed");

    assert!(result.allowed, "First request should be allowed");
    assert_eq!(result.limit, 3, "Limit should be 3");
    assert_eq!(
        result.remaining, 2,
        "Remaining should be 2 after first request"
    );
    assert_eq!(result.retry_after, 0, "No retry needed when allowed");

    // Second request should also be allowed
    let result = limiter
        .check(RateLimitCategory::AuthLogin, &identifier)
        .await
        .expect("Rate limit check failed");

    assert!(result.allowed, "Second request should be allowed");
    assert_eq!(
        result.remaining, 1,
        "Remaining should be 1 after second request"
    );

    // Third request should be allowed (at the limit)
    let result = limiter
        .check(RateLimitCategory::AuthLogin, &identifier)
        .await
        .expect("Rate limit check failed");

    assert!(result.allowed, "Third request should be allowed");
    assert_eq!(result.remaining, 0, "Remaining should be 0 at limit");

    println!("Under limit test passed: requests within limit are allowed");
}

/// Test that requests over the rate limit are blocked with retry information.
#[tokio::test]
#[ignore] // Requires Redis
async fn test_over_limit_blocks_with_retry_info() {
    let redis = create_test_redis().await;
    let limiter = create_test_limiter(redis).await;

    // Unique identifier for this test
    let identifier = format!("test-ip-over-{}", uuid::Uuid::new_v4());

    // Use up all allowed requests (3)
    for i in 0..3 {
        let result = limiter
            .check(RateLimitCategory::AuthLogin, &identifier)
            .await
            .expect("Rate limit check failed");
        assert!(
            result.allowed,
            "Request {} should be allowed within limit",
            i + 1
        );
    }

    // Fourth request should be blocked
    let result = limiter
        .check(RateLimitCategory::AuthLogin, &identifier)
        .await
        .expect("Rate limit check failed");

    assert!(!result.allowed, "Fourth request should be blocked");
    assert_eq!(result.remaining, 0, "No requests remaining when blocked");
    assert!(
        result.retry_after > 0,
        "Should provide retry_after when blocked"
    );
    assert!(
        result.retry_after <= 60,
        "retry_after should be <= window size"
    );

    // Fifth request should also be blocked
    let result = limiter
        .check(RateLimitCategory::AuthLogin, &identifier)
        .await
        .expect("Rate limit check failed");

    assert!(!result.allowed, "Fifth request should also be blocked");
    assert!(result.retry_after > 0, "Still should have retry_after");

    println!(
        "Over limit test passed: excess requests blocked with retry_after = {}s",
        result.retry_after
    );
}

/// Test that failed authentication attempts block IP after threshold.
#[tokio::test]
#[ignore] // Requires Redis
async fn test_failed_auth_blocks_ip_after_threshold() {
    let redis = create_test_redis().await;

    // Create limiter with custom config for predictable testing
    let config = RateLimitConfig {
        enabled: true,
        redis_key_prefix: format!("test:rl:{}", uuid::Uuid::new_v4()),
        fail_open: false,
        trusted_proxies: vc_server::ratelimit::TrustedProxies::default(),
        allowlist: HashSet::new(),
        limits: RateLimits {
            failed_auth: FailedAuthConfig {
                max_failures: 3,
                block_duration_secs: 60,
                window_secs: 300,
            },
            ..RateLimits::default()
        },
    };

    let mut limiter = RateLimiter::new(redis, config);
    limiter.init().await.expect("Failed to initialize limiter");

    let ip = format!("test-ip-fail-{}", uuid::Uuid::new_v4());

    // Initially, IP should not be blocked
    let blocked = limiter
        .is_blocked(&ip)
        .await
        .expect("is_blocked check failed");
    assert!(!blocked, "IP should not be blocked initially");

    // Record failures until threshold (3)
    for i in 0..3 {
        let now_blocked = limiter
            .record_failed_auth(&ip)
            .await
            .expect("record_failed_auth failed");

        if i < 2 {
            assert!(
                !now_blocked,
                "IP should not be blocked after {} failures",
                i + 1
            );
        } else {
            assert!(
                now_blocked,
                "IP should be blocked after {} failures (threshold reached)",
                i + 1
            );
        }
    }

    // Verify IP is now blocked
    let blocked = limiter
        .is_blocked(&ip)
        .await
        .expect("is_blocked check failed");
    assert!(blocked, "IP should be blocked after exceeding threshold");

    // Check block TTL is set
    let ttl = limiter.get_block_ttl(&ip).await;
    assert!(ttl.is_some(), "Block should have a TTL");
    assert!(ttl.unwrap() > 0, "Block TTL should be positive");
    assert!(
        ttl.unwrap() <= 60,
        "Block TTL should not exceed block_duration_secs"
    );

    println!("Failed auth test passed: IP blocked after 3 failures, TTL = {ttl:?}s");
}

/// Test that clearing failed auth removes block.
#[tokio::test]
#[ignore] // Requires Redis
async fn test_clear_failed_auth_removes_block() {
    let redis = create_test_redis().await;

    let config = RateLimitConfig {
        enabled: true,
        redis_key_prefix: format!("test:rl:{}", uuid::Uuid::new_v4()),
        fail_open: false,
        trusted_proxies: vc_server::ratelimit::TrustedProxies::default(),
        allowlist: HashSet::new(),
        limits: RateLimits {
            failed_auth: FailedAuthConfig {
                max_failures: 2,
                block_duration_secs: 60,
                window_secs: 300,
            },
            ..RateLimits::default()
        },
    };

    let mut limiter = RateLimiter::new(redis, config);
    limiter.init().await.expect("Failed to initialize limiter");

    let ip = format!("test-ip-clear-{}", uuid::Uuid::new_v4());

    // Block the IP
    for _ in 0..2 {
        limiter
            .record_failed_auth(&ip)
            .await
            .expect("record_failed_auth failed");
    }

    // Verify blocked
    let blocked = limiter.is_blocked(&ip).await.expect("is_blocked failed");
    assert!(blocked, "IP should be blocked after failures");

    // Clear the block
    limiter
        .clear_failed_auth(&ip)
        .await
        .expect("clear_failed_auth failed");

    // Verify no longer blocked
    let blocked = limiter.is_blocked(&ip).await.expect("is_blocked failed");
    assert!(!blocked, "IP should not be blocked after clear");

    println!("Clear failed auth test passed: block removed successfully");
}

/// Test that allowlisted IPs bypass rate limiting.
#[tokio::test]
#[ignore] // Requires Redis
async fn test_allowlist_bypasses_rate_limiting() {
    let redis = create_test_redis().await;

    let allowlisted_ip = "192.168.1.100";
    let non_allowlisted_ip = format!("test-ip-noallow-{}", uuid::Uuid::new_v4());

    let config = RateLimitConfig {
        enabled: true,
        redis_key_prefix: format!("test:rl:{}", uuid::Uuid::new_v4()),
        fail_open: false,
        trusted_proxies: vc_server::ratelimit::TrustedProxies::default(),
        allowlist: HashSet::from([allowlisted_ip.to_string()]),
        limits: RateLimits {
            auth_login: LimitConfig {
                requests: 1, // Very low limit
                window_secs: 60,
            },
            ..RateLimits::default()
        },
    };

    let mut limiter = RateLimiter::new(redis, config);
    limiter.init().await.expect("Failed to initialize limiter");

    // Allowlisted IP should always be allowed, even with many requests
    for i in 0..10 {
        let result = limiter
            .check(RateLimitCategory::AuthLogin, allowlisted_ip)
            .await
            .expect("Rate limit check failed");
        assert!(
            result.allowed,
            "Allowlisted IP should always be allowed (request {})",
            i + 1
        );
    }

    // Non-allowlisted IP should be rate limited
    let result = limiter
        .check(RateLimitCategory::AuthLogin, &non_allowlisted_ip)
        .await
        .expect("Rate limit check failed");
    assert!(result.allowed, "First request should be allowed");

    // Second request should be blocked (limit is 1)
    let result = limiter
        .check(RateLimitCategory::AuthLogin, &non_allowlisted_ip)
        .await
        .expect("Rate limit check failed");
    assert!(
        !result.allowed,
        "Non-allowlisted IP should be blocked after limit"
    );

    println!("Allowlist test passed: allowlisted IPs bypass rate limits");
}

/// Test that allowlisted IPs are also not blocked for failed auth.
#[tokio::test]
#[ignore] // Requires Redis
async fn test_allowlist_bypasses_failed_auth_blocking() {
    let redis = create_test_redis().await;

    let allowlisted_ip = "10.0.0.50";

    let config = RateLimitConfig {
        enabled: true,
        redis_key_prefix: format!("test:rl:{}", uuid::Uuid::new_v4()),
        fail_open: false,
        trusted_proxies: vc_server::ratelimit::TrustedProxies::default(),
        allowlist: HashSet::from([allowlisted_ip.to_string()]),
        limits: RateLimits {
            failed_auth: FailedAuthConfig {
                max_failures: 1, // Very low threshold
                block_duration_secs: 60,
                window_secs: 300,
            },
            ..RateLimits::default()
        },
    };

    let mut limiter = RateLimiter::new(redis, config);
    limiter.init().await.expect("Failed to initialize limiter");

    // Allowlisted IP should never be blocked even with many failures
    for _ in 0..10 {
        let blocked = limiter
            .record_failed_auth(allowlisted_ip)
            .await
            .expect("record_failed_auth failed");
        assert!(!blocked, "Allowlisted IP should never report as blocked");
    }

    // Verify not blocked
    let blocked = limiter
        .is_blocked(allowlisted_ip)
        .await
        .expect("is_blocked failed");
    assert!(!blocked, "Allowlisted IP should not be blocked");

    println!("Allowlist failed auth test passed: allowlisted IPs bypass blocking");
}

/// Test that different categories have independent limits.
#[tokio::test]
#[ignore] // Requires Redis
async fn test_categories_are_independent() {
    let redis = create_test_redis().await;
    let limiter = create_test_limiter(redis).await;

    let identifier = format!("test-ip-cat-{}", uuid::Uuid::new_v4());

    // Use up auth_login limit (3)
    for _ in 0..3 {
        limiter
            .check(RateLimitCategory::AuthLogin, &identifier)
            .await
            .expect("Rate limit check failed");
    }

    // Auth login should be blocked
    let result = limiter
        .check(RateLimitCategory::AuthLogin, &identifier)
        .await
        .expect("Rate limit check failed");
    assert!(!result.allowed, "Auth login should be blocked");

    // But auth_register should still work (different category)
    let result = limiter
        .check(RateLimitCategory::AuthRegister, &identifier)
        .await
        .expect("Rate limit check failed");
    assert!(
        result.allowed,
        "Auth register should still be allowed (different category)"
    );

    // And read operations should work (different category)
    let result = limiter
        .check(RateLimitCategory::Read, &identifier)
        .await
        .expect("Rate limit check failed");
    assert!(
        result.allowed,
        "Read should still be allowed (different category)"
    );

    println!("Category independence test passed: different categories have separate limits");
}

/// Test that different identifiers have independent limits.
#[tokio::test]
#[ignore] // Requires Redis
async fn test_identifiers_are_independent() {
    let redis = create_test_redis().await;
    let limiter = create_test_limiter(redis).await;

    let ip1 = format!("test-ip-a-{}", uuid::Uuid::new_v4());
    let ip2 = format!("test-ip-b-{}", uuid::Uuid::new_v4());

    // Use up IP1's limit
    for _ in 0..3 {
        limiter
            .check(RateLimitCategory::AuthLogin, &ip1)
            .await
            .expect("Rate limit check failed");
    }

    // IP1 should be blocked
    let result = limiter
        .check(RateLimitCategory::AuthLogin, &ip1)
        .await
        .expect("Rate limit check failed");
    assert!(!result.allowed, "IP1 should be blocked");

    // IP2 should still be allowed
    let result = limiter
        .check(RateLimitCategory::AuthLogin, &ip2)
        .await
        .expect("Rate limit check failed");
    assert!(result.allowed, "IP2 should still be allowed");
    assert_eq!(result.remaining, 2, "IP2 should have fresh limit");

    println!("Identifier independence test passed: different IPs have separate limits");
}

/// Test rate limiter behavior when disabled.
#[tokio::test]
#[ignore] // Requires Redis
async fn test_disabled_rate_limiter_allows_all() {
    let redis = create_test_redis().await;

    let config = RateLimitConfig {
        enabled: false, // Disabled
        redis_key_prefix: format!("test:rl:{}", uuid::Uuid::new_v4()),
        fail_open: false,
        trusted_proxies: vc_server::ratelimit::TrustedProxies::default(),
        allowlist: HashSet::new(),
        limits: RateLimits {
            auth_login: LimitConfig {
                requests: 1,
                window_secs: 60,
            },
            ..RateLimits::default()
        },
    };

    let mut limiter = RateLimiter::new(redis, config);
    limiter.init().await.expect("Failed to initialize limiter");

    let identifier = format!("test-ip-disabled-{}", uuid::Uuid::new_v4());

    // All requests should be allowed when disabled
    for i in 0..100 {
        let result = limiter
            .check(RateLimitCategory::AuthLogin, &identifier)
            .await
            .expect("Rate limit check failed");
        assert!(
            result.allowed,
            "Request {} should be allowed when rate limiting is disabled",
            i + 1
        );
    }

    // Failed auth should not block when disabled
    for _ in 0..100 {
        let blocked = limiter
            .record_failed_auth(&identifier)
            .await
            .expect("record_failed_auth failed");
        assert!(!blocked, "Should never block when disabled");
    }

    let blocked = limiter
        .is_blocked(&identifier)
        .await
        .expect("is_blocked failed");
    assert!(!blocked, "Should never report blocked when disabled");

    println!("Disabled limiter test passed: all requests allowed when disabled");
}

/// Test concurrent requests are handled atomically.
///
/// This verifies that the Lua script correctly handles concurrent requests
/// without race conditions. Multiple requests should each get an accurate
/// count and the total should not exceed the limit.
#[tokio::test]
#[ignore] // Requires Redis
async fn test_concurrent_requests_are_atomic() {
    let redis = create_test_redis().await;

    let config = RateLimitConfig {
        enabled: true,
        redis_key_prefix: format!("test:rl:{}", uuid::Uuid::new_v4()),
        fail_open: false,
        trusted_proxies: vc_server::ratelimit::TrustedProxies::default(),
        allowlist: HashSet::new(),
        limits: RateLimits {
            auth_login: LimitConfig {
                requests: 10,
                window_secs: 60,
            },
            failed_auth: FailedAuthConfig {
                max_failures: 3,
                block_duration_secs: 60,
                window_secs: 300,
            },
            failed_auth_as_limit: LimitConfig {
                requests: 3,
                window_secs: 300,
            },
            ..RateLimits::default()
        },
    };

    let mut limiter = RateLimiter::new(redis, config);
    limiter.init().await.expect("Failed to initialize limiter");

    let identifier = format!("test-ip-concurrent-{}", uuid::Uuid::new_v4());

    // Spawn 20 concurrent requests (limit is 10)
    let mut handles = Vec::new();
    for _ in 0..20 {
        let limiter = limiter.clone();
        let id = identifier.clone();
        handles.push(tokio::spawn(async move {
            limiter
                .check(RateLimitCategory::AuthLogin, &id)
                .await
                .expect("Rate limit check failed")
        }));
    }

    // Collect all results
    let mut allowed_count = 0;
    let mut blocked_count = 0;
    for handle in handles {
        let result = handle.await.expect("Task panicked");
        if result.allowed {
            allowed_count += 1;
        } else {
            blocked_count += 1;
        }
    }

    // Exactly 10 should be allowed (the limit)
    assert_eq!(
        allowed_count, 10,
        "Exactly 10 requests should be allowed (got {allowed_count})"
    );
    assert_eq!(
        blocked_count, 10,
        "Exactly 10 requests should be blocked (got {blocked_count})"
    );

    println!("Concurrent requests test passed: {allowed_count} allowed, {blocked_count} blocked");
}

/// Test concurrent failed auth attempts are handled atomically.
///
/// This verifies that the failed auth Lua script correctly handles concurrent
/// requests without race conditions. The IP should be blocked after exactly
/// `max_failures` attempts.
#[tokio::test]
#[ignore] // Requires Redis
async fn test_concurrent_failed_auth_is_atomic() {
    let redis = create_test_redis().await;

    let config = RateLimitConfig {
        enabled: true,
        redis_key_prefix: format!("test:rl:{}", uuid::Uuid::new_v4()),
        fail_open: false,
        trusted_proxies: vc_server::ratelimit::TrustedProxies::default(),
        allowlist: HashSet::new(),
        limits: RateLimits {
            failed_auth: FailedAuthConfig {
                max_failures: 5,
                block_duration_secs: 60,
                window_secs: 300,
            },
            failed_auth_as_limit: LimitConfig {
                requests: 5,
                window_secs: 300,
            },
            ..RateLimits::default()
        },
    };

    let mut limiter = RateLimiter::new(redis, config);
    limiter.init().await.expect("Failed to initialize limiter");

    let ip = format!("test-ip-concurrent-fail-{}", uuid::Uuid::new_v4());

    // Spawn 10 concurrent failed auth attempts (threshold is 5)
    let mut handles = Vec::new();
    for _ in 0..10 {
        let limiter = limiter.clone();
        let ip = ip.clone();
        handles.push(tokio::spawn(async move {
            limiter
                .record_failed_auth(&ip)
                .await
                .expect("record_failed_auth failed")
        }));
    }

    // Collect all results
    let mut blocked_count = 0;
    for handle in handles {
        let is_blocked = handle.await.expect("Task panicked");
        if is_blocked {
            blocked_count += 1;
        }
    }

    // At least 6 should report blocked (those that pushed count to >= 5)
    assert!(
        blocked_count >= 6,
        "At least 6 requests should report blocked (got {blocked_count})"
    );

    // IP should definitely be blocked now
    let blocked = limiter.is_blocked(&ip).await.expect("is_blocked failed");
    assert!(blocked, "IP should be blocked after concurrent failures");

    println!("Concurrent failed auth test passed: {blocked_count} reported blocked, IP is blocked");
}

/// Test that rate limit response includes correct `reset_at` timestamp.
#[tokio::test]
#[ignore] // Requires Redis
async fn test_reset_at_is_in_future() {
    let redis = create_test_redis().await;
    let limiter = create_test_limiter(redis).await;

    let identifier = format!("test-ip-reset-{}", uuid::Uuid::new_v4());

    let result = limiter
        .check(RateLimitCategory::AuthLogin, &identifier)
        .await
        .expect("Rate limit check failed");

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    assert!(
        result.reset_at > now,
        "reset_at ({}) should be in the future (now: {})",
        result.reset_at,
        now
    );
    assert!(
        result.reset_at <= now + 60,
        "reset_at should be within window_secs from now"
    );

    println!(
        "Reset timestamp test passed: reset_at = {}, now = {}",
        result.reset_at, now
    );
}

/// Test that all rate limit categories work correctly.
#[tokio::test]
#[ignore] // Requires Redis
async fn test_all_categories_work() {
    let redis = create_test_redis().await;
    let limiter = create_test_limiter(redis).await;

    let identifier = format!("test-ip-allcat-{}", uuid::Uuid::new_v4());

    let categories = [
        RateLimitCategory::AuthLogin,
        RateLimitCategory::AuthRegister,
        RateLimitCategory::AuthPasswordReset,
        RateLimitCategory::AuthOther,
        RateLimitCategory::Write,
        RateLimitCategory::Social,
        RateLimitCategory::Read,
        RateLimitCategory::WsConnect,
        RateLimitCategory::WsMessage,
    ];

    for category in categories {
        let result = limiter
            .check(category, &identifier)
            .await
            .expect("Rate limit check failed");

        assert!(
            result.allowed,
            "First request for {category:?} should be allowed"
        );
        assert!(
            result.limit > 0,
            "Limit for {category:?} should be positive"
        );
    }

    println!("All categories test passed");
}

// --- Start: an enabled limiter must load --------------------------------

fn start_config(enabled: bool) -> RateLimitConfig {
    RateLimitConfig {
        enabled,
        redis_key_prefix: format!("test:rl:{}", uuid::Uuid::new_v4()),
        ..RateLimitConfig::default()
    }
}

/// A Valkey client for an address nobody listens on (connecting is started,
/// not awaited, as the server would find a Valkey that went away).
fn unreachable_redis() -> fred::clients::Client {
    use fred::interfaces::ClientLike;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let client = db::build_redis_client(
        &format!("redis://127.0.0.1:{port}"),
        std::time::Duration::from_millis(500),
    )
    .unwrap();
    let _ = client.connect();
    client
}

#[tokio::test]
async fn start_refuses_an_enabled_limiter_without_valkey() {
    let err = vc_server::ratelimit::start_rate_limiter(
        unreachable_redis(),
        start_config(true),
        &vc_server::ratelimit::EnabledValue::On,
    )
    .await
    .err()
    .expect("an enabled limiter that cannot load is an error");
    let text = err.to_string();
    assert!(text.contains("could not load its scripts"), "{text}");
    assert!(text.contains("RATE_LIMIT_ENABLED=false"), "{text}");
}

#[tokio::test]
async fn start_without_rate_limiting_needs_no_valkey() {
    let got = vc_server::ratelimit::start_rate_limiter(
        unreachable_redis(),
        start_config(false),
        &vc_server::ratelimit::EnabledValue::Off,
    )
    .await
    .unwrap();
    assert!(got.is_none());
}

#[tokio::test]
async fn start_loads_the_limiter_from_valkey() {
    let redis = create_test_redis().await;
    let limiter = vc_server::ratelimit::start_rate_limiter(
        redis,
        start_config(true),
        &vc_server::ratelimit::EnabledValue::Unset,
    )
    .await
    .unwrap()
    .expect("enabled and loaded");
    let result = limiter
        .check(RateLimitCategory::AuthLogin, "203.0.113.9")
        .await
        .unwrap();
    assert!(result.allowed);
}

#[tokio::test]
async fn start_refuses_when_valkey_refuses_script_loading() {
    use fred::interfaces::{ClientLike, KeysInterface};
    let admin = create_test_redis().await;
    let user = format!("noscript-{}", uuid::Uuid::new_v4().simple());
    let password = uuid::Uuid::new_v4().simple().to_string();
    let _: fred::types::Value = admin
        .custom(
            fred::types::CustomCommand::new_static("ACL", None::<u16>, false),
            vec![
                "SETUSER".to_string(),
                user.clone(),
                "on".into(),
                format!(">{password}"),
                "~*".into(),
                "+@all".into(),
                "-script".into(),
                "-eval".into(),
                "-evalsha".into(),
            ],
        )
        .await
        .unwrap();
    let base = Config::default_for_test().redis_url;
    let host = base.trim_start_matches("redis://");
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let restricted = db::create_redis_client(&format!("redis://{user}:{password}@{host}"))
        .await
        .unwrap();
    // The restricted user works otherwise.
    let _: Option<String> = restricted.get("test:rl:probe").await.unwrap();
    let result = vc_server::ratelimit::start_rate_limiter(
        restricted.clone(),
        start_config(true),
        &vc_server::ratelimit::EnabledValue::On,
    )
    .await;
    let _ = restricted.quit().await;
    let _: fred::types::Value = admin
        .custom(
            fred::types::CustomCommand::new_static("ACL", None::<u16>, false),
            vec!["DELUSER".to_string(), user],
        )
        .await
        .unwrap();
    let err = result
        .err()
        .expect("a Valkey that refuses script loading stops the start");
    assert!(
        err.to_string().contains("could not load its scripts"),
        "{err}"
    );
}

#[tokio::test]
async fn start_warns_about_an_unexpected_value_and_names_it_in_the_error() {
    let value = vc_server::ratelimit::EnabledValue::read(Some("0"));
    let mut result = None;
    let logs = super::oidc_flow_http::capture(async {
        result = Some(
            vc_server::ratelimit::start_rate_limiter(
                unreachable_redis(),
                start_config(true),
                &value,
            )
            .await,
        );
    })
    .await;
    let warnings: Vec<&str> = logs
        .lines()
        .filter(|l| l.contains("WARN") && l.contains("RATE_LIMIT_ENABLED is \"0\""))
        .collect();
    assert_eq!(warnings.len(), 1, "one warning naming the value: {logs}");
    assert!(warnings[0].contains("true, false"), "{logs}");
    let err = result
        .unwrap()
        .err()
        .expect("an enabled limiter that cannot load is an error");
    let text = err.to_string();
    assert!(
        text.contains("RATE_LIMIT_ENABLED is \"0\", which is read as enabled"),
        "{text}"
    );
}
