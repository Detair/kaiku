//! The v1.0 login upgrade (F8), the security overview and removed routes.
//!
//! Run with: `cargo test --test integration upgrade_migration`

use axum::body::Body;
use axum::http::Method;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use super::helpers::approvals::KeyDeviceFixture;
use super::helpers::migrations::{apply_from, apply_until};
use super::helpers::{
    body_to_json_lenient, create_probation_session_with_token, create_test_user, TestApp,
};

const UPGRADE: i64 = 20_261_121_000_600;

async fn user(pool: &PgPool, name: &str, password: bool, totp: bool) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO users (username, display_name, password_hash, mfa_secret, must_change_password)
         VALUES ($1, $1, $2, $3, true) RETURNING id",
    )
    .bind(name)
    .bind(password.then_some("x"))
    .bind(totp.then_some("enc"))
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn session(pool: &PgPool, user: Uuid) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO sessions (id, user_id, token_hash, expires_at)
         VALUES ($1, $2, $3, NOW() + INTERVAL '1 day')",
    )
    .bind(id)
    .bind(user)
    .bind(format!("hash-{id}"))
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn end_reason(pool: &PgPool, sid: Uuid) -> Option<String> {
    sqlx::query_scalar("SELECT end_reason FROM sessions WHERE id = $1")
        .bind(sid)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn bootstrap_open(pool: &PgPool, user: Uuid) -> bool {
    sqlx::query_scalar("SELECT bootstrap_open FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = false)]
async fn password_only_sessions_end_at_the_upgrade(pool: PgPool) {
    apply_until(&pool, UPGRADE).await;
    let only = user(&pool, "pw_only", true, false).await;
    let both = user(&pool, "pw_totp", true, true).await;
    let s_only = session(&pool, only).await;
    let s_both = session(&pool, both).await;
    apply_from(&pool, UPGRADE).await;
    assert_eq!(end_reason(&pool, s_only).await.as_deref(), Some("upgrade"));
    assert_eq!(end_reason(&pool, s_both).await, None);
    let forced: bool = sqlx::query_scalar("SELECT must_change_password FROM users WHERE id = $1")
        .bind(both)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!forced, "temporary passwords are gone");
}

#[sqlx::test(migrations = false)]
async fn password_totp_and_oidc_totp_accounts_get_bootstrap_open(pool: PgPool) {
    apply_until(&pool, UPGRADE).await;
    let pw = user(&pool, "pw_totp", true, true).await;
    let oidc = user(&pool, "oidc_totp", false, true).await;
    sqlx::query(
        "INSERT INTO user_identities (user_id, provider_slug, subject) VALUES ($1, 'kc', 'sub-1')",
    )
    .bind(oidc)
    .execute(&pool)
    .await
    .unwrap();
    let no_totp = user(&pool, "pw_only", true, false).await;
    apply_from(&pool, UPGRADE).await;
    assert!(bootstrap_open(&pool, pw).await);
    assert!(bootstrap_open(&pool, oidc).await);
    assert!(!bootstrap_open(&pool, no_totp).await);
}

#[sqlx::test(migrations = false)]
async fn accounts_with_a_strong_authenticator_do_not(pool: PgPool) {
    apply_until(&pool, UPGRADE).await;
    let u = user(&pool, "has_phone", true, true).await;
    sqlx::query(
        "INSERT INTO approval_devices (id, user_id, device_id, public_key, public_key_hash,
             uv_class, label, state, enrolled_via, activated_at)
         VALUES (gen_random_uuid(), $1, gen_random_uuid(), '\\x00', decode(repeat('00', 32), 'hex'), 'per_use', 'P',
                 'active', 'cli', NOW())",
    )
    .bind(u)
    .execute(&pool)
    .await
    .unwrap();
    let phone_only = user(&pool, "phone_only", false, false).await;
    sqlx::query(
        "INSERT INTO approval_devices (id, user_id, device_id, public_key, public_key_hash,
             uv_class, label, state, enrolled_via, activated_at)
         VALUES (gen_random_uuid(), $1, gen_random_uuid(), '\\x01', decode(repeat('01', 32), 'hex'), 'per_use', 'P',
                 'active', 'cli', NOW())",
    )
    .bind(phone_only)
    .execute(&pool)
    .await
    .unwrap();
    let s = session(&pool, phone_only).await;
    apply_from(&pool, UPGRADE).await;
    assert!(!bootstrap_open(&pool, u).await);
    assert_eq!(
        end_reason(&pool, s).await,
        None,
        "a strong authenticator keeps its sessions"
    );
}

#[sqlx::test]
async fn legacy_backup_codes_are_gone(pool: PgPool) {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.tables
          WHERE table_name IN ('mfa_backup_codes', 'password_reset_tokens')",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(tables.is_empty(), "{tables:?}");
}

