//! The approvals core and device step-up (spec B.2, C26).
//!
//! Run with: `cargo test --test integration approvals_core`

use axum::body::Body;
use axum::http::Method;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::approvals::context::{CanonicalContext, LoginContext, Variant};
use vc_server::approvals::core::{expire_due, issue, NewChallenge, Requester};
use vc_server::approvals::secrets::b64u_decode;
use vc_server::approvals::signing::{Decision, Kind};
use vc_server::auth::client_class::ClientClass;
use vc_server::auth::ctx::AuthCtx;

use super::helpers::approvals::KeyDeviceFixture;
use super::helpers::{body_to_json, create_session_with_token, create_test_user, TestApp};

pub(super) async fn post(app: &TestApp, uri: &str, token: &str, body: Value) -> (u16, Value) {
    let req = TestApp::request(Method::POST, uri)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    (status, body_to_json(resp).await)
}

/// Ask for a `device_stepup` challenge from the device's own session.
pub(super) async fn stepup_challenge(
    app: &TestApp,
    dev: &KeyDeviceFixture,
    action: &str,
    target: &str,
) -> Value {
    let (status, body) = post(
        app,
        "/api/approvals/step-up",
        &dev.access_token,
        json!({"action": action, "target": target}),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    body
}

/// Sign and answer a challenge with `dev`.
pub(super) async fn answer(
    app: &TestApp,
    dev: &KeyDeviceFixture,
    challenge: &Value,
    response: &[u8],
) -> (u16, Value) {
    let prefix = b64u_decode(challenge["signing_prefix"].as_str().unwrap()).unwrap();
    let sig = dev.sign(&prefix, Decision::Approve, response);
    let id = challenge["challenge_id"].as_str().unwrap();
    post(
        app,
        &format!("/api/approvals/{id}/answer"),
        &dev.access_token,
        json!({"decision": "approve", "response": vc_server::approvals::secrets::b64u_encode(response), "signature": sig}),
    )
    .await
}

#[sqlx::test]
async fn step_up_challenge_is_canonical(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let other = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let c = stepup_challenge(&app, &dev, "key_device.remove", &other.row_id.to_string()).await;
    assert_eq!(c["kind"], "device_stepup");
    assert!(c["variant"].is_null());
    let ctx = &c["context"];
    assert_eq!(ctx["sid"], dev.session_id.unwrap().to_string());
    assert_eq!(ctx["action"], "key_device.remove");
    assert_eq!(ctx["target"], other.row_id.to_string());
    assert_eq!(ctx["target_label"], "Pixel 8");

    // The prefix parses to the stored row: magic, kind, id, origin, user,
    // device, and the hash of the context sent.
    let prefix = b64u_decode(c["signing_prefix"].as_str().unwrap()).unwrap();
    assert_eq!(&prefix[..12], b"kaiku-sig-v1");
    assert_eq!(prefix[12], Kind::DeviceStepup as u8);
    let id: Uuid = c["challenge_id"].as_str().unwrap().parse().unwrap();
    assert_eq!(&prefix[13..29], id.as_bytes());
    let olen = u32::from_be_bytes(prefix[69..73].try_into().unwrap()) as usize;
    assert_eq!(&prefix[73..73 + olen], b"https://chat.example.com");
    let rest = &prefix[73 + olen..];
    assert_eq!(&rest[..16], user.as_bytes());
    assert_eq!(&rest[16..32], dev.device_id.as_bytes());
    let parsed = CanonicalContext::from_json(Kind::DeviceStepup, ctx).unwrap();
    assert_eq!(&rest[32..64], &parsed.hash());
    let expires: chrono::DateTime<chrono::Utc> = c["expires_at"].as_str().unwrap().parse().unwrap();
    assert!(expires <= chrono::Utc::now() + chrono::Duration::seconds(121));
    let exp_secs = i64::from_be_bytes(prefix[61..69].try_into().unwrap());
    assert_eq!(exp_secs, expires.timestamp());
}

#[sqlx::test]
async fn valid_answer_returns_a_grant(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let other = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let c = stepup_challenge(&app, &dev, "key_device.remove", &other.row_id.to_string()).await;
    let (status, body) = answer(&app, &dev, &c, b"").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["state"], "consumed");
    assert_eq!(body["grant"]["action"], "key_device.remove");
    assert_eq!(body["grant"]["target"], other.row_id.to_string());
    assert_eq!(body["grant"]["via"], "device");
    assert!(body["grant"]["expires_at"].as_str().is_some());
}

