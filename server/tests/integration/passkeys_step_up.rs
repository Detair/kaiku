//! Passkey step-up on L1a's action-bound grants (LP Task 5; F7, C29).

use axum::body::Body;
use axum::http::Method;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::auth::passkeys::queries::{self, PasskeyRow, PasskeyState};
use vc_server::auth::step_up::{self, StepUpAction, StepUpTarget};
use vc_server::auth::{AuthError, AuthUser};

use crate::helpers::login::{failing_valkey, peer_request, state_with_redis, TEST_ORIGIN};
use crate::helpers::passkeys::{limited_router, seed_passkey, SoftAuthenticator};
use crate::helpers::{
    body_to_json, create_probation_session_with_token, create_session_with_token, create_test_user,
    make_admin, TestApp,
};

pub async fn post(app: &TestApp, path: &str, token: Option<&str>, body: Value) -> (u16, Value) {
    let mut req = peer_request(Method::POST, path).header("Content-Type", "application/json");
    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    let resp = app
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await;
    let status = resp.status().as_u16();
    (status, body_to_json(resp).await)
}

/// An `AuthUser` for direct `step_up::*_tx` calls on session `sid`.
pub fn auth_user(user_id: Uuid, sid: Uuid) -> AuthUser {
    AuthUser {
        id: user_id,
        username: String::new(),
        display_name: String::new(),
        email: None,
        avatar_url: None,
        mfa_enabled: false,
        deletion_scheduled_at: None,
        must_change_password: false,
        session_id: Some(sid),
        session: None,
    }
}

/// Run `step-up/start` and `step-up/finish` with `auth` answering for
/// `row`, as the account `handle_of` (its user handle).
pub async fn step_up_with(
    app: &TestApp,
    token: &str,
    auth: &mut SoftAuthenticator,
    row: &PasskeyRow,
    handle_of: Uuid,
    action: &str,
    target: &str,
) -> (u16, Value) {
    let (s, start) = post(
        app,
        "/auth/passkeys/step-up/start",
        Some(token),
        json!({"action": action, "target": target}),
    )
    .await;
    if s != 200 {
        return (s, start);
    }
    let handle = queries::user_handle(&app.pool, handle_of).await.unwrap();
    let cred = auth.assert(
        TEST_ORIGIN,
        &start["options"],
        &row.credential_id,
        Some(handle),
        true,
    );
    post(
        app,
        "/auth/passkeys/step-up/finish",
        Some(token),
        json!({"ceremony_id": start["ceremony_id"], "credential": cred}),
    )
    .await
}

async fn consume(
    app: &TestApp,
    user: Uuid,
    sid: Uuid,
    action: StepUpAction,
    target: &StepUpTarget,
) -> Result<(), AuthError> {
    let mut tx = app.pool.begin().await.unwrap();
    let r = step_up::consume_tx(&mut tx, &auth_user(user, sid), action, target).await;
    tx.commit().await.unwrap();
    r.map(|_| ())
}

#[sqlx::test]
async fn step_up_grants_one_action_once(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (sid, token) = create_session_with_token(&app.pool, &app.config, user).await;
    let (s, g) = step_up_with(&app, &token, &mut auth, &row, user, "passkey.add", "").await;
    assert_eq!(s, 200, "{g}");
    assert_eq!(g["action"], "passkey.add");
    assert_eq!(g["target"], "");
    assert_eq!(g["via"], "passkey");
    assert!(g["expires_at"].is_string());
    consume(
        &app,
        user,
        sid,
        StepUpAction::PasskeyAdd,
        &StepUpTarget::None,
    )
    .await
    .unwrap();
    assert!(matches!(
        consume(
            &app,
            user,
            sid,
            StepUpAction::PasskeyAdd,
            &StepUpTarget::None
        )
        .await,
        Err(AuthError::StepUpRequired)
    ));
    // The grant records the passkey that produced it.
    let (via, kind, by): (String, String, Uuid) = sqlx::query_as(
        "SELECT via, authenticator_type, authenticator_id FROM step_up_grants WHERE session_id = $1",
    )
    .bind(sid)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(
        (via.as_str(), kind.as_str(), by),
        ("passkey", "passkey", row.id)
    );
    // record_use ran.
    let used: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT last_used_at FROM passkeys WHERE id = $1")
            .bind(row.id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert!(used.is_some());
}

#[sqlx::test]
async fn grant_is_bound_to_target(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, other) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (sid, token) = create_session_with_token(&app.pool, &app.config, user).await;
    let (s, _) = step_up_with(
        &app,
        &token,
        &mut auth,
        &row,
        user,
        "passkey.remove",
        &row.id.to_string(),
    )
    .await;
    assert_eq!(s, 200);
    assert!(matches!(
        consume(
            &app,
            user,
            sid,
            StepUpAction::PasskeyRemove,
            &StepUpTarget::Id(other.id)
        )
        .await,
        Err(AuthError::StepUpRequired)
    ));
    consume(
        &app,
        user,
        sid,
        StepUpAction::PasskeyRemove,
        &StepUpTarget::Id(row.id),
    )
    .await
    .unwrap();
}

