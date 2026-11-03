//! Enrolment of a key device (SM2, C26) with the attestation minimum.
//!
//! Run with: `cargo test --test integration enrollments`

use axum::body::Body;
use axum::http::Method;
use p256::ecdsa::SigningKey;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::approvals::context::ClaimantCheck;
use vc_server::approvals::enrollments::{
    cancel_tx, confirm_tx, issue_tx, Confirmer, EnrollmentCanceller, EnrollmentIssued,
    EnrollmentVariant, IssueParams, Issuer,
};
use vc_server::approvals::error::ApprovalError;
use vc_server::approvals::secrets::{b64u_decode, b64u_encode};
use vc_server::approvals::signing::{message, Decision};
use vc_server::auth::ctx::AuthCtx;

use super::approvals_core::post;
use super::helpers::approvals::{spki_der, KeyDeviceFixture};
use super::helpers::attestation::{chain, chain_for, chain_without_extension, Reported};
use super::helpers::login::peer_request;
use super::helpers::{body_to_json, create_test_user, TestApp};

async fn public(app: &TestApp, path: &str, body: Value) -> (u16, Value) {
    let req = peer_request(Method::POST, path)
        .header("Content-Type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    (status, body_to_json(resp).await)
}

async fn issue(
    app: &TestApp,
    subject: Uuid,
    variant: EnrollmentVariant,
    issuer: Issuer,
) -> Result<EnrollmentIssued, ApprovalError> {
    let ctx = AuthCtx::from(&app.state);
    let mut tx = app.pool.begin().await.unwrap();
    let r = issue_tx(
        &mut tx,
        &ctx,
        IssueParams {
            variant,
            subject_id: subject,
            issuer,
            claimant: ClaimantCheck::None,
            claimant_note: None,
            lost_device_ids: Vec::new(),
        },
    )
    .await;
    tx.commit().await.unwrap();
    r
}

/// The new phone: its key and install id.
pub(super) struct Phone {
    pub(super) key: SigningKey,
    pub(super) device_id: Uuid,
}

impl Phone {
    pub(super) fn new() -> Self {
        Self {
            key: SigningKey::random(&mut rand::rngs::OsRng),
            device_id: Uuid::new_v4(),
        }
    }

    fn sign(&self, prefix_b64u: &str, decision: Decision, response: &[u8]) -> String {
        use p256::ecdsa::signature::Signer;
        let prefix = b64u_decode(prefix_b64u).unwrap();
        let sig: p256::ecdsa::Signature = self.key.sign(&message(&prefix, decision, response));
        b64u_encode(sig.to_der().as_bytes())
    }

    fn public_key(&self) -> String {
        b64u_encode(&spki_der(&self.key))
    }
}

/// open → pop-challenge → claim with `label`, `reported` and `uv_class`.
pub(super) async fn claim_with(
    app: &TestApp,
    token: &str,
    phone: &Phone,
    label: &str,
    reported: Option<Reported>,
    uv_class: &str,
) -> (u16, Value) {
    let (status, opened) =
        public(app, "/auth/enrollments/open", json!({"setup_token": token})).await;
    assert_eq!(status, 200, "{opened}");
    let nonce = b64u_decode(opened["enrollment_nonce"].as_str().unwrap()).unwrap();
    let (status, pop) = public(
        app,
        "/auth/enrollments/pop-challenge",
        json!({"setup_token": token, "device_id": phone.device_id, "public_key": phone.public_key()}),
    )
    .await;
    assert_eq!(status, 201, "{pop}");
    let c = &pop["challenge"];
    let chain: Vec<String> = match reported {
        Some(mut r) => {
            if r.challenge.is_empty() {
                r.challenge = nonce;
            }
            chain_for(&r, &phone.key)
                .iter()
                .map(|d| b64u_encode(d))
                .collect()
        }
        None => Vec::new(),
    };
    public(
        app,
        "/auth/enrollments/claim",
        json!({
            "setup_token": token,
            "challenge_id": c["challenge_id"],
            "signature": phone.sign(c["signing_prefix"].as_str().unwrap(), Decision::Approve, b""),
            "public_key": phone.public_key(),
            "attestation_chain": chain,
            "label": label,
            "uv_class": uv_class,
            "api_level": 34,
        }),
    )
    .await
}

pub(super) fn good() -> Option<Reported> {
    Some(Reported::good(&[]))
}

async fn confirm_cli(app: &TestApp, id: Uuid, code: &str) -> Result<Value, ApprovalError> {
    let ctx = AuthCtx::from(&app.state);
    let mut tx = app.pool.begin().await.unwrap();
    let r = confirm_tx(&mut tx, &ctx, id, Confirmer::Cli, code.as_bytes()).await;
    tx.commit().await.unwrap();
    r.map(|(e, after)| {
        let state = app.state.clone();
        tokio::spawn(
            async move { vc_server::approvals::enrollments::after_done(&state, after).await },
        );
        serde_json::to_value(e).unwrap()
    })
}

async fn status(app: &TestApp, id: Uuid, secret: &str) -> (u16, Value) {
    public(
        app,
        "/auth/enrollments/status",
        json!({"enrollment_id": id, "claim_secret": secret}),
    )
    .await
}

#[sqlx::test]
async fn cli_enrolment_end_to_end(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, username) = create_test_user(&pool).await;
    let issued = issue(&app, user, EnrollmentVariant::Cli, Issuer::Cli)
        .await
        .unwrap();
    let phone = Phone::new();
    let (status_code, (pop_subject, claimed)) = {
        let (s, pop) = public(
            &app,
            "/auth/enrollments/pop-challenge",
            json!({"setup_token": issued.setup_token, "device_id": phone.device_id, "public_key": phone.public_key()}),
        )
        .await;
        assert_eq!(s, 201);
        let subject = pop["subject"].clone();
        let (s, claimed) = claim_with(
            &app,
            &issued.setup_token,
            &phone,
            "Pixel 8",
            good(),
            "per_use",
        )
        .await;
        (s, (subject, claimed))
    };
    assert_eq!(pop_subject["username"], username);
    assert_eq!(status_code, 201, "{claimed}");
    assert_eq!(claimed["state"], "claimed");
    let code = claimed["code"].as_str().unwrap().to_string();
    assert!(code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit()));
    let secret = claimed["claim_secret"].as_str().unwrap().to_string();
    assert_eq!(secret.len(), 43);
    let (s, st) = status(&app, issued.enrollment_id, &secret).await;
    assert_eq!((s, st["state"].as_str()), (200, Some("claimed")));

    let e = confirm_cli(&app, issued.enrollment_id, &code)
        .await
        .unwrap();
    assert_eq!(e["state"], "done");
    assert_eq!(e["claimed"]["label"], "Pixel 8");
    let (state, via): (String, String) = sqlx::query_as(
        "SELECT state, enrolled_via FROM approval_devices WHERE user_id = $1 AND device_id = $2",
    )
    .bind(user)
    .bind(phone.device_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((state.as_str(), via.as_str()), ("active", "cli"));
    let (_, st) = status(&app, issued.enrollment_id, &secret).await;
    assert_eq!(st["state"], "done");
    assert_eq!(st["authenticator_state"], "active");
    let added: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM security_events WHERE subject_id = $1 AND event_type = 'authenticator_added' AND severity = 'high'",
    )
    .bind(user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(added, 1);
    // A wrong claim secret learns nothing.
    let (s, _) = status(&app, issued.enrollment_id, &b64u_encode(&[7u8; 32])).await;
    assert_eq!(s, 404);
}

