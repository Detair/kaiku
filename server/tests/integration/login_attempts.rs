//! Login attempts (SM1), QR sign-in (F2) and the `login` approval (C26, C27).
//!
//! Run with: `cargo test --test integration login_attempts`

use axum::body::Body;
use axum::http::Method;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::approvals::secrets::{b64u_decode, b64u_encode};
use vc_server::approvals::signing::Decision;

use super::helpers::approvals::KeyDeviceFixture;
use super::helpers::login::{peer_request, TEST_ORIGIN};
use super::helpers::{body_to_json_lenient, create_test_user, TestApp};

/// How the PC proves it owns an attempt.
#[derive(Clone)]
pub(super) enum Secret {
    /// Browser: the attempt cookie (`name=value`).
    Cookie(String),
    /// Native: the PKCE-style verifier.
    Verifier(String),
}

/// A request to the attempt routes as the PC.
pub(super) async fn pc_call(
    app: &TestApp,
    uri: &str,
    secret: &Secret,
    mut body: Value,
) -> (u16, Value, axum::http::HeaderMap) {
    let mut req = peer_request(Method::POST, uri).header("Content-Type", "application/json");
    match secret {
        Secret::Cookie(c) => req = req.header("Cookie", c),
        Secret::Verifier(v) => {
            req = req.header("X-Kaiku-Client", "desktop");
            body["verifier"] = json!(v);
        }
    }
    let resp = app
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await;
    let status = resp.status().as_u16();
    let headers = resp.headers().clone();
    (status, body_to_json_lenient(resp).await, headers)
}

