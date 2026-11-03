//! Helpers for the v1.0 login tests (Tasks 1–14).

use sqlx::PgPool;
use vc_server::config::Config;

use super::TestApp;

/// The documentation origin every login test signs against.
pub const TEST_ORIGIN: &str = "https://chat.example.com";

/// The test config with the login origin set (as `Config::default_for_test`).
pub fn login_config() -> Config {
    let config = Config::default_for_test();
    assert_eq!(config.public_origin.origin, TEST_ORIGIN);
    config
}

/// A test app on [`login_config`].
pub async fn login_app(pool: PgPool) -> TestApp {
    TestApp::with_pool_and_config(pool, login_config()).await
}

/// The socket peer the login tests come from (documentation range).
pub const TEST_PEER: ([u8; 4], u16) = ([203, 0, 113, 9], 40000);

/// A request builder carrying `ConnectInfo` (the public auth routes read
/// the socket peer for per-IP limits and audit).
pub fn peer_request(method: axum::http::Method, uri: &str) -> axum::http::request::Builder {
    TestApp::request(method, uri).extension(axum::extract::ConnectInfo(std::net::SocketAddr::from(
        TEST_PEER,
    )))
}

/// A Valkey client that fails every command: connected, then closed, with
/// a short command timeout (Valkey "down" for the code under test).
pub async fn failing_valkey(config: &Config) -> fred::clients::Client {
    use fred::prelude::{ClientLike, Config as ValkeyConfig, PerformanceConfig};
    let perf = PerformanceConfig {
        default_command_timeout: std::time::Duration::from_millis(300),
        ..PerformanceConfig::default()
    };
    let redis = fred::clients::Client::new(
        ValkeyConfig::from_url(&config.redis_url).expect("Valkey URL"),
        Some(perf),
        None,
        None,
    );
    redis.connect();
    redis.wait_for_connect().await.expect("connect Valkey");
    redis.quit().await.expect("quit Valkey");
    redis
}

/// An `AppState` on `pool` with the given Valkey client (no S3, no limiter).
pub fn state_with_redis(
    pool: PgPool,
    config: Config,
    redis: fred::clients::Client,
) -> vc_server::api::AppState {
    let sfu = vc_server::voice::sfu::SfuServer::new(std::sync::Arc::new(config.clone()), None)
        .expect("SfuServer");
    vc_server::api::AppState::new(vc_server::api::AppStateConfig {
        db: pool,
        redis,
        config,
        s3: None,
        sfu,
        rate_limiter: None,
        screen_share_limiter: None,
        email: None,
        oidc_manager: None,
        http_client: reqwest::Client::new(),
    })
}
