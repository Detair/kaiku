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
pub struct Pc {
    pub app: TestApp,
    pub user: Uuid,
    pub sid: Uuid,
    pub token: String,
    pub dev: KeyDeviceFixture,
}

async fn pc(pool: &PgPool) -> Pc {
    let app = TestApp::with_passkeys(pool.clone()).await;
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
pub async fn call(
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
pub async fn started(p: &Pc) -> Value {
    let (s, body) = start(&p.app, &p.token).await;
    assert_eq!(s, 201, "{body}");
    body
}

/// The b64u token and its bytes from a QR payload.
pub fn qr_token(payload: &Value) -> (String, Vec<u8>) {
    let p = payload.as_str().expect("qr payload");
    let t = p
        .strip_prefix(QR_PREFIX)
        .expect("pcsetup payload")
        .to_string();
    let bytes = b64u_decode(&t).unwrap();
    (t, bytes)
}

pub fn id_of(v: &Value) -> String {
    v["attempt_id"].as_str().unwrap().to_string()
}

pub async fn claim(app: &TestApp, dev: &KeyDeviceFixture, token: &str) -> (u16, Value) {
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
pub async fn answer(
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

pub async fn answer_raw(
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
pub async fn claimed(p: &Pc) -> (String, Value, Vec<u8>) {
    let a = started(p).await;
    let (token, bytes) = qr_token(&a["qr_payload"]);
    let (s, c) = claim(&p.app, &p.dev, &token).await;
    assert_eq!(s, 201, "{c}");
    (id_of(&a), c, bytes)
}

pub async fn attempt_state(pool: &PgPool, id: &str) -> (String, Option<String>) {
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
    // Nothing is written for a refused scanner; the PC's poll ends it.
    assert_eq!(attempt_state(&pool, &id_of(&a)).await.0, "waiting");
    let (_, st) = pc_poll(&p, &id_of(&a)).await;
    assert_eq!(st["reason"].as_str(), Some("may_approve_off"), "{st}");
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
pub fn contains_code(text: &str, code: &str) -> bool {
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
    // The revoked device gets the key-device refusal; the attempt ends.
    let (s, body) = answer(&p.app, &p.dev, &c, &bytes, Decision::Report).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (403, Some("KEY_DEVICE_REQUIRED")),
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
            SET created_user_agent = 'Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0',
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
async fn a_refresh_cannot_change_the_description_the_phone_signs(pool: PgPool) {
    let p = pc(&pool).await;
    // Created by Firefox on Linux; a later refresh sent another User-Agent.
    sqlx::query(
        "UPDATE sessions
            SET created_user_agent = 'Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0',
                user_agent = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 14_5) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15'
          WHERE id = $1",
    )
    .bind(p.sid)
    .execute(&pool)
    .await
    .unwrap();
    let (_, c, _) = claimed(&p).await;
    let desc = c["context"]["client_description"].as_str().unwrap();
    assert!(desc.contains("Firefox") && desc.contains("Linux"), "{desc}");
    assert!(
        !desc.contains("Safari") && !desc.contains("macOS"),
        "{desc}"
    );
}

#[sqlx::test]
async fn a_session_created_without_a_client_never_gets_one_later(pool: PgPool) {
    let p = pc(&pool).await;
    // Created without a User-Agent; a later refresh sent one.
    sqlx::query(
        "UPDATE sessions
            SET created_user_agent = NULL,
                user_agent = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 14_5) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15'
          WHERE id = $1",
    )
    .bind(p.sid)
    .execute(&pool)
    .await
    .unwrap();
    let (_, c, _) = claimed(&p).await;
    // The fixed description of its client class, nothing the holder sent later.
    assert_eq!(c["context"]["client_description"], "Web browser");
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
    // The old approval goes nowhere: its attempt ended (C26: 409).
    let (s, body) = answer(&p.app, &p.dev, &c, &bytes, Decision::Approve).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (409, Some("APPROVAL_STATE")),
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

// ============================================================================
// Code entry, the `pc_setup` enrolment and the web registration (Task 3)
// ============================================================================

use vc_server::approvals::pc_setup::enrolment::{enter_code, CodeOutcome, CodePath};

use super::helpers::login::TEST_ORIGIN;
use super::helpers::passkeys::SoftAuthenticator;

/// A 6-digit code that is not `code`.
pub fn wrong(code: &str) -> String {
    let first = (code.as_bytes()[0] - b'0' + 1) % 10;
    format!("{first}{}", &code[1..])
}

/// Start, scan, approve: `(attempt id, code)`.
pub async fn with_code(p: &Pc) -> (String, String) {
    let (id, c, bytes) = claimed(p).await;
    let (s, ok) = answer(&p.app, &p.dev, &c, &bytes, Decision::Approve).await;
    assert_eq!(s, 200, "{ok}");
    (id, ok["code"].as_str().unwrap().to_string())
}

/// Type `code` on the PC for attempt `id` with `bearer`.
pub async fn enter(p: &Pc, bearer: &str, id: &str, code: &str) -> (u16, Value) {
    call(
        &p.app,
        Method::POST,
        &format!("/auth/pc-setups/{id}/code"),
        bearer,
        Some(json!({ "code": code })),
    )
    .await
}

/// The attempt in `enrolled`: `(attempt id, enrolment id)`.
async fn enrolled(p: &Pc) -> (String, String) {
    let (id, code) = with_code(p).await;
    let (s, body) = enter(p, &p.token, &id, &code).await;
    assert_eq!(
        (s, body["state"].as_str()),
        (200, Some("enrolled")),
        "{body}"
    );
    (id, body["enrollment_id"].as_str().unwrap().to_string())
}

pub async fn passkey_start(p: &Pc, bearer: &str, id: &str) -> (u16, Value) {
    call(
        &p.app,
        Method::POST,
        &format!("/auth/pc-setups/{id}/passkey/start"),
        bearer,
        Some(json!({})),
    )
    .await
}

pub async fn passkey_finish(p: &Pc, bearer: &str, id: &str, body: Value) -> (u16, Value) {
    call(
        &p.app,
        Method::POST,
        &format!("/auth/pc-setups/{id}/passkey/finish"),
        bearer,
        Some(body),
    )
    .await
}

/// Start the ceremony and answer it with a soft passkey: the finish body.
async fn ceremony(p: &Pc, id: &str) -> Value {
    let (s, start) = passkey_start(p, &p.token, id).await;
    assert_eq!(s, 200, "{start}");
    let credential = SoftAuthenticator::new().register(TEST_ORIGIN, &start["options"]);
    json!({"ceremony_id": start["ceremony_id"], "credential": credential, "label": "Work PC"})
}

/// Run the whole flow to a passkey: `(attempt id, passkey id)`.
async fn completed(p: &Pc) -> (String, Uuid) {
    let (id, _) = enrolled(p).await;
    let body = ceremony(p, &id).await;
    let (s, pk) = passkey_finish(p, &p.token, &id, body).await;
    assert_eq!(s, 201, "{pk}");
    (id, pk["id"].as_str().unwrap().parse().unwrap())
}

pub async fn enrollment_state(pool: &PgPool, id: &str) -> String {
    sqlx::query_scalar("SELECT state FROM enrollments WHERE id = $1::uuid")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

pub async fn passkeys_of(pool: &PgPool, user: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM passkeys WHERE user_id = $1 AND state = 'active'")
        .bind(user)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn phone_state(p: &Pc, id: &str) -> Value {
    let (s, st) = call(
        &p.app,
        Method::GET,
        &format!("/api/pc-setups/{id}?wait=0"),
        &p.dev.access_token,
        None,
    )
    .await;
    assert_eq!(s, 200, "{st}");
    st
}

async fn logout(p: &Pc) -> u16 {
    call(
        &p.app,
        Method::POST,
        "/auth/logout",
        &p.token,
        Some(json!({})),
    )
    .await
    .0
}

#[sqlx::test]
async fn third_wrong_code_ends_the_attempt(pool: PgPool) {
    let p = pc(&pool).await;
    let (id, code) = with_code(&p).await;
    let bad = wrong(&code);
    let (s, body) = enter(&p, &p.token, &id, &bad).await;
    assert_eq!(
        (s, body["state"].as_str(), body["tries_left"].as_i64()),
        (200, Some("code"), Some(2)),
        "{body}"
    );
    let (s, body) = enter(&p, &p.token, &id, &bad).await;
    assert_eq!((s, body["tries_left"].as_i64()), (200, Some(1)), "{body}");
    let (s, body) = enter(&p, &p.token, &id, &bad).await;
    assert_eq!((s, body["state"].as_str()), (200, Some("failed")), "{body}");
    assert_eq!(
        attempt_state(&pool, &id).await,
        ("failed".into(), Some("too_many_codes".into()))
    );
    // The right code no longer helps.
    let (s, body) = enter(&p, &p.token, &id, &code).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (409, Some("APPROVAL_STATE")),
        "{body}"
    );
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM enrollments WHERE subject_id = $1")
        .bind(p.user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
    // One `pc_setup_failed`, normal severity: the account's notice only.
    let (n, severity, details): (i64, String, Value) = sqlx::query_as(
        "SELECT COUNT(*) OVER (), severity, details FROM security_events
          WHERE subject_id = $1 AND event_type = 'pc_setup_failed'",
    )
    .bind(p.user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((n, severity.as_str()), (1, "normal"));
    assert_eq!(details["attempt_id"].as_str(), Some(id.as_str()));
    // Both sides say so.
    let st = phone_state(&p, &id).await;
    assert_eq!(
        (st["state"].as_str(), st["reason"].as_str()),
        (Some("failed"), Some("too_many_codes"))
    );
    let (_, st) = pc_poll(&p, &id).await;
    assert_eq!(st["state"], "failed");
}

#[sqlx::test]
async fn malformed_code_spends_no_try(pool: PgPool) {
    let p = pc(&pool).await;
    let (id, _) = with_code(&p).await;
    for bad in ["12345", "1234567", "12345a", ""] {
        let (s, body) = enter(&p, &p.token, &id, bad).await;
        assert_eq!(
            (s, body["error"].as_str()),
            (400, Some("VALIDATION_ERROR")),
            "{body}"
        );
    }
    let tries: i16 = sqlx::query_scalar("SELECT tries FROM pc_setup_attempts WHERE id = $1::uuid")
        .bind(&id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(tries, 0);
}

#[sqlx::test]
async fn expired_code_refused(pool: PgPool) {
    let p = pc(&pool).await;
    let (id, code) = with_code(&p).await;
    sqlx::query("UPDATE pc_setup_attempts SET code_expires_at = NOW() - INTERVAL '1 second' WHERE id = $1::uuid")
        .bind(&id)
        .execute(&pool)
        .await
        .unwrap();
    let (s, body) = enter(&p, &p.token, &id, &code).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (410, Some("APPROVAL_EXPIRED")),
        "{body}"
    );
    assert!(
        !code_hash_set(&pool, &id).await,
        "an expiry spends the code"
    );
    assert_eq!(
        attempt_state(&pool, &id).await,
        ("expired".into(), Some("code_expired".into()))
    );
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM enrollments WHERE subject_id = $1")
        .bind(p.user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test]
async fn right_code_creates_the_pc_setup_enrolment(pool: PgPool) {
    let p = pc(&pool).await;
    let (id, eid) = enrolled(&p).await;
    let (variant, kind, sid, dev, token, state, ttl): (
        String,
        String,
        Uuid,
        Uuid,
        Option<Vec<u8>>,
        String,
        f64,
    ) = sqlx::query_as(
        "SELECT variant, issuer_kind, issuer_session_id, issuer_device_id, setup_token_hash,
                    state, EXTRACT(EPOCH FROM expires_at - NOW())::float8
               FROM enrollments WHERE id = $1::uuid",
    )
    .bind(&eid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        (variant.as_str(), kind.as_str(), state.as_str()),
        ("pc_setup", "pc_setup", "issued")
    );
    assert_eq!((sid, dev), (p.sid, p.dev.row_id));
    assert!(token.is_none());
    assert!(ttl > 590.0 && ttl <= 600.0, "{ttl}");
    let (state, linked): (String, Option<Uuid>) =
        sqlx::query_as("SELECT state, enrollment_id FROM pc_setup_attempts WHERE id = $1::uuid")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        (state.as_str(), linked.map(|u| u.to_string())),
        ("enrolled", Some(eid))
    );
}

#[sqlx::test]
async fn registration_needs_the_requesting_session(pool: PgPool) {
    let p = pc(&pool).await;
    let (_, other) = create_probation_session_with_token(&pool, &p.app.config, p.user).await;
    let (id, code) = with_code(&p).await;
    // Another session of the account cannot type the code.
    let (s, _) = enter(&p, &other, &id, &code).await;
    assert_eq!(s, 404);
    let (s, _) = enter(&p, &p.token, &id, &code).await;
    assert_eq!(s, 200);
    // … nor start or finish the registration; no session at all gets 401.
    let (s, _) = passkey_start(&p, &other, &id).await;
    assert_eq!(s, 404);
    let body = ceremony(&p, &id).await;
    let (s, _) = passkey_finish(&p, &other, &id, body.clone()).await;
    assert_eq!(s, 404);
    let req = TestApp::request(Method::POST, &format!("/auth/pc-setups/{id}/passkey/start"))
        .header("Content-Type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    assert_eq!(p.app.oneshot(req).await.status().as_u16(), 401);
    let req = TestApp::request(
        Method::POST,
        &format!("/auth/pc-setups/{id}/passkey/finish"),
    )
    .header("Content-Type", "application/json")
    .body(Body::from(body.to_string()))
    .unwrap();
    assert_eq!(p.app.oneshot(req).await.status().as_u16(), 401);
    assert_eq!(passkeys_of(&pool, p.user).await, 0);
    // A ceremony of another attempt does not finish this one.
    let (s, _) = passkey_finish(&p, &p.token, &Uuid::new_v4().to_string(), body).await;
    assert_eq!(s, 404);
}

#[sqlx::test]
async fn start_refused_before_the_right_code(pool: PgPool) {
    let p = pc(&pool).await;
    let a = started(&p).await;
    let id = id_of(&a);
    let (s, body) = passkey_start(&p, &p.token, &id).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (409, Some("APPROVAL_STATE")),
        "{body}"
    );
    let (s, _) = enter(&p, &p.token, &id, "123456").await;
    assert_eq!(s, 409, "no code before the approval");
    let (token, bytes) = qr_token(&a["qr_payload"]);
    let (_, c) = claim(&p.app, &p.dev, &token).await;
    let (s, _) = passkey_start(&p, &p.token, &id).await;
    assert_eq!(s, 409);
    let (_, ok) = answer(&p.app, &p.dev, &c, &bytes, Decision::Approve).await;
    let code = ok["code"].as_str().unwrap();
    let (s, _) = passkey_start(&p, &p.token, &id).await;
    assert_eq!(s, 409);
    let (s, _) = enter(&p, &p.token, &id, &wrong(code)).await;
    assert_eq!(s, 200);
    let (s, _) = passkey_start(&p, &p.token, &id).await;
    assert_eq!(s, 409);
    let (s, _) = enter(&p, &p.token, &id, code).await;
    assert_eq!(s, 200);
    let (s, body) = passkey_start(&p, &p.token, &id).await;
    assert_eq!(s, 200, "{body}");
    assert!(
        body["options"]["publicKey"]["challenge"].is_string(),
        "{body}"
    );
}

#[sqlx::test]
async fn lost_code_entry_response_recovers_by_poll(pool: PgPool) {
    let p = pc(&pool).await;
    let (id, code) = with_code(&p).await;
    // The response is lost: the PC's next poll carries on.
    let _ = enter(&p, &p.token, &id, &code).await;
    let (s, st) = pc_poll(&p, &id).await;
    assert_eq!((s, st["state"].as_str()), (200, Some("enrolled")), "{st}");
    let eid = st["enrollment_id"].as_str().unwrap().to_string();
    // Typing the code again is refused and changes nothing.
    let (s, _) = enter(&p, &p.token, &id, &code).await;
    assert_eq!(s, 409);
    assert_eq!(enrollment_state(&pool, &eid).await, "issued");
    let body = ceremony(&p, &id).await;
    let (s, pk) = passkey_finish(&p, &p.token, &id, body).await;
    assert_eq!(s, 201, "{pk}");
    assert_eq!(pk["label"], "Work PC");
    assert_eq!(pk["state"], "active");
    assert_eq!(attempt_state(&pool, &id).await.0, "completed");
    assert_eq!(enrollment_state(&pool, &eid).await, "done");
    let linked: Option<Uuid> =
        sqlx::query_scalar("SELECT enrollment_id FROM passkeys WHERE id = $1::uuid")
            .bind(pk["id"].as_str().unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(linked.map(|u| u.to_string()), Some(eid));
    // The phone can say "Done".
    assert_eq!(phone_state(&p, &id).await["state"], "completed");
}

#[sqlx::test]
async fn current_attempt_resumes_after_reload(pool: PgPool) {
    let p = pc(&pool).await;
    let (id, code) = with_code(&p).await;
    // A reload in `code`: the attempt is found and the code can be typed.
    let (s, cur) = call(
        &p.app,
        Method::GET,
        "/auth/pc-setups/current",
        &p.token,
        None,
    )
    .await;
    assert_eq!(
        (s, cur["attempt_id"].as_str(), cur["state"].as_str()),
        (200, Some(id.as_str()), Some("code")),
        "{cur}"
    );
    assert!(cur.get("qr_payload").is_none());
    let (s, _) = enter(&p, &p.token, &id, &code).await;
    assert_eq!(s, 200);
    // A reload in `enrolled`: it carries the enrolment and the passkey follows.
    let (s, cur) = call(
        &p.app,
        Method::GET,
        "/auth/pc-setups/current",
        &p.token,
        None,
    )
    .await;
    assert_eq!((s, cur["state"].as_str()), (200, Some("enrolled")), "{cur}");
    assert!(cur["enrollment_id"].is_string(), "{cur}");
    let body = ceremony(&p, &id).await;
    let (s, _) = passkey_finish(&p, &p.token, &id, body).await;
    assert_eq!(s, 201);
    // Done: nothing live is left to resume.
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
async fn ending_the_session_or_revoking_the_device_voids_the_enrolment(pool: PgPool) {
    // Logout voids the enrolment in its own transaction.
    let p = pc(&pool).await;
    let (id, eid) = enrolled(&p).await;
    assert_eq!(logout(&p).await, 200);
    assert_eq!(enrollment_state(&pool, &eid).await, "expired");
    // The attempt is expired by the next locked look (here the sweep).
    vc_server::approvals::pc_setup::transition::expire_due(&pool)
        .await
        .unwrap();
    assert_eq!(
        attempt_state(&pool, &id).await,
        ("expired".into(), Some("session_ended".into()))
    );

    // Revoking the approving phone voids it too, and the finish is refused.
    let p = pc(&pool).await;
    let (id, eid) = enrolled(&p).await;
    let body = ceremony(&p, &id).await;
    revoke_device(&p).await;
    assert_eq!(enrollment_state(&pool, &eid).await, "expired");
    let (s, b) = passkey_finish(&p, &p.token, &id, body).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("APPROVAL_STATE")),
        "{b}"
    );
    assert_eq!(
        attempt_state(&pool, &id).await,
        ("expired".into(), Some("device_revoked".into()))
    );
    assert_eq!(passkeys_of(&pool, p.user).await, 0);
}

/// Revoke the fixture's phone as "Remove device" does: account lock first.
async fn revoke_device(p: &Pc) {
    let ctx = vc_server::auth::ctx::AuthCtx::from(&p.app.state);
    let mut tx = p.app.pool.begin().await.unwrap();
    vc_server::approvals::transition::lock_accounts(&mut tx, &[p.user])
        .await
        .unwrap();
    revoke_in(&mut tx, &ctx, p).await;
    tx.commit().await.unwrap();
}

async fn revoke_in(tx: &mut sqlx::PgConnection, ctx: &vc_server::auth::ctx::AuthCtx<'_>, p: &Pc) {
    vc_server::approvals::devices::revoke_tx(
        tx,
        ctx,
        p.dev.row_id,
        vc_server::approvals::devices::RevokeReason::Owner,
        vc_server::security_events::Actor::User(p.user),
    )
    .await
    .unwrap();
}

#[sqlx::test]
async fn enrolment_expiry_ends_the_attempt(pool: PgPool) {
    let p = pc(&pool).await;
    let (id, eid) = enrolled(&p).await;
    sqlx::query(
        "UPDATE enrollments SET expires_at = NOW() - INTERVAL '1 second' WHERE id = $1::uuid",
    )
    .bind(&eid)
    .execute(&pool)
    .await
    .unwrap();
    let (s, b) = passkey_start(&p, &p.token, &id).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("APPROVAL_STATE")),
        "{b}"
    );
    assert_eq!(
        attempt_state(&pool, &id).await,
        ("expired".into(), Some("enrollment_expired".into()))
    );
    assert_eq!(enrollment_state(&pool, &eid).await, "expired");
}

#[sqlx::test]
async fn restart_cancels_the_previous_enrolment_and_its_ceremonies(pool: PgPool) {
    let p = pc(&pool).await;
    let (id, eid) = enrolled(&p).await;
    let body = ceremony(&p, &id).await;
    let fresh = started(&p).await;
    assert_ne!(id_of(&fresh), id);
    assert_eq!(
        attempt_state(&pool, &id).await,
        ("cancelled".into(), Some("restarted".into()))
    );
    assert_eq!(enrollment_state(&pool, &eid).await, "cancelled");
    // The old ceremony finishes nothing.
    let (s, b) = passkey_finish(&p, &p.token, &id, body).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("APPROVAL_STATE")),
        "{b}"
    );
    assert_eq!(passkeys_of(&pool, p.user).await, 0);
    // The PC's cancel does the same.
    let p = pc(&pool).await;
    let (id, eid) = enrolled(&p).await;
    let (s, _) = call(
        &p.app,
        Method::DELETE,
        &format!("/auth/pc-setups/{id}"),
        &p.token,
        None,
    )
    .await;
    assert_eq!(s, 204);
    assert_eq!(enrollment_state(&pool, &eid).await, "cancelled");

    // Every end spends the code: a restart and a cancel in `code` drop it.
    let p = pc(&pool).await;
    let (restarted, _) = with_code(&p).await;
    assert!(code_hash_set(&pool, &restarted).await);
    started(&p).await;
    assert_eq!(attempt_state(&pool, &restarted).await.0, "cancelled");
    assert!(
        !code_hash_set(&pool, &restarted).await,
        "a restart spends the code"
    );
    let p = pc(&pool).await;
    let (cancelled, _) = with_code(&p).await;
    let (s, _) = call(
        &p.app,
        Method::DELETE,
        &format!("/auth/pc-setups/{cancelled}"),
        &p.token,
        None,
    )
    .await;
    assert_eq!(s, 204);
    assert!(
        !code_hash_set(&pool, &cancelled).await,
        "a cancel spends the code"
    );
}

async fn code_hash_set(pool: &PgPool, id: &str) -> bool {
    sqlx::query_scalar("SELECT code_hash IS NOT NULL FROM pc_setup_attempts WHERE id = $1::uuid")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Locking 6: a restart racing the finish has one winner.
#[sqlx::test]
async fn restart_racing_finish_has_one_winner(pool: PgPool) {
    for _ in 0..4 {
        let p = pc(&pool).await;
        let (id, eid) = enrolled(&p).await;
        let body = ceremony(&p, &id).await;
        let ((fs, fb), (rs, rb)) = tokio::join!(
            passkey_finish(&p, &p.token, &id, body),
            start(&p.app, &p.token)
        );
        assert_eq!(rs, 201, "{rb}");
        let (state, reason) = attempt_state(&pool, &id).await;
        if fs == 201 {
            assert_eq!(state, "completed", "{fb}");
            assert_eq!(enrollment_state(&pool, &eid).await, "done");
            assert_eq!(passkeys_of(&pool, p.user).await, 1);
        } else {
            assert_eq!(
                (fs, fb["error"].as_str()),
                (409, Some("APPROVAL_STATE")),
                "{fb}"
            );
            assert_eq!(
                (state.as_str(), reason.as_deref()),
                ("cancelled", Some("restarted"))
            );
            assert_eq!(enrollment_state(&pool, &eid).await, "cancelled");
            assert_eq!(passkeys_of(&pool, p.user).await, 0);
        }
    }
    // The restart committed first: the finish fails.
    let p = pc(&pool).await;
    let (id, _) = enrolled(&p).await;
    let body = ceremony(&p, &id).await;
    let mut holder = pool.begin().await.unwrap();
    let mut locked =
        vc_server::approvals::pc_setup::transition::lock_attempt(&mut holder, id.parse().unwrap())
            .await
            .unwrap();
    let finish = passkey_finish(&p, &p.token, &id, body);
    let release = async {
        super::helpers::wait_for_lock_waiters(&pool, 1).await;
        vc_server::approvals::pc_setup::transition::cancel_attempt_tx(
            &mut holder,
            &mut locked,
            vc_server::approvals::pc_setup::transition::EndCause::Restarted,
        )
        .await
        .unwrap();
        holder.commit().await.unwrap();
    };
    let ((s, b), ()) = tokio::join!(finish, release);
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("APPROVAL_STATE")),
        "{b}"
    );
    assert_eq!(passkeys_of(&pool, p.user).await, 0);
}

#[sqlx::test]
async fn passkey_limit_at_finish_leaves_a_readable_state(pool: PgPool) {
    use vc_server::auth::passkeys::queries::PasskeyState;
    let p = pc(&pool).await;
    for _ in 0..9 {
        super::helpers::passkeys::seed_passkey(&p.app, p.user, PasskeyState::Active).await;
    }
    let (id, eid) = enrolled(&p).await;
    let body = ceremony(&p, &id).await;
    // The account fills up between start and finish.
    super::helpers::passkeys::seed_passkey(&p.app, p.user, PasskeyState::Active).await;
    let (s, b) = passkey_finish(&p, &p.token, &id, body).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("PASSKEY_LIMIT")),
        "{b}"
    );
    assert_eq!(passkeys_of(&pool, p.user).await, 10);
    // The PC reads a live attempt it can leave; the phone never says "Done".
    let (s, st) = pc_poll(&p, &id).await;
    assert_eq!((s, st["state"].as_str()), (200, Some("enrolled")), "{st}");
    assert_eq!(enrollment_state(&pool, &eid).await, "issued");
    assert_eq!(phone_state(&p, &id).await["state"], "enrolled");
    let (s, b) = passkey_start(&p, &p.token, &id).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("PASSKEY_LIMIT")),
        "{b}"
    );
    let (s, _) = call(
        &p.app,
        Method::DELETE,
        &format!("/auth/pc-setups/{id}"),
        &p.token,
        None,
    )
    .await;
    assert_eq!(s, 204);
    assert_eq!(phone_state(&p, &id).await["state"], "cancelled");
}

