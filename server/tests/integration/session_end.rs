//! Ending sessions in the committed revocation order (spec B.4 rev 4/5):
//! `ended_at` → denylist (best effort) → `NOTIFY` → commit; sockets close
//! through the listener after commit.
//!
//! Run with: `cargo test --test integration session_end`

use std::time::Duration;

use axum::body::Body;
use axum::http::Method;
use futures::StreamExt;
use sqlx::postgres::PgListener;
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::auth::ctx::AuthCtx;
use vc_server::auth::revocation::{
    end_all_sessions_tx, end_sessions_tx, EndReason, SESSION_END_CHANNEL,
};

use super::helpers::login::{failing_valkey, state_with_redis};
use super::helpers::{
    body_to_json, create_elevated_session, create_session_with_token, create_test_user,
    next_ws_event, spawn_test_server, ws_connect, TestApp,
};

async fn me(app: &TestApp, token: &str) -> u16 {
    let req = TestApp::request(Method::GET, "/auth/me")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    app.oneshot(req).await.status().as_u16()
}

async fn end_row(
    pool: &PgPool,
    sid: Uuid,
) -> (Option<chrono::DateTime<chrono::Utc>>, Option<String>) {
    sqlx::query_as("SELECT ended_at, end_reason FROM sessions WHERE id = $1")
        .bind(sid)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn ending_keeps_the_row_with_its_reason(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (sid, token) = create_session_with_token(&pool, &app.config, user).await;
    let (_, other) = create_session_with_token(&pool, &app.config, user).await;

    let req = TestApp::request(Method::POST, "/auth/logout")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.oneshot(req).await.status(), 200);

    let (ended_at, reason) = end_row(&pool, sid).await;
    assert!(ended_at.is_some(), "the row is kept");
    assert_eq!(reason.as_deref(), Some("logout"));
    assert_eq!(me(&app, &token).await, 401);

    let req = TestApp::request(Method::GET, "/auth/sessions")
        .header("Authorization", format!("Bearer {other}"))
        .body(Body::empty())
        .unwrap();
    let list = body_to_json(app.oneshot(req).await).await;
    assert!(list["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["id"] != sid.to_string()));
}

#[sqlx::test]
async fn valkey_down_still_ends_the_session(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (sid, token) = create_session_with_token(&pool, &app.config, user).await;

    let config = (*app.config).clone();
    let broken = state_with_redis(pool.clone(), config.clone(), failing_valkey(&config).await);
    let ctx = AuthCtx::from(&broken);
    let mut tx = pool.begin().await.unwrap();
    let ended = end_sessions_tx(&mut tx, &ctx, user, &[sid], EndReason::Revoked)
        .await
        .expect("a denylist failure does not stop the transition");
    tx.commit().await.unwrap();
    assert_eq!(ended, vec![sid]);
    assert_eq!(end_row(&pool, sid).await.1.as_deref(), Some("revoked"));
    assert_eq!(me(&app, &token).await, 401, "the commit is what ends it");
}

#[sqlx::test]
async fn rolled_back_end_only_over_revokes(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (sid, token) = create_session_with_token(&pool, &app.config, user).await;
    let ctx = AuthCtx::from(&app.state);

    let mut tx = pool.begin().await.unwrap();
    end_sessions_tx(&mut tx, &ctx, user, &[sid], EndReason::Revoked)
        .await
        .unwrap();
    tx.rollback().await.unwrap();

    // The row is live again, but the denylist entry written before the
    // commit stays until its TTL: the request is refused (the safe
    // direction, B.4 "Revocation order").
    assert!(end_row(&pool, sid).await.0.is_none());
    assert_eq!(me(&app, &token).await, 401);
    let ttl: i64 = fred::interfaces::KeysInterface::ttl(
        &app.state.redis,
        vc_server::auth::session_check::denylist_key(sid),
    )
    .await
    .unwrap();
    assert!(
        ttl > 0 && ttl <= app.config.jwt_access_expiry + 60,
        "ttl {ttl}"
    );
}

#[sqlx::test]
async fn notify_arrives_only_after_commit(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (sid, _) = create_session_with_token(&pool, &app.config, user).await;
    let ctx = AuthCtx::from(&app.state);
    // Its own connection, outside the test pool (as the server's listener).
    let own = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect_with((*pool.connect_options()).clone())
        .await
        .unwrap();
    let mut listener = PgListener::connect_with(&own).await.unwrap();
    listener.listen(SESSION_END_CHANNEL).await.unwrap();

    let mut tx = pool.begin().await.unwrap();
    end_sessions_tx(&mut tx, &ctx, user, &[sid], EndReason::Revoked)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    let nothing = tokio::time::timeout(Duration::from_millis(500), listener.recv()).await;
    assert!(nothing.is_err(), "a rolled-back end notifies nobody");

    let mut tx = pool.begin().await.unwrap();
    end_sessions_tx(&mut tx, &ctx, user, &[sid], EndReason::Revoked)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let n = tokio::time::timeout(Duration::from_secs(2), listener.recv())
        .await
        .expect("a payload after commit")
        .unwrap();
    let payload: serde_json::Value = serde_json::from_str(n.payload()).unwrap();
    assert_eq!(payload["user_id"], user.to_string());
    assert_eq!(payload["session_ids"], serde_json::json!([sid]));
    assert_eq!(payload["all"], false);
    let more = tokio::time::timeout(Duration::from_millis(300), listener.recv()).await;
    assert!(more.is_err(), "exactly one payload");
}

#[sqlx::test]
async fn sockets_close_through_the_listener(pool: PgPool) {
    use tokio_tungstenite::tungstenite::Message;
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (sid, token) = create_session_with_token(&pool, &app.config, user).await;
    let mut ws = ws_connect(&server, &token).await;
    next_ws_event(&mut ws, "ready", Duration::from_secs(5)).await;

    let ctx = AuthCtx::from(&app.state);
    let mut tx = pool.begin().await.unwrap();
    end_sessions_tx(&mut tx, &ctx, user, &[sid], EndReason::Revoked)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let err = next_ws_event(&mut ws, "error", Duration::from_secs(1)).await;
    assert_eq!(err["code"], "SESSION_REVOKED");
    let close = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Close(frame))) => return frame,
                Some(Ok(_)) => {}
                other => panic!("socket ended without a close frame: {other:?}"),
            }
        }
    })
    .await
    .expect("closed in time");
    let code: u16 = close.expect("close code").code.into();
    assert_eq!(code, 1008);
}