#[sqlx::test]
async fn grant_is_bound_to_session(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (sid1, token1) = create_session_with_token(&app.pool, &app.config, user).await;
    let (sid2, _) = create_session_with_token(&app.pool, &app.config, user).await;
    let (s, _) = step_up_with(&app, &token1, &mut auth, &row, user, "passkey.add", "").await;
    assert_eq!(s, 200);
    assert!(matches!(
        consume(
            &app,
            user,
            sid2,
            StepUpAction::PasskeyAdd,
            &StepUpTarget::None
        )
        .await,
        Err(AuthError::StepUpRequired)
    ));
    consume(
        &app,
        user,
        sid1,
        StepUpAction::PasskeyAdd,
        &StepUpTarget::None,
    )
    .await
    .unwrap();
}

#[sqlx::test]
async fn grant_ttl_is_300s(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (sid, token) = create_session_with_token(&app.pool, &app.config, user).await;
    step_up_with(&app, &token, &mut auth, &row, user, "passkey.add", "").await;
    let secs: f64 = sqlx::query_scalar(
        "SELECT EXTRACT(EPOCH FROM expires_at - created_at)::float8 FROM step_up_grants WHERE session_id = $1",
    )
    .bind(sid)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert!((299.0..=300.5).contains(&secs), "{secs}");
}

#[sqlx::test]
async fn grant_dies_with_the_session(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let ctx = vc_server::auth::ctx::AuthCtx::from(&app.state);
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (sid, token) = create_session_with_token(&app.pool, &app.config, user).await;
    step_up_with(&app, &token, &mut auth, &row, user, "passkey.add", "").await;
    let mut tx = app.pool.begin().await.unwrap();
    vc_server::auth::revocation::end_sessions_tx(
        &mut tx,
        &ctx,
        user,
        &[sid],
        vc_server::auth::revocation::EndReason::Logout,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert!(consume(
        &app,
        user,
        sid,
        StepUpAction::PasskeyAdd,
        &StepUpTarget::None
    )
    .await
    .is_err());
}

#[sqlx::test]
async fn grant_dies_with_its_passkey(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (sid, token) = create_session_with_token(&app.pool, &app.config, user).await;
    step_up_with(&app, &token, &mut auth, &row, user, "passkey.add", "").await;
    // Revoked without the cascade: the at-use check alone refuses it.
    sqlx::query("UPDATE passkeys SET state = 'revoked', revoked_at = NOW() WHERE id = $1")
        .bind(row.id)
        .execute(&app.pool)
        .await
        .unwrap();
    assert!(matches!(
        consume(
            &app,
            user,
            sid,
            StepUpAction::PasskeyAdd,
            &StepUpTarget::None
        )
        .await,
        Err(AuthError::StepUpRequired)
    ));
}

#[sqlx::test]
async fn grant_needs_the_callers_own_authenticator(pool: PgPool) {
    // Fable m7 (L1a review): grant_tx checks the owner of `by`.
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (other, _) = create_test_user(&app.pool).await;
    let (_, theirs) = seed_passkey(&app, other, PasskeyState::Active).await;
    let (sid, _) = create_session_with_token(&app.pool, &app.config, user).await;
    let mut tx = app.pool.begin().await.unwrap();
    let r = step_up::grant_tx(
        &mut tx,
        &auth_user(user, sid),
        StepUpAction::PasskeyAdd,
        &StepUpTarget::None,
        step_up::StepUpVia::Passkey,
        vc_server::auth::strong::AuthenticatorRef::passkey(theirs.id),
    )
    .await;
    assert!(matches!(r, Err(AuthError::StepUpRequired)), "{r:?}");
}

#[sqlx::test]
async fn ceremony_from_other_session_is_refused(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, token1) = create_session_with_token(&app.pool, &app.config, user).await;
    let (_, token2) = create_session_with_token(&app.pool, &app.config, user).await;
    let (_, start) = post(
        &app,
        "/auth/passkeys/step-up/start",
        Some(&token1),
        json!({"action": "passkey.add", "target": ""}),
    )
    .await;
    let handle = queries::user_handle(&app.pool, user).await.unwrap();
    let cred = auth.assert(
        TEST_ORIGIN,
        &start["options"],
        &row.credential_id,
        Some(handle),
        true,
    );
    let body = json!({"ceremony_id": start["ceremony_id"], "credential": cred});
    let (s, b) = post(
        &app,
        "/auth/passkeys/step-up/finish",
        Some(&token2),
        body.clone(),
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (400, Some("PASSKEY_CEREMONY_INVALID"))
    );
    let (s, b) = post(&app, "/auth/passkeys/step-up/finish", Some(&token1), body).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (400, Some("PASSKEY_CEREMONY_INVALID"))
    );
}

