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
