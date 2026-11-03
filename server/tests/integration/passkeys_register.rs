//! Adding a passkey from a full session with a `passkey.add` grant, and the
//! registration core L1b calls (LP Task 6; C29, DC1).

use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::auth::passkeys::queries::{self, PasskeyState};
use vc_server::auth::passkeys::register::{self, RegistrationVia};
use vc_server::auth::AuthError;

use crate::approvals_core::{answer, stepup_challenge};
use crate::helpers::approvals::KeyDeviceFixture;
use crate::helpers::login::TEST_ORIGIN;
use crate::helpers::passkeys::{seed_passkey, test_passkey_rp, SoftAuthenticator, TEST_APK_ORIGIN};
use crate::helpers::{
    create_probation_session_with_token, create_session_with_token, create_test_user, TestApp,
};
use crate::passkeys_step_up::{post, step_up_with};

/// `register/start` + `register/finish` with a fresh soft passkey.
async fn register_with(app: &TestApp, token: &str, origin: &str, label: Value) -> (u16, Value) {
    let (s, start) = post(
        app,
        "/auth/passkeys/register/start",
        Some(token),
        json!({ "label": label }),
    )
    .await;
    if s != 200 {
        return (s, start);
    }
    let cred = SoftAuthenticator::new().register(origin, &start["options"]);
    post(
        app,
        "/auth/passkeys/register/finish",
        Some(token),
        json!({"ceremony_id": start["ceremony_id"], "credential": cred}),
    )
    .await
}

/// A user with one active passkey, a full session and a `passkey.add`
/// grant from that passkey.
async fn with_grant(app: &TestApp) -> (Uuid, Uuid, String, queries::PasskeyRow) {
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(app, user, PasskeyState::Active).await;
    let (sid, token) = create_session_with_token(&app.pool, &app.config, user).await;
    let (s, g) = step_up_with(app, &token, &mut auth, &row, user, "passkey.add", "").await;
    assert_eq!(s, 200, "{g}");
    (user, sid, token, row)
}

async fn count_rows(app: &TestApp, user: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM passkeys WHERE user_id = $1")
        .bind(user)
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

async fn events(app: &TestApp, user: Uuid) -> Vec<(String, Value)> {
    sqlx::query_as("SELECT event_type, details FROM security_events WHERE subject_id = $1 ORDER BY created_at, id")
        .bind(user)
        .fetch_all(&app.pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn add_with_step_up_grant(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _, token, _) = with_grant(&app).await;
    let (s, p) = register_with(&app, &token, TEST_ORIGIN, json!("Laptop")).await;
    assert_eq!(s, 201, "{p}");
    assert_eq!(p["state"], "active");
    assert_eq!(p["label"], "Laptop");
    assert!(p["ready_at"].is_null());
    assert!(p.get("credential").is_none());
    let id: Uuid = p["id"].as_str().unwrap().parse().unwrap();
    let row = queries::find_own(&app.pool, user, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.state, PasskeyState::Active);
    let ev = events(&app, user).await;
    assert!(
        ev.iter().any(|(t, d)| t == "authenticator_added"
            && d["type"] == "passkey"
            && d["via"] == "step_up"),
        "{ev:?}"
    );
    assert!(
        ev.iter()
            .any(|(t, d)| t == "step_up_action" && d["action"] == "passkey.add"),
        "{ev:?}"
    );
    // The grant was consumed.
    let (s, b) = register_with(&app, &token, TEST_ORIGIN, json!(null)).await;
    assert_eq!((s, b["error"].as_str()), (403, Some("STEP_UP_REQUIRED")));
}

#[sqlx::test]
async fn register_start_without_grant_is_refused(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
    let (s, b) = register_with(&app, &token, TEST_ORIGIN, json!(null)).await;
    assert_eq!((s, b["error"].as_str()), (403, Some("STEP_UP_REQUIRED")));
}

#[sqlx::test]
async fn grant_for_another_action_does_not_count(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
    let target = row.id.to_string();
    let (s, _) = step_up_with(
        &app,
        &token,
        &mut auth,
        &row,
        user,
        "passkey.remove",
        &target,
    )
    .await;
    assert_eq!(s, 200);
    let (s, b) = register_with(&app, &token, TEST_ORIGIN, json!(null)).await;
    assert_eq!((s, b["error"].as_str()), (403, Some("STEP_UP_REQUIRED")));
}

async fn start_then(app: &TestApp, token: &str) -> (Value, Value) {
    let (s, start) = post(app, "/auth/passkeys/register/start", Some(token), json!({})).await;
    assert_eq!(s, 200, "{start}");
    let cred = SoftAuthenticator::new().register(TEST_ORIGIN, &start["options"]);
    (start, cred)
}

#[sqlx::test]
async fn grant_expired_before_finish_is_refused(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, sid, token, _) = with_grant(&app).await;
    let (start, cred) = start_then(&app, &token).await;
    sqlx::query(
        "UPDATE step_up_grants SET created_at = NOW() - INTERVAL '400 seconds',
                                   expires_at = NOW() - INTERVAL '100 seconds'
          WHERE session_id = $1",
    )
    .bind(sid)
    .execute(&app.pool)
    .await
    .unwrap();
    let (s, b) = post(
        &app,
        "/auth/passkeys/register/finish",
        Some(&token),
        json!({"ceremony_id": start["ceremony_id"], "credential": cred}),
    )
    .await;
    assert_eq!((s, b["error"].as_str()), (403, Some("STEP_UP_REQUIRED")));
    assert_eq!(count_rows(&app, user).await, 1);
}

#[sqlx::test]
async fn grant_passkey_revoked_before_finish_is_refused(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _, token, row) = with_grant(&app).await;
    let (start, cred) = start_then(&app, &token).await;
    sqlx::query("UPDATE passkeys SET state = 'revoked', revoked_at = NOW() WHERE id = $1")
        .bind(row.id)
        .execute(&app.pool)
        .await
        .unwrap();
    let (s, b) = post(
        &app,
        "/auth/passkeys/register/finish",
        Some(&token),
        json!({"ceremony_id": start["ceremony_id"], "credential": cred}),
    )
    .await;
    assert_eq!((s, b["error"].as_str()), (403, Some("STEP_UP_REQUIRED")));
    assert_eq!(count_rows(&app, user).await, 1);
}

