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
