//! "Set up this PC": the attempt table, the `pc_setup` enrolment variant and
//! the challenge kind's schema (spec Step 5, SM2 and SM5).
//!
//! Run with: `cargo test --test integration pc_setup`

use sqlx::PgPool;
use uuid::Uuid;
use vc_server::approvals::pc_setup::AttemptState;

use super::helpers::{create_test_user, insert_session_row};

/// A probation session of `user_id`, as a PC without a key would have.
async fn probation_session(pool: &PgPool, user_id: Uuid) -> Uuid {
    let sid = Uuid::now_v7();
    insert_session_row(pool, sid, user_id, false).await;
    sid
}

/// Insert an attempt row of `session_id` in `state`; the error is the DB's.
async fn insert_attempt(
    pool: &PgPool,
    user_id: Uuid,
    session_id: Uuid,
    state: &str,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO pc_setup_attempts (id, user_id, session_id, state, qr_token_hash,
                                        qr_rotates_at, expires_at)
         VALUES ($1, $2, $3, $4, $5, NOW() + INTERVAL '60 seconds', NOW() + INTERVAL '10 minutes')",
    )
    .bind(id)
    .bind(user_id)
    .bind(session_id)
    .bind(state)
    .bind(Uuid::new_v4().as_bytes().repeat(2))
    .execute(pool)
    .await?;
    Ok(id)
}

/// Insert an enrolment row; `variant`, `issuer_kind`, the token hash and the
/// authenticator type are the knobs the CHECKs look at.
async fn insert_enrollment(
    pool: &PgPool,
    user_id: Uuid,
    variant: &str,
    issuer_kind: &str,
    token_hash: Option<Vec<u8>>,
    authenticator_type: &str,
    (issuer_session, issuer_device): (Option<Uuid>, Option<Uuid>),
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO enrollments (id, variant, subject_id, issuer_kind, issuer_session_id,
                                  issuer_device_id, setup_token_hash, nonce, authenticator_type,
                                  state, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'issued', NOW() + INTERVAL '10 minutes')",
    )
    .bind(Uuid::new_v4())
    .bind(variant)
    .bind(user_id)
    .bind(issuer_kind)
    .bind(issuer_session)
    .bind(issuer_device)
    .bind(token_hash)
    .bind(vec![7u8; 32])
    .bind(authenticator_type)
    .execute(pool)
    .await
    .map(|_| ())
}

fn is_check_violation(e: &sqlx::Error) -> bool {
    e.as_database_error()
        .and_then(|d| d.code())
        .is_some_and(|c| c == "23514")
}

fn is_unique_violation(e: &sqlx::Error) -> bool {
    e.as_database_error()
        .is_some_and(|d| d.is_unique_violation())
}

#[sqlx::test]
async fn schema_allows_pc_setup_enrolment_without_a_token(pool: PgPool) {
    let (user, _) = create_test_user(&pool).await;
    let sid = probation_session(&pool, user).await;
    let device = Uuid::new_v4();

    // A passkey enrolment issued by an attempt: no setup token at all.
    insert_enrollment(
        &pool,
        user,
        "pc_setup",
        "pc_setup",
        None,
        "passkey",
        (Some(sid), Some(device)),
    )
    .await
    .expect("pc_setup enrolment without a token");

    // Every other variant still needs its token ...
    let e = insert_enrollment(
        &pool,
        user,
        "own",
        "key_device",
        None,
        "key_device",
        (Some(sid), Some(device)),
    )
    .await
    .unwrap_err();
    assert!(is_check_violation(&e), "{e:?}");
    // ... and a pc_setup enrolment can only add a passkey,
    let e = insert_enrollment(
        &pool,
        user,
        "pc_setup",
        "pc_setup",
        None,
        "key_device",
        (Some(sid), Some(device)),
    )
    .await
    .unwrap_err();
    assert!(is_check_violation(&e), "{e:?}");
    // ... records both the session and the approving device,
    let e = insert_enrollment(
        &pool,
        user,
        "pc_setup",
        "pc_setup",
        None,
        "passkey",
        (Some(sid), None),
    )
    .await
    .unwrap_err();
    assert!(is_check_violation(&e), "{e:?}");
    // ... and the variant and the issuer kind belong together.
    let e = insert_enrollment(
        &pool,
        user,
        "pc_setup",
        "session",
        None,
        "passkey",
        (Some(sid), Some(device)),
    )
    .await
    .unwrap_err();
    assert!(is_check_violation(&e), "{e:?}");
    let e = insert_enrollment(
        &pool,
        user,
        "own",
        "pc_setup",
        Some(vec![1u8; 32]),
        "passkey",
        (Some(sid), Some(device)),
    )
    .await
    .unwrap_err();
    assert!(is_check_violation(&e), "{e:?}");
    // The token column stays unique where present.
    insert_enrollment(
        &pool,
        user,
        "own",
        "key_device",
        Some(vec![9u8; 32]),
        "key_device",
        (Some(sid), Some(device)),
    )
    .await
    .expect("own with a token");
    let e = insert_enrollment(
        &pool,
        user,
        "own",
        "key_device",
        Some(vec![9u8; 32]),
        "key_device",
        (Some(sid), Some(device)),
    )
    .await
    .unwrap_err();
    assert!(is_unique_violation(&e), "{e:?}");
}

#[sqlx::test]
async fn schema_allows_the_pc_setup_challenge_kind_and_requester(pool: PgPool) {
    let (user, _) = create_test_user(&pool).await;
    sqlx::query(
        "INSERT INTO approvals (id, kind, approver_id, subject_id, device_id, requester_kind,
                                requester_id, nonce, context_hash, context, display, state,
                                expires_at)
         VALUES ($1, 'pc_setup', $2, $2, $3, 'pc_setup_attempt', $4, $5, $5, '{}', '{}',
                 'issued', NOW() + INTERVAL '2 minutes')",
    )
    .bind(Uuid::new_v4())
    .bind(user)
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .bind(vec![3u8; 32])
    .execute(&pool)
    .await
    .expect("pc_setup challenge for an attempt");
}

