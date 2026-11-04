//! Refresh rotation with reuse detection, absolute caps and the session
//! limit (spec B.4, RFC 9700 §4.14.2, DC2).
//!
//! Run with: `cargo test --test integration refresh_rotation`

use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Method};
use base64::Engine as _;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::auth::hash_password;

use super::helpers::login::{peer_request, TEST_ORIGIN};
use super::helpers::{body_to_json, next_ws_event, spawn_test_server, ws_connect, TestApp};

const PASSWORD: &str = "refresh rotation password";

async fn password_user(pool: &PgPool) -> (Uuid, String) {
    let username = format!("rr_{}", &Uuid::new_v4().simple().to_string()[..10]);
    let hash = hash_password(PASSWORD).unwrap();
    let user = vc_server::db::create_user(pool, &username, "Refresh User", None, &hash)
        .await
        .unwrap();
    (user.id, username)
}

/// Native sign-in (F4: password, then TOTP): tokens in the body.
async fn login(app: &TestApp, username: &str) -> Value {
    let resp = sign_in(app, username, &[("X-Kaiku-Client", "desktop")]).await;
    assert_eq!(resp.status(), 200);
    body_to_json(resp).await
}

/// An F4 sign-in of `username` with `headers` (TOTP turned on for it).
async fn sign_in(
    app: &TestApp,
    username: &str,
    headers: &[(&str, &str)],
) -> axum::response::Response {
    let user: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
        .bind(username)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let totp = super::helpers::login::enable_totp(&app.pool, &app.config, user, username).await;
    super::helpers::login::f4_sign_in(app, user, username, PASSWORD, &totp, headers).await
}

