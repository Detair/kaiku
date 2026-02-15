//! HTTP Integration Tests for Connection History API
//!
//! Tests the 3 connectivity endpoints:
//! - GET /api/me/connection/summary
//! - GET /api/me/connection/sessions
//! - GET /api/me/connection/sessions/:id
//!
//! Run with: `cargo test --test connectivity_http_test -- --nocapture`

mod helpers;

use axum::body::Body;
use axum::http::Method;
use helpers::{body_to_json, create_test_user, generate_access_token, TestApp};
use serial_test::serial;
use sqlx::PgPool;
use uuid::Uuid;

// ============================================================================
// Test Data Helpers
// ============================================================================

/// Insert a connection session and optional metric rows for testing.
///
/// Inserts directly (bypassing RLS) using the shared superuser pool.
/// Returns the session ID.
async fn insert_test_session(
    pool: &PgPool,
    user_id: Uuid,
    channel_id: Uuid,
    guild_id: Option<Uuid>,
    metric_count: usize,
) -> Uuid {
    let session_id = Uuid::now_v7();

    sqlx::query(
        r"INSERT INTO connection_sessions
            (id, user_id, channel_id, guild_id, started_at, ended_at,
             avg_latency, avg_loss, avg_jitter, worst_quality)
          VALUES ($1, $2, $3, $4,
                  NOW() - INTERVAL '1 hour', NOW(),
                  25, 0.01, 5, 2)",
    )
    .bind(session_id)
    .bind(user_id)
    .bind(channel_id)
    .bind(guild_id)
    .execute(pool)
    .await
    .expect("Failed to insert test session");

    for i in 0..metric_count {
        sqlx::query(
            r"INSERT INTO connection_metrics
                (time, user_id, session_id, channel_id, guild_id,
                 latency_ms, packet_loss, jitter_ms, quality)
              VALUES (NOW() - ($1 || ' seconds')::INTERVAL,
                      $2, $3, $4, $5,
                      25, 0.01, 5, 2)",
        )
        .bind(i.to_string())
        .bind(user_id)
        .bind(session_id)
        .bind(channel_id)
        .bind(guild_id)
        .execute(pool)
        .await
        .expect("Failed to insert test metric");
    }

    session_id
}