#[sqlx::test]
async fn open_never_consumes(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let issued = issue(&app, user, EnrollmentVariant::Cli, Issuer::Cli)
        .await
        .unwrap();
    for _ in 0..3 {
        let (s, b) = public(
            &app,
            "/auth/enrollments/open",
            json!({"setup_token": issued.setup_token}),
        )
        .await;
        assert_eq!(s, 200, "{b}");
        assert!(b["server_name"].as_str().is_some());
    }
    let (s, _) = claim_with(
        &app,
        &issued.setup_token,
        &Phone::new(),
        "Pixel 8",
        good(),
        "per_use",
    )
    .await;
    assert_eq!(s, 201);
    let (s, b) = public(
        &app,
        "/auth/enrollments/open",
        json!({"setup_token": b64u_encode(&[1u8; 32])}),
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (404, Some("ENROLLMENT_TOKEN_INVALID"))
    );
}

#[sqlx::test]
async fn second_claim_is_already_used(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let issued = issue(&app, user, EnrollmentVariant::Cli, Issuer::Cli)
        .await
        .unwrap();
    assert_eq!(
        claim_with(
            &app,
            &issued.setup_token,
            &Phone::new(),
            "A",
            good(),
            "per_use"
        )
        .await
        .0,
        201
    );
    let (s, b) = public(
        &app,
        "/auth/enrollments/open",
        json!({"setup_token": issued.setup_token}),
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("ENROLLMENT_ALREADY_CLAIMED"))
    );

    // Concurrent claims of one valid challenge, 25×: one wins.
    for _ in 0..25 {
        let issued = issue(&app, user, EnrollmentVariant::Cli, Issuer::Cli)
            .await
            .unwrap();
        sqlx::query("UPDATE enrollments SET state = 'cancelled' WHERE subject_id = $1 AND id <> $2 AND state IN ('issued','claimed')")
            .bind(user)
            .bind(issued.enrollment_id)
            .execute(&pool)
            .await
            .unwrap();
        let phone = Phone::new();
        let (_, opened) = public(
            &app,
            "/auth/enrollments/open",
            json!({"setup_token": issued.setup_token}),
        )
        .await;
        let nonce = b64u_decode(opened["enrollment_nonce"].as_str().unwrap()).unwrap();
        let (_, pop) = public(&app, "/auth/enrollments/pop-challenge", json!({"setup_token": issued.setup_token, "device_id": phone.device_id, "public_key": phone.public_key()})).await;
        let body = json!({
            "setup_token": issued.setup_token,
            "challenge_id": pop["challenge"]["challenge_id"],
            "signature": phone.sign(pop["challenge"]["signing_prefix"].as_str().unwrap(), Decision::Approve, b""),
            "public_key": phone.public_key(),
            "attestation_chain": chain_for(&Reported::good(&nonce), &phone.key).iter().map(|d| b64u_encode(d)).collect::<Vec<_>>(),
            "label": "A",
            "uv_class": "per_use",
        });
        let (a, b) = tokio::join!(
            public(&app, "/auth/enrollments/claim", body.clone()),
            public(&app, "/auth/enrollments/claim", body.clone())
        );
        let mut statuses = [a.0, b.0];
        statuses.sort_unstable();
        assert_eq!(statuses[0], 201, "{} / {}", a.1, b.1);
        assert_eq!(statuses[1], 409, "{} / {}", a.1, b.1);
    }
}

#[sqlx::test]
async fn wrong_codes_count_and_the_third_fails(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let issued = issue(&app, user, EnrollmentVariant::Cli, Issuer::Cli)
        .await
        .unwrap();
    let (_, c) = claim_with(
        &app,
        &issued.setup_token,
        &Phone::new(),
        "A",
        good(),
        "per_use",
    )
    .await;
    let code = c["code"].as_str().unwrap();
    let wrong = if code == "000000" { "111111" } else { "000000" };
    for left in [2, 1] {
        match confirm_cli(&app, issued.enrollment_id, wrong).await {
            Err(ApprovalError::EnrollCodeWrong { tries_left }) => assert_eq!(tries_left, left),
            other => panic!("{other:?}"),
        }
    }
    assert!(matches!(
        confirm_cli(&app, issued.enrollment_id, wrong).await,
        Err(ApprovalError::EnrollmentFailed)
    ));
    assert!(
        confirm_cli(&app, issued.enrollment_id, code).await.is_err(),
        "failed is terminal"
    );
}

#[sqlx::test]
async fn code_expires_after_5_minutes(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let issued = issue(&app, user, EnrollmentVariant::Cli, Issuer::Cli)
        .await
        .unwrap();
    let (_, c) = claim_with(
        &app,
        &issued.setup_token,
        &Phone::new(),
        "A",
        good(),
        "per_use",
    )
    .await;
    let until: chrono::DateTime<chrono::Utc> =
        c["code_expires_at"].as_str().unwrap().parse().unwrap();
    assert!((until - chrono::Utc::now()).num_seconds() <= 300);
    sqlx::query(
        "UPDATE enrollments SET code_expires_at = NOW() - INTERVAL '1 second' WHERE id = $1",
    )
    .bind(issued.enrollment_id)
    .execute(&pool)
    .await
    .unwrap();
    assert!(matches!(
        confirm_cli(&app, issued.enrollment_id, c["code"].as_str().unwrap()).await,
        Err(ApprovalError::EnrollmentState)
    ));
}

#[sqlx::test]
async fn setup_token_expires_after_10_minutes(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let issued = issue(&app, user, EnrollmentVariant::Cli, Issuer::Cli)
        .await
        .unwrap();
    assert!((issued.expires_at - chrono::Utc::now()).num_seconds() <= 600);
    sqlx::query("UPDATE enrollments SET created_at = created_at - INTERVAL '11 minutes', expires_at = created_at - INTERVAL '1 minute' WHERE id = $1")
        .bind(issued.enrollment_id)
        .execute(&pool)
        .await
        .unwrap();
    let (s, _) = public(
        &app,
        "/auth/enrollments/open",
        json!({"setup_token": issued.setup_token}),
    )
    .await;
    assert_eq!(s, 404);
    vc_server::auth::timers::run_once(&app.state).await.unwrap();
    let state: String = sqlx::query_scalar("SELECT state FROM enrollments WHERE id = $1")
        .bind(issued.enrollment_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "expired");
}