#[sqlx::test]
async fn other_users_passkey_cannot_step_up(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (other, _) = create_test_user(&app.pool).await;
    seed_passkey(&app, user, PasskeyState::Active).await;
    let (mut theirs_auth, theirs) = seed_passkey(&app, other, PasskeyState::Active).await;
    let (sid, token) = create_session_with_token(&app.pool, &app.config, user).await;
    // With the other account's user handle, and with the caller's.
    for handle_of in [other, user] {
        let (s, b) = step_up_with(
            &app,
            &token,
            &mut theirs_auth,
            &theirs,
            handle_of,
            "passkey.add",
            "",
        )
        .await;
        assert_eq!(
            (s, b["error"].as_str()),
            (400, Some("PASSKEY_USER_MISMATCH")),
            "{b}"
        );
    }
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM step_up_grants WHERE session_id = $1")
        .bind(sid)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test]
async fn pending_passkey_cannot_step_up(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    seed_passkey(&app, user, PasskeyState::Active).await;
    let (mut auth, pending) = seed_passkey(&app, user, PasskeyState::Pending).await;
    let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
    let (s, b) = step_up_with(&app, &token, &mut auth, &pending, user, "passkey.add", "").await;
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("PASSKEY_PENDING")),
        "{b}"
    );
    assert!(b["ready_at"].is_string(), "{b}");
}

#[sqlx::test]
async fn remove_target_must_be_own_passkey(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (other, _) = create_test_user(&app.pool).await;
    seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, theirs) = seed_passkey(&app, other, PasskeyState::Active).await;
    let (_, revoked) = seed_passkey(&app, user, PasskeyState::Revoked).await;
    let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
    for target in [theirs.id, revoked.id, Uuid::new_v4()] {
        let (s, _) = post(
            &app,
            "/auth/passkeys/step-up/start",
            Some(&token),
            json!({"action": "passkey.remove", "target": target.to_string()}),
        )
        .await;
        assert_eq!(s, 404, "{target}");
    }
    // Bad action or target shape → 400.
    for body in [
        json!({"action": "passkey.nope", "target": ""}),
        json!({"action": "passkey.remove", "target": "not-a-uuid"}),
        json!({"action": "passkey.add", "target": Uuid::new_v4().to_string()}),
    ] {
        let (s, b) = post(&app, "/auth/passkeys/step-up/start", Some(&token), body).await;
        assert_eq!(
            (s, b["error"].as_str()),
            (400, Some("VALIDATION_ERROR")),
            "{b}"
        );
    }
}

#[sqlx::test]
async fn step_up_without_passkeys_is_refused(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    seed_passkey(&app, user, PasskeyState::Pending).await;
    let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
    let (s, b) = post(
        &app,
        "/auth/passkeys/step-up/start",
        Some(&token),
        json!({"action": "passkey.add", "target": ""}),
    )
    .await;
    assert_eq!((s, b["error"].as_str()), (400, Some("VALIDATION_ERROR")));
}

#[sqlx::test]
async fn probation_session_cannot_step_up(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, token) = create_probation_session_with_token(&app.pool, &app.config, user).await;
    for path in [
        "/auth/passkeys/step-up/start",
        "/auth/passkeys/step-up/finish",
    ] {
        let (s, b) = post(
            &app,
            path,
            Some(&token),
            json!({"action": "passkey.add", "target": "", "ceremony_id": Uuid::new_v4(), "credential": {}}),
        )
        .await;
        assert_eq!(
            (s, b["error"].as_str()),
            (403, Some("FULL_SESSION_REQUIRED")),
            "{path}"
        );
    }
}