#[sqlx::test]
async fn one_live_attempt_per_session(pool: PgPool) {
    let (user, _) = create_test_user(&pool).await;
    let sid = probation_session(&pool, user).await;
    let other = probation_session(&pool, user).await;

    let first = insert_attempt(&pool, user, sid, "waiting").await.unwrap();
    // A second live attempt of the same session is refused in every live state ...
    for state in ["waiting", "claimed", "code", "enrolled"] {
        let e = insert_attempt(&pool, user, sid, state).await.unwrap_err();
        assert!(is_unique_violation(&e), "{state}: {e:?}");
    }
    // ... another session of the account is independent ...
    insert_attempt(&pool, user, other, "waiting").await.unwrap();
    // ... and a finished attempt makes room for the next one.
    for state in AttemptState::ALL.into_iter().filter(|s| s.is_terminal()) {
        sqlx::query("UPDATE pc_setup_attempts SET state = $2 WHERE id = $1")
            .bind(first)
            .bind(state.as_str())
            .execute(&pool)
            .await
            .unwrap();
        let next = insert_attempt(&pool, user, sid, "waiting").await.unwrap();
        sqlx::query("UPDATE pc_setup_attempts SET state = 'cancelled' WHERE id = $1")
            .bind(next)
            .execute(&pool)
            .await
            .unwrap();
    }
    // The table refuses a state SM5 does not know and an attempt that outlives 10 minutes.
    let e = insert_attempt(&pool, user, sid, "approved")
        .await
        .unwrap_err();
    assert!(is_check_violation(&e), "{e:?}");
    let e = sqlx::query(
        "INSERT INTO pc_setup_attempts (id, user_id, session_id, state, qr_token_hash, qr_rotates_at, expires_at)
         VALUES ($1, $2, $3, 'cancelled', $4, NOW(), NOW() + INTERVAL '11 minutes')",
    )
    .bind(Uuid::new_v4())
    .bind(user)
    .bind(sid)
    .bind(vec![5u8; 32])
    .execute(&pool)
    .await
    .unwrap_err();
    assert!(is_check_violation(&e), "{e:?}");
}

#[sqlx::test]
async fn deleting_the_session_row_leaves_the_attempt_alone(pool: PgPool) {
    let (user, _) = create_test_user(&pool).await;
    let sid = probation_session(&pool, user).await;
    let id = insert_attempt(&pool, user, sid, "waiting").await.unwrap();
    // Logout and refresh delete the session row without the account lock.
    sqlx::query("DELETE FROM sessions WHERE id = $1")
        .bind(sid)
        .execute(&pool)
        .await
        .unwrap();
    let (state, session): (String, Uuid) =
        sqlx::query_as("SELECT state, session_id FROM pc_setup_attempts WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("attempt row survives");
    assert_eq!((state.as_str(), session), ("waiting", sid));
}

// ============================================================================
// The attempt lifecycle over HTTP (Task 2): start, scan, approve, refuse,
// report, release, cancel and restart
// ============================================================================

use axum::body::Body;
use axum::http::Method;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use vc_server::approvals::secrets::{b64u_decode, b64u_encode};
use vc_server::approvals::signing::Decision;

use super::helpers::approvals::KeyDeviceFixture;
use super::helpers::{body_to_json_lenient, create_probation_session_with_token, TestApp};

/// The QR payload prefix for the test origin.
const QR_PREFIX: &str = "kaiku://pcsetup/v1?o=https%3A%2F%2Fchat.example.com&t=";

/// A user with an active key phone and a probation session on a PC.
struct Pc {
    app: TestApp,
    user: Uuid,
    sid: Uuid,
    token: String,
    dev: KeyDeviceFixture,
}

async fn pc(pool: &PgPool) -> Pc {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(pool).await;
    let dev = KeyDeviceFixture::insert(pool, &app.config, user, "active").await;
    let (sid, token) = create_probation_session_with_token(pool, &app.config, user).await;
    Pc {
        app,
        user,
        sid,
        token,
        dev,
    }
}

/// One bearer request; `(status, body)`.
async fn call(
    app: &TestApp,
    method: Method,
    uri: &str,
    bearer: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let req = TestApp::request(method, uri)
        .header("Authorization", format!("Bearer {bearer}"))
        .header("Content-Type", "application/json")
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) \
             Chrome/129.0 Safari/537.36",
        )
        .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    (status, body_to_json_lenient(resp).await)
}

async fn start(app: &TestApp, bearer: &str) -> (u16, Value) {
    call(
        app,
        Method::POST,
        "/auth/pc-setups",
        bearer,
        Some(json!({})),
    )
    .await
}

/// Start and expect 201.
async fn started(p: &Pc) -> Value {
    let (s, body) = start(&p.app, &p.token).await;
    assert_eq!(s, 201, "{body}");
    body
}

/// The b64u token and its bytes from a QR payload.
fn qr_token(payload: &Value) -> (String, Vec<u8>) {
    let p = payload.as_str().expect("qr payload");
    let t = p
        .strip_prefix(QR_PREFIX)
        .expect("pcsetup payload")
        .to_string();
    let bytes = b64u_decode(&t).unwrap();
    (t, bytes)
}

fn id_of(v: &Value) -> String {
    v["attempt_id"].as_str().unwrap().to_string()
}