#[sqlx::test]
async fn first_passkey_from_a_key_device_session(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    sqlx::query("UPDATE users SET first_activity_at = NULL WHERE id = $1")
        .bind(user)
        .execute(&app.pool)
        .await
        .unwrap();
    let dev = KeyDeviceFixture::insert(&app.pool, &app.config, user, "active").await;
    let c = stepup_challenge(&app, &dev, "passkey.add", "").await;
    let (s, b) = answer(&app, &dev, &c, &[]).await;
    assert_eq!(s, 200, "{b}");
    let (s, p) = register_with(&app, &dev.access_token, TEST_APK_ORIGIN, json!("Pixel 8")).await;
    assert_eq!(s, 201, "{p}");
    assert_eq!(p["state"], "active");
    let first: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT first_activity_at FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert!(first.is_some());
    let ev = events(&app, user).await;
    assert!(
        ev.iter()
            .any(|(t, d)| t == "authenticator_added" && d["type"] == "passkey"),
        "{ev:?}"
    );
    assert!(ev.iter().any(|(t, _)| t == "step_up_action"), "{ev:?}");
}

#[sqlx::test]
async fn account_without_strong_authenticator_cannot_register(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    // Password + TOTP gives a probation session.
    let (_, token) = create_probation_session_with_token(&app.pool, &app.config, user).await;
    let (s, b) = register_with(&app, &token, TEST_ORIGIN, json!(null)).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (403, Some("FULL_SESSION_REQUIRED"))
    );
    assert_eq!(count_rows(&app, user).await, 0);
}

#[sqlx::test]
async fn probation_session_cannot_register(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (_, token) = create_probation_session_with_token(&app.pool, &app.config, user).await;
    for (path, body) in [
        (
            "/auth/passkeys/register/start",
            json!({"label": null, "grant": {"action": "passkey.add"}}),
        ),
        (
            "/auth/passkeys/register/finish",
            json!({"ceremony_id": Uuid::new_v4(), "credential": {}}),
        ),
    ] {
        let (s, b) = post(&app, path, Some(&token), body).await;
        assert_eq!(
            (s, b["error"].as_str()),
            (403, Some("FULL_SESSION_REQUIRED")),
            "{path}"
        );
    }
}