#[sqlx::test]
async fn already_registered_passkey_leaves_a_readable_state(pool: PgPool) {
    let p = pc(&pool).await;
    let (id, eid) = enrolled(&p).await;
    let body = ceremony(&p, &id).await;
    // The same credential is registered meanwhile (another path).
    let cred_id = b64u_decode(body["credential"]["rawId"].as_str().unwrap()).unwrap();
    sqlx::query(
        "INSERT INTO passkeys (id, user_id, credential_id, credential, label, state,
                               backup_eligible, backup_state)
         VALUES ($1, $2, $3, '{}'::jsonb, 'Elsewhere', 'active', false, false)",
    )
    .bind(Uuid::new_v4())
    .bind(p.user)
    .bind(cred_id)
    .execute(&pool)
    .await
    .unwrap();
    let (s, b) = passkey_finish(&p, &p.token, &id, body).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("PASSKEY_ALREADY_REGISTERED")),
        "{b}"
    );
    assert_eq!(passkeys_of(&pool, p.user).await, 1);
    let (s, st) = pc_poll(&p, &id).await;
    assert_eq!((s, st["state"].as_str()), (200, Some("enrolled")), "{st}");
    assert_eq!(enrollment_state(&pool, &eid).await, "issued");
    assert_eq!(phone_state(&p, &id).await["state"], "enrolled");
}