async fn claim(app: &TestApp, dev: &KeyDeviceFixture, token: &str) -> (u16, Value) {
    call(
        app,
        Method::POST,
        "/api/pc-setups/claim",
        &dev.access_token,
        Some(json!({ "token": token })),
    )
    .await
}

/// Sign and answer a `pc_setup` challenge.
async fn answer(
    app: &TestApp,
    dev: &KeyDeviceFixture,
    challenge: &Value,
    response: &[u8],
    decision: Decision,
) -> (u16, Value) {
    let prefix = b64u_decode(challenge["signing_prefix"].as_str().unwrap()).unwrap();
    let sig = dev.sign(&prefix, decision, response);
    answer_raw(app, dev, challenge, response, decision, &sig).await
}

async fn answer_raw(
    app: &TestApp,
    dev: &KeyDeviceFixture,
    challenge: &Value,
    response: &[u8],
    decision: Decision,
    signature: &str,
) -> (u16, Value) {
    let id = challenge["challenge_id"].as_str().unwrap();
    let decision = match decision {
        Decision::Report => "report",
        _ => "approve",
    };
    call(
        app,
        Method::POST,
        &format!("/api/pc-setups/{id}/answer"),
        &dev.access_token,
        Some(json!({"decision": decision, "response": b64u_encode(response), "signature": signature})),
    )
    .await
}

/// Start, scan: `(attempt id, challenge, token bytes)`.
async fn claimed(p: &Pc) -> (String, Value, Vec<u8>) {
    let a = started(p).await;
    let (token, bytes) = qr_token(&a["qr_payload"]);
    let (s, c) = claim(&p.app, &p.dev, &token).await;
    assert_eq!(s, 201, "{c}");
    (id_of(&a), c, bytes)
}

async fn attempt_state(pool: &PgPool, id: &str) -> (String, Option<String>) {
    sqlx::query_as("SELECT state, reason FROM pc_setup_attempts WHERE id = $1::uuid")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn session_live(pool: &PgPool, sid: Uuid) -> bool {
    sqlx::query_scalar("SELECT ended_at IS NULL FROM sessions WHERE id = $1")
        .bind(sid)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn set_may_approve(pool: &PgPool, user: Uuid, on: bool) {
    sqlx::query("UPDATE users SET may_approve = $2 WHERE id = $1")
        .bind(user)
        .bind(on)
        .execute(pool)
        .await
        .unwrap();
}

async fn end_session(pool: &PgPool, sid: Uuid) {
    sqlx::query("UPDATE sessions SET ended_at = NOW(), end_reason = 'logout' WHERE id = $1")
        .bind(sid)
        .execute(pool)
        .await
        .unwrap();
}

async fn pc_poll(p: &Pc, id: &str) -> (u16, Value) {
    call(
        &p.app,
        Method::GET,
        &format!("/auth/pc-setups/{id}?wait=0"),
        &p.token,
        None,
    )
    .await
}

#[sqlx::test]
async fn pc_setup_needs_a_live_probation_session(pool: PgPool) {
    let p = pc(&pool).await;
    // A full session already has everything: no attempt.
    let (_, full) = super::helpers::create_session_with_token(&pool, &p.app.config, p.user).await;
    let (s, body) = start(&p.app, &full).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (403, Some("PC_SETUP_UNAVAILABLE")),
        "{body}"
    );

    // A live probation session gets an attempt and its first QR.
    let a = started(&p).await;
    let payload = a["qr_payload"].as_str().unwrap();
    assert!(payload.starts_with(QR_PREFIX), "{payload}");
    assert_eq!(payload.len(), QR_PREFIX.len() + 43);
    assert!(a["rotates_at"].is_string(), "{a}");
    assert_eq!(attempt_state(&pool, &id_of(&a)).await.0, "waiting");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pc_setup_attempts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);

    // An ended session cannot start one.
    end_session(&pool, p.sid).await;
    let (s, _) = start(&p.app, &p.token).await;
    assert_eq!(s, 401);
}

#[sqlx::test]
async fn start_refused_without_an_eligible_key_device(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (_, token) = create_probation_session_with_token(&pool, &app.config, user).await;
    // No key device at all.
    let (s, body) = start(&app, &token).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (403, Some("PC_SETUP_UNAVAILABLE")),
        "{body}"
    );
    // A pending or revoked one does not count.
    KeyDeviceFixture::insert(&pool, &app.config, user, "pending").await;
    KeyDeviceFixture::insert(&pool, &app.config, user, "revoked").await;
    let (s, _) = start(&app, &token).await;
    assert_eq!(s, 403);
    // An active one with `may_approve` off neither.
    KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    set_may_approve(&pool, user, false).await;
    let (s, body) = start(&app, &token).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (403, Some("PC_SETUP_UNAVAILABLE"))
    );
    set_may_approve(&pool, user, true).await;
    let (s, _) = start(&app, &token).await;
    assert_eq!(s, 201);
}

#[sqlx::test]
async fn can_set_up_this_pc_follows_may_approve(pool: PgPool) {
    let p = pc(&pool).await;
    let read = |bearer: String| {
        let app = &p.app;
        async move {
            let (s, body) = call(app, Method::GET, "/api/me/security", &bearer, None).await;
            assert_eq!(s, 200, "{body}");
            body["can_set_up_this_pc"]
                .as_bool()
                .expect("can_set_up_this_pc")
        }
    };
    assert!(read(p.token.clone()).await);
    set_may_approve(&pool, p.user, false).await;
    assert!(!read(p.token.clone()).await);
    set_may_approve(&pool, p.user, true).await;
    // Never for a full session.
    let (_, full) = super::helpers::create_session_with_token(&pool, &p.app.config, p.user).await;
    assert!(!read(full).await);
    // Not without an active key device.
    sqlx::query("UPDATE approval_devices SET state = 'revoked', revoked_at = NOW(), revoke_reason = 'owner' WHERE user_id = $1")
        .bind(p.user)
        .execute(&pool)
        .await
        .unwrap();
    assert!(!read(p.token.clone()).await);
}