#[sqlx::test]
async fn answer_retry_creates_no_second_grant(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let c = stepup_challenge(&app, &dev, "admin.elevate", "").await;
    assert_eq!(answer(&app, &dev, &c, b"").await.0, 200);
    let (status, body) = answer(&app, &dev, &c, b"").await;
    assert_eq!(
        (status, body["error"].as_str()),
        (409, Some("APPROVAL_STATE"))
    );
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM step_up_grants WHERE user_id = $1")
        .bind(user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
}

#[sqlx::test]
async fn concurrent_answers_one_wins(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    for _ in 0..25 {
        let c = stepup_challenge(&app, &dev, "admin.elevate", "").await;
        let (a, b) = tokio::join!(answer(&app, &dev, &c, b""), answer(&app, &dev, &c, b""));
        let mut statuses = [a.0, b.0];
        statuses.sort_unstable();
        assert_eq!(statuses, [200, 409], "{} / {}", a.1, b.1);
    }
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM step_up_grants WHERE user_id = $1")
        .bind(user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 25);
}

async fn age_challenge(pool: &PgPool, id: &str) {
    sqlx::query(
        "UPDATE approvals SET created_at = created_at - INTERVAL '5 minutes',
                              expires_at = created_at - INTERVAL '4 minutes'
          WHERE id = $1",
    )
    .bind(Uuid::parse_str(id).unwrap())
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test]
async fn expired_challenge_is_gone(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let c = stepup_challenge(&app, &dev, "admin.elevate", "").await;
    age_challenge(&pool, c["challenge_id"].as_str().unwrap()).await;
    let (status, body) = answer(&app, &dev, &c, b"").await;
    assert_eq!(
        (status, body["error"].as_str()),
        (410, Some("APPROVAL_EXPIRED"))
    );

    let d = stepup_challenge(&app, &dev, "admin.elevate", "").await;
    age_challenge(&pool, d["challenge_id"].as_str().unwrap()).await;
    let mut conn = pool.acquire().await.unwrap();
    assert!(expire_due(&mut conn).await.unwrap() >= 1);
    let state: String = sqlx::query_scalar("SELECT state FROM approvals WHERE id = $1")
        .bind(Uuid::parse_str(d["challenge_id"].as_str().unwrap()).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "expired");
}

#[sqlx::test]
async fn answer_and_sweep_race(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    for _ in 0..25 {
        let c = stepup_challenge(&app, &dev, "admin.elevate", "").await;
        let id = Uuid::parse_str(c["challenge_id"].as_str().unwrap()).unwrap();
        // Due right now.
        sqlx::query(
            "UPDATE approvals SET expires_at = NOW() + INTERVAL '50 milliseconds' WHERE id = $1",
        )
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let sweep = async {
            let mut conn = pool.acquire().await.unwrap();
            expire_due(&mut conn).await.unwrap()
        };
        let (ans, _) = tokio::join!(answer(&app, &dev, &c, b""), sweep);
        let state: String = sqlx::query_scalar("SELECT state FROM approvals WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
        let grants: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM step_up_grants WHERE user_id = $1 AND created_at > NOW() - INTERVAL '2 seconds'",
        )
        .bind(user)
        .fetch_one(&pool)
        .await
        .unwrap();
        match (ans.0, state.as_str()) {
            (200, "consumed") => assert!(grants >= 1),
            (410 | 409, "expired") => {}
            other => panic!("inconsistent outcome {other:?}: {}", ans.1),
        }
    }
}

#[sqlx::test]
async fn other_device_sees_not_found(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let other = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let c = stepup_challenge(&app, &dev, "admin.elevate", "").await;
    let (status, body) = answer(&app, &other, &c, b"").await;
    assert_eq!(
        (status, body["error"].as_str()),
        (404, Some("APPROVAL_NOT_FOUND"))
    );
    let (status, _) = post(
        &app,
        &format!("/api/approvals/{}/answer", Uuid::new_v4()),
        &dev.access_token,
        json!({"decision": "approve", "response": "", "signature": "AA"}),
    )
    .await;
    assert_eq!(status, 404);
}

#[sqlx::test]
async fn bad_signature_keeps_the_challenge_issued(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let stranger = KeyDeviceFixture::insert(&pool, &app.config, user, "pending").await;
    let c = stepup_challenge(&app, &dev, "admin.elevate", "").await;
    let prefix = b64u_decode(c["signing_prefix"].as_str().unwrap()).unwrap();
    let wrong_key = stranger.sign(&prefix, Decision::Approve, b"");
    let id = c["challenge_id"].as_str().unwrap();
    let (status, body) = post(
        &app,
        &format!("/api/approvals/{id}/answer"),
        &dev.access_token,
        json!({"decision": "approve", "response": "", "signature": wrong_key}),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (400, Some("SIGNATURE_INVALID"))
    );
    // A report decision is not allowed for a proof.
    let (status, _) = post(
        &app,
        &format!("/api/approvals/{id}/answer"),
        &dev.access_token,
        json!({"decision": "report", "response": "", "signature": wrong_key}),
    )
    .await;
    assert_eq!(status, 400);
    assert_eq!(answer(&app, &dev, &c, b"").await.0, 200, "still issued");
}

#[sqlx::test]
async fn non_key_device_sessions_are_refused(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (_, token) = create_session_with_token(&pool, &app.config, user).await;
    let (status, body) = post(
        &app,
        "/api/approvals/step-up",
        &token,
        json!({"action": "admin.elevate", "target": ""}),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (403, Some("KEY_DEVICE_REQUIRED"))
    );
}

#[sqlx::test]
async fn pending_and_revoked_devices_cannot_answer(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let c = stepup_challenge(&app, &dev, "admin.elevate", "").await;
    for state in ["pending", "revoked"] {
        sqlx::query(
            "UPDATE approval_devices SET state = $2,
                    revoked_at = CASE WHEN $2 = 'revoked' THEN NOW() END,
                    revoke_reason = CASE WHEN $2 = 'revoked' THEN 'owner' END
              WHERE id = $1",
        )
        .bind(dev.row_id)
        .bind(state)
        .execute(&pool)
        .await
        .unwrap();
        let (status, body) = answer(&app, &dev, &c, b"").await;
        assert_eq!(
            (status, body["error"].as_str()),
            (403, Some("KEY_DEVICE_REQUIRED")),
            "{state}"
        );
    }
}

#[sqlx::test]
async fn revoking_a_device_ends_sessions_voids_challenges_and_grants(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let c = stepup_challenge(&app, &dev, "admin.elevate", "").await;
    let g = stepup_challenge(&app, &dev, "admin.elevate", "").await;
    assert_eq!(answer(&app, &dev, &g, b"").await.0, 200);
    let ctx = AuthCtx::from(&app.state);
    let mut tx = pool.begin().await.unwrap();
    vc_server::approvals::devices::revoke_tx(
        &mut tx,
        &ctx,
        dev.row_id,
        vc_server::approvals::devices::RevokeReason::Admin,
        vc_server::security_events::Actor::Cli,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let (ended, ch_state, voided): (Option<String>, String, bool) = sqlx::query_as(
        "SELECT (SELECT end_reason FROM sessions WHERE id = $1),
                (SELECT state FROM approvals WHERE id = $2),
                (SELECT voided_at IS NOT NULL OR used_at IS NOT NULL FROM step_up_grants WHERE user_id = $3 LIMIT 1)",
    )
    .bind(dev.session_id.unwrap())
    .bind(Uuid::parse_str(c["challenge_id"].as_str().unwrap()).unwrap())
    .bind(user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(ended.as_deref(), Some("authenticator_revoked"));
    assert_eq!(ch_state, "expired", "open challenges are void");
    assert!(voided);
}

#[sqlx::test]
async fn db_rejects_long_or_unknown_challenges(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let ctx = AuthCtx::from(&app.state);
    let login = CanonicalContext::Login(LoginContext {
        attempt_id: Uuid::new_v4(),
        collection_hash: [1; 32],
        variant: Variant::Qr,
        client_class: ClientClass::Web,
        client_label: "x".into(),
        created_at: 0,
        server_name: "s".into(),
        place: String::new(),
        username: "u".into(),
    });
    let mut tx = pool.begin().await.unwrap();
    let too_long = issue(
        &mut tx,
        &ctx,
        NewChallenge {
            context: login,
            approver_id: user,
            subject_id: user,
            device_id: Uuid::new_v4(),
            requester: Requester::LoginAttempt(Uuid::new_v4()),
            display: json!({}),
            transfer_hash: None,
            ttl: chrono::Duration::minutes(11),
        },
    )
    .await;
    assert!(too_long.is_err(), "ttl > 10 min refused");
    tx.rollback().await.unwrap();
    let raw = sqlx::query(
        "INSERT INTO approvals (id, kind, approver_id, subject_id, device_id, requester_kind,
                                requester_id, nonce, context_hash, context, display, state, expires_at)
         VALUES ($1, 'agent', $2, $2, $1, 'session', $1, $3, $3, '{}', '{}', 'issued', NOW() + INTERVAL '1 minute')",
    )
    .bind(Uuid::new_v4())
    .bind(user)
    .bind(vec![0u8; 32])
    .execute(&pool)
    .await;
    assert!(raw.is_err(), "kind agent refused");
    let long = sqlx::query(
        "INSERT INTO approvals (id, kind, approver_id, subject_id, device_id, requester_kind,
                                requester_id, nonce, context_hash, context, display, state, expires_at)
         VALUES ($1, 'login', $2, $2, $1, 'session', $1, $3, $3, '{}', '{}', 'issued', NOW() + INTERVAL '11 minutes')",
    )
    .bind(Uuid::new_v4())
    .bind(user)
    .bind(vec![0u8; 32])
    .execute(&pool)
    .await;
    assert!(long.is_err(), "> 10 min refused");
}