#[sqlx::test]
async fn reset_routes_are_gone(pool: PgPool) {
    let app = TestApp::with_pool(pool).await;
    for uri in ["/auth/forgot-password", "/auth/reset-password"] {
        let req = TestApp::request(Method::POST, uri)
            .header("Content-Type", "application/json")
            .body(Body::from(
                json!({"email": "a@b.c", "token": "t", "new_password": "x"}).to_string(),
            ))
            .unwrap();
        assert_eq!(app.oneshot(req).await.status().as_u16(), 404, "{uri}");
    }
}

async fn overview(app: &TestApp, token: &str) -> Value {
    let req = TestApp::request(Method::GET, "/api/me/security")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status().as_u16(), 200);
    body_to_json_lenient(resp).await
}

fn prompts(v: &Value) -> Vec<String> {
    v["prompts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap().to_string())
        .collect()
}

#[sqlx::test]
async fn security_overview_prompts(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    // A fresh account on a probation session: nothing set up.
    let (u, _) = create_test_user(&pool).await;
    let (_, token) = create_probation_session_with_token(&pool, &app.config, u).await;
    let v = overview(&app, &token).await;
    let p = prompts(&v);
    for want in [
        "add_passkey",
        "set_up_phone",
        "name_recovery_contact",
        "renew_recovery_codes",
    ] {
        assert!(p.contains(&want.to_string()), "{want}: {v}");
    }
    for not in [
        "sign_in_with_passkey",
        "secure_my_account",
        "renew_totp",
        "open_request",
    ] {
        assert!(!p.contains(&not.to_string()), "{not}: {v}");
    }
    assert_eq!(v["session"]["assurance"], "probation");
    assert_eq!(v["key_devices"], 0);
    // A passkey, bootstrap_open, a compromised TOTP, recovery codes.
    sqlx::query(
        "INSERT INTO passkeys (id, user_id, credential_id, credential, label, state,
                               backup_eligible, backup_state)
         VALUES (gen_random_uuid(), $1, '\\x0102', '{}'::jsonb, 'Laptop', 'active', false, false)",
    )
    .bind(u)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE users SET bootstrap_open = true, mfa_secret = 'enc', totp_state = 'compromised'
          WHERE id = $1",
    )
    .bind(u)
    .execute(&pool)
    .await
    .unwrap();
    {
        let ctx = vc_server::auth::ctx::AuthCtx::from(&app.state);
        let mut tx = pool.begin().await.unwrap();
        vc_server::auth::recovery_codes::replace_tx(&mut tx, &ctx, u)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
    let v = overview(&app, &token).await;
    let p = prompts(&v);
    for want in [
        "sign_in_with_passkey",
        "secure_my_account",
        "renew_totp",
        "set_up_phone",
    ] {
        assert!(p.contains(&want.to_string()), "{want}: {v}");
    }
    for not in ["add_passkey", "renew_recovery_codes"] {
        assert!(!p.contains(&not.to_string()), "{not}: {v}");
    }
    assert_eq!(v["totp"]["state"], "compromised");
    assert_eq!(v["recovery_codes_left"], 10);
    // A full session with a key phone: no passkey sign-in prompt.
    let dev = KeyDeviceFixture::insert(&pool, &app.config, u, "active").await;
    let v = overview(&app, &dev.access_token).await;
    let p = prompts(&v);
    assert!(!p.contains(&"sign_in_with_passkey".to_string()), "{v}");
    assert!(!p.contains(&"set_up_phone".to_string()), "{v}");
    assert_eq!(v["session"]["assurance"], "full");
}

#[sqlx::test]
async fn bootstrap_reminder_every_30_days(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (u, _) = create_test_user(&pool).await;
    sqlx::query(
        "UPDATE users SET bootstrap_open = true, created_at = NOW() - INTERVAL '31 days'
          WHERE id = $1",
    )
    .bind(u)
    .execute(&pool)
    .await
    .unwrap();
    let count = || async {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM security_events
              WHERE subject_id = $1 AND event_type = 'bootstrap_reminder'",
        )
        .bind(u)
        .fetch_one(&pool)
        .await
        .unwrap()
    };
    vc_server::auth::timers::run_once(&app.state).await.unwrap();
    vc_server::auth::timers::run_once(&app.state).await.unwrap();
    assert_eq!(count().await, 1, "once, not every sweep");
    sqlx::query(
        "UPDATE users SET bootstrap_reminded_at = NOW() - INTERVAL '31 days' WHERE id = $1",
    )
    .bind(u)
    .execute(&pool)
    .await
    .unwrap();
    vc_server::auth::timers::run_once(&app.state).await.unwrap();
    assert_eq!(count().await, 2);
    // Secured: no more reminders.
    sqlx::query(
        "UPDATE users SET bootstrap_open = false, bootstrap_reminded_at = NOW() - INTERVAL '31 days'
          WHERE id = $1",
    )
    .bind(u)
    .execute(&pool)
    .await
    .unwrap();
    vc_server::auth::timers::run_once(&app.state).await.unwrap();
    assert_eq!(count().await, 2);
}