#[sqlx::test]
async fn scan_by_other_account_gets_its_own_answer_without_a_name(pool: PgPool) {
    let p = pc(&pool).await;
    let username: String = sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
        .bind(p.user)
        .fetch_one(&pool)
        .await
        .unwrap();
    let (other, _) = create_test_user(&pool).await;
    let foreign = KeyDeviceFixture::insert(&pool, &p.app.config, other, "active").await;
    let a = started(&p).await;
    let (token, _) = qr_token(&a["qr_payload"]);
    let (s, body) = claim(&p.app, &foreign, &token).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (409, Some("PC_SETUP_OTHER_ACCOUNT")),
        "{body}"
    );
    assert!(!body.to_string().contains(&username), "{body}");
    assert_eq!(attempt_state(&pool, &id_of(&a)).await.0, "waiting");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM approvals WHERE kind = 'pc_setup'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
    // The owner's phone still can.
    let (s, _) = claim(&p.app, &p.dev, &token).await;
    assert_eq!(s, 201);
}

#[sqlx::test]
async fn may_approve_off_refuses_scan_and_approval(pool: PgPool) {
    let p = pc(&pool).await;
    // Off before the scan.
    let a = started(&p).await;
    let (token, _) = qr_token(&a["qr_payload"]);
    set_may_approve(&pool, p.user, false).await;
    let (s, body) = claim(&p.app, &p.dev, &token).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (403, Some("APPROVAL_NOT_ALLOWED")),
        "{body}"
    );
    assert_eq!(
        attempt_state(&pool, &id_of(&a)).await,
        ("expired".into(), Some("may_approve_off".into()))
    );
    // Off between the scan and the approval.
    set_may_approve(&pool, p.user, true).await;
    let (id, c, bytes) = claimed(&p).await;
    set_may_approve(&pool, p.user, false).await;
    let (s, body) = answer(&p.app, &p.dev, &c, &bytes, Decision::Approve).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (403, Some("APPROVAL_NOT_ALLOWED")),
        "{body}"
    );
    assert!(body.get("code").is_none());
    assert_eq!(
        attempt_state(&pool, &id).await,
        ("expired".into(), Some("may_approve_off".into()))
    );
    // The PC can read why.
    let (s, st) = pc_poll(&p, &id).await;
    assert_eq!(
        (s, st["state"].as_str(), st["reason"].as_str()),
        (200, Some("expired"), Some("may_approve_off"))
    );
}

#[sqlx::test]
async fn other_session_of_the_account_gets_404_on_every_attempt_route(pool: PgPool) {
    let p = pc(&pool).await;
    let (_, other) = create_probation_session_with_token(&pool, &p.app.config, p.user).await;
    let (id, _, _) = claimed(&p).await;
    for (method, uri) in [
        (Method::GET, "/auth/pc-setups/current".to_string()),
        (Method::GET, format!("/auth/pc-setups/{id}?wait=0")),
        (Method::DELETE, format!("/auth/pc-setups/{id}")),
    ] {
        let (s, body) = call(&p.app, method.clone(), &uri, &other, None).await;
        assert_eq!(s, 404, "{method} {uri}: {body}");
    }
    assert_eq!(attempt_state(&pool, &id).await.0, "claimed");
    // Another phone of the account cannot read the attempt either.
    let second = KeyDeviceFixture::insert(&pool, &p.app.config, p.user, "active").await;
    let (s, _) = call(
        &p.app,
        Method::GET,
        &format!("/api/pc-setups/{id}?wait=0"),
        &second.access_token,
        None,
    )
    .await;
    assert_eq!(s, 404);
    // The requesting session and the scanning phone can.
    let (s, body) = pc_poll(&p, &id).await;
    assert_eq!((s, body["state"].as_str()), (200, Some("claimed")));
    let (s, body) = call(
        &p.app,
        Method::GET,
        &format!("/api/pc-setups/{id}?wait=0"),
        &p.dev.access_token,
        None,
    )
    .await;
    assert_eq!(
        (s, body["state"].as_str()),
        (200, Some("claimed")),
        "{body}"
    );
    let (s, body) = call(
        &p.app,
        Method::GET,
        "/auth/pc-setups/current",
        &p.token,
        None,
    )
    .await;
    assert_eq!(
        (s, body["attempt_id"].as_str()),
        (200, Some(id.as_str())),
        "{body}"
    );
}

#[sqlx::test]
async fn token_rotates_while_waiting_and_pauses_while_claimed(pool: PgPool) {
    let p = pc(&pool).await;
    let a = started(&p).await;
    let id = id_of(&a);
    let (first, _) = qr_token(&a["qr_payload"]);
    // Not yet due: no new QR.
    let (_, st) = pc_poll(&p, &id).await;
    assert!(st.get("qr_payload").is_none_or(Value::is_null), "{st}");
    // Due: a new one, and the old one is stale.
    sqlx::query("UPDATE pc_setup_attempts SET qr_rotates_at = NOW() - INTERVAL '1 second' WHERE id = $1::uuid")
        .bind(&id)
        .execute(&pool)
        .await
        .unwrap();
    let (_, st) = pc_poll(&p, &id).await;
    let (second, bytes) = qr_token(&st["qr_payload"]);
    assert_ne!(first, second);
    let (s, body) = claim(&p.app, &p.dev, &first).await;
    assert_eq!((s, body["error"].as_str()), (404, Some("LOGIN_CODE_STALE")));
    // Claimed: no rotation, and the signed token stays the scanned one.
    let (s, c) = claim(&p.app, &p.dev, &second).await;
    assert_eq!(s, 201, "{c}");
    sqlx::query("UPDATE pc_setup_attempts SET qr_rotates_at = NOW() - INTERVAL '1 second' WHERE id = $1::uuid")
        .bind(&id)
        .execute(&pool)
        .await
        .unwrap();
    let (_, st) = pc_poll(&p, &id).await;
    assert_eq!(st["state"], "claimed");
    assert!(st.get("qr_payload").is_none_or(Value::is_null), "{st}");
    let (s, body) = answer(&p.app, &p.dev, &c, &bytes, Decision::Approve).await;
    assert_eq!(s, 200, "{body}");
}