/// A new verifier and its S256 challenge.
pub(super) fn verifier_pair() -> (String, String) {
    let verifier = b64u_encode(&rand::random::<[u8; 32]>());
    let challenge = b64u_encode(&Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

/// `POST /auth/login-attempts` from the desktop app.
pub(super) async fn create_native(app: &TestApp) -> (Value, Secret) {
    let (verifier, challenge) = verifier_pair();
    let req = peer_request(Method::POST, "/auth/login-attempts")
        .header("Content-Type", "application/json")
        .header("X-Kaiku-Client", "desktop")
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Kaiku/1.0",
        )
        .body(Body::from(
            json!({"client_challenge": challenge}).to_string(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status().as_u16(), 201);
    (body_to_json_lenient(resp).await, Secret::Verifier(verifier))
}

/// `POST /auth/login-attempts` from a browser: the cookie comes back.
pub(super) async fn create_web(app: &TestApp) -> (Value, Secret, String) {
    let req = peer_request(Method::POST, "/auth/login-attempts")
        .header("Content-Type", "application/json")
        .header(
            "User-Agent",
            "Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0",
        )
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status().as_u16(), 201);
    let set_cookie = resp
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .find(|c| c.contains("kaiku_attempt="))
        .expect("attempt cookie");
    let pair = set_cookie.split(';').next().unwrap().to_string();
    (
        body_to_json_lenient(resp).await,
        Secret::Cookie(pair),
        set_cookie,
    )
}

/// The QR token of a `LoginAttempt` (b64u and bytes).
pub(super) fn qr_token(attempt: &Value) -> (String, Vec<u8>) {
    let payload = attempt["qr"]["payload"].as_str().expect("qr payload");
    let t = payload.split("&t=").nth(1).expect("token").to_string();
    let bytes = b64u_decode(&t).unwrap();
    (t, bytes)
}

pub(super) fn attempt_id(attempt: &Value) -> String {
    attempt["attempt_id"].as_str().unwrap().to_string()
}

/// `POST /api/approvals/claim {token}` from the key device.
pub(super) async fn claim(app: &TestApp, dev: &KeyDeviceFixture, token: &str) -> (u16, Value) {
    let req = TestApp::request(Method::POST, "/api/approvals/claim")
        .header("Authorization", format!("Bearer {}", dev.access_token))
        .header("Content-Type", "application/json")
        .body(Body::from(json!({ "token": token }).to_string()))
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    (status, body_to_json_lenient(resp).await)
}

/// Sign and answer a `login` challenge.
pub(super) async fn answer_login(
    app: &TestApp,
    dev: &KeyDeviceFixture,
    challenge: &Value,
    response: &[u8],
    decision: Decision,
) -> (u16, Value) {
    let prefix = b64u_decode(challenge["signing_prefix"].as_str().unwrap()).unwrap();
    let sig = dev.sign(&prefix, decision, response);
    let id = challenge["challenge_id"].as_str().unwrap();
    let decision = match decision {
        Decision::Report => "report",
        _ => "approve",
    };
    let req = TestApp::request(Method::POST, &format!("/api/approvals/{id}/answer"))
        .header("Authorization", format!("Bearer {}", dev.access_token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({"decision": decision, "response": b64u_encode(response), "signature": sig})
                .to_string(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    (status, body_to_json_lenient(resp).await)
}

async fn device_post(app: &TestApp, dev: &KeyDeviceFixture, uri: &str) -> u16 {
    let req = TestApp::request(Method::POST, uri)
        .header("Authorization", format!("Bearer {}", dev.access_token))
        .body(Body::empty())
        .unwrap();
    app.oneshot(req).await.status().as_u16()
}

/// Create (native), claim and approve: the attempt is `approved`.
pub(super) async fn approved_attempt(app: &TestApp, dev: &KeyDeviceFixture) -> (String, Secret) {
    let (attempt, secret) = create_native(app).await;
    let (token, bytes) = qr_token(&attempt);
    let (s, c) = claim(app, dev, &token).await;
    assert_eq!(s, 201, "{c}");
    let (s, a) = answer_login(app, dev, &c, &bytes, Decision::Approve).await;
    assert_eq!((s, a["state"].as_str()), (200, Some("approved")), "{a}");
    (attempt_id(&attempt), secret)
}

async fn attempt_state(pool: &PgPool, id: &str) -> String {
    sqlx::query_scalar("SELECT state FROM login_attempts WHERE id = $1::uuid")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn create_returns_the_qr_payload_and_cookie(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (attempt, _, set_cookie) = create_web(&app).await;
    assert_eq!(attempt["kind"], "qr");
    assert_eq!(attempt["state"], "open");
    assert_eq!(attempt["totp_available"], false);
    let payload = attempt["qr"]["payload"].as_str().unwrap();
    let prefix = "kaiku://login/v1?o=https%3A%2F%2Fchat.example.com&t=";
    assert!(payload.starts_with(prefix), "{payload}");
    assert_eq!(payload.len(), prefix.len() + 43);
    assert!(attempt["qr"]["expires_at"].is_string());
    for attr in ["HttpOnly", "SameSite=Strict", "Path=/auth/login-attempts"] {
        assert!(set_cookie.contains(attr), "{set_cookie}");
    }
    // The secret appears nowhere but in the cookie.
    assert!(!attempt.to_string().contains(
        set_cookie
            .split(';')
            .next()
            .unwrap()
            .split('=')
            .nth(1)
            .unwrap()
    ));

    // Native: client_challenge is required (else CLIENT_OUTDATED) and a
    // browser must not send one (VALIDATION_ERROR).
    let req = peer_request(Method::POST, "/auth/login-attempts")
        .header("Content-Type", "application/json")
        .header("X-Kaiku-Client", "android")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status().as_u16(), 400);
    assert_eq!(body_to_json_lenient(resp).await["error"], "CLIENT_OUTDATED");
    let (_, challenge) = verifier_pair();
    let req = peer_request(Method::POST, "/auth/login-attempts")
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({"client_challenge": challenge}).to_string(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status().as_u16(), 400);
    assert_eq!(
        body_to_json_lenient(resp).await["error"],
        "VALIDATION_ERROR"
    );
    let (native, _) = create_native(&app).await;
    assert!(native["qr"]["payload"].is_string());
}

/// One live attempt per screen: a create that carries the attempt cookie
/// cancels the previous attempt.
#[sqlx::test]
async fn a_new_create_cancels_the_screens_old_attempt(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (first, Secret::Cookie(cookie), _) = create_web(&app).await else {
        unreachable!()
    };
    let req = peer_request(Method::POST, "/auth/login-attempts")
        .header("Content-Type", "application/json")
        .header("Cookie", &cookie)
        .body(Body::from("{}"))
        .unwrap();
    assert_eq!(app.oneshot(req).await.status().as_u16(), 201);
    assert_eq!(attempt_state(&pool, &attempt_id(&first)).await, "cancelled");
}

#[sqlx::test]
async fn poll_rotates_after_60s_and_after_a_claim(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (attempt, secret) = create_native(&app).await;
    let id = attempt_id(&attempt);
    let (old, _) = qr_token(&attempt);

    // A fresh token is not rotated (and not sent again).
    let (s, polled, _) = pc_call(
        &app,
        &format!("/auth/login-attempts/{id}/poll"),
        &secret,
        json!({}),
    )
    .await;
    assert_eq!(s, 200, "{polled}");
    assert!(polled["qr"].is_null(), "{polled}");

    // ≥ 60 s old → a new token; the old one is stale.
    sqlx::query(
        "UPDATE login_attempts SET qr_token_at = qr_token_at - INTERVAL '61 seconds'
          WHERE id = $1::uuid",
    )
    .bind(&id)
    .execute(&pool)
    .await
    .unwrap();
    let (_, polled, _) = pc_call(
        &app,
        &format!("/auth/login-attempts/{id}/poll"),
        &secret,
        json!({}),
    )
    .await;
    let (new, _) = qr_token(&polled);
    assert_ne!(new, old);
    let (s, body) = claim(&app, &dev, &old).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (404, Some("LOGIN_CODE_STALE")),
        "{body}"
    );

    // A claim uses the token: after a release the next poll mints one.
    let (s, c) = claim(&app, &dev, &new).await;
    assert_eq!(s, 201, "{c}");
    let cid = c["challenge_id"].as_str().unwrap();
    assert_eq!(
        device_post(&app, &dev, &format!("/api/approvals/{cid}/release")).await,
        204
    );
    let (_, polled, _) = pc_call(
        &app,
        &format!("/auth/login-attempts/{id}/poll"),
        &secret,
        json!({}),
    )
    .await;
    assert_eq!(polled["state"], "open");
    let (third, _) = qr_token(&polled);
    assert_ne!(third, new);
}

#[sqlx::test]
async fn claim_creates_the_login_challenge(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, username) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (attempt, _) = create_native(&app).await;
    let (token, _) = qr_token(&attempt);
    let (s, c) = claim(&app, &dev, &token).await;
    assert_eq!(s, 201, "{c}");
    assert_eq!(c["kind"], "login");
    assert_eq!(c["variant"], "qr");
    let ctx = &c["context"];
    assert_eq!(ctx["attempt_id"], attempt["attempt_id"]);
    assert_eq!(ctx["variant"], "qr");
    assert_eq!(ctx["client_class"], "desktop");
    assert_eq!(ctx["client_label"], "Kaiku desktop on Windows 10");
    let server_name = {
        let mut conn = pool.acquire().await.unwrap();
        vc_server::approvals::server_name(&mut conn).await.unwrap()
    };
    assert_eq!(ctx["server_name"], server_name.as_str());
    assert_eq!(ctx["place"], "");
    assert_eq!(ctx["username"], username);
    assert_eq!(ctx["collection_hash"].as_str().unwrap().len(), 43);
    let expires: chrono::DateTime<chrono::Utc> = c["expires_at"].as_str().unwrap().parse().unwrap();
    let ttl = expires - chrono::Utc::now();
    assert!(ttl <= chrono::Duration::seconds(120) && ttl > chrono::Duration::seconds(100));
    assert_eq!(attempt_state(&pool, &attempt_id(&attempt)).await, "claimed");
    // A second scan of the same code: already claimed.
    let other = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (s, body) = claim(&app, &other, &token).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (409, Some("LOGIN_ATTEMPT_CLAIMED"))
    );
}

#[sqlx::test]
async fn claim_needs_may_approve(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    sqlx::query("UPDATE users SET may_approve = false WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let (attempt, _) = create_native(&app).await;
    let (token, _) = qr_token(&attempt);
    let (s, body) = claim(&app, &dev, &token).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (403, Some("APPROVAL_NOT_ALLOWED"))
    );
    assert_eq!(attempt_state(&pool, &attempt_id(&attempt)).await, "open");
}

#[sqlx::test]
async fn may_approve_turned_off_before_approve_is_refused(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (attempt, _) = create_native(&app).await;
    let (token, bytes) = qr_token(&attempt);
    let (_, c) = claim(&app, &dev, &token).await;
    sqlx::query("UPDATE users SET may_approve = false WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let (s, body) = answer_login(&app, &dev, &c, &bytes, Decision::Approve).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (403, Some("APPROVAL_NOT_ALLOWED"))
    );
    assert_ne!(
        attempt_state(&pool, &attempt_id(&attempt)).await,
        "approved"
    );
}

#[sqlx::test]
async fn stale_token_creates_nothing(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let unknown = b64u_encode(&rand::random::<[u8; 32]>());
    let (s, body) = claim(&app, &dev, &unknown).await;
    assert_eq!((s, body["error"].as_str()), (404, Some("LOGIN_CODE_STALE")));
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM approvals WHERE kind = 'login'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test]
async fn concurrent_claims_one_wins(pool: PgPool) {
    let app = std::sync::Arc::new(TestApp::with_pool(pool.clone()).await);
    let (attempt, _) = create_native(&app).await;
    let (token, _) = qr_token(&attempt);
    let mut devices = Vec::new();
    for _ in 0..25 {
        let (user, _) = create_test_user(&pool).await;
        devices.push(KeyDeviceFixture::insert(&pool, &app.config, user, "active").await);
    }
    let mut tasks = Vec::new();
    for dev in devices {
        let app = app.clone();
        let token = token.clone();
        tasks.push(tokio::spawn(
            async move { claim(&app, &dev, &token).await.0 },
        ));
    }
    let mut codes = Vec::new();
    for t in tasks {
        codes.push(t.await.unwrap());
    }
    assert_eq!(codes.iter().filter(|c| **c == 201).count(), 1, "{codes:?}");
    assert!(
        codes.iter().all(|c| *c == 201 || *c == 409 || *c == 404),
        "{codes:?}"
    );
}

#[sqlx::test]
async fn response_is_compared_first(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (attempt, _) = create_native(&app).await;
    let (token, _) = qr_token(&attempt);
    let (_, c) = claim(&app, &dev, &token).await;
    // A valid signature over the wrong response: refused before the
    // signature, nothing changes.
    let (s, body) = answer_login(&app, &dev, &c, &[7u8; 32], Decision::Approve).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (400, Some("APPROVAL_RESPONSE_MISMATCH"))
    );
    assert_eq!(attempt_state(&pool, &attempt_id(&attempt)).await, "claimed");
    let state: String = sqlx::query_scalar("SELECT state FROM approvals WHERE id = $1::uuid")
        .bind(c["challenge_id"].as_str().unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "issued");
}

#[sqlx::test]
async fn approve_then_collect_mints_probation(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (id, secret) = approved_attempt(&app, &dev).await;
    let (s, auth, _) = pc_call(
        &app,
        &format!("/auth/login-attempts/{id}/collect"),
        &secret,
        json!({}),
    )
    .await;
    assert_eq!(s, 200, "{auth}");
    assert_eq!(auth["assurance"], "probation");
    assert!(
        auth["refresh_token"].is_string(),
        "native gets the refresh token"
    );
    let methods: Vec<String> =
        sqlx::query_scalar("SELECT auth_methods FROM sessions WHERE id = $1::uuid")
            .bind(auth["session_id"].as_str().unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(methods, vec!["qr".to_string()]);
    let details: Vec<Value> = sqlx::query_scalar(
        "SELECT details FROM security_events WHERE subject_id = $1 AND event_type = 'sign_in'",
    )
    .bind(user)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        details,
        vec![json!({"auth_methods": ["qr"], "assurance": "probation"})]
    );
    assert_eq!(attempt_state(&pool, &id).await, "consumed");
}

/// Review Focus 2: a PC that lost the collect response retries and learns
/// what happened; it never gets a second session.
#[sqlx::test]
async fn collect_retry_answers_ended_consumed(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (id, secret) = approved_attempt(&app, &dev).await;
    let uri = format!("/auth/login-attempts/{id}/collect");
    let (s, _, _) = pc_call(&app, &uri, &secret, json!({})).await;
    assert_eq!(s, 200);
    let (s, body, _) = pc_call(&app, &uri, &secret, json!({})).await;
    assert_eq!(
        (s, body["error"].as_str(), body["state"].as_str()),
        (410, Some("LOGIN_ATTEMPT_ENDED"), Some("consumed")),
        "{body}"
    );
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sessions WHERE user_id = $1 AND 'qr' = ANY(auth_methods)",
    )
    .bind(user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 1);
}