/// The `authenticator_added` event of the flow's passkey.
async fn added_event(pool: &PgPool, passkey: Uuid) -> (Uuid, String, Value) {
    sqlx::query_as(
        "SELECT id, severity, details FROM security_events
          WHERE event_type = 'authenticator_added' AND authenticator_id = $1",
    )
    .bind(passkey)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test]
async fn notice_names_pc_and_phone(pool: PgPool) {
    let p = pc(&pool).await;
    let (id, c, bytes) = claimed(&p).await;
    let desc = c["context"]["client_description"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, ok) = answer(&p.app, &p.dev, &c, &bytes, Decision::Approve).await;
    let (s, _) = enter(&p, &p.token, &id, ok["code"].as_str().unwrap()).await;
    assert_eq!(s, 200);
    let body = ceremony(&p, &id).await;
    let (s, pk) = passkey_finish(&p, &p.token, &id, body).await;
    assert_eq!(s, 201, "{pk}");
    let (_, severity, details) =
        added_event(&pool, pk["id"].as_str().unwrap().parse().unwrap()).await;
    assert_eq!(severity, "high", "the notice goes to every address");
    assert_eq!(details["type"], "passkey");
    assert_eq!(details["via"], "pc_setup");
    assert_eq!(
        details["requesting_sid"].as_str(),
        Some(p.sid.to_string().as_str())
    );
    assert_eq!(details["attempt_id"].as_str(), Some(id.as_str()));
    assert_eq!(
        details["client_description"].as_str(),
        Some(desc.as_str()),
        "the PC"
    );
    assert_eq!(details["approving_device_label"], "Pixel 8", "the phone");
    assert_eq!(
        details["approving_device_id"].as_str(),
        Some(p.dev.row_id.to_string().as_str())
    );
    // The owner sees it with "That wasn't me".
    let (_, log) = call(
        &p.app,
        Method::GET,
        "/api/me/security-events",
        &p.dev.access_token,
        None,
    )
    .await;
    let ev = log["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["event_type"] == "authenticator_added")
        .unwrap_or_else(|| panic!("{log}"))
        .clone();
    assert_eq!(ev["details"]["approving_device_label"], "Pixel 8");
    assert!(
        ev["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["action"] == "repudiate"),
        "{ev}"
    );
}

/// "That wasn't me" from the phone (part 1: the phone is older).
async fn repudiate(p: &Pc, event: Uuid) -> (u16, Value) {
    super::helpers::grant_step_up(
        &p.app.pool,
        p.dev.session_id.unwrap(),
        "event.repudiate",
        &event.to_string(),
    )
    .await;
    call(
        &p.app,
        Method::POST,
        &format!("/api/me/security-events/{event}/repudiate"),
        &p.dev.access_token,
        Some(json!({})),
    )
    .await
}

#[sqlx::test]
async fn that_wasnt_me_revokes_the_passkey_and_ends_the_requesting_session(pool: PgPool) {
    let p = pc(&pool).await;
    let (_, pk) = completed(&p).await;
    // A sign-in with the new passkey.
    let (pk_sid, _) =
        super::helpers::passkeys::passkey_session(&pool, &p.app.config, p.user, pk).await;
    let (other_sid, _) = create_probation_session_with_token(&pool, &p.app.config, p.user).await;
    let (event, _, _) = added_event(&pool, pk).await;
    let (s, out) = repudiate(&p, event).await;
    assert_eq!(s, 200, "{out}");
    let state: String = sqlx::query_scalar("SELECT state FROM passkeys WHERE id = $1")
        .bind(pk)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "revoked");
    assert!(
        !session_live(&pool, pk_sid).await,
        "the passkey's sessions end"
    );
    assert!(
        !session_live(&pool, p.sid).await,
        "the requesting session ends"
    );
    assert!(session_live(&pool, other_sid).await);
    assert!(session_live(&pool, p.dev.session_id.unwrap()).await);
    let ended: Vec<String> = out["ended_session_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(ended.contains(&p.sid.to_string()), "{out}");
}

#[sqlx::test]
async fn that_wasnt_me_marks_no_credential(pool: PgPool) {
    let p = pc(&pool).await;
    let (_, pk) = completed(&p).await;
    let (event, _, _) = added_event(&pool, pk).await;
    let (s, out) = repudiate(&p, event).await;
    assert_eq!(s, 200, "{out}");
    assert_eq!(out["compromised"], json!([]));
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
}

/// B.2 policy under part 2 (a fresh password, TOTP or recovery code, no
/// grant): nothing is revoked, so the passkey stays, but the requesting
/// session still ends. The route offers part 2 only to an account without an
/// active strong authenticator, which this flow's account never is (the phone
/// and the new passkey are both active), so the core is called directly
/// under the account lock, as the route calls it.
#[sqlx::test]
async fn that_wasnt_me_through_part2_ends_the_session_but_keeps_the_passkey(pool: PgPool) {
    let p = pc(&pool).await;
    let (_, pk) = completed(&p).await;
    let (pk_sid, _) =
        super::helpers::passkeys::passkey_session(&pool, &p.app.config, p.user, pk).await;
    let (event, _, _) = added_event(&pool, pk).await;
    let ctx = vc_server::auth::ctx::AuthCtx::from(&p.app.state);
    let mut tx = pool.begin().await.unwrap();
    vc_server::approvals::transition::lock_accounts(&mut tx, &[p.user])
        .await
        .unwrap();
    let (out, _) = vc_server::auth::repudiate::repudiate_tx(
        &mut tx,
        &ctx,
        event,
        &vc_server::auth::owner_rule::OwnerAuthority::Part2 { factor: "totp" },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert!(out.revoked_authenticator_ids.is_empty(), "{out:?}");
    assert!(out.compromised.is_empty(), "{out:?}");
    assert_eq!(out.ended_session_ids, vec![p.sid]);
    let state: String = sqlx::query_scalar("SELECT state FROM passkeys WHERE id = $1")
        .bind(pk)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "active", "part 2 never revokes");
    assert!(
        session_live(&pool, pk_sid).await,
        "the passkey's sessions stay"
    );
    assert!(
        !session_live(&pool, p.sid).await,
        "the requesting session ends"
    );
}

// ----------------------------------------------------------------------------
// Lock order (spec Locking 3–6): each pair runs concurrently a few times
// (no deadlock, no 500, one consistent outcome) and once with the
// session-ending side committing first (the other side sees it).
// ----------------------------------------------------------------------------

/// Hold the PC session's row as logout and refresh do (no account lock);
/// `end` then ends it in that transaction with `reason`.
async fn hold_session(pool: &PgPool, sid: Uuid) -> sqlx::Transaction<'static, sqlx::Postgres> {
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM sessions WHERE id = $1 FOR UPDATE")
        .bind(sid)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx
}

async fn end_held(
    mut tx: sqlx::Transaction<'static, sqlx::Postgres>,
    p: &Pc,
    reason: vc_server::auth::revocation::EndReason,
) {
    let ctx = vc_server::auth::ctx::AuthCtx::from(&p.app.state);
    vc_server::auth::revocation::end_sessions_tx(&mut tx, &ctx, p.user, &[p.sid], reason)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

#[sqlx::test]
async fn logout_against_report(pool: PgPool) {
    for _ in 0..4 {
        let p = pc(&pool).await;
        let (id, c, bytes) = claimed(&p).await;
        let ((s, b), ls) = tokio::join!(
            answer(&p.app, &p.dev, &c, &bytes, Decision::Report),
            logout(&p)
        );
        assert!(s == 200 || s == 409, "{s} {b}");
        assert!(ls == 200 || ls == 401, "{ls}");
        assert!(!session_live(&pool, p.sid).await);
        let (state, _) = attempt_state(&pool, &id).await;
        assert_eq!(state, if s == 200 { "reported" } else { "expired" });
    }
    // Logout commits first: the report sees the ended session and does nothing.
    let p = pc(&pool).await;
    let (id, c, bytes) = claimed(&p).await;
    let held = hold_session(&pool, p.sid).await;
    let release = async {
        super::helpers::wait_for_lock_waiters(&pool, 1).await;
        end_held(held, &p, vc_server::auth::revocation::EndReason::Logout).await;
    };
    let ((s, b), ()) = tokio::join!(
        answer(&p.app, &p.dev, &c, &bytes, Decision::Report),
        release
    );
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("APPROVAL_STATE")),
        "{b}"
    );
    assert_eq!(
        attempt_state(&pool, &id).await,
        ("expired".into(), Some("session_ended".into()))
    );
    let reported: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM security_events WHERE subject_id = $1 AND event_type = 'pc_setup_reported'",
    )
    .bind(p.user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(reported, 0);
}