#[sqlx::test]
async fn challenge_expiry_returns_to_waiting(pool: PgPool) {
    let p = pc(&pool).await;
    let (id, c, bytes) = claimed(&p).await;
    // The phone scanned and walked away: its challenge runs out.
    sqlx::query(
        "UPDATE approvals SET created_at = NOW() - INTERVAL '3 minutes',
                              expires_at = NOW() - INTERVAL '1 minute'
          WHERE id = $1::uuid",
    )
    .bind(c["challenge_id"].as_str().unwrap())
    .execute(&pool)
    .await
    .unwrap();
    let (_, st) = pc_poll(&p, &id).await;
    assert_eq!(st["state"], "waiting", "{st}");
    let (token, _) = qr_token(&st["qr_payload"]);
    let (s, body) = answer(&p.app, &p.dev, &c, &bytes, Decision::Approve).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (410, Some("APPROVAL_EXPIRED")),
        "{body}"
    );
    // A new scan works, and the phone's own release returns to waiting too.
    let (s, c2) = claim(&p.app, &p.dev, &token).await;
    assert_eq!(s, 201, "{c2}");
    let cid = c2["challenge_id"].as_str().unwrap();
    let (s, _) = call(
        &p.app,
        Method::POST,
        &format!("/api/approvals/{cid}/release"),
        &p.dev.access_token,
        None,
    )
    .await;
    assert_eq!(s, 204);
    assert_eq!(attempt_state(&pool, &id).await.0, "waiting");
    let st: String = sqlx::query_scalar("SELECT state FROM approvals WHERE id = $1::uuid")
        .bind(cid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(st, "expired");
    let (_, st) = pc_poll(&p, &id).await;
    assert!(
        st["qr_payload"].is_string(),
        "a new QR after the release: {st}"
    );
}

/// Every occurrence of `code` stands inside a longer run of digits or after
/// a decimal point (timestamps), never on its own.
fn contains_code(text: &str, code: &str) -> bool {
    text.match_indices(code).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + code.len()..].chars().next();
        !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '.')
            && !after.is_some_and(|c| c.is_ascii_alphanumeric())
    })
}

#[sqlx::test]
async fn code_is_returned_to_the_device_only(pool: PgPool) {
    let p = pc(&pool).await;
    let mut pc_bodies = Vec::new();
    let mut code = String::new();
    let mut code_expires_at = Value::Null;
    let logs = super::oidc_flow_http::capture(async {
        let a = started(&p).await;
        pc_bodies.push(a.clone());
        let id = id_of(&a);
        let (token, bytes) = qr_token(&a["qr_payload"]);
        let (_, c) = claim(&p.app, &p.dev, &token).await;
        let (s, ok) = answer(&p.app, &p.dev, &c, &bytes, Decision::Approve).await;
        assert_eq!(s, 200, "{ok}");
        assert_eq!(ok["attempt_id"].as_str(), Some(id.as_str()));
        code = ok["code"].as_str().unwrap().to_string();
        code_expires_at = ok["code_expires_at"].clone();
        pc_bodies.push(pc_poll(&p, &id).await.1);
        pc_bodies.push(
            call(
                &p.app,
                Method::GET,
                "/auth/pc-setups/current",
                &p.token,
                None,
            )
            .await
            .1,
        );
    })
    .await;
    assert!(
        code.len() == 6 && code.chars().all(|c| c.is_ascii_digit()),
        "{code}"
    );
    let expires: chrono::DateTime<chrono::Utc> = code_expires_at.as_str().unwrap().parse().unwrap();
    let left = expires - chrono::Utc::now();
    assert!(left <= chrono::Duration::minutes(5) && left > chrono::Duration::minutes(4));
    assert_eq!(pc_bodies[1]["state"], "code");
    for b in &pc_bodies {
        assert!(
            !contains_code(&b.to_string(), &code),
            "PC body has the code: {b}"
        );
    }
    assert!(!logs.is_empty());
    assert!(!contains_code(&logs, &code), "a log line has the code");
    // Only its keyed hash is stored, and no audit or event row carries it.
    let stored: Vec<u8> = sqlx::query_scalar("SELECT code_hash FROM pc_setup_attempts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored.len(), 32);
    assert_ne!(stored, Sha256::digest(code.as_bytes()).to_vec());
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT COALESCE(details::text, '') FROM system_audit_log
         UNION ALL SELECT details::text FROM security_events",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(
        rows.iter().any(|r| r.contains("attempt")),
        "the start is audited: {rows:?}"
    );
    for r in rows {
        assert!(!contains_code(&r, &code), "{r}");
    }
}