#[sqlx::test]
async fn five_open_per_issuer(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let issuer = Issuer::KeyDevice {
        session_id: dev.session_id.unwrap(),
        device_id: dev.row_id,
    };
    for _ in 0..5 {
        issue(&app, user, EnrollmentVariant::Own, issuer)
            .await
            .unwrap();
    }
    assert!(matches!(
        issue(&app, user, EnrollmentVariant::Own, issuer).await,
        Err(ApprovalError::EnrollmentLimit)
    ));
}

/// An `own` enrolment issued by a key device, claimed by a new phone.
async fn own_claimed(app: &TestApp, dev: &KeyDeviceFixture) -> (EnrollmentIssued, Value, Phone) {
    let issuer = Issuer::KeyDevice {
        session_id: dev.session_id.unwrap(),
        device_id: dev.row_id,
    };
    let issued = issue(app, dev.user_id, EnrollmentVariant::Own, issuer)
        .await
        .unwrap();
    let phone = Phone::new();
    let (s, claimed) = claim_with(
        app,
        &issued.setup_token,
        &phone,
        "Pixel 9",
        good(),
        "per_use",
    )
    .await;
    assert_eq!(s, 201, "{claimed}");
    (issued, claimed, phone)
}

async fn enroll_challenge(app: &TestApp, dev: &KeyDeviceFixture, id: Uuid) -> (u16, Value) {
    post(
        app,
        &format!("/auth/enrollments/{id}/challenge"),
        &dev.access_token,
        json!({}),
    )
    .await
}

#[sqlx::test]
async fn enroll_approval_on_the_issuing_key_device(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, username) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (issued, claimed, phone) = own_claimed(&app, &dev).await;
    let (s, c) = enroll_challenge(&app, &dev, issued.enrollment_id).await;
    assert_eq!(s, 201, "{c}");
    assert_eq!(c["kind"], "enroll");
    assert_eq!(c["variant"], "own");
    let ctx = &c["context"];
    assert_eq!(ctx["enrollment_id"], issued.enrollment_id.to_string());
    assert_eq!(ctx["subject_username"], username);
    assert_eq!(ctx["claimant_check"], "none");
    assert_eq!(ctx["authenticator_type"], "key_device");
    assert_eq!(ctx["label"], "Pixel 9");
    let key_hash = b64u_encode(&sha2::Digest::finalize(
        <sha2::Sha256 as sha2::Digest>::new_with_prefix(spki_der(&phone.key)),
    ));
    assert_eq!(ctx["new_key_hash"], key_hash);

    let code = claimed["code"].as_str().unwrap();
    let (s, body) = super::approvals_core::answer(&app, &dev, &c, code.as_bytes()).await;
    assert_eq!(s, 200, "{body}");
    assert_eq!(body["state"], "consumed");
    assert_eq!(body["enrollment"]["state"], "done");
    let (state, via): (String, String) =
        sqlx::query_as("SELECT state, enrolled_via FROM approval_devices WHERE device_id = $1")
            .bind(phone.device_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((state.as_str(), via.as_str()), ("active", "own"));
}

