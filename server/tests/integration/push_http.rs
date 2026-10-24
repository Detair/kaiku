//! Integration tests for push-subscription CRUD (user-scoped).
//! Run: `cargo test --test integration push_http`

use axum::body::Body;
use axum::http::{Method, StatusCode};
use serde_json::json;
use sqlx::PgPool;

use crate::helpers::{body_to_json, create_test_user, generate_access_token, TestApp};

fn req(
    method: Method,
    uri: &str,
    token: &str,
    body: Option<serde_json::Value>,
) -> axum::http::Request<Body> {
    let b = TestApp::request(method, uri).header("Authorization", format!("Bearer {token}"));
    match body {
        Some(j) => b
            .header("Content-Type", "application/json")
            .body(Body::from(j.to_string()))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    }
}

#[sqlx::test]
async fn register_list_and_delete_own_subscription(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let token = generate_access_token(&app.config, user);

    // Register a device.
    let create = app
        .oneshot(req(
            Method::POST,
            "/api/me/push-subscriptions",
            &token,
            Some(json!({
                "provider": "unifiedpush",
                "endpoint": "https://ntfy.example.com/up?id=abc",
                "device_label": "Pixel"
            })),
        ))
        .await;
    assert_eq!(create.status(), StatusCode::CREATED);
    let body = body_to_json(create).await;
    let sub_id = body["id"].as_str().unwrap().to_string();
    // The response must not leak the auth/public keys.
    assert!(body.get("auth_key").is_none() || body["auth_key"].is_null());

    // List shows it.
    let list = app
        .oneshot(req(Method::GET, "/api/me/push-subscriptions", &token, None))
        .await;
    assert_eq!(body_to_json(list).await.as_array().unwrap().len(), 1);

    // Delete own.
    let del = app
        .oneshot(req(
            Method::DELETE,
            &format!("/api/me/push-subscriptions/{sub_id}"),
            &token,
            None,
        ))
        .await;
    assert_eq!(del.status(), StatusCode::OK);
}

#[sqlx::test]
async fn upsert_on_duplicate_endpoint(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let token = generate_access_token(&app.config, user);
    let sub = json!({ "provider": "unifiedpush", "endpoint": "https://ntfy.example.com/up?id=x" });

    app.oneshot(req(
        Method::POST,
        "/api/me/push-subscriptions",
        &token,
        Some(sub.clone()),
    ))
    .await;
    app.oneshot(req(
        Method::POST,
        "/api/me/push-subscriptions",
        &token,
        Some(sub),
    ))
    .await;

    // Same (user, endpoint) → one row (upsert), not two.
    let list = app
        .oneshot(req(Method::GET, "/api/me/push-subscriptions", &token, None))
        .await;
    assert_eq!(body_to_json(list).await.as_array().unwrap().len(), 1);
}

#[sqlx::test]
async fn cannot_delete_another_users_subscription(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let owner_token = generate_access_token(&app.config, owner);
    let create = app
        .oneshot(req(
            Method::POST,
            "/api/me/push-subscriptions",
            &owner_token,
            Some(json!({ "provider": "unifiedpush", "endpoint": "https://ntfy.example.com/up?id=own" })),
        ))
        .await;
    let sub_id = body_to_json(create).await["id"]
        .as_str()
        .unwrap()
        .to_string();

    // A different user cannot delete it.
    let (attacker, _) = create_test_user(&pool).await;
    let attacker_token = generate_access_token(&app.config, attacker);
    let del = app
        .oneshot(req(
            Method::DELETE,
            &format!("/api/me/push-subscriptions/{sub_id}"),
            &attacker_token,
            None,
        ))
        .await;
    assert_eq!(del.status(), StatusCode::NOT_FOUND);

    // And their own list is empty (subscriptions are per-user).
    let list = app
        .oneshot(req(
            Method::GET,
            "/api/me/push-subscriptions",
            &attacker_token,
            None,
        ))
        .await;
    assert_eq!(body_to_json(list).await.as_array().unwrap().len(), 0);
}

#[sqlx::test]
async fn test_create_push_subscription_rejects_http_and_private(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let token = generate_access_token(&app.config, user);

    for (endpoint, expected) in [
        ("http://push.example/x", StatusCode::BAD_REQUEST),
        ("https://127.0.0.1/x", StatusCode::BAD_REQUEST),
        ("https://push.example/x", StatusCode::CREATED),
    ] {
        let resp = app
            .oneshot(req(
                Method::POST,
                "/api/me/push-subscriptions",
                &token,
                Some(json!({ "provider": "unifiedpush", "endpoint": endpoint })),
            ))
            .await;
        assert_eq!(resp.status(), expected, "endpoint {endpoint}");
    }
}

#[sqlx::test]
async fn test_create_push_subscription_rejects_unknown_provider(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let token = generate_access_token(&app.config, user);

    for provider in ["fcm", "apns", "UnifiedPush", "other"] {
        let resp = app
            .oneshot(req(
                Method::POST,
                "/api/me/push-subscriptions",
                &token,
                Some(json!({ "provider": provider, "endpoint": "https://push.example/x" })),
            ))
            .await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "provider {provider}"
        );
    }

    // Nothing was stored.
    let list = app
        .oneshot(req(Method::GET, "/api/me/push-subscriptions", &token, None))
        .await;
    assert!(body_to_json(list).await.as_array().unwrap().is_empty());
}