#[sqlx::test]
async fn bystander_without_secret_gets_unknown(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (id, _) = approved_attempt(&app, &dev).await;
    let (other_verifier, _) = verifier_pair();
    for secret in [
        Secret::Verifier(other_verifier),
        Secret::Cookie(format!(
            "__Secure-kaiku_attempt={}",
            b64u_encode(&[1u8; 32])
        )),
    ] {
        for route in ["collect", "poll", "cancel"] {
            let (s, body, _) = pc_call(
                &app,
                &format!("/auth/login-attempts/{id}/{route}"),
                &secret,
                json!({}),
            )
            .await;
            assert_eq!(
                (s, body["error"].as_str()),
                (404, Some("LOGIN_ATTEMPT_UNKNOWN")),
                "{route}"
            );
        }
    }
    assert_eq!(attempt_state(&pool, &id).await, "approved");
}

#[sqlx::test]
async fn collect_after_the_approver_is_revoked_is_refused(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (id, secret) = approved_attempt(&app, &dev).await;
    sqlx::query("UPDATE approval_devices SET state = 'revoked', revoked_at = NOW(), revoke_reason = 'owner' WHERE id = $1")
        .bind(dev.row_id)
        .execute(&pool)
        .await
        .unwrap();
    let (s, body, _) = pc_call(
        &app,
        &format!("/auth/login-attempts/{id}/collect"),
        &secret,
        json!({}),
    )
    .await;
    assert_eq!(s, 410, "{body}");
    assert_eq!(body["error"], "LOGIN_ATTEMPT_ENDED");
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sessions WHERE user_id = $1 AND 'qr' = ANY(auth_methods)",
    )
    .bind(user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test]