#[sqlx::test]
async fn signature_checked_before_code(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (issued, claimed, phone) = own_claimed(&app, &dev).await;
    let (_, c) = enroll_challenge(&app, &dev, issued.enrollment_id).await;
    // Signed by the new phone's key (not the issuer's), with the right code.
    let code = claimed["code"].as_str().unwrap();
    let sig = phone.sign(
        c["signing_prefix"].as_str().unwrap(),
        Decision::Approve,
        code.as_bytes(),
    );
    let (s, b) = post(
        &app,
        &format!(
            "/api/approvals/{}/answer",
            c["challenge_id"].as_str().unwrap()
        ),
        &dev.access_token,
        json!({"decision": "approve", "response": b64u_encode(code.as_bytes()), "signature": sig}),
    )
    .await;
    assert_eq!((s, b["error"].as_str()), (400, Some("SIGNATURE_INVALID")));
    let tries: i16 = sqlx::query_scalar("SELECT tries FROM enrollments WHERE id = $1")
        .bind(issued.enrollment_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(tries, 0);
    // A wrong code with a valid signature counts and spends the challenge.
    let wrong = if code == "000000" { "111111" } else { "000000" };
    let (s, b) = super::approvals_core::answer(&app, &dev, &c, wrong.as_bytes()).await;
    assert_eq!(
        (s, b["error"].as_str(), b["tries_left"].as_i64()),
        (400, Some("ENROLL_CODE_WRONG"), Some(2))
    );
    let (s, _) = super::approvals_core::answer(&app, &dev, &c, code.as_bytes()).await;
    assert_eq!(s, 409, "the challenge is spent");
}

#[sqlx::test]
async fn expired_enroll_challenge_is_replaced_by_a_new_one(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (issued, claimed, _) = own_claimed(&app, &dev).await;
    let (_, first) = enroll_challenge(&app, &dev, issued.enrollment_id).await;
    let first_id = Uuid::parse_str(first["challenge_id"].as_str().unwrap()).unwrap();
    sqlx::query("UPDATE approvals SET created_at = created_at - INTERVAL '5 minutes', expires_at = created_at - INTERVAL '4 minutes' WHERE id = $1")
        .bind(first_id)
        .execute(&pool)
        .await
        .unwrap();
    let code = claimed["code"].as_str().unwrap();
    let (s, b) = super::approvals_core::answer(&app, &dev, &first, code.as_bytes()).await;
    assert_eq!((s, b["error"].as_str()), (410, Some("APPROVAL_EXPIRED")));
    // A new challenge works while the code is valid, and voids the open one.
    let (_, second) = enroll_challenge(&app, &dev, issued.enrollment_id).await;
    let (_, third) = enroll_challenge(&app, &dev, issued.enrollment_id).await;
    let (s, _) = super::approvals_core::answer(&app, &dev, &second, code.as_bytes()).await;
    assert_eq!(s, 410, "voided by the newer one");
    let (s, b) = super::approvals_core::answer(&app, &dev, &third, code.as_bytes()).await;
    assert_eq!(s, 200, "{b}");
}

#[sqlx::test]
async fn deny_cancels_the_enrolment(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (issued, claimed, _) = own_claimed(&app, &dev).await;
    let (_, c) = enroll_challenge(&app, &dev, issued.enrollment_id).await;
    let req = TestApp::request(
        Method::POST,
        &format!(
            "/api/approvals/{}/deny",
            c["challenge_id"].as_str().unwrap()
        ),
    )
    .header("Authorization", format!("Bearer {}", dev.access_token))
    .body(Body::empty())
    .unwrap();
    assert_eq!(app.oneshot(req).await.status(), 204);
    let (_, st) = status(
        &app,
        issued.enrollment_id,
        claimed["claim_secret"].as_str().unwrap(),
    )
    .await;
    assert_eq!(st["state"], "cancelled");
}

#[sqlx::test]
async fn confirm_and_cancel_race_one_wins(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    for _ in 0..25 {
        let issued = issue(&app, user, EnrollmentVariant::Cli, Issuer::Cli)
            .await
            .unwrap();
        let (_, c) = claim_with(
            &app,
            &issued.setup_token,
            &Phone::new(),
            "A",
            good(),
            "per_use",
        )
        .await;
        let code = c["code"].as_str().unwrap().to_string();
        let cancel = async {
            let ctx = AuthCtx::from(&app.state);
            let mut tx = app.pool.begin().await.unwrap();
            let r = cancel_tx(
                &mut tx,
                &ctx,
                issued.enrollment_id,
                EnrollmentCanceller::Issuer,
            )
            .await;
            tx.commit().await.unwrap();
            r.is_ok()
        };
        let (confirmed, cancelled) =
            tokio::join!(confirm_cli(&app, issued.enrollment_id, &code), cancel);
        assert!(confirmed.is_ok() != cancelled, "exactly one wins");
    }
}

#[sqlx::test]
async fn confirm_retry_after_done_creates_no_second_device(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let issued = issue(&app, user, EnrollmentVariant::Cli, Issuer::Cli)
        .await
        .unwrap();
    let (_, c) = claim_with(
        &app,
        &issued.setup_token,
        &Phone::new(),
        "A",
        good(),
        "per_use",
    )
    .await;
    let code = c["code"].as_str().unwrap();
    confirm_cli(&app, issued.enrollment_id, code).await.unwrap();
    assert!(matches!(
        confirm_cli(&app, issued.enrollment_id, code).await,
        Err(ApprovalError::EnrollmentState)
    ));
    let n: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM approval_devices WHERE enrollment_id = $1")
            .bind(issued.enrollment_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(n, 1);
}

#[sqlx::test]
async fn issuer_session_end_expires_the_enrolment(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (issued, _, _) = own_claimed(&app, &dev).await;
    let ctx = AuthCtx::from(&app.state);
    let mut tx = pool.begin().await.unwrap();
    vc_server::auth::revocation::end_sessions_tx(
        &mut tx,
        &ctx,
        user,
        &[dev.session_id.unwrap()],
        vc_server::auth::revocation::EndReason::Logout,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let state: String = sqlx::query_scalar("SELECT state FROM enrollments WHERE id = $1")
        .bind(issued.enrollment_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "expired");
}

async fn unsuitable(
    app: &TestApp,
    user: Uuid,
    reported: Option<Reported>,
    uv_class: &str,
) -> Value {
    let issued = issue(app, user, EnrollmentVariant::Cli, Issuer::Cli)
        .await
        .unwrap();
    let (s, b) = claim_with(
        app,
        &issued.setup_token,
        &Phone::new(),
        "A",
        reported,
        uv_class,
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (400, Some("DEVICE_UNSUITABLE")),
        "{b}"
    );
    sqlx::query("UPDATE enrollments SET state = 'cancelled' WHERE id = $1")
        .bind(issued.enrollment_id)
        .execute(&app.pool)
        .await
        .unwrap();
    b
}

#[sqlx::test]
async fn attestation_minimum(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    // no_chain_is_unsuitable
    assert_eq!(
        unsuitable(&app, user, None, "per_use").await["reason"],
        "attestation_missing"
    );
    // A leaf without the extension.
    let issued = issue(&app, user, EnrollmentVariant::Cli, Issuer::Cli)
        .await
        .unwrap();
    let phone = Phone::new();
    let (_, pop) = public(&app, "/auth/enrollments/pop-challenge", json!({"setup_token": issued.setup_token, "device_id": phone.device_id, "public_key": phone.public_key()})).await;
    let (s, b) = public(&app, "/auth/enrollments/claim", json!({
        "setup_token": issued.setup_token, "challenge_id": pop["challenge"]["challenge_id"],
        "signature": phone.sign(pop["challenge"]["signing_prefix"].as_str().unwrap(), Decision::Approve, b""),
        "public_key": phone.public_key(),
        "attestation_chain": chain_without_extension().iter().map(|d| b64u_encode(d)).collect::<Vec<_>>(),
        "label": "A", "uv_class": "per_use",
    })).await;
    assert_eq!(
        (s, b["reason"].as_str()),
        (400, Some("attestation_missing"))
    );
    sqlx::query("UPDATE enrollments SET state = 'cancelled' WHERE id = $1")
        .bind(issued.enrollment_id)
        .execute(&pool)
        .await
        .unwrap();
    let props = |f: &dyn Fn(&mut Reported)| {
        let mut r = Reported::good(&[]);
        f(&mut r);
        Some(r)
    };
    // software_level_is_unsuitable, no_user_auth_is_unsuitable
    assert_eq!(
        unsuitable(&app, user, props(&|r| r.level = 0), "per_use").await["reason"],
        "attestation_properties"
    );
    assert_eq!(
        unsuitable(&app, user, props(&|r| r.user_auth = false), "per_use").await["reason"],
        "attestation_properties"
    );
    // timeout_over_10s_is_unsuitable, uv_class_mismatch_is_unsuitable
    assert_eq!(
        unsuitable(&app, user, props(&|r| r.timeout = Some(30)), "window_10s").await["reason"],
        "attestation_properties"
    );
    assert_eq!(
        unsuitable(&app, user, props(&|r| r.timeout = Some(5)), "per_use").await["reason"],
        "attestation_properties"
    );
    assert_eq!(
        unsuitable(&app, user, props(&|_| {}), "window_10s").await["reason"],
        "attestation_properties"
    );
    // wrong_challenge_is_unsuitable
    assert_eq!(
        unsuitable(&app, user, props(&|r| r.challenge = vec![9; 32]), "per_use").await["reason"],
        "attestation_properties"
    );
    // A 10 s window with a PIN-only phone is fine.
    let issued = issue(&app, user, EnrollmentVariant::Cli, Issuer::Cli)
        .await
        .unwrap();
    let (s, _) = claim_with(
        &app,
        &issued.setup_token,
        &Phone::new(),
        "A",
        props(&|r| r.timeout = Some(10)),
        "window_10s",
    )
    .await;
    assert_eq!(s, 201);
    // chain_is_stored_unverified (a self-signed leaf is accepted and kept).
    let stored: Vec<Vec<u8>> =
        sqlx::query_scalar("SELECT claimed_attestation FROM enrollments WHERE id = $1")
            .bind(issued.enrollment_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored.len(), 1);
}

#[sqlx::test]
async fn label_is_cleaned_and_truncated_before_it_enters_the_context(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let issuer = Issuer::KeyDevice {
        session_id: dev.session_id.unwrap(),
        device_id: dev.row_id,
    };
    let issued = issue(&app, user, EnrollmentVariant::Own, issuer)
        .await
        .unwrap();
    let raw = format!("Ev\u{202e}il\n{}", "x".repeat(300));
    let (s, _) = claim_with(
        &app,
        &issued.setup_token,
        &Phone::new(),
        &raw,
        good(),
        "per_use",
    )
    .await;
    assert_eq!(s, 201);
    let stored: String = sqlx::query_scalar("SELECT claimed_label FROM enrollments WHERE id = $1")
        .bind(issued.enrollment_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored.chars().count(), 64);
    assert!(!stored.contains('\u{202e}') && !stored.contains('\n'));
    assert!(stored.starts_with("Evilx"));
    let (_, c) = enroll_challenge(&app, &dev, issued.enrollment_id).await;
    assert_eq!(c["context"]["label"], stored);
}

#[sqlx::test]
async fn valkey_flush_mid_enrolment_completes(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let issued = issue(&app, user, EnrollmentVariant::Cli, Issuer::Cli)
        .await
        .unwrap();
    let (_, c) = claim_with(
        &app,
        &issued.setup_token,
        &Phone::new(),
        "A",
        good(),
        "per_use",
    )
    .await;
    // Enrolments live in PostgreSQL only: nothing in Valkey to lose. (The
    // shared test Valkey is not flushed; no enrolment key exists there.)
    // A Valkey "flush" for this flow: the only Valkey state it touches are
    // rate-limit counters, which a flush resets; the enrolment itself is a
    // PostgreSQL row, so the flow completes.
    let state: String = sqlx::query_scalar("SELECT state FROM enrollments WHERE id = $1")
        .bind(issued.enrollment_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "claimed");
    assert!(
        confirm_cli(&app, issued.enrollment_id, c["code"].as_str().unwrap())
            .await
            .is_ok()
    );
}

#[test]
fn setup_url_format() {
    let token = b64u_encode(&[3u8; 32]);
    let url = format!("https://chat.example.com/setup#v=1&t={token}");
    assert_eq!(token.len(), 43);
    assert!(url.starts_with("https://chat.example.com/setup#v=1&t="));
}

#[sqlx::test]
async fn issued_setup_url_points_to_the_origin(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let issued = issue(&app, user, EnrollmentVariant::Cli, Issuer::Cli)
        .await
        .unwrap();
    assert_eq!(
        issued.setup_url,
        format!(
            "https://chat.example.com/setup#v=1&t={}",
            issued.setup_token
        )
    );
    assert_eq!(issued.setup_token.len(), 43);
}

#[sqlx::test]
async fn public_routes_never_ip_blocked(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    // The test peer's IP is blocked for failed logins.
    let _: () = fred::interfaces::KeysInterface::set(
        &app.state.redis,
        "ratelimit:block:203.0.113.9",
        "1",
        Some(fred::types::Expiration::EX(60)),
        None,
        false,
    )
    .await
    .unwrap();
    let issued = issue(&app, user, EnrollmentVariant::Cli, Issuer::Cli)
        .await
        .unwrap();
    let (s, _) = claim_with(
        &app,
        &issued.setup_token,
        &Phone::new(),
        "A",
        good(),
        "per_use",
    )
    .await;
    assert_eq!(s, 201);
}

#[sqlx::test]
async fn re_enrolment_ends_the_replaced_keys_sessions(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let old = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let issued = issue(&app, user, EnrollmentVariant::Cli, Issuer::Cli)
        .await
        .unwrap();
    // The same install (same per-install id) with a new key.
    let phone = Phone {
        key: SigningKey::random(&mut rand::rngs::OsRng),
        device_id: old.device_id,
    };
    let (s, claim) = claim_with(
        &app,
        &issued.setup_token,
        &phone,
        "Pixel 8",
        good(),
        "per_use",
    )
    .await;
    assert_eq!(s, 201, "{claim}");
    confirm_cli(&app, issued.enrollment_id, claim["code"].as_str().unwrap())
        .await
        .unwrap();
    let (state, reason): (String, Option<String>) = sqlx::query_as(
        "SELECT d.state, s.end_reason FROM approval_devices d
           JOIN sessions s ON s.authenticator_id = d.id
          WHERE d.id = $1",
    )
    .bind(old.row_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(state, "revoked");
    assert_eq!(
        reason.as_deref(),
        Some("authenticator_revoked"),
        "the replaced key's session ends with it"
    );
}

/// Review round 1 (Fable M2 / Codex M2): the `enroll` answer locks the
/// accounts before the challenge and the enrolment, so it never forms a
/// cycle with a revocation of the issuing device (accounts → device →
/// sessions → enrolments → challenges).
#[sqlx::test]
async fn enroll_answer_and_revocation_never_deadlock(pool: PgPool) {
    use vc_server::approvals::devices::{revoke_tx, RevokeReason};
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (issued, claimed, _) = own_claimed(&app, &dev).await;
    let (_, c) = enroll_challenge(&app, &dev, issued.enrollment_id).await;
    let code = claimed["code"].as_str().unwrap().to_string();
    let ctx = AuthCtx::from(&app.state);
    let mut revoker = app.pool.begin().await.unwrap();
    vc_server::approvals::transition::lock_accounts(&mut revoker, &[user])
        .await
        .unwrap();
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
    let ((status, body), revoked) = tokio::join!(
        super::approvals_core::answer(&app, &dev, &c, code.as_bytes()),
        revoke
    );
    assert!(revoked.is_ok(), "the revocation commits: {revoked:?}");
    assert_ne!(status, 500, "no deadlock on the answer side: {body}");
    assert_eq!(
        (status, body["error"].as_str()),
        (403, Some("KEY_DEVICE_REQUIRED"))
    );
}

/// A key device of another account that carries the same per-install id
/// (install ids are unique only per account).
async fn same_install_elsewhere(
    pool: &PgPool,
    app: &TestApp,
    of: &KeyDeviceFixture,
) -> KeyDeviceFixture {
    let (other, _) = create_test_user(pool).await;
    let mut dev = KeyDeviceFixture::insert(pool, &app.config, other, "active").await;
    sqlx::query("UPDATE approval_devices SET device_id = $1 WHERE id = $2")
        .bind(of.device_id)
        .bind(dev.row_id)
        .execute(pool)
        .await
        .unwrap();
    dev.device_id = of.device_id;
    dev
}

/// Review round 1 (Fable M3 / Codex M1 / Argon M1.2): a challenge is bound
/// to its approver's device row, not only to the self-reported install id;
/// and an `enroll` answer must come from the enrolment's issuing device.
#[sqlx::test]
async fn same_install_id_of_another_account_cannot_answer(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (issued, claimed, phone) = own_claimed(&app, &dev).await;
    let (_, c) = enroll_challenge(&app, &dev, issued.enrollment_id).await;
    let intruder = same_install_elsewhere(&pool, &app, &dev).await;
    let code = claimed["code"].as_str().unwrap();
    let (s, b) = super::approvals_core::answer(&app, &intruder, &c, code.as_bytes()).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (404, Some("APPROVAL_NOT_FOUND")),
        "{b}"
    );
    let devices: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM approval_devices WHERE device_id = $1")
            .bind(phone.device_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(devices, 0, "no key was enrolled");
    // The issuing device still can.
    let (s, b) = super::approvals_core::answer(&app, &dev, &c, code.as_bytes()).await;
    assert_eq!(s, 200, "{b}");
}

#[sqlx::test]
async fn same_install_id_of_another_account_cannot_deny(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (issued, claimed, _) = own_claimed(&app, &dev).await;
    let (_, c) = enroll_challenge(&app, &dev, issued.enrollment_id).await;
    let intruder = same_install_elsewhere(&pool, &app, &dev).await;
    let req = TestApp::request(
        Method::POST,
        &format!(
            "/api/approvals/{}/deny",
            c["challenge_id"].as_str().unwrap()
        ),
    )
    .header("Authorization", format!("Bearer {}", intruder.access_token))
    .body(Body::empty())
    .unwrap();
    assert_eq!(app.oneshot(req).await.status(), 404);
    let (_, st) = status(
        &app,
        issued.enrollment_id,
        claimed["claim_secret"].as_str().unwrap(),
    )
    .await;
    assert_eq!(st["state"], "claimed", "the enrolment is untouched");
}

/// Review round 1 (Fable M4): a re-enrolment on the same install removes
/// the earlier key, and that removal is a notice of its own.
#[sqlx::test]
async fn re_enrolment_records_the_removed_key(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let old = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let issued = issue(&app, user, EnrollmentVariant::Cli, Issuer::Cli)
        .await
        .unwrap();
    let phone = Phone {
        key: SigningKey::random(&mut rand::rngs::OsRng),
        device_id: old.device_id,
    };
    let (s, claim) = claim_with(
        &app,
        &issued.setup_token,
        &phone,
        "Pixel 8",
        good(),
        "per_use",
    )
    .await;
    assert_eq!(s, 201, "{claim}");
    confirm_cli(&app, issued.enrollment_id, claim["code"].as_str().unwrap())
        .await
        .unwrap();
    let removed: Vec<(String, Value)> = sqlx::query_as(
        "SELECT severity, details FROM security_events
          WHERE subject_id = $1 AND event_type = 'authenticator_removed'
            AND authenticator_type = 'key_device' AND authenticator_id = $2",
    )
    .bind(user)
    .bind(old.row_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(removed.len(), 1, "one notice for the replaced key");
    assert_eq!(removed[0].0, "high");
    assert_eq!(removed[0].1, json!({"type": "key_device", "by": "cli"}));
}

/// Review round 3 (Argon coverage, part 1 B1): the `(subject, install id)`
/// that a new phone names is its own choice, and the account's install ids
/// are listed to its sessions. A `pending` key (an `admin` enrolment of an
/// existing account, a `request` child) never removes a live key of that
/// install: replacement happens only when its account request completes
/// (L1b), so the wait, its cancellers and `ADMIN_TARGET_IS_ADMIN` hold.
async fn existing_account_with_key(pool: &PgPool, app: &TestApp, admin: bool) -> KeyDeviceFixture {
    let (victim, _) = create_test_user(pool).await;
    sqlx::query("UPDATE users SET first_activity_at = NOW() - INTERVAL '30 days' WHERE id = $1")
        .bind(victim)
        .execute(pool)
        .await
        .unwrap();
    if admin {
        super::helpers::make_admin(pool, victim).await;
    }
    KeyDeviceFixture::insert(pool, &app.config, victim, "active").await
}

async fn pop_status(app: &TestApp, token: &str, phone: &Phone) -> (u16, Value) {
    public(
        app,
        "/auth/enrollments/pop-challenge",
        json!({"setup_token": token, "device_id": phone.device_id, "public_key": phone.public_key()}),
    )
    .await
}

/// The live key is untouched: still active, its session live, nothing new
/// on the install.
async fn assert_key_untouched(pool: &PgPool, key: &KeyDeviceFixture) {
    let (state, ended): (String, Option<String>) = sqlx::query_as(
        "SELECT d.state, s.end_reason FROM approval_devices d
           JOIN sessions s ON s.authenticator_id = d.id
          WHERE d.id = $1",
    )
    .bind(key.row_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        (state.as_str(), ended),
        ("active", None),
        "the live key stays"
    );
    let rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM approval_devices WHERE user_id = $1 AND device_id = $2",
    )
    .bind(key.user_id)
    .bind(key.device_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(rows, 1, "no second row on the install");
}

async fn pending_admin_enrolment_never_replaces(pool: PgPool, target_is_admin: bool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let victim = existing_account_with_key(&pool, &app, target_is_admin).await;
    let (admin, _) = create_test_user(&pool).await;
    super::helpers::make_admin(&pool, admin).await;
    let admin_dev = KeyDeviceFixture::insert(&pool, &app.config, admin, "active").await;
    let issuer = Issuer::KeyDevice {
        session_id: admin_dev.session_id.unwrap(),
        device_id: admin_dev.row_id,
    };
    let issued = issue(&app, victim.user_id, EnrollmentVariant::Admin, issuer)
        .await
        .unwrap();
    // The victim's install id: refused before any code exists.
    let clone = Phone {
        key: SigningKey::random(&mut rand::rngs::OsRng),
        device_id: victim.device_id,
    };
    let (s, b) = pop_status(&app, &issued.setup_token, &clone).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (400, Some("VALIDATION_ERROR")),
        "{b}"
    );
    assert_key_untouched(&pool, &victim).await;

    // Claimed on a fresh install id, then (a race) carrying the victim's:
    // the confirm refuses too, and nothing changes.
    let phone = Phone::new();
    let (s, claimed) = claim_with(
        &app,
        &issued.setup_token,
        &phone,
        "Pixel 9",
        good(),
        "per_use",
    )
    .await;
    assert_eq!(s, 201, "{claimed}");
    sqlx::query("UPDATE enrollments SET claimed_device_id = $2 WHERE id = $1")
        .bind(issued.enrollment_id)
        .bind(victim.device_id)
        .execute(&pool)
        .await
        .unwrap();
    let (_, c) = enroll_challenge(&app, &admin_dev, issued.enrollment_id).await;
    let code = claimed["code"].as_str().unwrap();
    let (s, b) = super::approvals_core::answer(&app, &admin_dev, &c, code.as_bytes()).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (400, Some("VALIDATION_ERROR")),
        "{b}"
    );
    assert_key_untouched(&pool, &victim).await;
    let removed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM security_events
          WHERE subject_id = $1 AND event_type = 'authenticator_removed'",
    )
    .bind(victim.user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(removed, 0);
}

#[sqlx::test]
async fn pending_admin_enrolment_never_replaces_a_live_key(pool: PgPool) {
    pending_admin_enrolment_never_replaces(pool, false).await;
}

/// `ADMIN_TARGET_IS_ADMIN`: another admin's key is never removed at once,
/// so an `admin` enrolment of that admin cannot remove it either.
#[sqlx::test]
async fn pending_admin_enrolment_never_replaces_another_admins_key(pool: PgPool) {
    pending_admin_enrolment_never_replaces(pool, true).await;
}

#[sqlx::test]
async fn pending_request_enrolment_never_replaces_a_live_key(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let victim = existing_account_with_key(&pool, &app, false).await;
    let issued = issue(
        &app,
        victim.user_id,
        EnrollmentVariant::Request,
        Issuer::Request {
            request_id: Uuid::new_v4(),
        },
    )
    .await
    .unwrap();
    let clone = Phone {
        key: SigningKey::random(&mut rand::rngs::OsRng),
        device_id: victim.device_id,
    };
    let (s, b) = pop_status(&app, &issued.setup_token, &clone).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (400, Some("VALIDATION_ERROR")),
        "{b}"
    );
    assert_key_untouched(&pool, &victim).await;
    // A new install id is fine.
    let (s, b) = pop_status(&app, &issued.setup_token, &Phone::new()).await;
    assert_eq!(s, 201, "{b}");
}

/// An enrolment never removes the key device that issued it.
#[sqlx::test]
async fn own_enrolment_cannot_replace_its_issuer(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let issuer = Issuer::KeyDevice {
        session_id: dev.session_id.unwrap(),
        device_id: dev.row_id,
    };
    let issued = issue(&app, user, EnrollmentVariant::Own, issuer)
        .await
        .unwrap();
    let clone = Phone {
        key: SigningKey::random(&mut rand::rngs::OsRng),
        device_id: dev.device_id,
    };
    let (s, b) = pop_status(&app, &issued.setup_token, &clone).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (400, Some("VALIDATION_ERROR")),
        "{b}"
    );

    let phone = Phone::new();
    let (s, claimed) = claim_with(
        &app,
        &issued.setup_token,
        &phone,
        "Pixel 9",
        good(),
        "per_use",
    )
    .await;
    assert_eq!(s, 201, "{claimed}");
    sqlx::query("UPDATE enrollments SET claimed_device_id = $2 WHERE id = $1")
        .bind(issued.enrollment_id)
        .bind(dev.device_id)
        .execute(&pool)
        .await
        .unwrap();
    let (_, c) = enroll_challenge(&app, &dev, issued.enrollment_id).await;
    let code = claimed["code"].as_str().unwrap();
    let (s, b) = super::approvals_core::answer(&app, &dev, &c, code.as_bytes()).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (400, Some("VALIDATION_ERROR")),
        "{b}"
    );
    assert_key_untouched(&pool, &dev).await;
}