#[sqlx::test]
async fn no_window_after_commit(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let ctx = AuthCtx::from(&app.state);
    for _ in 0..25 {
        let (sid, token) = create_session_with_token(&pool, &app.config, user).await;
        let mut tx = pool.begin().await.unwrap();
        end_sessions_tx(&mut tx, &ctx, user, &[sid], EndReason::Revoked)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(me(&app, &token).await, 401);
    }
}

#[sqlx::test]
async fn elevation_ends_with_the_session(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    super::helpers::make_admin(&pool, user).await;
    let token = create_elevated_session(&pool, &app.config, user).await;
    let sid: Uuid =
        sqlx::query_scalar("SELECT session_id FROM elevated_sessions WHERE user_id = $1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
    let ctx = AuthCtx::from(&app.state);
    let mut tx = pool.begin().await.unwrap();
    let ended = end_all_sessions_tx(&mut tx, &ctx, user, None, EndReason::Ban)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(ended.contains(&sid));
    let left: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM elevated_sessions WHERE session_id = $1")
            .bind(sid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(left, 0);
    assert_eq!(me(&app, &token).await, 401);
}

/// Review Focus 3: a flushed Valkey only loses the early reject. Deleting
/// the session's denylist key stands in for `FLUSHALL`, which would disturb
/// the other tests sharing the test Valkey.
#[sqlx::test]
async fn valkey_flush_keeps_ended_sessions_refused(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (sid, token) = create_session_with_token(&pool, &app.config, user).await;
    let ctx = AuthCtx::from(&app.state);
    let mut tx = pool.begin().await.unwrap();
    end_sessions_tx(&mut tx, &ctx, user, &[sid], EndReason::Logout)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let _: i64 = fred::interfaces::KeysInterface::del(
        &app.state.redis,
        vc_server::auth::session_check::denylist_key(sid),
    )
    .await
    .unwrap();
    assert_eq!(me(&app, &token).await, 401);
}

#[sqlx::test]
async fn cleanup_deletes_ended_rows_after_30_days(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let mut ids = Vec::new();
    for _ in 0..4 {
        ids.push(create_session_with_token(&pool, &app.config, user).await.0);
    }
    let (old_ended, recent_ended, live, long_expired) = (ids[0], ids[1], ids[2], ids[3]);
    sqlx::query(
        "UPDATE sessions SET ended_at = NOW() - INTERVAL '31 days', end_reason = 'logout'
          WHERE id = $1",
    )
    .bind(old_ended)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE sessions SET ended_at = NOW() - INTERVAL '1 day', end_reason = 'logout'
          WHERE id = $1",
    )
    .bind(recent_ended)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE sessions SET created_at = NOW() - INTERVAL '40 days',
                             expires_at = NOW() - INTERVAL '31 days'
          WHERE id = $1",
    )
    .bind(long_expired)
    .execute(&pool)
    .await
    .unwrap();

    let deleted = vc_server::db::cleanup_expired_sessions(&pool)
        .await
        .unwrap();
    assert_eq!(deleted, 2);
    let left: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM sessions WHERE user_id = $1 ORDER BY id")
            .bind(user)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(left.contains(&recent_ended));
    assert!(left.contains(&live));
    assert!(!left.contains(&old_ended));
    assert!(!left.contains(&long_expired));
}