#[sqlx::test]
async fn failed_step_up_never_blocks_the_ip(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (router, limiter) = limited_router(&app).await;
    let (user, username) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
    let handle = queries::user_handle(&app.pool, user).await.unwrap();
    for _ in 0..12 {
        let start = peer_request(Method::POST, "/auth/passkeys/step-up/start")
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::from(
                json!({"action": "passkey.add", "target": ""}).to_string(),
            ))
            .unwrap();
        let start = body_to_json(
            tower::ServiceExt::oneshot(router.clone(), start)
                .await
                .unwrap(),
        )
        .await;
        // UV=0: refused.
        let cred = auth.assert(
            TEST_ORIGIN,
            &start["options"],
            &row.credential_id,
            Some(handle),
            false,
        );
        let finish = peer_request(Method::POST, "/auth/passkeys/step-up/finish")
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::from(
                json!({"ceremony_id": start["ceremony_id"], "credential": cred}).to_string(),
            ))
            .unwrap();
        let resp = tower::ServiceExt::oneshot(router.clone(), finish)
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
    }
    assert!(!limiter.is_blocked("203.0.113.9").await.unwrap());
    let login = peer_request(Method::POST, "/auth/login")
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({"username": username, "password": "wrong"}).to_string(),
        ))
        .unwrap();
    let resp = tower::ServiceExt::oneshot(router, login).await.unwrap();
    assert_eq!(resp.status(), 401, "a normal answer, not an IP block");
}

async fn methods(app: &TestApp, token: String) -> Value {
    let req = TestApp::request(Method::GET, "/auth/reauth/methods")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    body_to_json(app.oneshot(req).await).await
}

#[sqlx::test]
async fn reauth_methods_report_passkey(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
    assert_eq!(
        methods(&app, token.clone()).await,
        json!({"passkey": false, "device": false})
    );
    seed_passkey(&app, user, PasskeyState::Pending).await;
    assert_eq!(
        methods(&app, token.clone()).await,
        json!({"passkey": false, "device": false})
    );
    seed_passkey(&app, user, PasskeyState::Active).await;
    // A passkey alone is no key device.
    assert_eq!(
        methods(&app, token.clone()).await,
        json!({"passkey": true, "device": false})
    );
    crate::helpers::approvals::KeyDeviceFixture::insert(&app.pool, &app.config, user, "active")
        .await;
    assert_eq!(
        methods(&app, token).await,
        json!({"passkey": true, "device": true})
    );
}

#[sqlx::test]
async fn admin_elevates_with_passkey_step_up(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    make_admin(&app.pool, user).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
    let (s, g) = step_up_with(&app, &token, &mut auth, &row, user, "admin.elevate", "").await;
    assert_eq!(s, 200, "{g}");
    let (s, b) = post(
        &app,
        "/api/admin/elevate",
        Some(&token),
        json!({"reason": null}),
    )
    .await;
    assert_eq!(s, 200, "{b}");
    let req = TestApp::request(Method::GET, "/api/admin/status")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let status = body_to_json(app.oneshot(req).await).await;
    assert_eq!(status["is_elevated"], true, "{status}");
    let method: Option<String> = sqlx::query_scalar(
        "SELECT details->>'method' FROM system_audit_log
          WHERE actor_id = $1 AND action = 'admin.session.elevated' ORDER BY created_at DESC LIMIT 1",
    )
    .bind(user)
    .fetch_optional(&app.pool)
    .await
    .unwrap()
    .flatten();
    assert_eq!(method.as_deref(), Some("step_up:passkey"));
}

#[sqlx::test]
async fn device_step_up_label_is_scoped_to_the_caller(pool: PgPool) {
    // Fable m1 (L1a review): a `passkey.remove` target label never reads
    // another account's passkey.
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (other, _) = create_test_user(&app.pool).await;
    let (_, mine) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, theirs) = seed_passkey(&app, other, PasskeyState::Active).await;
    let dev =
        crate::helpers::approvals::KeyDeviceFixture::insert(&app.pool, &app.config, user, "active")
            .await;
    for (target, label) in [(mine.id, "Test passkey"), (theirs.id, "")] {
        let (s, c) = post(
            &app,
            "/api/approvals/step-up",
            Some(&dev.access_token),
            json!({"action": "passkey.remove", "target": target.to_string()}),
        )
        .await;
        assert_eq!(s, 201, "{c}");
        assert_eq!(c["context"]["target_label"], label, "{c}");
    }
}