/// The replaced key's `revoke_reason` names who replaced it: `owner` for
/// the owner's own enrolment, `cli` for the host CLI.
#[sqlx::test]
async fn replaced_key_carries_the_variants_reason(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    // CLI.
    let old = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let issued = issue(&app, user, EnrollmentVariant::Cli, Issuer::Cli)
        .await
        .unwrap();
    let phone = Phone {
        key: SigningKey::random(&mut rand::rngs::OsRng),
        device_id: old.device_id,
    };
    let (s, claim) = claim_with(
        &app,
        &issued.setup_token,
        &phone,
        "Pixel 8",
        good(),
        "per_use",
    )
    .await;
    assert_eq!(s, 201, "{claim}");
    confirm_cli(&app, issued.enrollment_id, claim["code"].as_str().unwrap())
        .await
        .unwrap();
    let reason: Option<String> =
        sqlx::query_scalar("SELECT revoke_reason FROM approval_devices WHERE id = $1")
            .bind(old.row_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(reason.as_deref(), Some("cli"));

    // `own`, issued by another key device of the account.
    let issuer_dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let other = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let issuer = Issuer::KeyDevice {
        session_id: issuer_dev.session_id.unwrap(),
        device_id: issuer_dev.row_id,
    };
    let issued = issue(&app, user, EnrollmentVariant::Own, issuer)
        .await
        .unwrap();
    let phone = Phone {
        key: SigningKey::random(&mut rand::rngs::OsRng),
        device_id: other.device_id,
    };
    let (s, claimed) = claim_with(
        &app,
        &issued.setup_token,
        &phone,
        "Pixel 9",
        good(),
        "per_use",
    )
    .await;
    assert_eq!(s, 201, "{claimed}");
    let (_, c) = enroll_challenge(&app, &issuer_dev, issued.enrollment_id).await;
    let code = claimed["code"].as_str().unwrap();
    let (s, b) = super::approvals_core::answer(&app, &issuer_dev, &c, code.as_bytes()).await;
    assert_eq!(s, 200, "{b}");
    let reason: Option<String> =
        sqlx::query_scalar("SELECT revoke_reason FROM approval_devices WHERE id = $1")
            .bind(other.row_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(reason.as_deref(), Some("owner"));
}

/// Review round 3 (Argon coverage, part 1 M3): an `admin` enrolment's
/// answer locks the admin's and the subject's accounts together, in id
/// order, before the device and session rows (`lock_challenge`). A
/// transaction that holds the lower account and then takes the higher one
/// never deadlocks with it, whichever role has the lower id.
async fn admin_enroll_answer_against(pool: PgPool, subject_is_lower: bool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (a, _) = create_test_user(&pool).await;
    let (b, _) = create_test_user(&pool).await;
    let (low, high) = if a < b { (a, b) } else { (b, a) };
    let (subject, admin) = if subject_is_lower {
        (low, high)
    } else {
        (high, low)
    };
    super::helpers::make_admin(&pool, admin).await;
    sqlx::query("UPDATE users SET first_activity_at = NULL WHERE id = $1")
        .bind(subject)
        .execute(&pool)
        .await
        .unwrap();
    let admin_dev = KeyDeviceFixture::insert(&pool, &app.config, admin, "active").await;
    let issuer = Issuer::KeyDevice {
        session_id: admin_dev.session_id.unwrap(),
        device_id: admin_dev.row_id,
    };
    let issued = issue(&app, subject, EnrollmentVariant::Admin, issuer)
        .await
        .unwrap();
    let (s, claimed) = claim_with(
        &app,
        &issued.setup_token,
        &Phone::new(),
        "Pixel 9",
        good(),
        "per_use",
    )
    .await;
    assert_eq!(s, 201, "{claimed}");
    let (_, c) = enroll_challenge(&app, &admin_dev, issued.enrollment_id).await;
    let code = claimed["code"].as_str().unwrap().to_string();

    let mut other = app.pool.begin().await.unwrap();
    vc_server::approvals::transition::lock_accounts(&mut other, &[low])
        .await
        .unwrap();
    let holder = async {
        super::helpers::wait_for_lock_waiters(&pool, 1).await;
        vc_server::approvals::transition::lock_accounts(&mut other, &[high])
            .await
            .map_err(|e| format!("{e:?}"))?;
        other.commit().await.map_err(|e| e.to_string())
    };
    let ((status, body), held) = tokio::join!(
        super::approvals_core::answer(&app, &admin_dev, &c, code.as_bytes()),
        holder
    );
    assert!(held.is_ok(), "the other transaction commits: {held:?}");
    assert_eq!(status, 200, "no deadlock on the answer side: {body}");
}

#[sqlx::test]
async fn admin_enroll_answer_never_deadlocks_subject_lower(pool: PgPool) {
    admin_enroll_answer_against(pool, true).await;
}

#[sqlx::test]
async fn admin_enroll_answer_never_deadlocks_admin_lower(pool: PgPool) {
    admin_enroll_answer_against(pool, false).await;
}

/// Review round 3 (Argon coverage, part 1 M4): an enrolment issued from a
/// PC session (`issuer_device_id` NULL) still tells its issuer's sockets.
#[sqlx::test]
async fn pc_session_issuer_gets_the_claimed_hint(pool: PgPool) {
    use fred::prelude::{ClientLike, EventInterface, PubsubInterface};
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let issued = issue(
        &app,
        user,
        EnrollmentVariant::Own,
        Issuer::PcSession {
            session_id: super::helpers::test_session_id(user),
            passkey_id: Uuid::new_v4(),
            grant_id: Uuid::new_v4(),
        },
    )
    .await
    .unwrap();
    let subscriber = app.state.redis.clone_new();
    let _ = subscriber.connect();
    subscriber.wait_for_connect().await.unwrap();
    let () = subscriber
        .subscribe(vc_server::ws::channels::user_events(user))
        .await
        .unwrap();
    let mut rx = subscriber.message_rx();
    let (s, b) = claim_with(
        &app,
        &issued.setup_token,
        &Phone::new(),
        "Pixel 9",
        good(),
        "per_use",
    )
    .await;
    assert_eq!(s, 201, "{b}");
    let hint = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let msg = rx.recv().await.unwrap();
            let v: Value = serde_json::from_str(&msg.value.convert::<String>().unwrap()).unwrap();
            if v["type"] == "enrollment_updated" {
                return v;
            }
        }
    })
    .await
    .expect("an enrollment_updated hint for the PC issuer");
    assert_eq!(hint["enrollment_id"], issued.enrollment_id.to_string());
    assert_eq!(hint["state"], "claimed");
}