async fn refresh(app: &TestApp, token: &str) -> (u16, Value) {
    let req = peer_request(Method::POST, "/auth/refresh")
        .header("Content-Type", "application/json")
        .header("X-Kaiku-Client", "desktop")
        .body(Body::from(
            serde_json::json!({"refresh_token": token}).to_string(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    (status, body_to_json(resp).await)
}

fn claims(token: &str) -> Value {
    let part = token.split('.').nth(1).unwrap();
    serde_json::from_slice(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(part)
            .unwrap(),
    )
    .unwrap()
}

fn s(v: &Value, k: &str) -> String {
    v[k].as_str()
        .unwrap_or_else(|| panic!("{k} in {v}"))
        .to_string()
}

async fn end_reason(pool: &PgPool, sid: Uuid) -> Option<String> {
    sqlx::query_scalar("SELECT end_reason FROM sessions WHERE id = $1")
        .bind(sid)
        .fetch_one(pool)
        .await
        .unwrap()
}

fn sid_of(v: &Value) -> Uuid {
    s(v, "session_id").parse().unwrap()
}

#[sqlx::test]
async fn refresh_rotates_in_place(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (_, username) = password_user(&pool).await;
    let first = login(&app, &username).await;
    let sid = sid_of(&first);
    let t0 = s(&first, "refresh_token");
    assert_eq!(claims(&t0)["sid"], sid.to_string());
    assert_eq!(claims(&t0)["gen"], 0);
    sqlx::query("UPDATE sessions SET last_used_at = NOW() - INTERVAL '1 hour' WHERE id = $1")
        .bind(sid)
        .execute(&pool)
        .await
        .unwrap();

    let (status, next) = refresh(&app, &t0).await;
    assert_eq!(status, 200, "{next}");
    let t1 = s(&next, "refresh_token");
    assert_ne!(t1, t0);
    assert_eq!(sid_of(&next), sid, "stable sid");
    assert_eq!(claims(&t1)["gen"], 1);
    assert_eq!(claims(&s(&next, "access_token"))["sid"], sid.to_string());
    let (gen, used): (i64, chrono::DateTime<chrono::Utc>) =
        sqlx::query_as("SELECT refresh_generation, last_used_at FROM sessions WHERE id = $1")
            .bind(sid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(gen, 1);
    assert!(chrono::Utc::now() - used < chrono::Duration::minutes(1));
}

#[sqlx::test]
async fn lost_response_retry_gets_the_same_successor(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (_, username) = password_user(&pool).await;
    let t0 = s(&login(&app, &username).await, "refresh_token");
    let (_, a) = refresh(&app, &t0).await;
    let (status, b) = refresh(&app, &t0).await;
    assert_eq!(status, 200, "{b}");
    assert_eq!(
        s(&a, "refresh_token"),
        s(&b, "refresh_token"),
        "identical successor"
    );
    assert_eq!(sid_of(&a), sid_of(&b));
}

#[sqlx::test]
async fn second_retry_within_24h_is_reuse(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (user, username) = password_user(&pool).await;
    let first = login(&app, &username).await;
    let sid = sid_of(&first);
    let t0 = s(&first, "refresh_token");
    let mut ws = ws_connect(&server, &s(&first, "access_token")).await;
    next_ws_event(&mut ws, "ready", Duration::from_secs(5)).await;

    let (_, r1) = refresh(&app, &t0).await;
    let t1 = s(&r1, "refresh_token");
    assert_eq!(refresh(&app, &t0).await.0, 200, "first retry");
    let (_, r2) = refresh(&app, &t1).await;
    assert!(r2["refresh_token"].is_string());
    // t1 is now the predecessor, within 10 s, but the 24 h retry is spent.
    let (status, body) = refresh(&app, &t1).await;
    assert_eq!(status, 401, "{body}");
    assert_eq!(body["error"], "SESSION_REUSED");
    assert_eq!(
        end_reason(&pool, sid).await.as_deref(),
        Some("refresh_reuse")
    );
    let events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM security_events
          WHERE subject_id = $1 AND event_type = 'session_ended_reuse'
            AND details->>'session_id' = $2",
    )
    .bind(user)
    .bind(sid.to_string())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(events, 1);
    let err = next_ws_event(&mut ws, "error", Duration::from_secs(3)).await;
    assert_eq!(err["code"], "SESSION_REVOKED");
    // Every token of the ended session is dead now.
    assert_eq!(refresh(&app, &s(&r2, "refresh_token")).await.0, 401);
}

#[sqlx::test]
async fn predecessor_after_10s_is_reuse(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (_, username) = password_user(&pool).await;
    let first = login(&app, &username).await;
    let sid = sid_of(&first);
    let t0 = s(&first, "refresh_token");
    assert_eq!(refresh(&app, &t0).await.0, 200);
    sqlx::query(
        "UPDATE sessions SET refresh_issued_at = refresh_issued_at - INTERVAL '11 seconds'
          WHERE id = $1",
    )
    .bind(sid)
    .execute(&pool)
    .await
    .unwrap();
    let (status, body) = refresh(&app, &t0).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (401, Some("SESSION_REUSED"))
    );
}

#[sqlx::test]
async fn older_token_is_reuse(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (_, username) = password_user(&pool).await;
    let first = login(&app, &username).await;
    let sid = sid_of(&first);
    let t0 = s(&first, "refresh_token");
    let t1 = s(&refresh(&app, &t0).await.1, "refresh_token");
    assert_eq!(refresh(&app, &t1).await.0, 200);
    let (status, body) = refresh(&app, &t0).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (401, Some("SESSION_REUSED"))
    );
    assert_eq!(
        end_reason(&pool, sid).await.as_deref(),
        Some("refresh_reuse")
    );
}

#[sqlx::test]
async fn unknown_or_forged_token_changes_nothing(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, username) = password_user(&pool).await;
    let first = login(&app, &username).await;
    let sid = sid_of(&first);
    let t0 = s(&first, "refresh_token");

    // A valid signature naming a session that does not exist.
    let ghost = vc_server::auth::jwt::encode_refresh(
        user,
        Uuid::new_v4(),
        0,
        Uuid::new_v4(),
        chrono::Utc::now().timestamp(),
        chrono::Utc::now().timestamp() + 3600,
        &app.config.jwt_private_key,
    )
    .unwrap();
    let (status, body) = refresh(&app, &ghost).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (401, Some("INVALID_TOKEN"))
    );

    // A tampered token.
    let mut forged = t0.clone();
    let last = forged.pop().unwrap();
    forged.push(if last == 'A' { 'B' } else { 'A' });
    assert_eq!(refresh(&app, &forged).await.0, 401);

    // A higher generation than the row (never issued).
    let ahead = vc_server::auth::jwt::encode_refresh(
        user,
        sid,
        5,
        Uuid::new_v4(),
        chrono::Utc::now().timestamp(),
        chrono::Utc::now().timestamp() + 3600,
        &app.config.jwt_private_key,
    )
    .unwrap();
    assert_eq!(refresh(&app, &ahead).await.0, 401);

    assert!(end_reason(&pool, sid).await.is_none(), "no session ended");
    assert_eq!(
        refresh(&app, &t0).await.0,
        200,
        "the real token still works"
    );
}

#[sqlx::test]
async fn parallel_refresh_never_ends_the_session(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (_, username) = password_user(&pool).await;
    for _ in 0..25 {
        let first = login(&app, &username).await;
        let sid = sid_of(&first);
        let t0 = s(&first, "refresh_token");
        let (a, b) = tokio::join!(refresh(&app, &t0), refresh(&app, &t0));
        assert_eq!((a.0, b.0), (200, 200), "{} / {}", a.1, b.1);
        assert_eq!(s(&a.1, "refresh_token"), s(&b.1, "refresh_token"));
        assert!(end_reason(&pool, sid).await.is_none());
    }
}