#[sqlx::test]
async fn valkey_down_step_up_fails_closed(pool: PgPool) {
    let app = TestApp::with_passkeys(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, token) = create_session_with_token(&pool, &app.config, user).await;
    let config = (*app.config).clone();
    let redis = failing_valkey(&config).await;
    let router = vc_server::api::create_router(state_with_redis(pool, config, redis));
    let req = TestApp::request(Method::POST, "/auth/passkeys/step-up/start")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(
            json!({"action": "passkey.add", "target": ""}).to_string(),
        ))
        .unwrap();
    let resp = tower::ServiceExt::oneshot(router, req).await.unwrap();
    assert_eq!(resp.status(), 503);
}

#[sqlx::test]
async fn regressed_counter_still_steps_up(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    sqlx::query("UPDATE passkeys SET credential = jsonb_set(credential, '{cred,counter}', to_jsonb(500)) WHERE id = $1")
        .bind(row.id)
        .execute(&app.pool)
        .await
        .unwrap();
    let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
    let (s, g) = step_up_with(&app, &token, &mut auth, &row, user, "passkey.add", "").await;
    assert_eq!(s, 200, "{g}");
    let counter: i64 = sqlx::query_scalar(
        "SELECT (credential #>> '{cred,counter}')::bigint FROM passkeys WHERE id = $1",
    )
    .bind(row.id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(counter, 500);
}

#[sqlx::test]
async fn a_foreign_revoked_credential_is_unknown(pool: PgPool) {
    // Review Argon-b m1: no USER_MISMATCH for a revoked row of another account.
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (other, _) = create_test_user(&app.pool).await;
    seed_passkey(&app, user, PasskeyState::Active).await;
    let (mut theirs_auth, theirs) = seed_passkey(&app, other, PasskeyState::Revoked).await;
    let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
    let (s, start) = post(
        &app,
        "/auth/passkeys/step-up/start",
        Some(&token),
        json!({"action": "passkey.add", "target": ""}),
    )
    .await;
    assert_eq!(s, 200);
    let cred = theirs_auth.assert(
        TEST_ORIGIN,
        &start["options"],
        &theirs.credential_id,
        None,
        true,
    );
    let (s, b) = post(
        &app,
        "/auth/passkeys/step-up/finish",
        Some(&token),
        json!({"ceremony_id": start["ceremony_id"], "credential": cred}),
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (400, Some("PASSKEY_UNKNOWN")),
        "{b}"
    );
}

#[sqlx::test]
async fn device_step_up_label_names_active_passkeys_only(pool: PgPool) {
    // Review Argon-b m4.
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (_, revoked) = seed_passkey(&app, user, PasskeyState::Revoked).await;
    let dev =
        crate::helpers::approvals::KeyDeviceFixture::insert(&app.pool, &app.config, user, "active")
            .await;
    let (s, c) = post(
        &app,
        "/api/approvals/step-up",
        Some(&dev.access_token),
        json!({"action": "passkey.remove", "target": revoked.id.to_string()}),
    )
    .await;
    assert_eq!(s, 201, "{c}");
    assert_eq!(c["context"]["target_label"], "", "{c}");
}

/// The `admin.*` actions with a target of another account (review round 2).
fn admin_actions(other: Uuid, device: Uuid) -> Vec<(&'static str, String)> {
    vec![
        ("admin.elevate", String::new()),
        ("admin.enrollment.issue", other.to_string()),
        ("admin.authenticator.remove", device.to_string()),
        ("admin.may_approve", other.to_string()),
        ("admin.factor.set_state", other.to_string()),
        ("admin.recovery_contact.set", other.to_string()),
    ]
}

#[sqlx::test]
async fn admin_step_ups_need_an_admin(pool: PgPool) {
    // Review round 2 (Codex 2.1, Fable R1): a non-admin starts no admin.*
    // step-up (NOT_ADMIN, as on /api/admin/*); an admin still does.
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (other, _) = create_test_user(&app.pool).await;
    seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, theirs) = seed_passkey(&app, other, PasskeyState::Active).await;
    let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
    for (action, target) in admin_actions(other, theirs.id) {
        let (s, b) = post(
            &app,
            "/auth/passkeys/step-up/start",
            Some(&token),
            json!({"action": action, "target": target}),
        )
        .await;
        assert_eq!(
            (s, b["error"].as_str()),
            (403, Some("NOT_ADMIN")),
            "{action}: {b}"
        );
    }
    make_admin(&app.pool, user).await;
    let (s, b) = post(
        &app,
        "/auth/passkeys/step-up/start",
        Some(&token),
        json!({"action": "admin.authenticator.remove", "target": theirs.id.to_string()}),
    )
    .await;
    assert_eq!(s, 200, "{b}");
}