#[sqlx::test]
async fn reauth_body_is_not_a_step_up(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
    let (s, b) = post(
        &app,
        "/auth/passkeys/register/start",
        Some(&token),
        json!({"label": "x", "reauth": {"password": "hunter2hunter2", "mfa_code": "123456"}}),
    )
    .await;
    assert_eq!((s, b["error"].as_str()), (403, Some("STEP_UP_REQUIRED")));
}

#[sqlx::test]
async fn register_finish_from_other_session_is_refused(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _, token, _) = with_grant(&app).await;
    let (_, token2) = create_session_with_token(&app.pool, &app.config, user).await;
    let (start, cred) = start_then(&app, &token).await;
    let (s, b) = post(
        &app,
        "/auth/passkeys/register/finish",
        Some(&token2),
        json!({"ceremony_id": start["ceremony_id"], "credential": cred}),
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (400, Some("PASSKEY_CEREMONY_INVALID"))
    );
    assert_eq!(count_rows(&app, user).await, 1);
}

#[sqlx::test]
async fn ceremony_reuse_is_refused(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _, token, _) = with_grant(&app).await;
    let (start, cred) = start_then(&app, &token).await;
    let body = json!({"ceremony_id": start["ceremony_id"], "credential": cred});
    let (s, _) = post(
        &app,
        "/auth/passkeys/register/finish",
        Some(&token),
        body.clone(),
    )
    .await;
    assert_eq!(s, 201);
    let (s, b) = post(&app, "/auth/passkeys/register/finish", Some(&token), body).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (400, Some("PASSKEY_CEREMONY_INVALID"))
    );
    assert_eq!(count_rows(&app, user).await, 2);
}

#[sqlx::test]
async fn concurrent_finishes_respect_the_limit(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    for _ in 0..8 {
        seed_passkey(&app, user, PasskeyState::Active).await;
    }
    let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
    for _ in 0..2 {
        let (s, _) = step_up_with(&app, &token, &mut auth, &row, user, "passkey.add", "").await;
        assert_eq!(s, 200);
    }
    let (a, ca) = start_then(&app, &token).await;
    let (b, cb) = start_then(&app, &token).await;
    let fa = post(
        &app,
        "/auth/passkeys/register/finish",
        Some(&token),
        json!({"ceremony_id": a["ceremony_id"], "credential": ca}),
    );
    let fb = post(
        &app,
        "/auth/passkeys/register/finish",
        Some(&token),
        json!({"ceremony_id": b["ceremony_id"], "credential": cb}),
    );
    let ((sa, ba), (sb, bb)) = tokio::join!(fa, fb);
    let mut statuses = vec![sa, sb];
    statuses.sort_unstable();
    assert_eq!(statuses, vec![201, 409], "{ba} {bb}");
    assert!([ba, bb].iter().any(|b| b["error"] == "PASSKEY_LIMIT"));
    assert_eq!(queries::active_count(&app.pool, user).await.unwrap(), 10);
}

#[sqlx::test]
async fn duplicate_credential_is_refused(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (_, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let verified = vc_server::auth::passkeys::rp::VerifiedRegistration {
        passkey: row.credential.0.clone(),
        aaguid: None,
        backup_eligible: false,
        backup_state: false,
        transports: vec![],
    };
    let mut tx = app.pool.begin().await.unwrap();
    let err = register::complete(
        &mut tx,
        user,
        "again",
        &verified,
        PasskeyState::Active,
        None,
        RegistrationVia::StepUp,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, AuthError::PasskeyAlreadyRegistered),
        "{err:?}"
    );
}

