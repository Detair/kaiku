//! Integration tests for `GET /api/admin/diagnostics` (operator
//! supportability pack, Phase 8 Goal 5).

use axum::body::Body;
use axum::http::{Method, StatusCode};
use sqlx::PgPool;

use super::helpers::{body_to_json, create_test_user, generate_access_token, make_admin, TestApp};

#[sqlx::test]
async fn admin_gets_diagnostics_snapshot(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (admin_id, _) = create_test_user(&pool).await;
    make_admin(&pool, admin_id).await;
    let token = generate_access_token(&app.config, admin_id);

    let resp = app
        .oneshot(
            TestApp::request(Method::GET, "/api/admin/diagnostics")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_to_json(resp).await;
    assert_eq!(body["status"], "ok", "test stores are up: {body}");
    assert_eq!(body["database"]["ok"], true);
    assert_eq!(body["valkey_ok"], true);
    assert!(body["database"]["pool_size"].as_u64().unwrap() >= 1);
    assert!(body["version"].is_string());
    assert!(body["uptime_seconds"].is_number());
    // Telemetry-derived fields exist (may be null without observability)
    assert!(body.get("voice_active_sessions").is_some());
    assert!(body.get("errors_last_5m").is_some());
}

#[sqlx::test]
async fn non_admin_cannot_access_diagnostics(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, _) = create_test_user(&pool).await;
    let token = generate_access_token(&app.config, user_id);

    let resp = app
        .oneshot(
            TestApp::request(Method::GET, "/api/admin/diagnostics")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}