#[sqlx::test]
async fn report_needs_uv_and_signature(pool: PgPool) {
    let p = pc(&pool).await;
    let (id, c, bytes) = claimed(&p).await;
    // A signature by another key, or over "approve", is no report.
    let stranger = KeyDeviceFixture::insert(&pool, &p.app.config, p.user, "pending").await;
    let prefix = b64u_decode(c["signing_prefix"].as_str().unwrap()).unwrap();
    let forged = stranger.sign(&prefix, Decision::Report, &bytes);
    let (s, body) = answer_raw(&p.app, &p.dev, &c, &bytes, Decision::Report, &forged).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (400, Some("SIGNATURE_INVALID")),
        "{body}"
    );
    let approve_sig = p.dev.sign(&prefix, Decision::Approve, &bytes);
    let (s, _) = answer_raw(&p.app, &p.dev, &c, &bytes, Decision::Report, &approve_sig).await;
    assert_eq!(s, 400);
    // The signed response must be the scanned token.
    let (s, body) = answer(&p.app, &p.dev, &c, &[0u8; 32], Decision::Report).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (400, Some("APPROVAL_RESPONSE_MISMATCH"))
    );
    assert_eq!(attempt_state(&pool, &id).await.0, "claimed");
    assert!(session_live(&pool, p.sid).await);
    // The real report works.
    let (s, body) = answer(&p.app, &p.dev, &c, &bytes, Decision::Report).await;
    assert_eq!(
        (s, body["attempt_id"].as_str()),
        (200, Some(id.as_str())),
        "{body}"
    );
    assert!(body.get("code").is_none());
}

#[sqlx::test]
async fn no_needs_no_signature_and_only_ends_the_attempt(pool: PgPool) {
    let p = pc(&pool).await;
    let (id, c, _) = claimed(&p).await;
    let cid = c["challenge_id"].as_str().unwrap();
    let (s, body) = call(
        &p.app,
        Method::POST,
        &format!("/api/pc-setups/{cid}/refuse"),
        &p.dev.access_token,
        None,
    )
    .await;
    assert_eq!(s, 204, "{body}");
    assert_eq!(
        attempt_state(&pool, &id).await,
        ("refused".into(), Some("refused".into()))
    );
    let st: String = sqlx::query_scalar("SELECT state FROM approvals WHERE id = $1::uuid")
        .bind(cid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(st, "denied");
    assert!(session_live(&pool, p.sid).await);
    let events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM security_events WHERE event_type = 'pc_setup_reported'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(events, 0);
    let (_, st) = pc_poll(&p, &id).await;
    assert_eq!(
        (st["state"].as_str(), st["reason"].as_str()),
        (Some("refused"), Some("refused"))
    );
}

#[sqlx::test]
async fn report_ends_only_the_requesting_session_and_marks_no_credential(pool: PgPool) {
    let p = pc(&pool).await;
    let (other_sid, _) = create_probation_session_with_token(&pool, &p.app.config, p.user).await;
    let (id, c, bytes) = claimed(&p).await;
    let (s, body) = answer(&p.app, &p.dev, &c, &bytes, Decision::Report).await;
    assert_eq!(s, 200, "{body}");
    assert_eq!(
        attempt_state(&pool, &id).await,
        ("reported".into(), Some("reported".into()))
    );
    assert!(!session_live(&pool, p.sid).await);
    assert!(session_live(&pool, other_sid).await);
    assert!(session_live(&pool, p.dev.session_id.unwrap()).await);
    let (password, totp): (String, String) =
        sqlx::query_as("SELECT password_state, totp_state FROM users WHERE id = $1")
            .bind(p.user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((password.as_str(), totp.as_str()), ("active", "active"));
    let marked: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM security_events
          WHERE subject_id = $1 AND event_type IN ('password_compromised', 'totp_compromised')",
    )
    .bind(p.user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(marked, 0);
    let reported: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM security_events WHERE subject_id = $1 AND event_type = 'pc_setup_reported'",
    )
    .bind(p.user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(reported, 1);
    let details: Value = sqlx::query_scalar(
        "SELECT details FROM security_events WHERE subject_id = $1 AND event_type = 'pc_setup_reported'",
    )
    .bind(p.user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        details["requesting_sid"].as_str(),
        Some(p.sid.to_string().as_str()),
        "{details}"
    );
    assert_eq!(details["attempt_id"].as_str(), Some(id.as_str()));
    let challenge: String = sqlx::query_scalar("SELECT state FROM approvals WHERE id = $1::uuid")
        .bind(c["challenge_id"].as_str().unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(challenge, "reported");
}

#[sqlx::test]
async fn two_sessions_run_independent_attempts(pool: PgPool) {
    let p = pc(&pool).await;
    let (sid2, token2) = create_probation_session_with_token(&pool, &p.app.config, p.user).await;
    let (id1, c1, bytes1) = claimed(&p).await;
    let (s, a2) = start(&p.app, &token2).await;
    assert_eq!(s, 201, "{a2}");
    let id2 = id_of(&a2);
    assert_eq!(attempt_state(&pool, &id1).await.0, "claimed");
    // A report on the first ends only the first PC and its attempt.
    let (s, _) = answer(&p.app, &p.dev, &c1, &bytes1, Decision::Report).await;
    assert_eq!(s, 200);
    assert_eq!(attempt_state(&pool, &id2).await.0, "waiting");
    assert!(session_live(&pool, sid2).await);
    // The second still runs to its code.
    let (token, bytes) = qr_token(&a2["qr_payload"]);
    let (s, c2) = claim(&p.app, &p.dev, &token).await;
    assert_eq!(s, 201, "{c2}");
    let (s, ok) = answer(&p.app, &p.dev, &c2, &bytes, Decision::Approve).await;
    assert_eq!(
        (s, ok["attempt_id"].as_str()),
        (200, Some(id2.as_str())),
        "{ok}"
    );
    assert_eq!(attempt_state(&pool, &id2).await.0, "code");
}

#[sqlx::test]
async fn session_ended_or_device_revoked_stops_every_transition(pool: PgPool) {
    // The session ends before the scan: the scan reads as stale.
    let p = pc(&pool).await;
    let a = started(&p).await;
    let (token, _) = qr_token(&a["qr_payload"]);
    end_session(&pool, p.sid).await;
    let (s, body) = claim(&p.app, &p.dev, &token).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (404, Some("LOGIN_CODE_STALE")),
        "{body}"
    );
    assert_eq!(
        attempt_state(&pool, &id_of(&a)).await,
        ("expired".into(), Some("session_ended".into()))
    );

    // The session ends between the scan and the approval.
    let p = pc(&pool).await;
    let (id, c, bytes) = claimed(&p).await;
    end_session(&pool, p.sid).await;
    let (s, body) = answer(&p.app, &p.dev, &c, &bytes, Decision::Approve).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (409, Some("APPROVAL_STATE")),
        "{body}"
    );
    assert!(body.get("code").is_none());
    assert_eq!(
        attempt_state(&pool, &id).await,
        ("expired".into(), Some("session_ended".into()))
    );
    // The phone learns why.
    let (s, st) = call(
        &p.app,
        Method::GET,
        &format!("/api/pc-setups/{id}?wait=0"),
        &p.dev.access_token,
        None,
    )
    .await;
    assert_eq!(
        (s, st["reason"].as_str()),
        (200, Some("session_ended")),
        "{st}"
    );

    // The device is revoked between the scan and the approval.
    let p = pc(&pool).await;
    let (id, c, bytes) = claimed(&p).await;
    sqlx::query(
        "UPDATE approval_devices SET state = 'revoked', revoked_at = NOW(), revoke_reason = 'owner'
          WHERE id = $1",
    )
    .bind(p.dev.row_id)
    .execute(&pool)
    .await
    .unwrap();
    let (s, body) = answer(&p.app, &p.dev, &c, &bytes, Decision::Report).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (409, Some("APPROVAL_STATE")),
        "{body}"
    );
    assert_eq!(
        attempt_state(&pool, &id).await,
        ("expired".into(), Some("device_revoked".into()))
    );
    assert!(
        session_live(&pool, p.sid).await,
        "a stale report ends nothing"
    );

    // The session ends while the code is shown: the PC poll and the phone see it.
    let p = pc(&pool).await;
    let (id, c, bytes) = claimed(&p).await;
    let (s, _) = answer(&p.app, &p.dev, &c, &bytes, Decision::Approve).await;
    assert_eq!(s, 200);
    end_session(&pool, p.sid).await;
    let (s, st) = call(
        &p.app,
        Method::GET,
        &format!("/api/pc-setups/{id}?wait=0"),
        &p.dev.access_token,
        None,
    )
    .await;
    assert_eq!(
        (s, st["state"].as_str(), st["reason"].as_str()),
        (200, Some("expired"), Some("session_ended"))
    );
}