async fn deny_report_release_cancel(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;

    // Deny.
    let (a, _) = create_native(&app).await;
    let (_, c) = claim(&app, &dev, &qr_token(&a).0).await;
    let cid = c["challenge_id"].as_str().unwrap();
    assert_eq!(
        device_post(&app, &dev, &format!("/api/approvals/{cid}/deny")).await,
        204
    );
    assert_eq!(attempt_state(&pool, &attempt_id(&a)).await, "denied");

    // Report: QR compromises nothing, records approval_reported.
    let (a, _) = create_native(&app).await;
    let (t, bytes) = qr_token(&a);
    let (_, c) = claim(&app, &dev, &t).await;
    let (s, r) = answer_login(&app, &dev, &c, &bytes, Decision::Report).await;
    assert_eq!((s, r["state"].as_str()), (200, Some("reported")), "{r}");
    assert_eq!(attempt_state(&pool, &attempt_id(&a)).await, "reported");
    let reported: Vec<Value> = sqlx::query_scalar(
        "SELECT details FROM security_events WHERE subject_id = $1 AND event_type = 'approval_reported'",
    )
    .bind(user)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(reported, vec![json!({"attempt_kind": "qr"})]);
    let state: String = sqlx::query_scalar("SELECT password_state FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "active");

    // Release → open again.
    let (a, secret) = create_native(&app).await;
    let (_, c) = claim(&app, &dev, &qr_token(&a).0).await;
    let cid = c["challenge_id"].as_str().unwrap();
    assert_eq!(
        device_post(&app, &dev, &format!("/api/approvals/{cid}/release")).await,
        204
    );
    assert_eq!(attempt_state(&pool, &attempt_id(&a)).await, "open");

    // Cancel by the PC.
    let id = attempt_id(&a);
    let (s, _, _) = pc_call(
        &app,
        &format!("/auth/login-attempts/{id}/cancel"),
        &secret,
        json!({}),
    )
    .await;
    assert_eq!(s, 204);
    assert_eq!(attempt_state(&pool, &id).await, "cancelled");
}

/// Review Focus 1: a slow approver finds the attempt open again.
#[sqlx::test]
async fn slow_approval_after_challenge_expiry_returns_attempt_to_open(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (a, secret) = create_native(&app).await;
    let id = attempt_id(&a);
    let (t, bytes) = qr_token(&a);
    let (_, c) = claim(&app, &dev, &t).await;
    // Two minutes pass on the context screen.
    sqlx::query(
        "UPDATE approvals SET expires_at = NOW() - INTERVAL '1 second' WHERE id = $1::uuid",
    )
    .bind(c["challenge_id"].as_str().unwrap())
    .execute(&pool)
    .await
    .unwrap();
    let (s, body) = answer_login(&app, &dev, &c, &bytes, Decision::Approve).await;
    assert_eq!((s, body["error"].as_str()), (410, Some("APPROVAL_EXPIRED")));
    let (s, polled, _) = pc_call(
        &app,
        &format!("/auth/login-attempts/{id}/poll"),
        &secret,
        json!({}),
    )
    .await;
    assert_eq!(s, 200);
    assert_eq!(polled["state"], "open");
    let (fresh, _) = qr_token(&polled);
    let (s, c2) = claim(&app, &dev, &fresh).await;
    assert_eq!(s, 201, "{c2}");
}

#[sqlx::test]
async fn attempt_expires_after_10_minutes(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (a, secret) = create_native(&app).await;
    let id = attempt_id(&a);
    let expires: chrono::DateTime<chrono::Utc> = a["expires_at"].as_str().unwrap().parse().unwrap();
    let ttl = expires - chrono::Utc::now();
    assert!(ttl <= chrono::Duration::minutes(10) && ttl > chrono::Duration::minutes(9));
    sqlx::query(
        "UPDATE login_attempts SET created_at = created_at - INTERVAL '11 minutes',
                expires_at = expires_at - INTERVAL '11 minutes' WHERE id = $1::uuid",
    )
    .bind(&id)
    .execute(&pool)
    .await
    .unwrap();
    vc_server::auth::timers::run_once(&app.state).await.unwrap();
    assert_eq!(attempt_state(&pool, &id).await, "expired");
    let (s, polled, _) = pc_call(
        &app,
        &format!("/auth/login-attempts/{id}/poll"),
        &secret,
        json!({}),
    )
    .await;
    assert_eq!((s, polled["state"].as_str()), (200, Some("expired")));
}

#[sqlx::test]
async fn approve_and_cancel_race(pool: PgPool) {
    let app = std::sync::Arc::new(TestApp::with_pool(pool.clone()).await);
    let (user, _) = create_test_user(&pool).await;
    let dev =
        std::sync::Arc::new(KeyDeviceFixture::insert(&pool, &app.config, user, "active").await);
    for _ in 0..25 {
        let (a, secret) = create_native(&app).await;
        let id = attempt_id(&a);
        let (t, bytes) = qr_token(&a);
        let (_, c) = claim(&app, &dev, &t).await;
        let approve = {
            let (app, dev) = (app.clone(), dev.clone());
            tokio::spawn(async move {
                answer_login(&app, &dev, &c, &bytes, Decision::Approve)
                    .await
                    .0
            })
        };
        let cancel = {
            let app = app.clone();
            let id = id.clone();
            tokio::spawn(async move {
                pc_call(
                    &app,
                    &format!("/auth/login-attempts/{id}/cancel"),
                    &secret,
                    json!({}),
                )
                .await
                .0
            })
        };
        let (a, c) = (approve.await.unwrap(), cancel.await.unwrap());
        assert_eq!(c, 204);
        let state = attempt_state(&pool, &id).await;
        match a {
            200 => assert!(state == "approved" || state == "cancelled", "{state}"),
            _ => assert_eq!(state, "cancelled"),
        }
    }
}

#[sqlx::test]
async fn collect_race_one_session(pool: PgPool) {
    let app = std::sync::Arc::new(TestApp::with_pool(pool.clone()).await);
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (id, secret) = approved_attempt(&app, &dev).await;
    let mut tasks = Vec::new();
    for _ in 0..25 {
        let (app, secret, id) = (app.clone(), secret.clone(), id.clone());
        tasks.push(tokio::spawn(async move {
            pc_call(
                &app,
                &format!("/auth/login-attempts/{id}/collect"),
                &secret,
                json!({}),
            )
            .await
            .0
        }));
    }
    let mut codes = Vec::new();
    for t in tasks {
        codes.push(t.await.unwrap());
    }
    assert_eq!(codes.iter().filter(|c| **c == 200).count(), 1, "{codes:?}");
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sessions WHERE user_id = $1 AND 'qr' = ANY(auth_methods)",
    )
    .bind(user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 1);
}

