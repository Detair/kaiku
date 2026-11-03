//! Key-device fixtures for the approval tests (Tasks 8–13).

use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::approvals::secrets::b64u_encode;
use vc_server::approvals::signing::{message, Decision};
use vc_server::config::Config;

/// SPKI DER of an uncompressed P-256 public key (Android `getEncoded()`).
pub fn spki_der(key: &SigningKey) -> Vec<u8> {
    let mut der = hex::decode("3059301306072a8648ce3d020106082a8648ce3d030107034200").unwrap();
    der.extend_from_slice(key.verifying_key().to_encoded_point(false).as_bytes());
    der
}

/// A key device row with a fresh P-256 key, and for `active` a full device
/// session whose authenticator is that row.
pub struct KeyDeviceFixture {
    pub user_id: Uuid,
    /// The per-install id (also the push `device_id`).
    pub device_id: Uuid,
    /// `approval_devices.id`.
    pub row_id: Uuid,
    pub signing_key: SigningKey,
    /// The device session (`None` unless `active`).
    pub session_id: Option<Uuid>,
    /// Access token of the device session (empty unless `active`).
    pub access_token: String,
}

impl KeyDeviceFixture {
    /// Insert a device in `state` (`pending`, `active` or `revoked`).
    pub async fn insert(pool: &PgPool, config: &Config, user_id: Uuid, state: &str) -> Self {
        let signing_key = SigningKey::random(&mut rand::rngs::OsRng);
        let der = spki_der(&signing_key);
        let row_id = Uuid::new_v4();
        let device_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO approval_devices
                (id, user_id, device_id, public_key, public_key_hash, uv_class, label, state,
                 enrolled_via, activated_at, revoked_at, revoke_reason)
             VALUES ($1, $2, $3, $4, $5, 'per_use', 'Pixel 8', $6, 'cli',
                     CASE WHEN $6 IN ('active', 'revoked') THEN NOW() END,
                     CASE WHEN $6 = 'revoked' THEN NOW() END,
                     CASE WHEN $6 = 'revoked' THEN 'owner' END)",
        )
        .bind(row_id)
        .bind(user_id)
        .bind(device_id)
        .bind(&der)
        .bind(Sha256::digest(&der).to_vec())
        .bind(state)
        .execute(pool)
        .await
        .expect("insert key device");
        let (session_id, access_token) = if state == "active" {
            let (sid, token) = device_session(pool, config, user_id, row_id).await;
            (Some(sid), token)
        } else {
            (None, String::new())
        };
        Self {
            user_id,
            device_id,
            row_id,
            signing_key,
            session_id,
            access_token,
        }
    }

    /// b64u DER signature over `prefix ‖ decision ‖ lp(response)`.
    pub fn sign(&self, prefix: &[u8], decision: Decision, response: &[u8]) -> String {
        let m = message(prefix, decision, response);
        let sig: Signature = self.signing_key.sign(&m);
        b64u_encode(sig.to_der().as_bytes())
    }

    /// The public key as sent by the app (b64u SPKI DER).
    pub fn public_key_b64u(&self) -> String {
        b64u_encode(&spki_der(&self.signing_key))
    }
}

/// A full session of the key device `row_id` (as device-key sign-in makes).
pub async fn device_session(
    pool: &PgPool,
    config: &Config,
    user_id: Uuid,
    row_id: Uuid,
) -> (Uuid, String) {
    let sid = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO sessions (id, user_id, token_hash, expires_at, assurance, auth_methods,
                               client_class, authenticator_type, authenticator_id,
                               absolute_expires_at)
         VALUES ($1, $2, $3, NOW() + INTERVAL '7 days', 'full', '{device_key}', 'android',
                 'key_device', $4, NOW() + INTERVAL '90 days')",
    )
    .bind(sid)
    .bind(user_id)
    .bind(format!("device-session-{sid}"))
    .bind(row_id)
    .execute(pool)
    .await
    .expect("insert device session");
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