#[sqlx::test]
async fn context_comes_from_the_session_row_not_the_request(pool: PgPool) {
    let p = pc(&pool).await;
    let created: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "UPDATE sessions
            SET user_agent = 'Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0',
                city = 'Berlin', country = 'Germany',
                created_at = NOW() - INTERVAL '3 days'
          WHERE id = $1
          RETURNING created_at",
    )
    .bind(p.sid)
    .fetch_one(&pool)
    .await
    .unwrap();
    // The start request claims to be Chrome on Windows (see `call`).
    let (id, c, _) = claimed(&p).await;
    let ctx = &c["context"];
    assert_eq!(c["kind"], "pc_setup");
    assert_eq!(ctx["attempt_id"].as_str(), Some(id.as_str()));
    let desc = ctx["client_description"].as_str().unwrap();
    assert!(desc.contains("Firefox") && desc.contains("Linux"), "{desc}");
    assert!(
        !desc.contains("Chrome") && !desc.contains("Windows"),
        "{desc}"
    );
    assert_eq!(
        ctx["session_created_at"].as_i64(),
        Some(created.timestamp())
    );
    assert_eq!(ctx["place"], "Berlin, Germany");
    assert_eq!(
        ctx["sid_hash"].as_str(),
        Some(b64u_encode(&Sha256::digest(p.sid.as_bytes())).as_str())
    );
    // The challenge lives 2 minutes from the scan.
    let expires: chrono::DateTime<chrono::Utc> = c["expires_at"].as_str().unwrap().parse().unwrap();
    let ttl = expires - chrono::Utc::now();
    assert!(ttl <= chrono::Duration::seconds(120) && ttl > chrono::Duration::seconds(100));
}

#[sqlx::test]
async fn no_route_returns_a_setup_token_for_pc_setup(pool: PgPool) {
    let p = pc(&pool).await;
    let mut bodies = Vec::new();
    let a = started(&p).await;
    bodies.push(a.clone());
    let id = id_of(&a);
    let (token, bytes) = qr_token(&a["qr_payload"]);
    let (_, c) = claim(&p.app, &p.dev, &token).await;
    bodies.push(c.clone());
    let (_, ok) = answer(&p.app, &p.dev, &c, &bytes, Decision::Approve).await;
    bodies.push(ok);
    bodies.push(pc_poll(&p, &id).await.1);
    bodies.push(
        call(
            &p.app,
            Method::GET,
            "/auth/pc-setups/current",
            &p.token,
            None,
        )
        .await
        .1,
    );
    bodies.push(
        call(
            &p.app,
            Method::GET,
            &format!("/api/pc-setups/{id}?wait=0"),
            &p.dev.access_token,
            None,
        )
        .await
        .1,
    );
    for b in &bodies {
        let text = b.to_string();
        assert!(
            !text.contains("setup_token") && !text.contains("setup_url"),
            "{text}"
        );
    }
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM enrollments WHERE subject_id = $1")
        .bind(p.user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0, "no enrolment before the code is typed");
}