#[sqlx::test]
async fn logout_against_finish(pool: PgPool) {
    for _ in 0..4 {
        let p = pc(&pool).await;
        let (id, eid) = enrolled(&p).await;
        let body = ceremony(&p, &id).await;
        let ((s, b), ls) = tokio::join!(passkey_finish(&p, &p.token, &id, body), logout(&p));
        assert!(ls == 200 || ls == 401, "{ls}");
        assert!(!session_live(&pool, p.sid).await);
        if s == 201 {
            // Finish first: the logout's cascade saw `done` and left it.
            assert_eq!(enrollment_state(&pool, &eid).await, "done");
            assert_eq!(passkeys_of(&pool, p.user).await, 1);
        } else {
            assert!(s == 401 || s == 409, "{s} {b}");
            assert_eq!(enrollment_state(&pool, &eid).await, "expired");
            assert_eq!(passkeys_of(&pool, p.user).await, 0);
        }
    }
    // Logout commits first: no passkey survives it.
    let p = pc(&pool).await;
    let (id, eid) = enrolled(&p).await;
    let body = ceremony(&p, &id).await;
    let held = hold_session(&pool, p.sid).await;
    let release = async {
        super::helpers::wait_for_lock_waiters(&pool, 1).await;
        end_held(held, &p, vc_server::auth::revocation::EndReason::Logout).await;
    };
    let ((s, b), ()) = tokio::join!(passkey_finish(&p, &p.token, &id, body), release);
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("APPROVAL_STATE")),
        "{b}"
    );
    assert_eq!(passkeys_of(&pool, p.user).await, 0);
    assert_eq!(enrollment_state(&pool, &eid).await, "expired");
    assert_eq!(
        attempt_state(&pool, &id).await,
        ("expired".into(), Some("session_ended".into()))
    );
}

