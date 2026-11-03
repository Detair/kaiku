//! Routes behind the "full session" guard (spec B.4 probation rules, C27
//! "What each may do"). L1a guards `/api/admin/*`; L1b Task 16 adds the
//! own-account routes and the step-up guard to this file.
//!
//! Run with: `cargo test --test integration guarded_routes`

use axum::body::Body;
use axum::http::Method;
use sqlx::PgPool;

use super::helpers::login::peer_request;
use super::helpers::{
    body_to_json, create_probation_session_with_token, create_session_with_token, create_test_user,
    make_admin, TestApp,
};

async fn call(app: &TestApp, method: Method, uri: &str, token: &str) -> (u16, serde_json::Value) {
    let req = peer_request(method, uri)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    (status, body_to_json(resp).await)
}

/// Review round 1 (Argon B1, Codex M5): C27 puts `/api/admin/*` in the
/// "full" row; a probation session of an admin (every pre-upgrade session,
/// QR and password/OIDC sign-ins) reads and changes nothing there.
/// `/api/admin/status` stays open to any session.
#[sqlx::test]
async fn admin_routes_need_full(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (admin, _) = create_test_user(&pool).await;
    make_admin(&pool, admin).await;
    let (target, _) = create_test_user(&pool).await;
    let (_, probation) = create_probation_session_with_token(&pool, &app.config, admin).await;

    let reads = [
        "/api/admin/users".to_string(),
        "/api/admin/users/export".to_string(),
        format!("/api/admin/users/{target}/details"),
        "/api/admin/audit-log".to_string(),
        "/api/admin/stats".to_string(),
        "/api/admin/guilds".to_string(),
        "/api/admin/diagnostics".to_string(),
    ];
    for uri in &reads {
        let (status, body) = call(&app, Method::GET, uri, &probation).await;
        assert_eq!(
            (status, body["error"].as_str()),
            (403, Some("FULL_SESSION_REQUIRED")),
            "GET {uri}: {body}"
        );
    }
    let (status, body) = call(&app, Method::DELETE, "/api/admin/elevate", &probation).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (403, Some("FULL_SESSION_REQUIRED"))
    );

    // `/status` answers any session.
    let (status, body) = call(&app, Method::GET, "/api/admin/status", &probation).await;
    assert_eq!(status, 200, "{body}");

    // A full session of the same admin reads the list.
    let (_, full) = create_session_with_token(&pool, &app.config, admin).await;
    let (status, body) = call(&app, Method::GET, "/api/admin/users", &full).await;
    assert_eq!(status, 200, "{body}");

    // A non-admin is told it is no admin, whatever its assurance.
    let (plain, _) = create_test_user(&pool).await;
    let (_, plain_probation) = create_probation_session_with_token(&pool, &app.config, plain).await;
    let (status, body) = call(&app, Method::GET, "/api/admin/users", &plain_probation).await;
    assert_eq!((status, body["error"].as_str()), (403, Some("NOT_ADMIN")));
}

/// An elevation row on a probation session (one from before the upgrade)
/// opens no elevated route.
#[sqlx::test]
async fn probation_elevation_opens_nothing(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (admin, _) = create_test_user(&pool).await;
    make_admin(&pool, admin).await;
    let (target, _) = create_test_user(&pool).await;
    let (sid, probation) = create_probation_session_with_token(&pool, &app.config, admin).await;
    sqlx::query(
        "INSERT INTO elevated_sessions (user_id, session_id, ip_address, expires_at, reason)
         VALUES ($1, $2, '127.0.0.1', NOW() + INTERVAL '15 minutes', 'legacy')",
    )
    .bind(admin)
    .bind(sid)
    .execute(&pool)
    .await
    .unwrap();
    let req = peer_request(Method::POST, &format!("/api/admin/users/{target}/ban"))
        .header("Authorization", format!("Bearer {probation}"))
        .header("Content-Type", "application/json")
        .body(Body::from(r#"{"reason":"x"}"#))
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    let body = body_to_json(resp).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (403, Some("FULL_SESSION_REQUIRED")),
        "{body}"
    );
    let banned: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM global_bans WHERE user_id = $1)")
            .bind(target)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!banned);
}