#[sqlx::test]
async fn long_poll_returns_early(pool: PgPool) {
    let app = std::sync::Arc::new(TestApp::with_pool(pool.clone()).await);
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (a, secret) = create_native(&app).await;
    let id = attempt_id(&a);
    let (t, _) = qr_token(&a);
    let poller = {
        let app = app.clone();
        tokio::spawn(async move {
            let start = std::time::Instant::now();
            let (_, body, _) = pc_call(
                &app,
                &format!("/auth/login-attempts/{id}/poll"),
                &secret,
                json!({"wait": 25}),
            )
            .await;
            (start.elapsed(), body)
        })
    };
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let claimed_at = std::time::Instant::now();
    let (s, _) = claim(&app, &dev, &t).await;
    assert_eq!(s, 201);
    let (elapsed, body) = poller.await.unwrap();
    assert_eq!(body["state"], "claimed", "{body}");
    assert!(elapsed < std::time::Duration::from_secs(5), "{elapsed:?}");
    assert!(claimed_at.elapsed() < std::time::Duration::from_secs(2));
}

/// Review Focus 3: attempts live in PostgreSQL; a Valkey outage between
/// approve and collect changes nothing for the flow.
#[sqlx::test]
async fn valkey_flush_mid_attempt_still_collects(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (id, Secret::Verifier(verifier)) = approved_attempt(&app, &dev).await else {
        unreachable!()
    };
    let failing = super::helpers::login::failing_valkey(&app.config).await;
    let state =
        super::helpers::login::state_with_redis(pool.clone(), (*app.config).clone(), failing);
    let router = vc_server::api::create_router(state);
    let req = peer_request(Method::POST, &format!("/auth/login-attempts/{id}/collect"))
        .header("Content-Type", "application/json")
        .header("X-Kaiku-Client", "desktop")
        .body(Body::from(json!({ "verifier": verifier }).to_string()))
        .unwrap();
    let resp = tower::ServiceExt::oneshot(router, req).await.unwrap();
    assert_eq!(resp.status().as_u16(), 200);
}

