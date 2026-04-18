//! Webhook Integration Tests

use axum::body::Body;
use axum::http::{Method, StatusCode};
use sqlx::PgPool;
use vc_server::config::Config;

use super::helpers::*;

async fn webhook_test_app(pool: PgPool) -> TestApp {
    let mut config = Config::default_for_test();
    config.mfa_encryption_key =
        Some("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f".to_string());
    TestApp::with_pool_and_config(pool, config).await
}

// ============================================================================
// CRUD Tests
// ============================================================================

#[sqlx::test]
async fn create_webhook_returns_signing_secret(pool: PgPool) {
    let app = webhook_test_app(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let (app_id, _, _) = create_bot_application(&app.pool, user_id).await;
    let token = generate_access_token(&app.config, user_id);

    let body = serde_json::json!({
        "url": "https://example.com/webhook",
        "subscribed_events": ["message.created"],
    });

    let req = TestApp::request(
        Method::POST,
        &format!("/api/applications/{app_id}/webhooks"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .header("Content-Type", "application/json")
    .body(Body::from(serde_json::to_string(&body).unwrap()))
    .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let json = body_to_json(resp).await;
    assert!(json["signing_secret"].is_string());
    assert_eq!(json["signing_secret"].as_str().unwrap().len(), 64);
    assert_eq!(json["url"], "https://example.com/webhook");
    assert_eq!(json["active"], true);
}

#[sqlx::test]
async fn list_webhooks_does_not_return_secret(pool: PgPool) {
    let app = webhook_test_app(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let (app_id, _, _) = create_bot_application(&app.pool, user_id).await;
    let token = generate_access_token(&app.config, user_id);

    // Create a webhook first
    create_test_webhook(
        &app.pool,
        app_id,
        "https://example.com/wh1",
        &["message.created"],
    )
    .await;

    let req = TestApp::request(Method::GET, &format!("/api/applications/{app_id}/webhooks"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_to_json(resp).await;
    let list = json.as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert!(list[0].get("signing_secret").is_none());
}

#[sqlx::test]
async fn get_webhook_returns_details(pool: PgPool) {
    let app = webhook_test_app(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let (app_id, _, _) = create_bot_application(&app.pool, user_id).await;
    let token = generate_access_token(&app.config, user_id);

    let wh_id = create_test_webhook(
        &app.pool,
        app_id,
        "https://example.com/wh",
        &["message.created", "member.joined"],
    )
    .await;

    let req = TestApp::request(
        Method::GET,
        &format!("/api/applications/{app_id}/webhooks/{wh_id}"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .body(Body::empty())
    .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_to_json(resp).await;
    assert_eq!(json["url"], "https://example.com/wh");
    assert_eq!(json["active"], true);
}

#[sqlx::test]
async fn update_webhook_url_and_events(pool: PgPool) {
    let app = webhook_test_app(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let (app_id, _, _) = create_bot_application(&app.pool, user_id).await;
    let token = generate_access_token(&app.config, user_id);

    let wh_id = create_test_webhook(
        &app.pool,
        app_id,
        "https://example.com/old",
        &["message.created"],
    )
    .await;

    let body = serde_json::json!({
        "url": "https://example.com/new-url",
        "subscribed_events": ["member.joined", "member.left"],
        "active": false,
    });

    let req = TestApp::request(
        Method::PATCH,
        &format!("/api/applications/{app_id}/webhooks/{wh_id}"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .header("Content-Type", "application/json")
    .body(Body::from(serde_json::to_string(&body).unwrap()))
    .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_to_json(resp).await;
    assert_eq!(json["url"], "https://example.com/new-url");
    assert_eq!(json["active"], false);
}

#[sqlx::test]
async fn delete_webhook_succeeds(pool: PgPool) {
    let app = webhook_test_app(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let (app_id, _, _) = create_bot_application(&app.pool, user_id).await;
    let token = generate_access_token(&app.config, user_id);

    let wh_id = create_test_webhook(
        &app.pool,
        app_id,
        "https://example.com/del",
        &["message.created"],
    )
    .await;

    let req = TestApp::request(
        Method::DELETE,
        &format!("/api/applications/{app_id}/webhooks/{wh_id}"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .body(Body::empty())
    .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

// ============================================================================
// Ownership Tests
// ============================================================================

#[sqlx::test]
async fn non_owner_cannot_manage_webhooks(pool: PgPool) {
    let app = webhook_test_app(pool.clone()).await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let (other_id, _) = create_test_user(&app.pool).await;
    let (app_id, _, _) = create_bot_application(&app.pool, owner_id).await;
    let other_token = generate_access_token(&app.config, other_id);

    let body = serde_json::json!({
        "url": "https://example.com/webhook",
        "subscribed_events": ["message.created"],
    });

    let req = TestApp::request(
        Method::POST,
        &format!("/api/applications/{app_id}/webhooks"),
    )
    .header("Authorization", format!("Bearer {other_token}"))
    .header("Content-Type", "application/json")
    .body(Body::from(serde_json::to_string(&body).unwrap()))
    .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// ============================================================================
// Validation Tests
// ============================================================================

#[sqlx::test]
async fn invalid_url_rejected(pool: PgPool) {
    let app = webhook_test_app(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let (app_id, _, _) = create_bot_application(&app.pool, user_id).await;
    let token = generate_access_token(&app.config, user_id);

    let body = serde_json::json!({
        "url": "not-a-url",
        "subscribed_events": ["message.created"],
    });

    let req = TestApp::request(
        Method::POST,
        &format!("/api/applications/{app_id}/webhooks"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .header("Content-Type", "application/json")
    .body(Body::from(serde_json::to_string(&body).unwrap()))
    .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn empty_events_rejected(pool: PgPool) {
    let app = webhook_test_app(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let (app_id, _, _) = create_bot_application(&app.pool, user_id).await;
    let token = generate_access_token(&app.config, user_id);

    let body = serde_json::json!({
        "url": "https://example.com/webhook",
        "subscribed_events": [],
    });

    let req = TestApp::request(
        Method::POST,
        &format!("/api/applications/{app_id}/webhooks"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .header("Content-Type", "application/json")
    .body(Body::from(serde_json::to_string(&body).unwrap()))
    .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn max_5_webhooks_enforced(pool: PgPool) {
    let app = webhook_test_app(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let (app_id, _, _) = create_bot_application(&app.pool, user_id).await;
    let token = generate_access_token(&app.config, user_id);

    // Create 5 webhooks
    for i in 0..5 {
        create_test_webhook(
            &app.pool,
            app_id,
            &format!("https://example.com/wh{i}"),
            &["message.created"],
        )
        .await;
    }

    // 6th should fail
    let body = serde_json::json!({
        "url": "https://example.com/wh6",
        "subscribed_events": ["message.created"],
    });

    let req = TestApp::request(
        Method::POST,
        &format!("/api/applications/{app_id}/webhooks"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .header("Content-Type", "application/json")
    .body(Body::from(serde_json::to_string(&body).unwrap()))
    .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

// ============================================================================
// Delivery Log Test
// ============================================================================

#[sqlx::test]
async fn delivery_log_initially_empty(pool: PgPool) {
    let app = webhook_test_app(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let (app_id, _, _) = create_bot_application(&app.pool, user_id).await;
    let token = generate_access_token(&app.config, user_id);

    let wh_id = create_test_webhook(
        &app.pool,
        app_id,
        "https://example.com/log",
        &["message.created"],
    )
    .await;

    let req = TestApp::request(
        Method::GET,
        &format!("/api/applications/{app_id}/webhooks/{wh_id}/deliveries"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .body(Body::empty())
    .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_to_json(resp).await;
    assert_eq!(json.as_array().unwrap().len(), 0);
}