#[sqlx::test]
async fn exclude_credentials_lists_live_only(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _, token, active) = with_grant(&app).await;
    let (_, pending) = seed_passkey(&app, user, PasskeyState::Pending).await;
    let (_, revoked) = seed_passkey(&app, user, PasskeyState::Revoked).await;
    let (s, start) = post(
        &app,
        "/auth/passkeys/register/start",
        Some(&token),
        json!({}),
    )
    .await;
    assert_eq!(s, 200, "{start}");
    let mut ids: Vec<Vec<u8>> = start["options"]["publicKey"]["excludeCredentials"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| vc_server::approvals::secrets::b64u_decode(c["id"].as_str().unwrap()).unwrap())
        .collect();
    ids.sort();
    let mut want = vec![active.credential_id.clone(), pending.credential_id.clone()];
    want.sort();
    assert_eq!(ids, want);
    assert!(!ids.contains(&revoked.credential_id));
    // The user handle is the account's, never its id.
    let handle = queries::user_handle(&app.pool, user).await.unwrap();
    let sent = vc_server::approvals::secrets::b64u_decode(
        start["options"]["publicKey"]["user"]["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(sent, handle.as_bytes());
}

#[sqlx::test]
async fn password_change_pending_account_is_refused(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _, token, _) = with_grant(&app).await;
    sqlx::query("UPDATE users SET must_change_password = true WHERE id = $1")
        .bind(user)
        .execute(&app.pool)
        .await
        .unwrap();
    let (s, b) = post(
        &app,
        "/auth/passkeys/register/start",
        Some(&token),
        json!({}),
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (403, Some("PASSWORD_CHANGE_REQUIRED"))
    );
}

#[sqlx::test]
async fn passkey_limit_is_ten(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _, token, _) = with_grant(&app).await;
    for _ in 0..9 {
        seed_passkey(&app, user, PasskeyState::Active).await;
    }
    let (s, b) = post(
        &app,
        "/auth/passkeys/register/start",
        Some(&token),
        json!({}),
    )
    .await;
    assert_eq!((s, b["error"].as_str()), (409, Some("PASSKEY_LIMIT")));
}

#[sqlx::test]
async fn label_rules(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (_, _, token, _) = with_grant(&app).await;
    for bad in [json!("x".repeat(65)), json!("   ")] {
        let (s, b) = post(
            &app,
            "/auth/passkeys/register/start",
            Some(&token),
            json!({"label": bad}),
        )
        .await;
        assert_eq!(
            (s, b["error"].as_str()),
            (400, Some("VALIDATION_ERROR")),
            "{bad}"
        );
    }
    let (s, p) = register_with(&app, &token, TEST_ORIGIN, json!("  Laptop  ")).await;
    assert_eq!(s, 201, "{p}");
    assert_eq!(p["label"], "Laptop");
    // 64 characters (not bytes) are fine; a missing label is "Passkey".
    let (user2, _, token2, _) = with_grant(&app).await;
    let (s, p) = register_with(&app, &token2, TEST_ORIGIN, json!("ü".repeat(64))).await;
    assert_eq!(s, 201, "{p}");
    let (mut auth, row) = seed_passkey(&app, user2, PasskeyState::Active).await;
    step_up_with(&app, &token2, &mut auth, &row, user2, "passkey.add", "").await;
    let (s, p) = register_with(&app, &token2, TEST_ORIGIN, json!(null)).await;
    assert_eq!(s, 201, "{p}");
    assert_eq!(p["label"], "Passkey");
}

#[sqlx::test]
async fn complete_creates_pending_with_ready_at(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let rp = test_passkey_rp();
    let handle = queries::user_handle(&app.pool, user).await.unwrap();
    let (ccr, st) = rp.start_registration(handle, "u", "U", vec![]).unwrap();
    let cred = SoftAuthenticator::new().register(TEST_ORIGIN, &serde_json::to_value(&ccr).unwrap());
    let verified = rp
        .finish_registration(&serde_json::from_value(cred).unwrap(), &st)
        .unwrap();
    let ready_at = chrono::Utc::now() + chrono::Duration::hours(48);
    let mut tx = app.pool.begin().await.unwrap();
    vc_server::approvals::transition::lock_accounts(&mut tx, &[user])
        .await
        .unwrap();
    let row = register::complete(
        &mut tx,
        user,
        "Laptop",
        &verified,
        PasskeyState::Pending,
        Some(ready_at),
        RegistrationVia::StepUp,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(row.state, PasskeyState::Pending);
    assert_eq!(
        row.ready_at.map(|t| t.timestamp()),
        Some(ready_at.timestamp())
    );
}