/// Review Focus 4: the JSON context hashes to the prefix's context hash,
/// with a server name and a client label beyond ASCII.
#[sqlx::test]
async fn context_json_round_trips_exact_utf8(pool: PgPool) {
    use vc_server::approvals::context::CanonicalContext;
    use vc_server::approvals::signing::Kind;
    let app = TestApp::with_pool(pool.clone()).await;
    sqlx::query(
        "INSERT INTO server_config (key, value) VALUES ('server_name', '\"Familie Müller\"')
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
    )
    .execute(&pool)
    .await
    .unwrap();
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (verifier, challenge) = verifier_pair();
    let req = peer_request(Method::POST, "/auth/login-attempts")
        .header("Content-Type", "application/json")
        .header("X-Kaiku-Client", "desktop")
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0) Kaiku 🦊 Ünïcode",
        )
        .body(Body::from(
            json!({"client_challenge": challenge}).to_string(),
        ))
        .unwrap();
    let a = body_to_json_lenient(app.oneshot(req).await).await;
    let _ = verifier;
    let (_, c) = claim(&app, &dev, &qr_token(&a).0).await;
    assert_eq!(c["context"]["server_name"], "Familie Müller");
    let parsed = CanonicalContext::from_json(Kind::Login, &c["context"]).unwrap();
    let prefix = b64u_decode(c["signing_prefix"].as_str().unwrap()).unwrap();
    assert_eq!(&prefix[prefix.len() - 32..], parsed.hash().as_slice());
}