// ============================================================================
// GET /api/me/connection/summary
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_summary_empty() {
    let app = helpers::fresh_test_app().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    let req = TestApp::request(Method::GET, "/api/me/connection/summary")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(
        resp.status(),
        200,
        "Summary should return 200 even with no data"
    );

    let json = body_to_json(resp).await;
    assert_eq!(json["period_days"], 30);
    assert_eq!(json["total_sessions"], 0);
    assert_eq!(json["total_duration_secs"], 0);
    assert!(json["daily_stats"].as_array().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_summary_with_data() {
    let app = helpers::fresh_test_app().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let channel_id = helpers::create_channel(
        &app.pool,
        helpers::create_guild(&app.pool, user_id).await,
        "voice-test",
    )
    .await;

    let guild_id = sqlx::query_scalar::<_, Uuid>("SELECT guild_id FROM channels WHERE id = $1")
        .bind(channel_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();

    let session_id = insert_test_session(&app.pool, user_id, channel_id, Some(guild_id), 3).await;

    let mut guard = app.cleanup_guard();
    let sid = session_id;
    guard.add(move |pool| async move {
        helpers::delete_connection_data(&pool, sid).await;
    });
    guard.add(move |pool| async move {
        helpers::delete_guild(&pool, guild_id).await;
    });
    guard.delete_user(user_id);

    let req = TestApp::request(Method::GET, "/api/me/connection/summary")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total_sessions"], 1);
    assert!(json["total_duration_secs"].as_i64().unwrap() > 0);
    assert!(json["avg_latency"].is_number());
    assert!(!json["daily_stats"].as_array().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_summary_unauthenticated() {
    let app = helpers::fresh_test_app().await;

    let req = TestApp::request(Method::GET, "/api/me/connection/summary")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 401, "Should return 401 without token");
}

// ============================================================================
// GET /api/me/connection/sessions
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_sessions_empty() {
    let app = helpers::fresh_test_app().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    let req = TestApp::request(Method::GET, "/api/me/connection/sessions")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 0);
    assert!(json["sessions"].as_array().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_sessions_with_data() {
    let app = helpers::fresh_test_app().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = helpers::create_guild(&app.pool, user_id).await;
    let channel_id = helpers::create_channel(&app.pool, guild_id, "voice-sess").await;

    let session_id = insert_test_session(&app.pool, user_id, channel_id, Some(guild_id), 0).await;

    let mut guard = app.cleanup_guard();
    let sid = session_id;
    guard.add(move |pool| async move {
        helpers::delete_connection_data(&pool, sid).await;
    });
    guard.add(move |pool| async move {
        helpers::delete_guild(&pool, guild_id).await;
    });
    guard.delete_user(user_id);

    let req = TestApp::request(Method::GET, "/api/me/connection/sessions")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 1);
    let sessions = json["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["id"], session_id.to_string());
    assert_eq!(sessions[0]["channel_name"], "voice-sess");
    assert!(sessions[0]["guild_name"].is_string());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_sessions_pagination() {
    let app = helpers::fresh_test_app().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = helpers::create_guild(&app.pool, user_id).await;
    let channel_id = helpers::create_channel(&app.pool, guild_id, "voice-page").await;

    let s1 = insert_test_session(&app.pool, user_id, channel_id, Some(guild_id), 0).await;
    let s2 = insert_test_session(&app.pool, user_id, channel_id, Some(guild_id), 0).await;
    let s3 = insert_test_session(&app.pool, user_id, channel_id, Some(guild_id), 0).await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move {
        helpers::delete_connection_data(&pool, s1).await;
        helpers::delete_connection_data(&pool, s2).await;
        helpers::delete_connection_data(&pool, s3).await;
    });
    guard.add(move |pool| async move {
        helpers::delete_guild(&pool, guild_id).await;
    });
    guard.delete_user(user_id);

    // Request page: limit=1, offset=1
    let req = TestApp::request(Method::GET, "/api/me/connection/sessions?limit=1&offset=1")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 3, "Total should reflect all sessions");
    assert_eq!(json["limit"], 1);
    assert_eq!(json["offset"], 1);
    assert_eq!(
        json["sessions"].as_array().unwrap().len(),
        1,
        "Should return exactly 1 session"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_sessions_unauthenticated() {
    let app = helpers::fresh_test_app().await;

    let req = TestApp::request(Method::GET, "/api/me/connection/sessions")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 401, "Should return 401 without token");
}

// ============================================================================
// GET /api/me/connection/sessions/:id
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_session_detail() {
    let app = helpers::fresh_test_app().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = helpers::create_guild(&app.pool, user_id).await;
    let channel_id = helpers::create_channel(&app.pool, guild_id, "voice-detail").await;

    // Insert session with 5 metrics (< 200, so no downsampling)
    let session_id = insert_test_session(&app.pool, user_id, channel_id, Some(guild_id), 5).await;

    let mut guard = app.cleanup_guard();
    let sid = session_id;
    guard.add(move |pool| async move {
        helpers::delete_connection_data(&pool, sid).await;
    });
    guard.add(move |pool| async move {
        helpers::delete_guild(&pool, guild_id).await;
    });
    guard.delete_user(user_id);

    let url = format!("/api/me/connection/sessions/{session_id}");
    let req = TestApp::request(Method::GET, &url)
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["id"], session_id.to_string());
    assert_eq!(json["channel_name"], "voice-detail");
    assert_eq!(json["downsampled"], false);
    let metrics = json["metrics"].as_array().unwrap();
    assert_eq!(metrics.len(), 5, "Should return all 5 metric points");
    // Verify metric shape
    assert!(metrics[0]["time"].is_string());
    assert!(metrics[0]["latency_ms"].is_number());
    assert!(metrics[0]["packet_loss"].is_number());
    assert!(metrics[0]["jitter_ms"].is_number());
    assert!(metrics[0]["quality"].is_number());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_session_detail_not_found() {
    let app = helpers::fresh_test_app().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    let random_id = Uuid::new_v4();
    let url = format!("/api/me/connection/sessions/{random_id}");
    let req = TestApp::request(Method::GET, &url)
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 404, "Non-existent session should return 404");
}

// ============================================================================
// RLS Isolation
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_session_rls_isolation() {
    let app = helpers::fresh_test_app().await;

    // Create two users
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let token_b = generate_access_token(&app.config, user_b);

    let guild_id = helpers::create_guild(&app.pool, user_a).await;
    let channel_id = helpers::create_channel(&app.pool, guild_id, "voice-rls").await;

    // Insert a session for user A
    let session_a = insert_test_session(&app.pool, user_a, channel_id, Some(guild_id), 0).await;

    let mut guard = app.cleanup_guard();
    let sa = session_a;
    guard.add(move |pool| async move {
        helpers::delete_connection_data(&pool, sa).await;
    });
    guard.add(move |pool| async move {
        helpers::delete_guild(&pool, guild_id).await;
    });
    guard.delete_user(user_a);
    guard.delete_user(user_b);

    // User B should not see user A's sessions
    let req = TestApp::request(Method::GET, "/api/me/connection/sessions")
        .header("Authorization", format!("Bearer {token_b}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 0, "User B should not see User A's sessions");
    assert!(json["sessions"].as_array().unwrap().is_empty());
}