#[sqlx::test]
async fn refresh_reuse_against_code_entry(pool: PgPool) {
    use vc_server::auth::revocation::EndReason;
    for _ in 0..4 {
        let p = pc(&pool).await;
        let (id, code) = with_code(&p).await;
        let ((s, b), ()) = tokio::join!(enter(&p, &p.token, &id, &code), async {
            let held = hold_session(&pool, p.sid).await;
            end_held(held, &p, EndReason::RefreshReuse).await;
        });
        assert!(s == 200 || s == 401 || s == 409, "{s} {b}");
        let (state, _) = attempt_state(&pool, &id).await;
        if s == 200 {
            // The code entry came first; the end's cascade voided its enrolment.
            assert_eq!(state, "enrolled");
            let eid = b["enrollment_id"].as_str().unwrap();
            assert_eq!(enrollment_state(&pool, eid).await, "expired");
        } else {
            // The end came first: nothing advanced and no enrolment exists.
            assert_ne!(state, "enrolled", "{s} {b}");
            let n: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM enrollments WHERE subject_id = $1")
                    .bind(p.user)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(n, 0, "{s} {b}");
        }
        assert!(!session_live(&pool, p.sid).await);
    }
    // The reuse commits first: the code entry creates nothing.
    let p = pc(&pool).await;
    let (id, code) = with_code(&p).await;
    let held = hold_session(&pool, p.sid).await;
    let release = async {
        super::helpers::wait_for_lock_waiters(&pool, 1).await;
        end_held(held, &p, EndReason::RefreshReuse).await;
    };
    let ((s, b), ()) = tokio::join!(enter(&p, &p.token, &id, &code), release);
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("APPROVAL_STATE")),
        "{b}"
    );
    assert_eq!(
        attempt_state(&pool, &id).await,
        ("expired".into(), Some("session_ended".into()))
    );
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM enrollments WHERE subject_id = $1")
        .bind(p.user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);

    // The code entry commits first: the end waits for the session row and its
    // cascade sees the new enrolment.
    let p = pc(&pool).await;
    let (id, code) = with_code(&p).await;
    let ctx = vc_server::auth::ctx::AuthCtx::from(&p.app.state);
    let mut holder = pool.begin().await.unwrap();
    let locked =
        vc_server::approvals::pc_setup::transition::lock_attempt(&mut holder, id.parse().unwrap())
            .await
            .unwrap();
    let CodeOutcome::Enrolled { enrollment_id } =
        enter_code(&mut holder, &ctx, &locked, &code, CodePath::Web)
            .await
            .unwrap()
    else {
        panic!("the right code enrols");
    };
    let end = async {
        let held = hold_session(&pool, p.sid).await;
        end_held(held, &p, EndReason::RefreshReuse).await;
    };
    let release = async {
        super::helpers::wait_for_lock_waiters(&pool, 1).await;
        holder.commit().await.unwrap();
    };
    tokio::join!(end, release);
    assert_eq!(
        enrollment_state(&pool, &enrollment_id.to_string()).await,
        "expired"
    );
    assert!(!session_live(&pool, p.sid).await);
}

#[sqlx::test]
async fn device_removal_against_finish(pool: PgPool) {
    for _ in 0..4 {
        let p = pc(&pool).await;
        let (id, eid) = enrolled(&p).await;
        let body = ceremony(&p, &id).await;
        let ((s, b), ()) = tokio::join!(passkey_finish(&p, &p.token, &id, body), revoke_device(&p));
        if s == 201 {
            assert_eq!(enrollment_state(&pool, &eid).await, "done");
            assert_eq!(passkeys_of(&pool, p.user).await, 1);
        } else {
            assert_eq!(
                (s, b["error"].as_str()),
                (409, Some("APPROVAL_STATE")),
                "{b}"
            );
            assert_eq!(enrollment_state(&pool, &eid).await, "expired");
            assert_eq!(passkeys_of(&pool, p.user).await, 0);
        }
    }
    // The removal commits first: the finish sees the revoked phone.
    let p = pc(&pool).await;
    let (id, eid) = enrolled(&p).await;
    let body = ceremony(&p, &id).await;
    let ctx = vc_server::auth::ctx::AuthCtx::from(&p.app.state);
    let mut holder = pool.begin().await.unwrap();
    vc_server::approvals::transition::lock_accounts(&mut holder, &[p.user])
        .await
        .unwrap();
    let release = async {
        super::helpers::wait_for_lock_waiters(&pool, 1).await;
        revoke_in(&mut holder, &ctx, &p).await;
        holder.commit().await.unwrap();
    };
    let ((s, b), ()) = tokio::join!(passkey_finish(&p, &p.token, &id, body), release);
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("APPROVAL_STATE")),
        "{b}"
    );
    assert_eq!(passkeys_of(&pool, p.user).await, 0);
    assert_eq!(enrollment_state(&pool, &eid).await, "expired");
    assert_eq!(
        attempt_state(&pool, &id).await,
        ("expired".into(), Some("device_revoked".into()))
    );
}

// ============================================================================
// Polling never starves the flow (real rate limiter)
// ============================================================================

/// What the two clients poll in about 90 seconds at their densest (one poll a
/// second each, as if every long-poll came back at once): the PC from the
/// start, the phone once it shows the code. Any answer is fine, also 429.
async fn ninety_seconds_of_polling(p: &Pc, id: &str, phone: bool) {
    for _ in 0..90 {
        call(
            &p.app,
            Method::GET,
            &format!("/auth/pc-setups/{id}?wait=0"),
            &p.token,
            None,
        )
        .await;
        if phone {
            // The phone's poll never runs into a limit the person can hit.
            let (s, b) = call(
                &p.app,
                Method::GET,
                &format!("/api/pc-setups/{id}?wait=0"),
                &p.dev.access_token,
                None,
            )
            .await;
            assert_eq!(s, 200, "{b}");
        }
    }
}

#[sqlx::test]
async fn a_slow_person_finishes_with_the_real_rate_limiter(pool: PgPool) {
    let mut p = pc(&pool).await;
    let (router, _limiter) = super::helpers::passkeys::limited_router(&p.app).await;
    p.app.router = router;
    let a = started(&p).await;
    let id = id_of(&a);
    ninety_seconds_of_polling(&p, &id, false).await;
    let (token, bytes) = qr_token(&a["qr_payload"]);
    // The QR rotated meanwhile in real life; here the first one is still current.
    let (s, c) = claim(&p.app, &p.dev, &token).await;
    assert_eq!(s, 201, "{c}");
    ninety_seconds_of_polling(&p, &id, false).await;
    let (s, ok) = answer(&p.app, &p.dev, &c, &bytes, Decision::Approve).await;
    assert_eq!(s, 200, "{ok}");
    ninety_seconds_of_polling(&p, &id, true).await;
    let (s, body) = enter(&p, &p.token, &id, ok["code"].as_str().unwrap()).await;
    assert_eq!(
        (s, body["state"].as_str()),
        (200, Some("enrolled")),
        "{body}"
    );
    ninety_seconds_of_polling(&p, &id, true).await;
    let body = ceremony(&p, &id).await;
    ninety_seconds_of_polling(&p, &id, true).await;
    let (s, pk) = passkey_finish(&p, &p.token, &id, body).await;
    assert_eq!(s, 201, "{pk}");
}

#[sqlx::test]
async fn starts_are_bounded_per_session_and_per_account(pool: PgPool) {
    let p = pc(&pool).await;
    // Ten starts of other sessions of the account do not block this one.
    for _ in 0..10 {
        let sid = probation_session(&pool, p.user).await;
        insert_attempt(&pool, p.user, sid, "cancelled")
            .await
            .unwrap();
    }
    started(&p).await;
    // Ten starts of this session within ten minutes are its limit.
    sqlx::query("UPDATE pc_setup_attempts SET state = 'cancelled', reason = 'cancelled' WHERE session_id = $1")
        .bind(p.sid)
        .execute(&pool)
        .await
        .unwrap();
    for _ in 0..9 {
        insert_attempt(&pool, p.user, p.sid, "cancelled")
            .await
            .unwrap();
    }
    let (s, body) = start(&p.app, &p.token).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (429, Some("TOO_MANY_ATTEMPTS")),
        "{body}"
    );
    // Older starts do not count.
    sqlx::query(
        "UPDATE pc_setup_attempts SET created_at = NOW() - INTERVAL '11 minutes',
                expires_at = NOW() - INTERVAL '1 minute' WHERE user_id = $1",
    )
    .bind(p.user)
    .execute(&pool)
    .await
    .unwrap();
    started(&p).await;
    // Thirty starts of the account within ten minutes are the account's ceiling.
    for _ in 0..29 {
        let sid = probation_session(&pool, p.user).await;
        insert_attempt(&pool, p.user, sid, "cancelled")
            .await
            .unwrap();
    }
    let (s, body) = start(&p.app, &p.token).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (429, Some("TOO_MANY_ATTEMPTS")),
        "{body}"
    );
}

