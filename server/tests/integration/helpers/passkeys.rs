//! Passkey test helpers (LP): the test relying party and a `TestApp` with
//! passkeys on.

use std::sync::Arc;

use sqlx::PgPool;
use vc_server::auth::passkeys::config::PasskeyConfig;
use vc_server::auth::passkeys::rp::PasskeyRp;

use super::login::login_config;
use super::TestApp;

/// SHA-256("kaiku-test-cert"), the C29 test vector, in `keytool` form.
pub const TEST_CERT_HEX: &str =
    "18:6F:F0:75:BF:FB:D4:76:5E:1C:5C:D9:E1:22:C4:09:44:58:A3:58:BC:E5:34:F5:3E:49:D7:B2:C3:81:AB:C0";

/// The Android origin of [`TEST_CERT_HEX`] (C29 vector).
pub const TEST_APK_ORIGIN: &str =
    "android:apk-key-hash:GG_wdb_71HZeHFzZ4SLECURYo1i85TT1PknXssOBq8A";

/// The test RP: `chat.example.com`, the web origin of `login_config()` and
/// one Android certificate (both default app ids).
pub fn test_passkey_rp() -> PasskeyRp {
    let config = login_config();
    PasskeyRp::new(
        PasskeyConfig::new(&config.public_origin, None, Some(TEST_CERT_HEX))
            .expect("test passkey config"),
    )
    .expect("test passkey RP")
}

impl TestApp {
    /// `login_app` with the test passkey RP.
    pub async fn with_passkeys(pool: PgPool) -> Self {
        let mut config = login_config();
        config.passkeys = Some(Arc::new(test_passkey_rp()));
        Self::with_pool_and_config(pool, config).await
    }
}