#[sqlx::test]
async fn inactivity_and_absolute_cap_expire(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (_, username) = password_user(&pool).await;

    // Absolute cap passed (a web session created 31 days ago).
    let a = login(&app, &username).await;
    let sid = sid_of(&a);
    sqlx::query(
        "UPDATE sessions SET created_at = NOW() - INTERVAL '31 days',
                             absolute_expires_at = NOW() - INTERVAL '1 day'
          WHERE id = $1",
    )
    .bind(sid)
    .execute(&pool)
    .await
    .unwrap();
    let (status, body) = refresh(&app, &s(&a, "refresh_token")).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (401, Some("SESSION_EXPIRED"))
    );
    assert_eq!(
        end_reason(&pool, sid).await.as_deref(),
        Some("absolute_cap")
    );

    // Inactivity: the row lapses (not ended), the refresh is refused.
    let b = login(&app, &username).await;
    let sid = sid_of(&b);
    sqlx::query(
        "UPDATE sessions SET created_at = NOW() - INTERVAL '8 days',
                             expires_at = NOW() - INTERVAL '1 second'
          WHERE id = $1",
    )
    .bind(sid)
    .execute(&pool)
    .await
    .unwrap();
    let (status, body) = refresh(&app, &s(&b, "refresh_token")).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (401, Some("SESSION_EXPIRED"))
    );

    // The sliding expiry never passes the absolute cap.
    let c = login(&app, &username).await;
    let sid = sid_of(&c);
    sqlx::query(
        "UPDATE sessions SET absolute_expires_at = NOW() + INTERVAL '1 hour' WHERE id = $1",
    )
    .bind(sid)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(refresh(&app, &s(&c, "refresh_token")).await.0, 200);
    let ok: bool =
        sqlx::query_scalar("SELECT expires_at <= absolute_expires_at FROM sessions WHERE id = $1")
            .bind(sid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(ok);
}

#[sqlx::test]
async fn legacy_refresh_token_rotates_once(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = password_user(&pool).await;
    // A token pair of the old format: the refresh JWT has no `sid`/`gen`.
    let sid = Uuid::new_v4();
    let pair = super::helpers::tokens::token_pair_for_session(
        user,
        sid,
        &app.config.jwt_private_key,
        app.config.jwt_access_expiry,
        app.config.jwt_refresh_expiry,
    )
    .unwrap();
    assert!(claims(&pair.refresh_token).get("gen").is_none());
    sqlx::query(
        "INSERT INTO sessions (id, user_id, token_hash, expires_at) VALUES ($1, $2, $3, NOW() + INTERVAL '7 days')",
    )
    .bind(sid)
    .bind(user)
    .bind(vc_server::auth::hash_token(&pair.refresh_token))
    .execute(&pool)
    .await
    .unwrap();

    let (status, body) = refresh(&app, &pair.refresh_token).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(claims(&s(&body, "refresh_token"))["gen"], 1);
    assert_eq!(sid_of(&body), sid);
    // The legacy token is spent: unknown now, and nothing ends.
    let (status, body) = refresh(&app, &pair.refresh_token).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (401, Some("INVALID_TOKEN"))
    );
    assert!(end_reason(&pool, sid).await.is_none());
}

async fn insert_live_sessions(pool: &PgPool, user: Uuid, n: i64) {
    sqlx::query(
        "INSERT INTO sessions (id, user_id, token_hash, expires_at, last_used_at)
         SELECT gen_random_uuid(), $1, 'limit-' || $1::text || '-' || g,
                NOW() + INTERVAL '7 days', NOW() - make_interval(mins => (1000 - g)::int)
           FROM generate_series(1, $2) g",
    )
    .bind(user)
    .bind(n)
    .execute(pool)
    .await
    .unwrap();
}

async fn live_count(pool: &PgPool, user: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM sessions WHERE user_id = $1 AND ended_at IS NULL
           AND expires_at > NOW() AND absolute_expires_at > NOW()",
    )
    .bind(user)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test]
async fn session_limit_ends_least_recently_used(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, username) = password_user(&pool).await;
    insert_live_sessions(&pool, user, 50).await;
    let oldest: Uuid = sqlx::query_scalar(
        "SELECT id FROM sessions WHERE user_id = $1 ORDER BY last_used_at LIMIT 1",
    )
    .bind(user)
    .fetch_one(&pool)
    .await
    .unwrap();
    login(&app, &username).await;
    assert_eq!(live_count(&pool, user).await, 50);
    assert_eq!(end_reason(&pool, oldest).await.as_deref(), Some("limit"));
    let events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM security_events
          WHERE subject_id = $1 AND event_type = 'session_ended_limit'
            AND details->>'session_id' = $2",
    )
    .bind(user)
    .bind(oldest.to_string())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(events, 1);
}