#[sqlx::test]
async fn old_attempts_are_cleaned_up(pool: PgPool) {
    let p = pc(&pool).await;
    let old = insert_attempt(&pool, p.user, p.sid, "cancelled")
        .await
        .unwrap();
    sqlx::query("UPDATE pc_setup_attempts SET created_at = NOW() - INTERVAL '31 days',
                        expires_at = NOW() - INTERVAL '31 days' + INTERVAL '10 minutes' WHERE id = $1")
        .bind(old)
        .execute(&pool)
        .await
        .unwrap();
    let recent = insert_attempt(&pool, p.user, p.sid, "expired")
        .await
        .unwrap();
    vc_server::db::cleanup_finished_approvals(&pool)
        .await
        .unwrap();
    let left: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM pc_setup_attempts WHERE user_id = $1")
        .bind(p.user)
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(left, vec![recent]);
}

#[sqlx::test]
async fn a_stale_locked_value_never_advances_an_attempt(pool: PgPool) {
    let p = pc(&pool).await;
    let (id, code) = with_code(&p).await;
    let ctx = vc_server::auth::ctx::AuthCtx::from(&p.app.state);
    let mut tx = pool.begin().await.unwrap();
    let locked =
        vc_server::approvals::pc_setup::transition::lock_attempt(&mut tx, id.parse().unwrap())
            .await
            .unwrap();
    // The row moved on after it was read (a caller holding an old value).
    sqlx::query("UPDATE pc_setup_attempts SET state = 'cancelled', reason = 'cancelled' WHERE id = $1::uuid")
        .bind(&id)
        .execute(&mut *tx)
        .await
        .unwrap();
    let out = enter_code(&mut tx, &ctx, &locked, &code, CodePath::Web).await;
    // An internal error: `settle` rolls the whole transition back.
    assert!(
        matches!(
            out,
            Err(vc_server::approvals::error::ApprovalError::Database(_))
        ),
        "{out:?}"
    );
    drop(tx);
    assert_eq!(attempt_state(&pool, &id).await.0, "code");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM enrollments WHERE subject_id = $1")
        .bind(p.user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test]
async fn long_polls_hold_no_slot_without_need(pool: PgPool) {
    let p = pc(&pool).await;
    let a = started(&p).await;
    let id = id_of(&a);
    // A second waiting poll of the same attempt answers at once.
    let poll = |wait: u8| {
        let uri = format!("/auth/pc-setups/{id}?wait={wait}");
        let p = &p;
        async move { call(&p.app, Method::GET, &uri, &p.token, None).await }
    };
    let quick = async {
        // Wait until the first poll holds its place.
        let attempt: Uuid = id.parse().unwrap();
        while !vc_server::approvals::pc_setup::handlers::long_poll_is_waiting(attempt, true) {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let t = std::time::Instant::now();
        let (s, _) = poll(25).await;
        (s, t.elapsed())
    };
    let ((s1, _), (s2, quick_took)) = tokio::join!(poll(3), quick);
    assert_eq!((s1, s2), (200, 200));
    assert!(
        quick_took < std::time::Duration::from_secs(2),
        "{quick_took:?}"
    );
    // An attempt that has ended never changes: no wait at all.
    let (s, _) = call(
        &p.app,
        Method::DELETE,
        &format!("/auth/pc-setups/{id}"),
        &p.token,
        None,
    )
    .await;
    assert_eq!(s, 204);
    let t = std::time::Instant::now();
    let (s, body) = poll(25).await;
    assert_eq!(
        (s, body["state"].as_str()),
        (200, Some("cancelled")),
        "{body}"
    );
    assert!(
        t.elapsed() < std::time::Duration::from_secs(2),
        "{:?}",
        t.elapsed()
    );
}

#[sqlx::test]
async fn logout_of_two_sessions_ends_the_bearers_own_last(pool: PgPool) {
    let p = pc(&pool).await;
    // A second session of the account, named by a refresh token.
    let other = probation_session(&pool, p.user).await;
    sqlx::query("UPDATE sessions SET token_hash = $2 WHERE id = $1")
        .bind(other)
        .bind(vc_server::auth::hash_token("refresh-of-the-other-session"))
        .execute(&pool)
        .await
        .unwrap();
    // A transition holds the bearer's own session row.
    let mut holder = pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM sessions WHERE id = $1 FOR UPDATE")
        .bind(p.sid)
        .execute(&mut *holder)
        .await
        .unwrap();
    let logout = call(
        &p.app,
        Method::POST,
        "/auth/logout",
        &p.token,
        Some(json!({"refresh_token": "refresh-of-the-other-session"})),
    );
    let check = async {
        super::helpers::wait_for_lock_waiters(&pool, 1).await;
        // The other session is already ended in its own transaction; the
        // bearer's own comes last, so a failure there leaves a request that
        // can be sent again with the same bearer.
        let other_ended = !session_live(&pool, other).await;
        let mine_live = session_live(&pool, p.sid).await;
        holder.commit().await.unwrap();
        (other_ended, mine_live)
    };
    let ((s, _), (other_ended, mine_live)) = tokio::join!(logout, check);
    assert_eq!(s, 200);
    assert!(
        other_ended && mine_live,
        "the other session first, one transaction each"
    );
    assert!(!session_live(&pool, p.sid).await);
}

#[sqlx::test]
async fn a_second_scan_of_a_claimed_code_is_refused(pool: PgPool) {
    let p = pc(&pool).await;
    let a = started(&p).await;
    let (token, _) = qr_token(&a["qr_payload"]);
    let (s, c) = claim(&p.app, &p.dev, &token).await;
    assert_eq!(s, 201, "{c}");
    let (s, b) = claim(&p.app, &p.dev, &token).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("LOGIN_ATTEMPT_CLAIMED")),
        "{b}"
    );
    assert_eq!(attempt_state(&pool, &id_of(&a)).await.0, "claimed");
}

#[sqlx::test]
async fn a_second_finish_adds_no_second_passkey(pool: PgPool) {
    let p = pc(&pool).await;
    let (id, _) = enrolled(&p).await;
    let first = ceremony(&p, &id).await;
    let second = ceremony(&p, &id).await;
    let (s, pk) = passkey_finish(&p, &p.token, &id, first).await;
    assert_eq!(s, 201, "{pk}");
    let (s, b) = passkey_finish(&p, &p.token, &id, second).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("APPROVAL_STATE")),
        "{b}"
    );
    assert_eq!(passkeys_of(&pool, p.user).await, 1);
}

#[sqlx::test]
async fn a_code_after_cancel_is_refused(pool: PgPool) {
    let p = pc(&pool).await;
    let (id, code) = with_code(&p).await;
    // The cancel holds the locks while the code entry waits for them.
    let mut holder = pool.begin().await.unwrap();
    let mut locked =
        vc_server::approvals::pc_setup::transition::lock_attempt(&mut holder, id.parse().unwrap())
            .await
            .unwrap();
    vc_server::approvals::pc_setup::transition::cancel_attempt_tx(
        &mut holder,
        &mut locked,
        vc_server::approvals::pc_setup::transition::EndCause::Cancelled,
    )
    .await
    .unwrap();
    let release = async {
        super::helpers::wait_for_lock_waiters(&pool, 1).await;
        holder.commit().await.unwrap();
    };
    let ((s, b), ()) = tokio::join!(enter(&p, &p.token, &id, &code), release);
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("APPROVAL_STATE")),
        "{b}"
    );
    assert_eq!(attempt_state(&pool, &id).await.0, "cancelled");
}

#[sqlx::test]
async fn a_scan_in_the_attempts_last_instant_is_stale(pool: PgPool) {
    let p = pc(&pool).await;
    let a = started(&p).await;
    sqlx::query("UPDATE pc_setup_attempts SET expires_at = NOW() + INTERVAL '900 milliseconds' WHERE id = $1::uuid")
        .bind(id_of(&a))
        .execute(&pool)
        .await
        .unwrap();
    let (token, _) = qr_token(&a["qr_payload"]);
    let (s, b) = claim(&p.app, &p.dev, &token).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (404, Some("LOGIN_CODE_STALE")),
        "{b}"
    );
}

// ============================================================================
// Answers for attempts that already ended, and for callers that are not the
// claiming device
// ============================================================================

#[sqlx::test]
async fn an_attempt_ended_by_a_poll_keeps_its_own_refusal(pool: PgPool) {
    // may_approve off while the phone shows the context; the PC's poll ends it first.
    let p = pc(&pool).await;
    let (id, c, bytes) = claimed(&p).await;
    set_may_approve(&pool, p.user, false).await;
    pc_poll(&p, &id).await;
    assert_eq!(
        attempt_state(&pool, &id).await.1.as_deref(),
        Some("may_approve_off")
    );
    let (s, b) = answer(&p.app, &p.dev, &c, &bytes, Decision::Approve).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (403, Some("APPROVAL_NOT_ALLOWED")),
        "{b}"
    );

    // The code ran out and the sweep ended the attempt before it was typed.
    let p = pc(&pool).await;
    let (id, code) = with_code(&p).await;
    sqlx::query("UPDATE pc_setup_attempts SET code_expires_at = NOW() - INTERVAL '1 second' WHERE id = $1::uuid")
        .bind(&id)
        .execute(&pool)
        .await
        .unwrap();
    vc_server::approvals::pc_setup::transition::expire_due(&pool)
        .await
        .unwrap();
    assert_eq!(
        attempt_state(&pool, &id).await.1.as_deref(),
        Some("code_expired")
    );
    let (s, b) = enter(&p, &p.token, &id, &code).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (410, Some("APPROVAL_EXPIRED")),
        "{b}"
    );
}

#[sqlx::test]
async fn another_phone_learns_nothing_about_a_claim(pool: PgPool) {
    let p = pc(&pool).await;
    let (id, c, bytes) = claimed(&p).await;
    let other = KeyDeviceFixture::insert(&pool, &p.app.config, p.user, "active").await;
    // The attempt ran out; the other phone answers the first phone's challenge.
    sqlx::query("UPDATE pc_setup_attempts SET expires_at = NOW() - INTERVAL '1 second', created_at = NOW() - INTERVAL '9 minutes' WHERE id = $1::uuid")
        .bind(&id)
        .execute(&pool)
        .await
        .unwrap();
    let (s, b) = answer(&p.app, &other, &c, &bytes, Decision::Approve).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (404, Some("APPROVAL_NOT_FOUND")),
        "{b}"
    );
    // Nothing changed on its behalf.
    assert_eq!(attempt_state(&pool, &id).await.0, "claimed");
}