/// Review round 3 (Argon coverage, part 1 m6): a claimed enrolment is open
/// while its code is valid (not its setup token), and its status says
/// `expired` as soon as the code is past due, before the sweep marks it.
#[sqlx::test]
async fn claimed_enrolment_is_open_while_its_code_is(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let issuer = Issuer::KeyDevice {
        session_id: dev.session_id.unwrap(),
        device_id: dev.row_id,
    };
    let mut claimed = Vec::new();
    for _ in 0..5 {
        let issued = issue(&app, user, EnrollmentVariant::Own, issuer)
            .await
            .unwrap();
        let (s, c) = claim_with(
            &app,
            &issued.setup_token,
            &Phone::new(),
            "Pixel 9",
            good(),
            "per_use",
        )
        .await;
        assert_eq!(s, 201, "{c}");
        claimed.push((issued.enrollment_id, c));
    }
    // Claimed late: the setup token's 10 minutes are over, the codes are not.
    sqlx::query(
        "UPDATE enrollments SET created_at = NOW() - INTERVAL '9 minutes',
                                expires_at = NOW() - INTERVAL '1 minute'
          WHERE subject_id = $1",
    )
    .bind(user)
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        matches!(
            issue(&app, user, EnrollmentVariant::Own, issuer).await,
            Err(ApprovalError::EnrollmentLimit)
        ),
        "five claimed enrolments with valid codes are open"
    );
    // A code past due: `expired` at once.
    let (id, c) = &claimed[0];
    sqlx::query(
        "UPDATE enrollments SET code_expires_at = NOW() - INTERVAL '1 second' WHERE id = $1",
    )
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();
    let (_, st) = status(&app, *id, c["claim_secret"].as_str().unwrap()).await;
    assert_eq!(st["state"], "expired");
}

