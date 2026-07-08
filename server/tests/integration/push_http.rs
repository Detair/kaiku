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
