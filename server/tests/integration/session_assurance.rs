//! Session assurance fixed at creation, `issue_session()`, and the
//! authoritative committed-session check (spec B.4 rev 5, CC4, C27).
//!
//! Run with: `cargo test --test integration session_assurance`

use std::time::Duration;

use axum::body::Body;
use axum::http::Method;
use base64::Engine as _;
use chrono::{DateTime, Utc};
use futures::StreamExt;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::auth::client_class::ClientClass;
use vc_server::auth::ctx::AuthCtx;
use vc_server::auth::hash_password;
use vc_server::auth::session_issue::{issue_session, NewSession, SessionClient, SignInMethod};
use vc_server::system_account::SYSTEM_USER_ID;

use super::helpers::migrations::{apply_from, apply_until};
use super::helpers::{
    body_to_json, create_probation_session_with_token, create_session_with_token, create_test_user,
    next_ws_event, spawn_test_server, test_authenticator_id, ws_connect, TestApp,
};

const PASSWORD: &str = "correct horse battery staple";

/// A user with a real password; returns (id, username).
async fn password_user(pool: &PgPool) -> (Uuid, String) {
    let username = format!("pw_{}", &Uuid::new_v4().simple().to_string()[..10]);
    let hash = hash_password(PASSWORD).unwrap();
    let user = vc_server::db::create_user(pool, &username, "Password User", None, &hash)
        .await
        .unwrap();
    (user.id, username)
}

