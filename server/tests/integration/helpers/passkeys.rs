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

/// A software passkey (`webauthn-authenticator-rs`'s soft passkey) that
/// writes `clientDataJSON` itself, so any origin string can be tested
/// exactly as a browser or Android would send it.
pub struct SoftAuthenticator {
    soft: webauthn_authenticator_rs::softpasskey::SoftPasskey,
}

impl Default for SoftAuthenticator {
    fn default() -> Self {
        Self::new()
    }
}

fn b64u(bytes: &[u8]) -> String {
    vc_server::approvals::secrets::b64u_encode(bytes)
}

fn client_data(kind: &str, options: &serde_json::Value, origin: &str) -> String {
    serde_json::json!({
        "type": kind,
        "challenge": options["publicKey"]["challenge"],
        "origin": origin,
        "crossOrigin": false,
    })
    .to_string()
}

impl SoftAuthenticator {
    /// UV-capable (the soft token claims UV when the options require it).
    pub fn new() -> Self {
        Self {
            soft: webauthn_authenticator_rs::softpasskey::SoftPasskey::new(true),
        }
    }

    /// Answer the server's `{"publicKey": …}` creation options from
    /// `origin`; returns the `RegisterPublicKeyCredential` JSON. The soft
    /// token cannot store resident keys, so its own copy of the options
    /// drops that requirement.
    pub fn register(&mut self, origin: &str, options: &serde_json::Value) -> serde_json::Value {
        use webauthn_authenticator_rs::AuthenticatorBackendHashedClientData;
        let mut own = options.clone();
        if let Some(sel) = own["publicKey"]["authenticatorSelection"].as_object_mut() {
            sel.remove("residentKey");
            sel.insert("requireResidentKey".into(), false.into());
        }
        let ccr: webauthn_rs::prelude::CreationChallengeResponse =
            serde_json::from_value(own).expect("creation options");
        let data = client_data("webauthn.create", options, origin);
        let hash = <sha2::Sha256 as sha2::Digest>::digest(data.as_bytes()).to_vec();
        let cred = self
            .soft
            .perform_register(hash, ccr.public_key, 60_000)
            .expect("soft register");
        let mut v = serde_json::to_value(cred).expect("credential JSON");
        v["response"]["clientDataJSON"] = b64u(data.as_bytes()).into();
        v
    }

    /// Answer `{"publicKey": …}` request options from `origin` with
    /// `credential_id`. `allowCredentials` is filled in client-side and
    /// `userHandle` set (not covered by the signature), which reproduces a
    /// discoverable response. `user_verified: false` gives UV=0.
    pub fn assert(
        &mut self,
        origin: &str,
        options: &serde_json::Value,
        credential_id: &[u8],
        user_handle: Option<uuid::Uuid>,
        user_verified: bool,
    ) -> serde_json::Value {
        use webauthn_authenticator_rs::AuthenticatorBackendHashedClientData;
        let mut own = options.clone();
        own["publicKey"]["allowCredentials"] =
            serde_json::json!([{ "type": "public-key", "id": b64u(credential_id) }]);
        if !user_verified {
            own["publicKey"]["userVerification"] = "discouraged".into();
        }
        let rcr: webauthn_rs::prelude::RequestChallengeResponse =
            serde_json::from_value(own).expect("request options");
        let data = client_data("webauthn.get", options, origin);
        let hash = <sha2::Sha256 as sha2::Digest>::digest(data.as_bytes()).to_vec();
        let cred = self
            .soft
            .perform_auth(hash, rcr.public_key, 60_000)
            .expect("soft assert");
        let mut v = serde_json::to_value(cred).expect("credential JSON");
        v["response"]["clientDataJSON"] = b64u(data.as_bytes()).into();
        v["response"]["userHandle"] = match user_handle {
            Some(h) => b64u(h.as_bytes()).into(),
            None => serde_json::Value::Null,
        };
        v
    }
}

