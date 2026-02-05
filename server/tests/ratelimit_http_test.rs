//! HTTP-Level Rate Limiting Tests
//!
//! Tests rate limiting behavior through a real HTTP server, verifying
//! that the middleware correctly tracks request counts across calls
//! and returns appropriate 429 responses.
//!
//! These tests use `spawn_test_server` instead of `oneshot` because
//! rate limiting is stateful middleware that needs to persist state
//! across multiple requests.
//!
//! Run with: `cargo test --test ratelimit_http_test --ignored -- --nocapture`

mod helpers;

use helpers::spawn_test_server;
use std::collections::HashSet;
use std::sync::Arc;
use vc_server::{
    api::{create_router, AppState},
    config::Config,
    ratelimit::{LimitConfig, RateLimitConfig, RateLimiter, RateLimits},
    voice::sfu::SfuServer,
};

/// Create a test app with rate limiting enabled using a real server.
async fn create_rate_limited_app(limits: RateLimits) -> (helpers::TestServer, Config) {
    let config = Config::default_for_test();
    let pool = helpers::shared_pool().await.clone();
    let redis = helpers::shared_redis().await.clone();
    let sfu =
        SfuServer::new(Arc::new(config.clone()), None).expect("Failed to create SfuServer");

    let rl_config = RateLimitConfig {
        enabled: true,
        redis_key_prefix: format!("test:rl:{}", uuid::Uuid::new_v4()),
        fail_open: false,
        trust_proxy: false,
        allowlist: HashSet::new(),
        limits,
    };
    let mut limiter = RateLimiter::new(redis.clone(), rl_config);
    limiter.init().await.expect("Failed to initialize limiter");

    let state = AppState::new(
        pool,
        redis,
        config.clone(),
        None,
        sfu,
        Some(limiter),
        None,
        None,
    );
    let router = create_router(state);
    let server = spawn_test_server(router).await;

    (server, config)
}

/// Test that requests under the rate limit succeed and over-limit returns 429.
#[tokio::test]
#[ignore] // Requires Redis
async fn test_http_rate_limiting_returns_429() {
    let limits = RateLimits {
        read: LimitConfig {
            requests: 3,
            window_secs: 60,
        },
        ..RateLimits::default()
    };

    let (server, _config) = create_rate_limited_app(limits).await;
    let client = reqwest::Client::new();

    // First 3 requests should succeed (200 from setup/status)
    for i in 0..3 {
        let resp = client
            .get(format!("{}/api/setup/status", server.url))
            .send()
            .await
            .expect("Request failed");
        assert_eq!(
            resp.status(),
            200,
            "Request {} should succeed (under limit)",
            i + 1
        );
    }

    // 4th request should be rate limited
    let resp = client
        .get(format!("{}/api/setup/status", server.url))
        .send()
        .await
        .expect("Request failed");
    assert_eq!(
        resp.status(),
        429,
        "4th request should be rate limited (429)"
    );

    // Verify Retry-After header is present
    let retry_after = resp.headers().get("retry-after");
    assert!(
        retry_after.is_some(),
        "429 response should include Retry-After header"
    );
}

/// Test that rate limit headers are present in responses.
#[tokio::test]
#[ignore] // Requires Redis
async fn test_http_rate_limit_headers() {
    let limits = RateLimits {
        read: LimitConfig {
            requests: 10,
            window_secs: 60,
        },
        ..RateLimits::default()
    };

    let (server, _config) = create_rate_limited_app(limits).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/api/setup/status", server.url))
        .send()
        .await
        .expect("Request failed");

    assert_eq!(resp.status(), 200);

    // Check for standard rate limit headers
    let limit = resp.headers().get("x-ratelimit-limit");
    let remaining = resp.headers().get("x-ratelimit-remaining");

    assert!(limit.is_some(), "Should include X-RateLimit-Limit header");
    assert!(
        remaining.is_some(),
        "Should include X-RateLimit-Remaining header"
    );
}

/// Test that request IDs are unique across multiple requests.
#[tokio::test]
#[ignore] // Requires Redis
async fn test_request_ids_are_unique() {
    let limits = RateLimits::default();
    let (server, _config) = create_rate_limited_app(limits).await;
    let client = reqwest::Client::new();

    let mut request_ids = HashSet::new();

    for _ in 0..10 {
        let resp = client
            .get(format!("{}/api/setup/status", server.url))
            .send()
            .await
            .expect("Request failed");

        if let Some(id) = resp.headers().get("x-request-id") {
            let id_str = id.to_str().unwrap().to_string();
            assert!(
                request_ids.insert(id_str.clone()),
                "Duplicate request ID found: {id_str}"
            );
        }
    }
}