/// A password sign-in (F4: password, then TOTP; TOTP is turned on for the
/// account) as `client`.
async fn login(app: &TestApp, username: &str, client: Option<&str>) -> (u16, Value) {
    let user: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
        .bind(username)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let totp = super::helpers::login::enable_totp(&app.pool, &app.config, user, username).await;
    let headers: Vec<(&str, &str)> = client.map(|c| ("X-Kaiku-Client", c)).into_iter().collect();
    let resp =
        super::helpers::login::f4_sign_in(app, user, username, PASSWORD, &totp, &headers).await;
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

fn jwt_payload(token: &str) -> Value {
    let part = token.split('.').nth(1).unwrap();
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(part)
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn days_from_now(v: &Value) -> i64 {
    let t: DateTime<Utc> = v.as_str().unwrap().parse().unwrap();
    ((t - Utc::now()).num_seconds() as f64 / 86_400.0).round() as i64
}

#[sqlx::test(migrations = false)]
async fn legacy_sessions_backfill_to_probation(pool: PgPool) {
    apply_until(&pool, 20_261_120_000_100).await;
    let user: Uuid = sqlx::query_scalar(
        // With TOTP: the later v1.0 upgrade (L1b) ends password-only sessions.
        "INSERT INTO users (username, display_name, password_hash, mfa_secret)
         VALUES ('legacy_user', 'L', 'x', 'enc')
         RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let recent = Uuid::new_v4();
    let old = Uuid::new_v4();
    for (id, age_days) in [(recent, 10), (old, 40)] {
        sqlx::query(
            "INSERT INTO sessions (id, user_id, token_hash, expires_at, created_at)
             VALUES ($1, $2, $3, NOW() + INTERVAL '1 day', NOW() - make_interval(days => $4))",
        )
        .bind(id)
        .bind(user)
        .bind(format!("hash-{id}"))
        .bind(age_days)
        .execute(&pool)
        .await
        .unwrap();
    }
    apply_from(&pool, 20_261_120_000_100).await;

    type Row = (
        String,
        Vec<String>,
        String,
        DateTime<Utc>,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
        Option<String>,
    );
    let read = |id: Uuid| {
        let pool = pool.clone();
        async move {
            sqlx::query_as::<_, Row>(
                "SELECT assurance, auth_methods, client_class, created_at, absolute_expires_at,
                        ended_at, end_reason
                   FROM sessions WHERE id = $1",
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap()
        }
    };
    let r = read(recent).await;
    assert_eq!(r.0, "probation");
    assert_eq!(r.1, vec!["legacy".to_string()]);
    assert_eq!(r.2, "web");
    assert_eq!(r.4, r.3 + chrono::Duration::days(30));
    assert!(r.5.is_none());
    let o = read(old).await;
    assert_eq!(o.0, "probation");
    assert!(o.5.is_some(), "a 40-day-old web session is past its cap");
    assert_eq!(o.6.as_deref(), Some("absolute_cap"));
    let first: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT first_activity_at FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        first.is_some(),
        "an account with a password is existing (B.0)"
    );
}

/// Review round 1 (Fable M1 / Argon B2): every account that exists at the
/// upgrade is an existing account (B.0), also a dormant OIDC-only account
/// whose expired session rows the old cleanup already deleted.
#[sqlx::test(migrations = false)]
async fn dormant_oidc_account_is_existing_after_the_upgrade(pool: PgPool) {
    apply_until(&pool, 20_261_120_000_100).await;
    let user: Uuid = sqlx::query_scalar(
        "INSERT INTO users (username, display_name, auth_method, external_id)
         VALUES ('oidc_only', 'O', 'oidc', 'idp|sub-1') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    apply_from(&pool, 20_261_120_000_100).await;
    let first: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT first_activity_at FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(first.is_some(), "an admin setup of it must wait (SM3)");
    let system: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT first_activity_at FROM users WHERE id = $1")
            .bind(SYSTEM_USER_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(system.is_none(), "the system account has no activity");
}

/// Review round 1 (Codex M5): an elevation from before the upgrade (by
/// password or OIDC re-auth) does not survive it; elevation now needs a
/// full session and a step-up.
#[sqlx::test(migrations = false)]
async fn legacy_elevations_end_at_the_upgrade(pool: PgPool) {
    apply_until(&pool, 20_261_120_000_100).await;
    let user: Uuid = sqlx::query_scalar(
        "INSERT INTO users (username, display_name, password_hash) VALUES ('legacy_admin', 'A', 'x')
         RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let sid = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO sessions (id, user_id, token_hash, expires_at)
         VALUES ($1, $2, 'legacy-elevated', NOW() + INTERVAL '1 day')",
    )
    .bind(sid)
    .bind(user)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO elevated_sessions (user_id, session_id, ip_address, expires_at, reason)
         VALUES ($1, $2, '127.0.0.1', NOW() + INTERVAL '15 minutes', 'legacy')",
    )
    .bind(user)
    .bind(sid)
    .execute(&pool)
    .await
    .unwrap();
    apply_from(&pool, 20_261_120_000_100).await;
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM elevated_sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
}

#[sqlx::test]
async fn new_password_session_is_probation(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, username) = password_user(&pool).await;
    let (status, body) = login(&app, &username, None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["assurance"], "probation");
    let sid: Uuid = body["session_id"].as_str().unwrap().parse().unwrap();
    assert_eq!(days_from_now(&body["absolute_expires_at"]), 30);
    let claims = jwt_payload(body["access_token"].as_str().unwrap());
    assert_eq!(claims["assurance"], "probation");
    assert_eq!(claims["sid"], sid.to_string());

    let row: (Uuid, String, Vec<String>, String, Option<Uuid>) = sqlx::query_as(
        "SELECT user_id, assurance, auth_methods, client_class, authenticator_id
           FROM sessions WHERE id = $1",
    )
    .bind(sid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0, user);
    assert_eq!(row.1, "probation");
    assert_eq!(row.2, vec!["password".to_string(), "totp".to_string()]);
    assert_eq!(row.3, "web");
    assert!(row.4.is_none());
}

#[sqlx::test]
async fn desktop_and_android_caps_are_90_days(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (_, username) = password_user(&pool).await;
    for client in ["desktop", "android"] {
        let (status, body) = login(&app, &username, Some(client)).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(days_from_now(&body["absolute_expires_at"]), 90, "{client}");
        let sid: Uuid = body["session_id"].as_str().unwrap().parse().unwrap();
        let (class, expires, absolute): (String, DateTime<Utc>, DateTime<Utc>) = sqlx::query_as(
            "SELECT client_class, expires_at, absolute_expires_at FROM sessions WHERE id = $1",
        )
        .bind(sid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(class, client);
        assert!(expires <= absolute);
        assert!(
            body["refresh_token"].as_str().is_some(),
            "native gets the refresh token"
        );
    }
}

#[sqlx::test]
async fn ended_session_is_refused_with_empty_denylist(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (sid, token) = create_session_with_token(&pool, &app.config, user).await;
    assert_eq!(get(&app, "/auth/me", &token).await.0, 200);
    sqlx::query("UPDATE sessions SET ended_at = NOW(), end_reason = 'logout' WHERE id = $1")
        .bind(sid)
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = get(&app, "/auth/me", &token).await;
    assert_eq!(status, 401);
    assert_eq!(body["error"], "INVALID_TOKEN");
}

#[sqlx::test]
async fn missing_row_and_missing_sid_are_refused(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;

    // A valid signature for a session that has no row.
    let ghost = super::helpers::tokens::token_pair_for_session(
        user,
        Uuid::new_v4(),
        &app.config.jwt_private_key,
        app.config.jwt_access_expiry,
        app.config.jwt_refresh_expiry,
    )
    .unwrap()
    .access_token;
    let (status, body) = get(&app, "/auth/me", &ghost).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (401, Some("INVALID_TOKEN"))
    );

    // A token without `sid`.
    let key = base64::engine::general_purpose::STANDARD
        .decode(&app.config.jwt_private_key)
        .unwrap();
    let claims = serde_json::json!({
        "sub": user.to_string(),
        "exp": Utc::now().timestamp() + 600,
        "iat": Utc::now().timestamp(),
        "typ": "access",
    });
    let no_sid = jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::EdDSA),
        &claims,
        &jsonwebtoken::EncodingKey::from_ed_pem(&key).unwrap(),
    )
    .unwrap();
    let (status, body) = get(&app, "/auth/me", &no_sid).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (401, Some("INVALID_TOKEN"))
    );

    // Another user's session id.
    let (other, _) = create_test_user(&pool).await;
    let (other_sid, _) = create_session_with_token(&pool, &app.config, other).await;
    let stolen_sid = super::helpers::tokens::token_pair_for_session(
        user,
        other_sid,
        &app.config.jwt_private_key,
        app.config.jwt_access_expiry,
        app.config.jwt_refresh_expiry,
    )
    .unwrap()
    .access_token;
    assert_eq!(get(&app, "/auth/me", &stolen_sid).await.0, 401);

    // Past the inactivity expiry or the absolute cap.
    for column in ["expires_at", "absolute_expires_at"] {
        let (sid, token) = create_session_with_token(&pool, &app.config, user).await;
        assert_eq!(get(&app, "/auth/me", &token).await.0, 200);
        let stmt = if column == "expires_at" {
            "UPDATE sessions SET created_at = created_at - INTERVAL '1 day', expires_at = NOW() - INTERVAL '1 second' WHERE id = $1"
        } else {
            "UPDATE sessions SET absolute_expires_at = NOW() - INTERVAL '1 second' WHERE id = $1"
        };
        sqlx::query(stmt).bind(sid).execute(&pool).await.unwrap();
        assert_eq!(get(&app, "/auth/me", &token).await.0, 401, "{column}");
    }
}