/// Register a passkey for `user_id` through the test RP and store it in
/// `state` (a fixture only: the product path to a first passkey is a key
/// device's full session, DC1). `pending` rows get `ready_at` = now + 24 h.
pub async fn seed_passkey(
    app: &TestApp,
    user_id: uuid::Uuid,
    state: vc_server::auth::passkeys::queries::PasskeyState,
) -> (
    SoftAuthenticator,
    vc_server::auth::passkeys::queries::PasskeyRow,
) {
    use vc_server::auth::passkeys::queries::{self, NewPasskey, PasskeyState};
    let rp = test_passkey_rp();
    let mut auth = SoftAuthenticator::new();
    let handle = queries::user_handle(&app.pool, user_id)
        .await
        .expect("user handle");
    let (ccr, st) = rp
        .start_registration(handle, "user", "User", vec![])
        .expect("start registration");
    let cred = auth.register(
        super::login::TEST_ORIGIN,
        &serde_json::to_value(&ccr).expect("options"),
    );
    let reg = serde_json::from_value(cred).expect("credential");
    let verified = rp.finish_registration(&reg, &st).expect("finish");
    let mut conn = app.pool.acquire().await.expect("connection");
    let (insert_state, ready_at) = match state {
        PasskeyState::Pending => (
            PasskeyState::Pending,
            Some(chrono::Utc::now() + chrono::Duration::hours(24)),
        ),
        PasskeyState::Active | PasskeyState::Revoked => (PasskeyState::Active, None),
    };
    let mut row = queries::insert(
        &mut conn,
        &NewPasskey {
            user_id,
            verified: &verified,
            label: "Test passkey",
            state: insert_state,
            ready_at,
        },
    )
    .await
    .expect("insert passkey");
    if state == PasskeyState::Revoked {
        sqlx::query(
            "UPDATE passkeys SET state = 'revoked', revoked_at = NOW(), revoke_reason = 'test'
              WHERE id = $1",
        )
        .bind(row.id)
        .execute(&mut *conn)
        .await
        .expect("revoke passkey");
        row.state = PasskeyState::Revoked;
    }
    (auth, row)
}

/// A live full session created by passkey `passkey_id` (as a passkey
/// sign-in would make it) → `(sid, access token)`.
pub async fn passkey_session(
    pool: &PgPool,
    config: &vc_server::config::Config,
    user_id: uuid::Uuid,
    passkey_id: uuid::Uuid,
) -> (uuid::Uuid, String) {
    let sid = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO sessions (id, user_id, token_hash, expires_at, assurance, auth_methods,
                               client_class, authenticator_type, authenticator_id,
                               absolute_expires_at)
         VALUES ($1, $2, $3, NOW() + INTERVAL '7 days', 'full', '{passkey}', 'web',
                 'passkey', $4, NOW() + INTERVAL '30 days')",
    )
    .bind(sid)
    .bind(user_id)
    .bind(format!("passkey-session-{sid}"))
    .bind(passkey_id)
    .execute(pool)
    .await
    .expect("insert passkey session");
    let pair = vc_server::auth::jwt::generate_token_pair_for_session(
        user_id,
        sid,
        &config.jwt_private_key,
        config.jwt_access_expiry,
        config.jwt_refresh_expiry,
    )
    .expect("tokens");
    (sid, pair.access_token)
}

/// The app's router with a real rate limiter (fresh key prefix, default
/// limits), for the limit tests. The limiter is returned for `is_blocked`.
pub async fn limited_router(app: &TestApp) -> (axum::Router, vc_server::ratelimit::RateLimiter) {
    limited_router_with(app, vc_server::ratelimit::RateLimits::default()).await
}

pub async fn limited_router_with(
    app: &TestApp,
    limits: vc_server::ratelimit::RateLimits,
) -> (axum::Router, vc_server::ratelimit::RateLimiter) {
    use vc_server::ratelimit::{RateLimitConfig, RateLimiter};
    let mut limiter = RateLimiter::new(
        app.state.redis.clone(),
        RateLimitConfig {
            enabled: true,
            redis_key_prefix: format!("test:rl:{}", uuid::Uuid::new_v4()),
            fail_open: false,
            trusted_proxies: vc_server::ratelimit::TrustedProxies::default(),
            allowlist: std::collections::HashSet::new(),
            limits,
        },
    );
    limiter.init().await.expect("limiter");
    let mut state = super::login::state_with_redis(
        app.pool.clone(),
        (*app.config).clone(),
        app.state.redis.clone(),
    );
    state.rate_limiter = Some(limiter.clone());
    (vc_server::api::create_router(state), limiter)
}