/// Codex M4: a `NOTIFY` payload is limited to 8000 bytes; ending many
/// sessions at once (here 1000, the configurable session limit) must still
/// commit, and every ended id reaches the listener.
#[sqlx::test]
async fn ending_a_thousand_sessions_commits(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let ctx = AuthCtx::from(&app.state);
    let (user, _) = create_test_user(&pool).await;
    sqlx::query(
        "INSERT INTO sessions (id, user_id, token_hash, expires_at, absolute_expires_at)
         SELECT gen_random_uuid(), $1, 'bulk-' || g, NOW() + INTERVAL '7 days',
                NOW() + INTERVAL '30 days'
           FROM generate_series(1, 1000) g",
    )
    .bind(user)
    .execute(&pool)
    .await
    .unwrap();
    let own = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect_with((*pool.connect_options()).clone())
        .await
        .unwrap();
    let mut listener = PgListener::connect_with(&own).await.unwrap();
    listener.listen(SESSION_END_CHANNEL).await.unwrap();

    let mut tx = pool.begin().await.unwrap();
    let ended = end_all_sessions_tx(&mut tx, &ctx, user, None, EndReason::Ban)
        .await
        .expect("ending 1000 sessions does not exceed the NOTIFY payload");
    tx.commit().await.unwrap();
    assert_eq!(ended.len(), 1001, "the bulk rows and the test session");

    let mut seen = std::collections::HashSet::new();
    let mut all = false;
    while seen.len() < ended.len() {
        let n = tokio::time::timeout(Duration::from_secs(2), listener.recv())
            .await
            .expect("every ended id is announced")
            .unwrap();
        let p: serde_json::Value = serde_json::from_str(n.payload()).unwrap();
        assert_eq!(p["user_id"], user.to_string());
        all |= p["all"].as_bool().unwrap();
        for id in p["session_ids"].as_array().unwrap() {
            seen.insert(id.as_str().unwrap().to_string());
        }
    }
    assert!(all, "the ban still closes every socket of the user");
}