#[sqlx::test]
async fn fresh_start_restarts_the_live_attempt(pool: PgPool) {
    let p = pc(&pool).await;
    let (first, c, bytes) = claimed(&p).await;
    let b = started(&p).await;
    let second = id_of(&b);
    assert_ne!(first, second);
    assert_eq!(
        attempt_state(&pool, &first).await,
        ("cancelled".into(), Some("restarted".into()))
    );
    assert_eq!(attempt_state(&pool, &second).await.0, "waiting");
    let st: String = sqlx::query_scalar("SELECT state FROM approvals WHERE id = $1::uuid")
        .bind(c["challenge_id"].as_str().unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(st, "expired");
    // The old approval goes nowhere.
    let (s, body) = answer(&p.app, &p.dev, &c, &bytes, Decision::Approve).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (410, Some("APPROVAL_EXPIRED")),
        "{body}"
    );
    // The PC's cancel ends the new one.
    let (s, _) = call(
        &p.app,
        Method::DELETE,
        &format!("/auth/pc-setups/{second}"),
        &p.token,
        None,
    )
    .await;
    assert_eq!(s, 204);
    assert_eq!(
        attempt_state(&pool, &second).await,
        ("cancelled".into(), Some("cancelled".into()))
    );
    let (s, _) = call(
        &p.app,
        Method::GET,
        "/auth/pc-setups/current",
        &p.token,
        None,
    )
    .await;
    assert_eq!(s, 404);
}

#[sqlx::test]
async fn sweep_expires_what_no_transition_reaches(pool: PgPool) {
    let p = pc(&pool).await;
    let a = started(&p).await;
    // Another session's attempt has nothing due.
    let (_, token2) = create_probation_session_with_token(&pool, &p.app.config, p.user).await;
    let (_, b) = start(&p.app, &token2).await;
    let keep = id_of(&b);
    end_session(&pool, p.sid).await;
    let n = vc_server::approvals::pc_setup::transition::expire_due(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
    assert_eq!(
        attempt_state(&pool, &id_of(&a)).await,
        ("expired".into(), Some("session_ended".into()))
    );
    assert_eq!(attempt_state(&pool, &keep).await.0, "waiting");
}

/// Locking 2: the answer waits for the account lock, then re-checks the
/// session under it. A session end that holds the lock (as every revocation
/// does) commits first, and the answer sees it: no code is handed out.
#[sqlx::test]
async fn answer_waits_for_the_account_lock_and_sees_the_session_end(pool: PgPool) {
    let p = pc(&pool).await;
    let (id, c, bytes) = claimed(&p).await;
    let mut holder = pool.begin().await.unwrap();
    vc_server::approvals::transition::lock_accounts(&mut holder, &[p.user])
        .await
        .unwrap();
    sqlx::query("UPDATE sessions SET ended_at = NOW(), end_reason = 'revoked' WHERE id = $1")
        .bind(p.sid)
        .execute(&mut *holder)
        .await
        .unwrap();
    let release = async {
        super::helpers::wait_for_lock_waiters(&pool, 1).await;
        holder.commit().await.unwrap();
    };
    let ((s, body), ()) = tokio::join!(
        answer(&p.app, &p.dev, &c, &bytes, Decision::Approve),
        release
    );
    assert_eq!(
        (s, body["error"].as_str()),
        (409, Some("APPROVAL_STATE")),
        "{body}"
    );
    assert_eq!(
        attempt_state(&pool, &id).await,
        ("expired".into(), Some("session_ended".into()))
    );
}

/// `can_set_up_this_pc` is true exactly when a start would be accepted: never
/// for a request without a real session snapshot (a legacy token), never for
/// a banned account.
#[sqlx::test]
async fn can_set_up_this_pc_matches_what_start_accepts(pool: PgPool) {
    let p = pc(&pool).await;
    let ctx = vc_server::auth::ctx::AuthCtx::from(&p.app.state);
    let mut conn = pool.acquire().await.unwrap();
    let auth = super::helpers::login::auth_user(&p.app, p.sid).await;
    let overview = vc_server::auth::security_overview::overview(&mut conn, &ctx, &auth)
        .await
        .unwrap();
    assert!(overview.can_set_up_this_pc);

    // A legacy request: no session id and no snapshot.
    let mut legacy = auth.clone();
    legacy.session_id = None;
    legacy.session = None;
    let overview = vc_server::auth::security_overview::overview(&mut conn, &ctx, &legacy)
        .await
        .unwrap();
    assert!(!overview.can_set_up_this_pc);
    let mut tx = pool.begin().await.unwrap();
    let e = vc_server::approvals::pc_setup::transition::start_tx(&mut tx, &ctx, &legacy)
        .await
        .unwrap_err();
    assert!(
        matches!(
            e,
            vc_server::approvals::error::ApprovalError::PcSetupUnavailable
        ),
        "{e:?}"
    );
    drop(tx);

    // A banned account: neither the button nor the start.
    sqlx::query("INSERT INTO global_bans (user_id, banned_by, reason) VALUES ($1, $1, 'test')")
        .bind(p.user)
        .execute(&pool)
        .await
        .unwrap();
    let overview = vc_server::auth::security_overview::overview(&mut conn, &ctx, &auth)
        .await
        .unwrap();
    assert!(!overview.can_set_up_this_pc);
    let mut tx = pool.begin().await.unwrap();
    let e = vc_server::approvals::pc_setup::transition::start_tx(&mut tx, &ctx, &auth)
        .await
        .unwrap_err();
    assert!(
        matches!(
            e,
            vc_server::approvals::error::ApprovalError::PcSetupUnavailable
        ),
        "{e:?}"
    );
}