// ============================================================================
// FCM delivery (local FCM mock on 127.0.0.1; no network)
// ============================================================================

use uuid::Uuid;
use vc_server::config::Config;
use vc_server::push::{deliver, WakeSignal};

use crate::helpers::{spawn_fcm_mock, test_service_account, FcmMockResponse};

/// FCM rows are inserted with SQL: registration of `fcm` lands in Task 2.
async fn insert_fcm_row(pool: &PgPool, user: Uuid, token: &str) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO push_subscriptions (user_id, provider, endpoint)
         VALUES ($1, 'fcm', $2) RETURNING id",
    )
    .bind(user)
    .bind(token)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn fcm_row(pool: &PgPool, id: Uuid) -> Option<Option<chrono::DateTime<chrono::Utc>>> {
    sqlx::query_scalar("SELECT last_used_at FROM push_subscriptions WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .unwrap()
}

async fn fcm_app(pool: PgPool, responder: FcmMockResponse) -> (TestApp, crate::helpers::FcmMock) {
    let mock = spawn_fcm_mock(responder).await;
    let mut config = Config::default_for_test();
    config.fcm_service_account = Some(test_service_account(mock.token_uri()));
    config.fcm_api_base = mock.base_url.clone();
    (TestApp::with_pool_and_config(pool, config).await, mock)
}

const FCM_TOKEN: &str = "fcm-registration-token-abc:DEF-ghi_0123456789";

#[sqlx::test]
async fn test_fcm_delivery_sends_data_message(pool: PgPool) {
    let (app, mock) = fcm_app(pool.clone(), FcmMockResponse::Ok).await;
    assert!(app.state.push.fcm().is_some());
    let (user, _) = create_test_user(&pool).await;
    let row = insert_fcm_row(&pool, user, FCM_TOKEN).await;
    let ch = Uuid::now_v7();

    deliver(&app.state, user, &WakeSignal::dm(ch, 1))
        .await
        .unwrap();

    let received = mock.received.lock().unwrap().clone();
    assert_eq!(received.len(), 1);
    let msg = &received[0]["message"];
    assert_eq!(msg["token"], FCM_TOKEN);
    assert!(msg.get("notification").is_none());
    assert_eq!(
        msg["data"],
        json!({"t": "dm", "channel_id": ch.to_string(), "count": "1"})
    );
    assert_eq!(msg["android"]["priority"], "HIGH");
    assert!(
        fcm_row(&pool, row).await.unwrap().is_some(),
        "last_used_at set"
    );
}

#[sqlx::test]
async fn test_unregistered_fcm_token_is_pruned(pool: PgPool) {
    let body: &'static str = r#"{"error":{"code":404,"status":"NOT_FOUND","details":[{"@type":"type.googleapis.com/google.firebase.fcm.v1.FcmError","errorCode":"UNREGISTERED"}]}}"#;
    let (app, mock) = fcm_app(pool.clone(), FcmMockResponse::Status(404, body)).await;
    let (user, _) = create_test_user(&pool).await;
    let row = insert_fcm_row(&pool, user, FCM_TOKEN).await;

    deliver(&app.state, user, &WakeSignal::dm(Uuid::now_v7(), 1))
        .await
        .unwrap();

    assert_eq!(mock.received.lock().unwrap().len(), 1);
    assert!(fcm_row(&pool, row).await.is_none(), "row must be deleted");
}

#[sqlx::test]
async fn test_fcm_400_keeps_subscription(pool: PgPool) {
    let body: &'static str = r#"{"error":{"code":400,"status":"INVALID_ARGUMENT","details":[{"@type":"type.googleapis.com/google.firebase.fcm.v1.FcmError","errorCode":"INVALID_ARGUMENT"}]}}"#;
    let (app, mock) = fcm_app(pool.clone(), FcmMockResponse::Status(400, body)).await;
    let (user, _) = create_test_user(&pool).await;
    let row = insert_fcm_row(&pool, user, FCM_TOKEN).await;

    deliver(&app.state, user, &WakeSignal::dm(Uuid::now_v7(), 1))
        .await
        .unwrap();

    assert_eq!(mock.received.lock().unwrap().len(), 1);
    let kept = fcm_row(&pool, row).await.expect("row must be kept");
    assert!(kept.is_none(), "a failed send must not touch last_used_at");
}

#[sqlx::test]
async fn test_fcm_row_without_fcm_config_is_kept(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    assert!(app.state.push.fcm().is_none());
    let (user, _) = create_test_user(&pool).await;
    let row = insert_fcm_row(&pool, user, FCM_TOKEN).await;

    deliver(&app.state, user, &WakeSignal::dm(Uuid::now_v7(), 1))
        .await
        .unwrap();

    assert!(fcm_row(&pool, row).await.is_some(), "row must be kept");
}