/// Review round 4 (Codex r3 P1 M2, round 1 Fable m6): the attestation leaf
/// must certify the key the phone claims with; a leaf for another key is
/// refused as unsuitable.
#[sqlx::test]
async fn attestation_leaf_must_certify_the_claimed_key(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let issued = issue(&app, user, EnrollmentVariant::Cli, Issuer::Cli)
        .await
        .unwrap();
    let phone = Phone::new();
    let (_, opened) = public(
        &app,
        "/auth/enrollments/open",
        json!({"setup_token": issued.setup_token}),
    )
    .await;
    let nonce = b64u_decode(opened["enrollment_nonce"].as_str().unwrap()).unwrap();
    let (_, pop) = public(&app, "/auth/enrollments/pop-challenge", json!({"setup_token": issued.setup_token, "device_id": phone.device_id, "public_key": phone.public_key()})).await;
    // Good properties, but the leaf certifies a key of its own.
    let (s, b) = public(&app, "/auth/enrollments/claim", json!({
        "setup_token": issued.setup_token, "challenge_id": pop["challenge"]["challenge_id"],
        "signature": phone.sign(pop["challenge"]["signing_prefix"].as_str().unwrap(), Decision::Approve, b""),
        "public_key": phone.public_key(),
        "attestation_chain": chain(&Reported::good(&nonce)).iter().map(|d| b64u_encode(d)).collect::<Vec<_>>(),
        "label": "A", "uv_class": "per_use",
    })).await;
    assert_eq!(
        (s, b["error"].as_str(), b["reason"].as_str()),
        (
            400,
            Some("DEVICE_UNSUITABLE"),
            Some("attestation_properties")
        ),
        "{b}"
    );
    // The same claim with a leaf for the phone's own key passes.
    let (s, b) = public(&app, "/auth/enrollments/claim", json!({
        "setup_token": issued.setup_token, "challenge_id": pop["challenge"]["challenge_id"],
        "signature": phone.sign(pop["challenge"]["signing_prefix"].as_str().unwrap(), Decision::Approve, b""),
        "public_key": phone.public_key(),
        "attestation_chain": chain_for(&Reported::good(&nonce), &phone.key).iter().map(|d| b64u_encode(d)).collect::<Vec<_>>(),
        "label": "A", "uv_class": "per_use",
    })).await;
    assert_eq!(s, 201, "{b}");
}