/// Review Focus 5: a hostile User-Agent becomes a bounded, clean label.
#[sqlx::test]
async fn client_label_from_user_agent_is_bounded(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let ua = format!("Mozilla/5.0 \u{202e}evil\t {}", "x".repeat(2048));
    let req = peer_request(Method::POST, "/auth/login-attempts")
        .header("Content-Type", "application/json")
        .header("User-Agent", ua)
        .body(Body::from("{}"))
        .unwrap();
    let a = body_to_json_lenient(app.oneshot(req).await).await;
    let (_, c) = claim(&app, &dev, &qr_token(&a).0).await;
    let label = c["context"]["client_label"].as_str().unwrap();
    assert!(label.chars().count() <= 64, "{label}");
    assert!(
        !label.chars().any(|ch| ch.is_control() || ch == '\u{202e}'),
        "{label:?}"
    );
}

#[sqlx::test]
async fn public_create_is_never_ip_blocked(pool: PgPool) {
    use vc_server::ratelimit::{RateLimitConfig, RateLimiter, RateLimits};
    let app = TestApp::with_pool(pool.clone()).await;
    let mut limiter = RateLimiter::new(
        app.state.redis.clone(),
        RateLimitConfig {
            enabled: true,
            redis_key_prefix: format!("test:rl:{}", Uuid::new_v4()),
            fail_open: false,
            trusted_proxies: vc_server::ratelimit::TrustedProxies::default(),
            allowlist: std::collections::HashSet::new(),
            limits: RateLimits::default(),
        },
    );
    limiter.init().await.unwrap();
    for _ in 0..20 {
        let _ = limiter.record_failed_auth("203.0.113.9").await;
    }
    assert!(limiter.is_blocked("203.0.113.9").await.unwrap());
    let mut state = super::helpers::login::state_with_redis(
        pool.clone(),
        (*app.config).clone(),
        app.state.redis.clone(),
    );
    state.rate_limiter = Some(limiter);
    let router = vc_server::api::create_router(state);
    let req = peer_request(Method::POST, "/auth/login-attempts")
        .header("Content-Type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp = tower::ServiceExt::oneshot(router, req).await.unwrap();
    assert_eq!(resp.status().as_u16(), 201);
}

#[sqlx::test]
async fn old_qr_routes_are_gone(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let token = super::helpers::generate_access_token(&app.config, user);
    for uri in ["/auth/qr/create", "/auth/qr/redeem"] {
        let req = peer_request(Method::POST, uri)
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .body(Body::from(json!({"token": "x"}).to_string()))
            .unwrap();
        assert_eq!(app.oneshot(req).await.status().as_u16(), 404, "{uri}");
    }
    let _ = TEST_ORIGIN;
}