#[sqlx::test]
async fn a_foreign_code_is_told_apart_only_when_live_and_only_to_a_scanner(pool: PgPool) {
    let p = pc(&pool).await;
    let a = started(&p).await;
    let (token, _) = qr_token(&a["qr_payload"]);
    let q = pc(&pool).await;
    // A scanner whose account may not confirm sign-ins gets that refusal.
    set_may_approve(&pool, q.user, false).await;
    let (s, b) = claim(&q.app, &q.dev, &token).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (403, Some("APPROVAL_NOT_ALLOWED")),
        "{b}"
    );
    set_may_approve(&pool, q.user, true).await;
    let (s, b) = claim(&q.app, &q.dev, &token).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("PC_SETUP_OTHER_ACCOUNT")),
        "{b}"
    );
    // The other PC's session ended: its code is no longer live.
    end_session(&pool, p.sid).await;
    let (s, b) = claim(&q.app, &q.dev, &token).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (404, Some("LOGIN_CODE_STALE")),
        "{b}"
    );
}

#[sqlx::test]
async fn a_new_passkey_hints_authenticators_changed(pool: PgPool) {
    use fred::interfaces::{ClientLike, EventInterface, PubsubInterface};
    let p = pc(&pool).await;
    let subscriber = p.app.state.redis.clone_new();
    let _ = subscriber.connect();
    subscriber.wait_for_connect().await.unwrap();
    let () = subscriber
        .subscribe(vc_server::ws::channels::user_events(p.user))
        .await
        .unwrap();
    let mut rx = subscriber.message_rx();
    completed(&p).await;
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let msg = rx.recv().await.unwrap();
            let v: Value = serde_json::from_str(&msg.value.convert::<String>().unwrap()).unwrap();
            if v["type"] == "authenticators_changed" {
                return;
            }
        }
    })
    .await
    .expect("an authenticators_changed hint");
}

#[sqlx::test]
async fn responses_with_a_token_or_code_are_not_stored(pool: PgPool) {
    let p = pc(&pool).await;
    let send = |method: Method, uri: String, bearer: String, body: Option<Value>| {
        let app = &p.app;
        async move {
            let req = TestApp::request(method, &uri)
                .header("Authorization", format!("Bearer {bearer}"))
                .header("Content-Type", "application/json")
                .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
                .unwrap();
            let resp = app.oneshot(req).await;
            let cc = resp
                .headers()
                .get("cache-control")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            (resp.status().as_u16(), cc, body_to_json_lenient(resp).await)
        }
    };
    let (s, cc, a) = send(
        Method::POST,
        "/auth/pc-setups".into(),
        p.token.clone(),
        Some(json!({})),
    )
    .await;
    assert_eq!((s, cc.as_deref()), (201, Some("no-store")), "{a}");
    let id = id_of(&a);
    for uri in [
        format!("/auth/pc-setups/{id}?wait=0"),
        "/auth/pc-setups/current".into(),
    ] {
        let (s, cc, b) = send(Method::GET, uri.clone(), p.token.clone(), None).await;
        assert_eq!((s, cc.as_deref()), (200, Some("no-store")), "{uri} {b}");
    }
    let (token, bytes) = qr_token(
        &send(
            Method::GET,
            "/auth/pc-setups/current".into(),
            p.token.clone(),
            None,
        )
        .await
        .2["qr_payload"],
    );
    let (_, c) = claim(&p.app, &p.dev, &token).await;
    let prefix = b64u_decode(c["signing_prefix"].as_str().unwrap()).unwrap();
    let sig = p.dev.sign(&prefix, Decision::Approve, &bytes);
    let cid = c["challenge_id"].as_str().unwrap();
    let (s, cc, b) = send(
        Method::POST,
        format!("/api/pc-setups/{cid}/answer"),
        p.dev.access_token.clone(),
        Some(json!({"decision": "approve", "response": b64u_encode(&bytes), "signature": sig})),
    )
    .await;
    assert_eq!((s, cc.as_deref()), (200, Some("no-store")), "{b}");
}

/// Hold `table`'s row `id` until `n` requests wait behind it, then release:
/// PostgreSQL grants the row to the waiters in their order.
async fn release_in_order(pool: &PgPool, holder: sqlx::Transaction<'_, sqlx::Postgres>, n: i64) {
    super::helpers::wait_for_lock_waiters(pool, n).await;
    holder.commit().await.unwrap();
}

#[sqlx::test]
async fn a_finish_committed_before_logout_keeps_its_passkey(pool: PgPool) {
    let p = pc(&pool).await;
    let (id, eid) = enrolled(&p).await;
    let body = ceremony(&p, &id).await;
    // Hold the requesting session's row: the finish queues first, the logout second.
    let mut holder = pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM sessions WHERE id = $1 FOR UPDATE")
        .bind(p.sid)
        .execute(&mut *holder)
        .await
        .unwrap();
    let finish = passkey_finish(&p, &p.token, &id, body);
    let logout_second = async {
        super::helpers::wait_for_lock_waiters(&pool, 1).await;
        logout(&p).await
    };
    let ((s, pk), out, ()) =
        tokio::join!(finish, logout_second, release_in_order(&pool, holder, 2));
    assert_eq!(s, 201, "{pk}");
    assert_eq!(out, 200);
    // The logout's cascade left the finished enrolment and the passkey alone.
    assert_eq!(enrollment_state(&pool, &eid).await, "done");
    assert_eq!(passkeys_of(&pool, p.user).await, 1);
    assert!(!session_live(&pool, p.sid).await);
}

#[sqlx::test]
async fn a_finish_committed_before_device_removal_keeps_its_passkey(pool: PgPool) {
    let p = pc(&pool).await;
    let (id, eid) = enrolled(&p).await;
    let body = ceremony(&p, &id).await;
    // Hold the account row: the finish queues first, the removal second.
    let mut holder = pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM users WHERE id = $1 FOR UPDATE")
        .bind(p.user)
        .execute(&mut *holder)
        .await
        .unwrap();
    let finish = passkey_finish(&p, &p.token, &id, body);
    let remove_second = async {
        super::helpers::wait_for_lock_waiters(&pool, 1).await;
        revoke_device(&p).await;
    };
    let ((s, pk), (), ()) = tokio::join!(finish, remove_second, release_in_order(&pool, holder, 2));
    assert_eq!(s, 201, "{pk}");
    assert_eq!(enrollment_state(&pool, &eid).await, "done");
    assert_eq!(passkeys_of(&pool, p.user).await, 1);
}

#[sqlx::test]
async fn a_scan_checks_the_scanning_device_before_the_account_setting(pool: PgPool) {
    let p = pc(&pool).await;
    let a = started(&p).await;
    let (token, _) = qr_token(&a["qr_payload"]);
    // The phone was removed and the account may no longer confirm.
    set_may_approve(&pool, p.user, false).await;
    sqlx::query(
        "UPDATE approval_devices SET state = 'revoked', revoked_at = NOW(), revoke_reason = 'owner' WHERE id = $1",
    )
    .bind(p.dev.row_id)
    .execute(&pool)
    .await
    .unwrap();
    let (s, b) = claim(&p.app, &p.dev, &token).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (403, Some("KEY_DEVICE_REQUIRED")),
        "{b}"
    );
}

#[sqlx::test]
async fn another_account_sees_a_claimed_code_as_its_own_answer(pool: PgPool) {
    let p = pc(&pool).await;
    let (id, _, bytes) = claimed(&p).await;
    // The QR stays on screen while claimed; its rotation time passes.
    sqlx::query("UPDATE pc_setup_attempts SET qr_rotates_at = NOW() - INTERVAL '5 seconds' WHERE id = $1::uuid")
        .bind(&id)
        .execute(&pool)
        .await
        .unwrap();
    let q = pc(&pool).await;
    let (s, b) = claim(&q.app, &q.dev, &b64u_encode(&bytes)).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("PC_SETUP_OTHER_ACCOUNT")),
        "{b}"
    );
}

#[sqlx::test]
async fn the_pc_setup_challenge_never_outlives_its_attempt(pool: PgPool) {
    let p = pc(&pool).await;
    let a = started(&p).await;
    let id = id_of(&a);
    sqlx::query("UPDATE pc_setup_attempts SET expires_at = NOW() + INTERVAL '20.7 seconds' WHERE id = $1::uuid")
        .bind(&id)
        .execute(&pool)
        .await
        .unwrap();
    let (token, _) = qr_token(&a["qr_payload"]);
    // The scan waits for the phone's device row after the attempt was locked.
    let mut holder = pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM approval_devices WHERE id = $1 FOR UPDATE")
        .bind(p.dev.row_id)
        .execute(&mut *holder)
        .await
        .unwrap();
    let release = async {
        super::helpers::wait_for_lock_waiters(&pool, 1).await;
        tokio::time::sleep(std::time::Duration::from_millis(2_500)).await;
        holder.commit().await.unwrap();
    };
    let ((s, c), ()) = tokio::join!(claim(&p.app, &p.dev, &token), release);
    assert_eq!(s, 201, "{c}");
    let within: bool = sqlx::query_scalar(
        "SELECT c.expires_at <= a.expires_at FROM approvals c JOIN pc_setup_attempts a ON a.id = c.requester_id
          WHERE c.id = $1::uuid",
    )
    .bind(c["challenge_id"].as_str().unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(within, "the stored expiry is capped by the attempt's");
}

// ============================================================================
// One clock per transition, read after its last lock (Locking 7)
// ============================================================================

/// Hold a row (touched by `sql`, so a waiter re-reads it after the wait).
async fn hold(
    pool: &PgPool,
    sql: &'static str,
    id: Uuid,
) -> sqlx::Transaction<'static, sqlx::Postgres> {
    let mut holder = pool.begin().await.unwrap();
    sqlx::query(sql)
        .bind(id)
        .execute(&mut *holder)
        .await
        .unwrap();
    holder
}