/// Merge of L1a review round 3: `grant_tx` now takes the account lock
/// (`FOR UPDATE` on `users`) before the session row. The passkey grant wrote
/// its audit row first, whose `actor_id` foreign key holds a key-share lock
/// on the same account, so two concurrent grants each held that lock and
/// waited for the other's to take the account (40P01, a 500). The grant
/// transaction now takes the account lock first.
#[sqlx::test]
async fn concurrent_passkey_step_ups_never_deadlock(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let handle = queries::user_handle(&app.pool, user).await.unwrap();
    let mut finishes = Vec::new();
    for _ in 0..2 {
        let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
        let (s, start) = post(
            &app,
            "/auth/passkeys/step-up/start",
            Some(&token),
            json!({"action": "passkey.add", "target": ""}),
        )
        .await;
        assert_eq!(s, 200, "{start}");
        let cred = auth.assert(
            TEST_ORIGIN,
            &start["options"],
            &row.credential_id,
            Some(handle),
            true,
        );
        finishes.push((
            token,
            json!({"ceremony_id": start["ceremony_id"], "credential": cred}),
        ));
    }
    // Another transaction holds a key-share lock on the account (as any
    // insert that references it does), so both grants reach their account
    // lock before either commits.
    let mut holder = app.pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM users WHERE id = $1 FOR KEY SHARE")
        .bind(user)
        .execute(&mut *holder)
        .await
        .unwrap();
    let release = async {
        crate::helpers::wait_for_lock_waiters(&app.pool, 2).await;
        holder.commit().await.unwrap();
    };
    let (a, b, ()) = tokio::join!(
        post(
            &app,
            "/auth/passkeys/step-up/finish",
            Some(&finishes[0].0),
            finishes[0].1.clone(),
        ),
        post(
            &app,
            "/auth/passkeys/step-up/finish",
            Some(&finishes[1].0),
            finishes[1].1.clone(),
        ),
        release,
    );
    assert_eq!(a.0, 200, "first grant: {}", a.1);
    assert_eq!(b.0, 200, "second grant: {}", b.1);
}

/// L1a review round 3, part 2 M2 (for LP): the target of
/// `admin.authenticator.remove` is a key device or a passkey id (C27), so an
/// admin's key-device challenge names another account's passkey too (the
/// desktop ceremony page already did).
#[sqlx::test]
async fn device_admin_step_up_names_a_passkey_target(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (admin, _) = create_test_user(&app.pool).await;
    let (other, _) = create_test_user(&app.pool).await;
    make_admin(&app.pool, admin).await;
    let (_, theirs) = seed_passkey(&app, other, PasskeyState::Active).await;
    sqlx::query("UPDATE passkeys SET label = 'Their security key' WHERE id = $1")
        .bind(theirs.id)
        .execute(&app.pool)
        .await
        .unwrap();
    let theirs_device = crate::helpers::approvals::KeyDeviceFixture::insert(
        &app.pool,
        &app.config,
        other,
        "active",
    )
    .await;
    let dev = crate::helpers::approvals::KeyDeviceFixture::insert(
        &app.pool,
        &app.config,
        admin,
        "active",
    )
    .await;
    for (target, label) in [
        (theirs.id, "Their security key"),
        (theirs_device.row_id, "Pixel 8"),
        (Uuid::new_v4(), ""),
    ] {
        let (s, c) = post(
            &app,
            "/api/approvals/step-up",
            Some(&dev.access_token),
            json!({"action": "admin.authenticator.remove", "target": target.to_string()}),
        )
        .await;
        assert_eq!(s, 201, "{c}");
        assert_eq!(c["context"]["target_label"], label, "{c}");
    }
}
