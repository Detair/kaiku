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

/// B.7 rule 1 (review round 1, Fable M2 / Codex M2): an answer takes the
/// account lock before the challenge. The revoking order is accounts →
/// device → sessions → challenges; an answer that held the challenge and
/// then waited for the session row formed a cycle with it (40P01).
#[sqlx::test]
async fn answer_and_revocation_never_deadlock(pool: PgPool) {
    use vc_server::approvals::devices::{revoke_tx, RevokeReason};
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let c = stepup_challenge(&app, &dev, "admin.elevate", "").await;
    let ctx = AuthCtx::from(&app.state);
    // The revoker holds the account and the device session's row (as
    // `end_sessions_tx` does while it ends the session) …
    let mut revoker = pool.begin().await.unwrap();
    vc_server::approvals::transition::lock_accounts(&mut revoker, &[user])
        .await
        .unwrap();
    sqlx::query("SELECT id FROM sessions WHERE id = $1 FOR UPDATE")
        .bind(dev.session_id.unwrap())
        .execute(&mut *revoker)
        .await
        .unwrap();
    // … and revokes the device once the answer waits for a lock.
    let revoke = async {
        super::helpers::wait_for_lock_waiters(&pool, 1).await;
        let r = revoke_tx(
            &mut revoker,
            &ctx,
            dev.row_id,
            RevokeReason::Admin,
            vc_server::security_events::Actor::Cli,
        )
        .await;
        match r {
            Ok(_) => revoker.commit().await.map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        }
    };
    let ((status, body), revoked) = tokio::join!(answer(&app, &dev, &c, b""), revoke);
    assert!(revoked.is_ok(), "the revocation commits: {revoked:?}");
    assert_ne!(status, 500, "no deadlock on the answer side: {body}");
    assert_eq!(
        (status, body["error"].as_str()),
        (403, Some("KEY_DEVICE_REQUIRED")),
        "the answer sees the revoked device"
    );
}

/// Codex M3: the authorising session is re-read under the account lock. A
/// device whose session ended while its request waited is no longer
/// authorised (here `/invalidated`, which revokes the key).
#[sqlx::test]
async fn ended_session_cannot_act_after_waiting_for_the_lock(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let mut holder = pool.begin().await.unwrap();
    vc_server::approvals::transition::lock_accounts(&mut holder, &[user])
        .await
        .unwrap();
    let logout = async {
        super::helpers::wait_for_lock_waiters(&pool, 1).await;
        sqlx::query("UPDATE sessions SET ended_at = NOW(), end_reason = 'logout' WHERE id = $1")
            .bind(dev.session_id.unwrap())
            .execute(&mut *holder)
            .await
            .unwrap();
        holder.commit().await.unwrap();
    };
    let uri = format!("/api/approval-devices/{}/invalidated", dev.row_id);
    let ((status, body), ()) =
        tokio::join!(post(&app, &uri, &dev.access_token, json!({}),), logout);
    assert_eq!(status, 401, "{body}");
    let state: String = sqlx::query_scalar("SELECT state FROM approval_devices WHERE id = $1")
        .bind(dev.row_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "active", "nothing was revoked by the ended session");
}

/// Argon m1: a remaining lifetime under a second never issues a challenge
/// that is already past due (the signed `expires_at` is whole seconds).
#[sqlx::test]
async fn sub_second_lifetime_never_issues_a_past_due_challenge(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let ctx = AuthCtx::from(&app.state);
    for _ in 0..5 {
        let mut tx = pool.begin().await.unwrap();
        let before = chrono::Utc::now();
        let r = issue(
            &mut tx,
            &ctx,
            NewChallenge {
                context: CanonicalContext::DeviceLogin {
                    client_class: ClientClass::Android,
                },
                approver_id: user,
                subject_id: user,
                device_id: Uuid::new_v4(),
                requester: Requester::Device(Uuid::new_v4()),
                display: json!({}),
                transfer_hash: None,
                ttl: chrono::Duration::milliseconds(1),
            },
        )
        .await;
        tx.rollback().await.unwrap();
        match r {
            Ok(c) => assert!(c.expires_at > before, "issued past due"),
            Err(e) => assert!(
                matches!(e, vc_server::approvals::error::ApprovalError::Expired),
                "{e:?}"
            ),
        }
    }
}

/// Fable m9: `lock_for_answer` and the sweep agree that an `approved`
/// challenge past its deadline is expired (410), not merely "answered".
#[sqlx::test]
async fn approved_past_due_is_expired(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let c = stepup_challenge(&app, &dev, "admin.elevate", "").await;
    let id = c["challenge_id"].as_str().unwrap();
    age_challenge(&pool, id).await;
    sqlx::query("UPDATE approvals SET state = 'approved' WHERE id = $1")
        .bind(Uuid::parse_str(id).unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = answer(&app, &dev, &c, b"").await;
    assert_eq!(
        (status, body["error"].as_str()),
        (410, Some("APPROVAL_EXPIRED"))
    );
    let state: String = sqlx::query_scalar("SELECT state FROM approvals WHERE id = $1")
        .bind(Uuid::parse_str(id).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "expired", "the mark is committed");
}

/// Fable m5: finished challenges and enrolments are deleted 30 days after
/// they ended; open ones and recent ones stay.
#[sqlx::test]
async fn finished_challenges_and_enrolments_are_cleaned_up(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let old = stepup_challenge(&app, &dev, "admin.elevate", "").await;
    let recent = stepup_challenge(&app, &dev, "admin.elevate", "").await;
    let open = stepup_challenge(&app, &dev, "admin.elevate", "").await;
    let id = |c: &Value| Uuid::parse_str(c["challenge_id"].as_str().unwrap()).unwrap();
    sqlx::query(
        "UPDATE approvals SET state = 'consumed', created_at = NOW() - INTERVAL '31 days',
                expires_at = NOW() - INTERVAL '31 days' + INTERVAL '2 minutes'
          WHERE id = $1",
    )
    .bind(id(&old))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE approvals SET state = 'consumed' WHERE id = $1")
        .bind(id(&recent))
        .execute(&pool)
        .await
        .unwrap();
    let enrolment = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO enrollments (id, variant, subject_id, issuer_kind, setup_token_hash, nonce,
                                  claimant_check, state, created_at, expires_at, ended_at)
         VALUES ($1, 'cli', $2, 'cli', $3, $3, 'none', 'expired', NOW() - INTERVAL '40 days',
                 NOW() - INTERVAL '40 days' + INTERVAL '10 minutes', NOW() - INTERVAL '31 days')",
    )
    .bind(enrolment)
    .bind(user)
    .bind(vec![7u8; 32])
    .execute(&pool)
    .await
    .unwrap();

    let n = vc_server::db::cleanup_finished_approvals(&pool)
        .await
        .unwrap();
    assert_eq!(n, 2, "the old challenge and the old enrolment");
    let left: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM approvals ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    let mut want = vec![id(&recent), id(&open)];
    want.sort();
    assert_eq!(left, want);
    let gone: bool =
        sqlx::query_scalar("SELECT NOT EXISTS (SELECT 1 FROM enrollments WHERE id = $1)")
            .bind(enrolment)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(gone);
}
