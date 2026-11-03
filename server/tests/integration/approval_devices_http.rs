//! Key devices over HTTP (SM4, C26): list, remove with a step-up, "Sign out
//! and remove this phone", invalidated keys.
//!
//! Run with: `cargo test --test integration approval_devices_http`

use std::time::Duration;

use axum::body::Body;
use axum::http::Method;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use super::approvals_core::{answer, post, stepup_challenge};
use super::helpers::approvals::KeyDeviceFixture;
use super::helpers::{
    body_to_json, create_probation_session_with_token, create_test_user, make_admin,
    spawn_fcm_mock, test_service_account, FcmMockResponse, TestApp,
};

async fn delete(app: &TestApp, uri: &str, token: &str) -> (u16, Value) {
    let req = TestApp::request(Method::DELETE, uri)
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    (status, body_to_json(resp).await)
}

async fn get(app: &TestApp, uri: &str, token: &str) -> (u16, Value) {
    let req = TestApp::request(Method::GET, uri)
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    (status, body_to_json(resp).await)
}

/// Step up for `key_device.remove` of `target` on `dev`'s session.
async fn step_up_remove(app: &TestApp, dev: &KeyDeviceFixture, target: Uuid) {
    let c = stepup_challenge(app, dev, "key_device.remove", &target.to_string()).await;
    assert_eq!(answer(app, dev, &c, b"").await.0, 200);
}

async fn events(pool: &PgPool, user: Uuid, event_type: &str) -> Vec<(String, Value)> {
    sqlx::query_as(
        "SELECT severity, details FROM security_events
          WHERE subject_id = $1 AND event_type = $2 ORDER BY created_at",
    )
    .bind(user)
    .bind(event_type)
    .fetch_all(pool)
    .await
    .unwrap()
}

#[sqlx::test]
async fn delete_device_needs_full_and_grant(pool: PgPool) {
    let mock = spawn_fcm_mock(FcmMockResponse::Ok).await;
    let mut config = vc_server::config::Config::default_for_test();
    config.fcm_service_account = Some(test_service_account(mock.token_uri()));
    config.fcm_api_base.clone_from(&mock.base_url);
    let app = TestApp::with_pool_and_config(pool.clone(), config).await;
    let (admin, _) = create_test_user(&pool).await;
    make_admin(&pool, admin).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let old = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    // The phone to be removed has a push subscription.
    sqlx::query(
        "INSERT INTO push_subscriptions (user_id, provider, endpoint, device_id)
         VALUES ($1, 'fcm', 'fcm-old-phone', $2)",
    )
    .bind(user)
    .bind(old.device_id)
    .execute(&pool)
    .await
    .unwrap();
    let uri = format!("/api/approval-devices/{}", old.row_id);

    let (_, probation) = create_probation_session_with_token(&pool, &app.config, user).await;
    let (status, body) = delete(&app, &uri, &probation).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (403, Some("FULL_SESSION_REQUIRED"))
    );
    let (status, body) = delete(&app, &uri, &dev.access_token).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (403, Some("STEP_UP_REQUIRED"))
    );

    step_up_remove(&app, &dev, old.row_id).await;
    let (status, body) = delete(&app, &uri, &dev.access_token).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["id"], old.row_id.to_string());
    assert_eq!(body["state"], "revoked");
    assert_eq!(body["revoke_reason"], "owner");
    assert_eq!(body["platform"], "android");

    let removed = events(&pool, user, "authenticator_removed").await;
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].0, "high");
    assert_eq!(removed[0].1["type"], "key_device");
    assert_eq!(removed[0].1["by"], "self");
    assert_eq!(events(&pool, user, "step_up_action").await.len(), 1);

    // The removed phone got its last notice, then its subscription went.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let subs: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM push_subscriptions WHERE device_id = $1")
                .bind(old.device_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        if subs == 0 {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "subscription not deleted"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let pushed = mock.received.lock().unwrap().clone();
    assert!(
        pushed
            .iter()
            .any(|p| p["message"]["token"] == "fcm-old-phone"
                && p["message"]["data"]["t"] == "security"),
        "{pushed:?}"
    );
    // The admins' copy.
    let copies: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM security_notices WHERE recipient_id = $1 AND subject_id = $2",
    )
    .bind(admin)
    .bind(user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(copies >= 1);
}

#[sqlx::test]
async fn sign_out_and_remove_this_phone(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    step_up_remove(&app, &dev, dev.row_id).await;
    let (status, body) = delete(
        &app,
        &format!("/api/approval-devices/{}", dev.row_id),
        &dev.access_token,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["is_current"], true);
    // Its own session ended with the cascade.
    assert_eq!(get(&app, "/auth/me", &dev.access_token).await.0, 401);
}

#[sqlx::test]
async fn last_strong_authenticator_cannot_be_removed(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    step_up_remove(&app, &dev, dev.row_id).await;
    let (status, body) = delete(
        &app,
        &format!("/api/approval-devices/{}", dev.row_id),
        &dev.access_token,
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (409, Some("LAST_STRONG_AUTHENTICATOR"))
    );
    let state: String = sqlx::query_scalar("SELECT state FROM approval_devices WHERE id = $1")
        .bind(dev.row_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "active");
}

#[sqlx::test]
async fn invalidated_device_is_revoked_with_notice(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let other = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    // Only the device's own session may report it.
    let (status, _) = post(
        &app,
        &format!("/api/approval-devices/{}/invalidated", dev.row_id),
        &other.access_token,
        json!({}),
    )
    .await;
    assert_eq!(status, 403);
    let (status, body) = post(
        &app,
        &format!("/api/approval-devices/{}/invalidated", dev.row_id),
        &dev.access_token,
        json!({}),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["state"], "revoked");
    assert_eq!(body["revoke_reason"], "key_invalidated");
    // Review round 3 (Argon coverage, part 1 m7): it is the caller's own device.
    assert_eq!(body["is_current"], true);
    let ev = events(&pool, user, "key_invalidated").await;
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].0, "high");
}

#[sqlx::test]
async fn device_list_marks_current(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let other = KeyDeviceFixture::insert(&pool, &app.config, user, "pending").await;
    let (status, body) = get(&app, "/api/approval-devices", &dev.access_token).await;
    assert_eq!(status, 200, "{body}");
    let list = body["devices"].as_array().unwrap();
    assert_eq!(list.len(), 2);
    let me = list
        .iter()
        .find(|d| d["id"] == dev.row_id.to_string())
        .unwrap();
    let them = list
        .iter()
        .find(|d| d["id"] == other.row_id.to_string())
        .unwrap();
    assert_eq!(me["is_current"], true);
    assert_eq!(them["is_current"], false);
    assert_eq!(me["device_id"], dev.device_id.to_string());
    assert_eq!(them["state"], "pending");
    assert_eq!(me["uv_class"], "per_use");
    assert_eq!(me["enrolled_via"], "cli");
    // Any session may read it, probation included.
    let (_, probation) = create_probation_session_with_token(&pool, &app.config, user).await;
    let (status, body) = get(&app, "/api/approval-devices", &probation).await;
    assert_eq!(status, 200);
    assert!(body["devices"]
        .as_array()
        .unwrap()
        .iter()
        .all(|d| d["is_current"] == false));
}