#[sqlx::test]
async fn ws_recheck_closes_ended_session(pool: PgPool) {
    use tokio_tungstenite::tungstenite::Message;
    let mut config = vc_server::config::Config::default_for_test();
    config.session_recheck_secs = 1;
    let app = TestApp::with_pool_and_config(pool.clone(), config).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (sid, token) = create_session_with_token(&pool, &app.config, user).await;
    let mut ws = ws_connect(&server, &token).await;
    next_ws_event(&mut ws, "ready", Duration::from_secs(5)).await;

    // End the session in SQL only: no denylist entry, no notice.
    sqlx::query("UPDATE sessions SET ended_at = NOW(), end_reason = 'revoked' WHERE id = $1")
        .bind(sid)
        .execute(&pool)
        .await
        .unwrap();
    let err = next_ws_event(&mut ws, "error", Duration::from_secs(3)).await;
    assert_eq!(err["code"], "SESSION_REVOKED");
    let close = tokio::time::timeout(Duration::from_secs(3), async {
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
async fn uploads_token_query_uses_the_row(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (sid, token) = create_session_with_token(&pool, &app.config, user).await;
    let uri = format!(
        "/api/messages/attachments/{}/download?token={token}",
        Uuid::new_v4()
    );
    let live = app
        .oneshot(
            TestApp::request(Method::GET, &uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .status();
    assert_ne!(live.as_u16(), 401, "a live session passes the check");
    sqlx::query("UPDATE sessions SET ended_at = NOW(), end_reason = 'logout' WHERE id = $1")
        .bind(sid)
        .execute(&pool)
        .await
        .unwrap();
    let resp = app
        .oneshot(
            TestApp::request(Method::GET, &uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(resp.status(), 401);
    assert_eq!(body_to_json(resp).await["error"], "INVALID_TOKEN");
}

#[sqlx::test]
async fn auth_me_and_sessions_report_assurance(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (full_sid, full) = create_session_with_token(&pool, &app.config, user).await;
    let (prob_sid, prob) = create_probation_session_with_token(&pool, &app.config, user).await;

    let (status, me) = get(&app, "/auth/me", &full).await;
    assert_eq!(status, 200, "{me}");
    assert_eq!(me["is_system"], false);
    let s = &me["session"];
    assert_eq!(s["id"], full_sid.to_string());
    assert_eq!(s["assurance"], "full");
    assert_eq!(s["auth_methods"], serde_json::json!(["device_key"]));
    assert_eq!(s["client_class"], "android");
    assert_eq!(s["key_device_id"], test_authenticator_id(user).to_string());
    assert!(s["auth_time"].as_str().is_some());
    assert!(s["absolute_expires_at"].as_str().is_some());

    let (_, me) = get(&app, "/auth/me", &prob).await;
    assert_eq!(me["session"]["assurance"], "probation");
    assert!(me["session"]["key_device_id"].is_null());

    // An ended session is not listed.
    let (ended, _) = create_session_with_token(&pool, &app.config, user).await;
    sqlx::query("UPDATE sessions SET ended_at = NOW(), end_reason = 'logout' WHERE id = $1")
        .bind(ended)
        .execute(&pool)
        .await
        .unwrap();
    let (status, list) = get(&app, "/auth/sessions", &prob).await;
    assert_eq!(status, 200, "{list}");
    let items = list["sessions"]
        .as_array()
        .unwrap_or_else(|| list.as_array().unwrap());
    let ids: Vec<&str> = items.iter().map(|i| i["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&full_sid.to_string().as_str()));
    assert!(ids.contains(&prob_sid.to_string().as_str()));
    assert!(!ids.contains(&ended.to_string().as_str()));
    let f = items
        .iter()
        .find(|i| i["id"] == full_sid.to_string())
        .unwrap();
    assert_eq!(f["assurance"], "full");
    assert_eq!(f["client_class"], "android");
    assert_eq!(f["auth_methods"], serde_json::json!(["device_key"]));
    assert_eq!(
        f["authenticator"]["id"],
        test_authenticator_id(user).to_string()
    );
    assert_eq!(f["authenticator"]["type"], "key_device");
    assert!(f["last_used_at"].as_str().is_some());
    assert!(f["absolute_expires_at"].as_str().is_some());
    let p = items
        .iter()
        .find(|i| i["id"] == prob_sid.to_string())
        .unwrap();
    assert!(p["authenticator"].is_null());
}

#[sqlx::test]
async fn no_geoip_lookup(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (_, username) = password_user(&pool).await;
    let (status, body) = login(&app, &username, None).await;
    assert_eq!(status, 200);
    let sid: Uuid = body["session_id"].as_str().unwrap().parse().unwrap();
    let (city, country, ip): (Option<String>, Option<String>, Option<String>) =
        sqlx::query_as("SELECT city, country, host(ip_address) FROM sessions WHERE id = $1")
            .bind(sid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(city.is_none() && country.is_none());
    assert_eq!(ip.as_deref(), Some("203.0.113.9"));
}

#[sqlx::test]
async fn first_activity_set_on_first_session(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, username) = password_user(&pool).await;
    let before: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT first_activity_at FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(before.is_none());
    assert_eq!(login(&app, &username, None).await.0, 200);
    let first: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT first_activity_at FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(first.is_some());
    assert_eq!(login(&app, &username, None).await.0, 200);
    let again: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT first_activity_at FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(first, again, "set once, never moved");
}

#[sqlx::test]
async fn full_assurance_needs_an_authenticator(pool: PgPool) {
    let (user, _) = create_test_user(&pool).await;
    let res = sqlx::query(
        "INSERT INTO sessions (id, user_id, token_hash, expires_at, assurance)
         VALUES ($1, $2, 'x-full-no-auth', NOW() + INTERVAL '1 day', 'full')",
    )
    .bind(Uuid::new_v4())
    .bind(user)
    .execute(&pool)
    .await;
    assert!(res.is_err());
}

#[sqlx::test]
async fn issue_session_refuses_the_system_account(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let ctx = AuthCtx::from(&app.state);
    let client = SessionClient::default();
    let res = issue_session(
        &ctx,
        &NewSession {
            user_id: SYSTEM_USER_ID,
            method: SignInMethod::LegacyPassword,
            client: &client,
            client_class: ClientClass::Web,
            authenticator: None,
        },
    )
    .await;
    assert!(res.is_err());
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE user_id = $1")
        .bind(SYSTEM_USER_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);

    // A full method without an authenticator is a programming error.
    let (user, _) = create_test_user(&pool).await;
    let res = issue_session(
        &ctx,
        &NewSession {
            user_id: user,
            method: SignInMethod::DeviceKey,
            client: &client,
            client_class: ClientClass::Android,
            authenticator: None,
        },
    )
    .await;
    assert!(res.is_err());
}

/// Review round 3 (Argon coverage, part 4 M2): C27 lists `authenticator:
/// {id, type, label}` for each session; the label is the key device's.
#[sqlx::test]
async fn session_list_names_the_key_device(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev =
        super::helpers::approvals::KeyDeviceFixture::insert(&pool, &app.config, user, "active")
            .await;
    let (status, list) = get(&app, "/auth/sessions", &dev.access_token).await;
    assert_eq!(status, 200, "{list}");
    let item = list["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == dev.session_id.unwrap().to_string())
        .cloned()
        .unwrap();
    assert_eq!(item["authenticator"]["id"], dev.row_id.to_string());
    assert_eq!(item["authenticator"]["label"], "Pixel 8");
}

/// Review round 3 (Argon coverage, part 4 m3): the access token's `sid` is
/// the current session; a refresh cookie of another session does not make
/// that one current too.
#[sqlx::test]
async fn session_list_marks_only_the_tokens_session_current(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (a, token_a) = create_session_with_token(&pool, &app.config, user).await;
    let (b, _) = create_session_with_token(&pool, &app.config, user).await;
    sqlx::query("UPDATE sessions SET token_hash = $2 WHERE id = $1")
        .bind(b)
        .bind(vc_server::auth::hash_token("cookie-of-b"))
        .execute(&pool)
        .await
        .unwrap();
    let req = TestApp::request(Method::GET, "/auth/sessions")
        .header("Authorization", format!("Bearer {token_a}"))
        .header("Cookie", "kaiku_refresh=cookie-of-b")
        .body(Body::empty())
        .unwrap();
    let list = body_to_json(app.oneshot(req).await).await;
    let current: Vec<String> = list["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["is_current"] == true)
        .map(|i| i["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(current, vec![a.to_string()]);
}