#[sqlx::test]
async fn concurrent_sign_ins_never_exceed_the_limit(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = password_user(&pool).await;
    insert_live_sessions(&pool, user, 49).await;
    // Five concurrent sign-ins (the session mint of every flow).
    let logins = (0..5).map(|_| {
        super::helpers::login::signed_in(
            &app.state,
            user,
            vc_server::auth::session_issue::SignInMethod::PasswordTotp,
        )
    });
    futures::future::join_all(logins).await;
    assert_eq!(live_count(&pool, user).await, 50);
}

#[sqlx::test]
async fn refresh_cookie_renamed_and_strict(pool: PgPool) {
    let mut config = vc_server::config::Config::default_for_test();
    config.cookie_secure = true;
    let app = TestApp::with_pool_and_config(pool.clone(), config).await;
    let (_, username) = password_user(&pool).await;

    // A browser sign-in gets the prefixed, strict cookie and no body token.
    let resp = sign_in(&app, &username, &[("Origin", TEST_ORIGIN)]).await;
    assert_eq!(resp.status(), 200);
    let set_cookie: Vec<String> = resp
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .collect();
    let cookie = set_cookie
        .iter()
        .find(|c| c.starts_with("__Secure-kaiku_refresh="))
        .unwrap_or_else(|| panic!("{set_cookie:?}"));
    for attr in ["HttpOnly", "Secure", "SameSite=Strict", "Path=/auth"] {
        assert!(cookie.contains(attr), "{attr} in {cookie}");
    }
    let body = body_to_json(resp).await;
    assert!(body.get("refresh_token").is_none());
    let token = cookie
        .trim_start_matches("__Secure-kaiku_refresh=")
        .split(';')
        .next()
        .unwrap()
        .to_string();

    // The old cookie name is accepted once, rewritten and cleared.
    let req = peer_request(Method::POST, "/auth/refresh")
        .header(header::ORIGIN, TEST_ORIGIN)
        .header(header::COOKIE, format!("kaiku_refresh={token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);
    let set_cookie: Vec<String> = resp
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .collect();
    assert!(set_cookie
        .iter()
        .any(|c| c.starts_with("__Secure-kaiku_refresh=") && c.contains("SameSite=Strict")));
    assert!(
        set_cookie
            .iter()
            .any(|c| c.starts_with("kaiku_refresh=") && c.contains("Max-Age=0")),
        "{set_cookie:?}"
    );
}

/// Review round 3 (Argon coverage, part 2 m4): a logout that read the
/// pre-v1.0 cookie also clears it.
#[sqlx::test]
async fn logout_clears_the_legacy_cookie(pool: PgPool) {
    let mut config = vc_server::config::Config::default_for_test();
    config.cookie_secure = true;
    let app = TestApp::with_pool_and_config(pool.clone(), config).await;
    let (user, _) = password_user(&pool).await;
    // L1b: a password alone mints no session; the session of any flow.
    let (access, token, _) = super::helpers::login::signed_in(
        &app.state,
        user,
        vc_server::auth::session_issue::SignInMethod::PasswordTotp,
    )
    .await;

    let req = peer_request(Method::POST, "/auth/logout")
        .header(header::ORIGIN, TEST_ORIGIN)
        .header(header::AUTHORIZATION, format!("Bearer {access}"))
        .header(header::COOKIE, format!("kaiku_refresh={token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert!(resp.status().is_success(), "{}", resp.status());
    let set_cookie: Vec<String> = resp
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .collect();
    assert!(
        set_cookie
            .iter()
            .any(|c| c.starts_with("kaiku_refresh=") && c.contains("Max-Age=0")),
        "{set_cookie:?}"
    );
}

/// Review round 3 (Argon coverage, part 3 M1): a rotation reads everything
/// through its own transaction. With every pool connection held by
/// rotations (here: a pool of one), a second connection for the ban check
/// would never come and the refresh would time out.
#[sqlx::test]
async fn rotation_needs_only_its_own_connection(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (_, username) = password_user(&pool).await;
    let tokens = login(&app, &username).await;
    let raw = tokens["refresh_token"].as_str().unwrap().to_string();
    let one = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(std::time::Duration::from_secs(2))
        .connect_with((*pool.connect_options()).clone())
        .await
        .unwrap();
    let ctx = vc_server::auth::ctx::AuthCtx {
        db: &one,
        redis: &app.state.redis,
        config: &app.state.config,
    };
    let client = vc_server::auth::session_issue::SessionClient {
        ip: None,
        user_agent: None,
    };
    let outcome = vc_server::auth::refresh::rotate(&ctx, &raw, &client).await;
    assert!(outcome.is_ok(), "{:?}", outcome.err());
}