const TOUCH_USER: &str = "UPDATE users SET may_approve = may_approve WHERE id = $1";
const TOUCH_SESSION: &str = "UPDATE sessions SET expires_at = expires_at WHERE id = $1";

/// Once a request waits behind `holder`, keep it 2 s more, then release.
/// Returns the database clock just before the release.
async fn release_after_wait(
    pool: &PgPool,
    mut holder: sqlx::Transaction<'static, sqlx::Postgres>,
) -> chrono::DateTime<chrono::Utc> {
    super::helpers::wait_for_lock_waiters(pool, 1).await;
    tokio::time::sleep(std::time::Duration::from_millis(2_000)).await;
    let at: chrono::DateTime<chrono::Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *holder)
        .await
        .unwrap();
    holder.commit().await.unwrap();
    at
}

#[sqlx::test]
async fn a_start_that_waited_for_a_lock_gets_its_full_times(pool: PgPool) {
    let p = pc(&pool).await;
    let holder = hold(&pool, TOUCH_USER, p.user).await;
    let ((s, a), released) =
        tokio::join!(start(&p.app, &p.token), release_after_wait(&pool, holder));
    assert_eq!(s, 201, "{a}");
    let (created, rotates, expires): (
        chrono::DateTime<chrono::Utc>,
        chrono::DateTime<chrono::Utc>,
        chrono::DateTime<chrono::Utc>,
    ) = sqlx::query_as(
        "SELECT created_at, qr_rotates_at, expires_at FROM pc_setup_attempts WHERE id = $1::uuid",
    )
    .bind(id_of(&a))
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        created >= released,
        "created after the wait: {created} < {released}"
    );
    assert_eq!(
        rotates - created,
        chrono::Duration::seconds(vc_server::approvals::pc_setup::QR_ROTATE_SECS)
    );
    assert_eq!(expires - created, chrono::Duration::minutes(10));
}

#[sqlx::test]
async fn an_approval_that_waited_for_a_lock_gets_the_full_five_minutes(pool: PgPool) {
    let p = pc(&pool).await;
    let (id, c, bytes) = claimed(&p).await;
    let holder = hold(&pool, TOUCH_USER, p.user).await;
    let ((s, b), released) = tokio::join!(
        answer(&p.app, &p.dev, &c, &bytes, Decision::Approve),
        release_after_wait(&pool, holder)
    );
    assert_eq!(s, 200, "{b}");
    let until: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT code_expires_at FROM pc_setup_attempts WHERE id = $1::uuid")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        until >= released + chrono::Duration::minutes(5),
        "five minutes from after the wait: {until} < {released} + 5 min"
    );
}

#[sqlx::test]
async fn a_code_that_waited_for_a_lock_opens_the_full_ten_minutes(pool: PgPool) {
    let p = pc(&pool).await;
    let (id, code) = with_code(&p).await;
    let holder = hold(&pool, TOUCH_USER, p.user).await;
    let ((s, b), released) = tokio::join!(
        enter(&p, &p.token, &id, &code),
        release_after_wait(&pool, holder)
    );
    assert_eq!((s, b["state"].as_str()), (200, Some("enrolled")), "{b}");
    let (created, expires): (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) =
        sqlx::query_as(
            "SELECT e.created_at, e.expires_at FROM enrollments e
               JOIN pc_setup_attempts a ON a.enrollment_id = e.id WHERE a.id = $1::uuid",
        )
        .bind(&id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        created >= released,
        "created after the wait: {created} < {released}"
    );
    assert_eq!(expires - created, chrono::Duration::minutes(10));
}

#[sqlx::test]
async fn a_scan_that_waited_past_the_qr_time_is_stale(pool: PgPool) {
    let p = pc(&pool).await;
    let a = started(&p).await;
    let (token, _) = qr_token(&a["qr_payload"]);
    sqlx::query(
        "UPDATE pc_setup_attempts SET qr_rotates_at = clock_timestamp() + INTERVAL '1 second'
          WHERE id = $1::uuid",
    )
    .bind(id_of(&a))
    .execute(&pool)
    .await
    .unwrap();
    // The phone's own session row is busy (a refresh) across the QR's time.
    let phone_sid = p.dev.session_id.unwrap();
    let holder = hold(&pool, TOUCH_SESSION, phone_sid).await;
    let ((s, b), _) = tokio::join!(
        claim(&p.app, &p.dev, &token),
        release_after_wait(&pool, holder)
    );
    assert_eq!(
        (s, b["error"].as_str()),
        (404, Some("LOGIN_CODE_STALE")),
        "{b}"
    );
    assert_eq!(attempt_state(&pool, &id_of(&a)).await.0, "waiting");
}

#[sqlx::test]
async fn a_phone_session_that_lapsed_during_the_wait_refuses_the_scan(pool: PgPool) {
    let p = pc(&pool).await;
    let a = started(&p).await;
    let (token, _) = qr_token(&a["qr_payload"]);
    let phone_sid = p.dev.session_id.unwrap();
    let holder = hold(
        &pool,
        "UPDATE sessions SET expires_at = clock_timestamp() + INTERVAL '1 second' WHERE id = $1",
        phone_sid,
    )
    .await;
    let ((s, b), _) = tokio::join!(
        claim(&p.app, &p.dev, &token),
        release_after_wait(&pool, holder)
    );
    assert_eq!(
        (s, b["error"].as_str()),
        (401, Some("INVALID_TOKEN")),
        "{b}"
    );
    assert_eq!(attempt_state(&pool, &id_of(&a)).await.0, "waiting");
}

// ============================================================================
// The scanner first (C26): its refusal does not depend on the token
// ============================================================================

#[sqlx::test]
async fn a_scanner_that_may_not_approve_changes_nothing_and_learns_nothing(pool: PgPool) {
    let p = pc(&pool).await;
    let a = started(&p).await;
    let (token, _) = qr_token(&a["qr_payload"]);
    set_may_approve(&pool, p.user, false).await;
    for t in [token.clone(), b64u_encode(&[7u8; 32])] {
        let (s, b) = claim(&p.app, &p.dev, &t).await;
        assert_eq!(
            (s, b["error"].as_str()),
            (403, Some("APPROVAL_NOT_ALLOWED")),
            "{b}"
        );
    }
    // Nothing was written for the refused scanner; the PC's poll ends it.
    assert_eq!(
        attempt_state(&pool, &id_of(&a)).await,
        ("waiting".into(), None)
    );
    let (s, st) = pc_poll(&p, &id_of(&a)).await;
    assert_eq!(
        (s, st["state"].as_str(), st["reason"].as_str()),
        (200, Some("expired"), Some("may_approve_off"))
    );
}

#[sqlx::test]
async fn a_session_without_a_key_device_finds_no_challenge(pool: PgPool) {
    let p = pc(&pool).await;
    let (id, c, _) = claimed(&p).await;
    let cid = c["challenge_id"].as_str().unwrap();
    // The PC's own probation session, and a full session of another kind.
    for route in ["answer", "refuse", "release"] {
        let body = (route == "answer").then(|| {
            json!({"decision": "approve", "response": b64u_encode(&[0u8; 32]), "signature": "AA"})
        });
        let uri = if route == "release" {
            format!("/api/approvals/{cid}/release")
        } else {
            format!("/api/pc-setups/{cid}/{route}")
        };
        let (s, b) = call(&p.app, Method::POST, &uri, &p.token, body).await;
        assert_eq!(
            (s, b["error"].as_str()),
            (404, Some("APPROVAL_NOT_FOUND")),
            "{route}: {b}"
        );
    }
    assert_eq!(attempt_state(&pool, &id).await.0, "claimed");
}

// ============================================================================
// Creation data only (Step 3): the migration copies nothing a refresh wrote
// ============================================================================

#[sqlx::test(migrations = false)]
async fn sessions_from_before_the_upgrade_keep_no_creation_description(pool: PgPool) {
    use super::helpers::migrations::{apply_from, apply_until};
    const PC_SETUP: i64 = 20_261_126_000_000;
    apply_until(&pool, PC_SETUP).await;
    let (user, _) = create_test_user(&pool).await;
    let sid = Uuid::new_v4();
    insert_session_row(&pool, sid, user, false).await;
    sqlx::query("UPDATE sessions SET user_agent = 'Refreshed/1.0' WHERE id = $1")
        .bind(sid)
        .execute(&pool)
        .await
        .unwrap();
    apply_from(&pool, PC_SETUP).await;
    let created: Option<String> =
        sqlx::query_scalar("SELECT created_user_agent FROM sessions WHERE id = $1")
            .bind(sid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(created, None);
}

#[sqlx::test]
async fn a_phone_session_that_lapsed_behind_a_plain_lock_refuses_the_scan(pool: PgPool) {
    let p = pc(&pool).await;
    let a = started(&p).await;
    let (token, _) = qr_token(&a["qr_payload"]);
    let phone_sid = p.dev.session_id.unwrap();
    sqlx::query(
        "UPDATE sessions SET expires_at = clock_timestamp() + INTERVAL '1.5 seconds' WHERE id = $1",
    )
    .bind(phone_sid)
    .execute(&pool)
    .await
    .unwrap();
    // Another step of the same phone holds its session row without changing it.
    let holder = hold(
        &pool,
        "SELECT 1 FROM sessions WHERE id = $1 FOR UPDATE",
        phone_sid,
    )
    .await;
    let ((s, b), _) = tokio::join!(
        claim(&p.app, &p.dev, &token),
        release_after_wait(&pool, holder)
    );
    assert_eq!(
        (s, b["error"].as_str()),
        (401, Some("INVALID_TOKEN")),
        "{b}"
    );
    assert_eq!(attempt_state(&pool, &id_of(&a)).await.0, "waiting");
}
