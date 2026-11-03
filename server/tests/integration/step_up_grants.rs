//! Action-bound step-up grants (F7, C27 common rules), the two guards and
//! admin elevation by step-up.
//!
//! Run with: `cargo test --test integration step_up_grants`

use axum::body::Body;
use axum::http::Method;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::auth::ctx::AuthCtx;
use vc_server::auth::step_up::{
    consume_tx, grant_tx, peek_tx, StepUpAction, StepUpTarget, StepUpVia,
};
use vc_server::auth::strong::AuthenticatorRef;
use vc_server::auth::AuthUser;

use super::helpers::approvals::KeyDeviceFixture;
use super::helpers::login::peer_request;
use super::helpers::{
    body_to_json, create_probation_session_with_token, create_test_user, make_admin, TestApp,
};

/// The `AuthUser` `require_auth` builds for `token`'s session.
async fn auth_user(app: &TestApp, sid: Uuid) -> AuthUser {
    let s = vc_server::auth::session_check::load_live(&app.pool, sid)
        .await
        .unwrap()
        .expect("live session");
    let meta = vc_server::auth::SessionMeta::from(&s);
    let mut auth = AuthUser::from(s.user);
    auth.session_id = Some(meta.id);
    auth.session = Some(meta);
    auth
}

async fn grant(app: &TestApp, dev: &KeyDeviceFixture, action: StepUpAction, target: &StepUpTarget) {
    let auth = auth_user(app, dev.session_id.unwrap()).await;
    let mut tx = app.pool.begin().await.unwrap();
    grant_tx(
        &mut tx,
        &auth,
        action,
        target,
        StepUpVia::Device,
        AuthenticatorRef::key_device(dev.row_id),
    )
    .await
    .expect("grant");
    tx.commit().await.unwrap();
}

async fn error_code(err: vc_server::auth::AuthError) -> (u16, String) {
    let resp = axum::response::IntoResponse::into_response(err);
    let status = resp.status().as_u16();
    (
        status,
        body_to_json(resp).await["error"]
            .as_str()
            .unwrap()
            .to_string(),
    )
}

async fn consume(
    app: &TestApp,
    sid: Uuid,
    action: StepUpAction,
    target: &StepUpTarget,
) -> Result<(), (u16, String)> {
    let auth = auth_user(app, sid).await;
    let mut tx = app.pool.begin().await.unwrap();
    match consume_tx(&mut tx, &auth, action, target).await {
        Ok(_) => {
            tx.commit().await.unwrap();
            Ok(())
        }
        Err(e) => Err(error_code(e).await),
    }
}

#[sqlx::test]
async fn grant_is_single_use_and_bound(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let other = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let sid = dev.session_id.unwrap();
    let target = StepUpTarget::Id(other.row_id);

    grant(&app, &dev, StepUpAction::KeyDeviceRemove, &target).await;
    // Another session of the same user, another action, another target.
    let refused = (403, "STEP_UP_REQUIRED".to_string());
    assert_eq!(
        consume(
            &app,
            other.session_id.unwrap(),
            StepUpAction::KeyDeviceRemove,
            &target
        )
        .await,
        Err(refused.clone())
    );
    assert_eq!(
        consume(&app, sid, StepUpAction::PasskeyRemove, &target).await,
        Err(refused.clone())
    );
    assert_eq!(
        consume(
            &app,
            sid,
            StepUpAction::KeyDeviceRemove,
            &StepUpTarget::Id(dev.row_id)
        )
        .await,
        Err(refused.clone())
    );
    // peek does not consume.
    let auth = auth_user(&app, sid).await;
    let mut conn = pool.acquire().await.unwrap();
    peek_tx(&mut conn, &auth, StepUpAction::KeyDeviceRemove, &target)
        .await
        .expect("peek");
    drop(conn);
    assert_eq!(
        consume(&app, sid, StepUpAction::KeyDeviceRemove, &target).await,
        Ok(())
    );
    assert_eq!(
        consume(&app, sid, StepUpAction::KeyDeviceRemove, &target).await,
        Err(refused)
    );
}

#[sqlx::test]
async fn grant_expires_after_300_seconds(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let sid = dev.session_id.unwrap();

    let long = sqlx::query(
        "INSERT INTO step_up_grants (id, session_id, user_id, action, target_hash, target, via,
                                     authenticator_type, authenticator_id, expires_at)
         VALUES ($1, $2, $3, 'admin.elevate', '\\x00', '', 'device', 'key_device', $4,
                 NOW() + INTERVAL '301 seconds')",
    )
    .bind(Uuid::new_v4())
    .bind(sid)
    .bind(user)
    .bind(dev.row_id)
    .execute(&pool)
    .await;
    assert!(long.is_err(), "the CHECK refuses a grant longer than 300 s");

    grant(&app, &dev, StepUpAction::AdminElevate, &StepUpTarget::None).await;
    let expires: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT expires_at FROM step_up_grants WHERE session_id = $1")
            .bind(sid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(expires - chrono::Utc::now() <= chrono::Duration::seconds(300));
    sqlx::query(
        "UPDATE step_up_grants SET created_at = created_at - INTERVAL '10 minutes',
                                   expires_at = created_at - INTERVAL '9 minutes'
          WHERE session_id = $1",
    )
    .bind(sid)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        consume(&app, sid, StepUpAction::AdminElevate, &StepUpTarget::None).await,
        Err((403, "STEP_UP_REQUIRED".to_string()))
    );
}

#[sqlx::test]
async fn probation_cannot_hold_a_grant(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (sid, _) = create_probation_session_with_token(&pool, &app.config, user).await;
    let auth = auth_user(&app, sid).await;
    let mut tx = pool.begin().await.unwrap();
    let err = grant_tx(
        &mut tx,
        &auth,
        StepUpAction::AdminElevate,
        &StepUpTarget::None,
        StepUpVia::Device,
        AuthenticatorRef::key_device(dev.row_id),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error_code(err).await,
        (403, "FULL_SESSION_REQUIRED".to_string())
    );
}

#[sqlx::test]
async fn ending_the_session_voids_its_grants(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let sid = dev.session_id.unwrap();
    grant(&app, &dev, StepUpAction::AdminElevate, &StepUpTarget::None).await;
    let ctx = AuthCtx::from(&app.state);
    let mut tx = pool.begin().await.unwrap();
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
    let voided: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT voided_at FROM step_up_grants WHERE session_id = $1")
            .bind(sid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(voided.is_some());
}

#[sqlx::test]
async fn revoked_authenticator_grant_refused_at_use(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    // The grant's authenticator is another device than the session's own.
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let approver = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let sid = dev.session_id.unwrap();
    let auth = auth_user(&app, sid).await;
    let mut tx = pool.begin().await.unwrap();
    grant_tx(
        &mut tx,
        &auth,
        StepUpAction::AdminElevate,
        &StepUpTarget::None,
        StepUpVia::Device,
        AuthenticatorRef::key_device(approver.row_id),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    // B.7 rule 4: the approver is revoked out of band (no cascade ran).
    sqlx::query(
        "UPDATE approval_devices SET state = 'revoked', revoked_at = NOW(), revoke_reason = 'owner'
          WHERE id = $1",
    )
    .bind(approver.row_id)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        consume(&app, sid, StepUpAction::AdminElevate, &StepUpTarget::None).await,
        Err((403, "STEP_UP_REQUIRED".to_string()))
    );
}

#[test]
fn targets_are_canonical() {
    let id = Uuid::new_v4();
    let lower = id.to_string();
    let upper = lower.to_uppercase();
    assert_eq!(
        StepUpTarget::parse(StepUpAction::KeyDeviceRemove, &lower).unwrap(),
        StepUpTarget::Id(id)
    );
    for bad in [
        upper.as_str(),
        "",
        "others",
        "not-a-uuid",
        &id.simple().to_string(),
    ] {
        assert!(
            StepUpTarget::parse(StepUpAction::KeyDeviceRemove, bad).is_err(),
            "{bad:?}"
        );
    }
    assert_eq!(
        StepUpTarget::parse(StepUpAction::SessionsEnd, "others").unwrap(),
        StepUpTarget::Others
    );
    assert!(StepUpTarget::parse(StepUpAction::SessionsEnd, "OTHERS").is_err());
    assert!(StepUpTarget::parse(StepUpAction::SessionsEnd, "").is_err());
    assert_eq!(
        StepUpTarget::parse(StepUpAction::RecoveryContactSet, "").unwrap(),
        StepUpTarget::None
    );
    assert_eq!(
        StepUpTarget::parse(StepUpAction::AdminElevate, "").unwrap(),
        StepUpTarget::None
    );
    assert!(StepUpTarget::parse(StepUpAction::AdminElevate, &lower).is_err());
    assert_eq!(StepUpTarget::Others.canonical(), "others");
    assert_eq!(StepUpTarget::None.canonical(), "");
    assert_eq!(
        StepUpTarget::Id(id).hash(),
        vc_server::approvals::context::target_hash(&lower)
    );
    for a in [
        (StepUpAction::PasskeyAdd, "passkey.add"),
        (StepUpAction::KeyDeviceRemove, "key_device.remove"),
        (StepUpAction::AdminElevate, "admin.elevate"),
        (
            StepUpAction::AdminRecoveryContactSet,
            "admin.recovery_contact.set",
        ),
    ] {
        assert_eq!(a.0.as_str(), a.1);
        assert_eq!(StepUpAction::parse(a.1), Some(a.0));
    }
    assert_eq!(StepUpAction::parse("admin.ELEVATE"), None);
}

async fn elevate(app: &TestApp, token: &str, body: Value) -> (u16, Value) {
    let req = peer_request(Method::POST, "/api/admin/elevate")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    (status, body_to_json(resp).await)
}

#[sqlx::test]
async fn elevation_needs_full_session_and_grant(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (admin, _) = create_test_user(&pool).await;
    make_admin(&pool, admin).await;
    let (_, probation) = create_probation_session_with_token(&pool, &app.config, admin).await;
    let (status, body) = elevate(&app, &probation, json!({"reason": null})).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (403, Some("FULL_SESSION_REQUIRED"))
    );

    let dev = KeyDeviceFixture::insert(&pool, &app.config, admin, "active").await;
    let (status, body) = elevate(&app, &dev.access_token, json!({"reason": null})).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (403, Some("STEP_UP_REQUIRED"))
    );

    grant(&app, &dev, StepUpAction::AdminElevate, &StepUpTarget::None).await;
    let (status, body) = elevate(&app, &dev.access_token, json!({"reason": "maintenance"})).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["elevated"], true);
    let expires: chrono::DateTime<chrono::Utc> =
        body["expires_at"].as_str().unwrap().parse().unwrap();
    let minutes = (expires - chrono::Utc::now()).num_minutes();
    assert!((14..=15).contains(&minutes), "{minutes}");
    let method: String = sqlx::query_scalar(
        "SELECT details->>'method' FROM system_audit_log
          WHERE action = 'admin.session.elevated' AND actor_id = $1",
    )
    .bind(admin)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(method, "step_up:device");
    // The grant is spent.
    let (status, _) = elevate(&app, &dev.access_token, json!({"reason": null})).await;
    assert_eq!(status, 403);
}

#[sqlx::test]
async fn elevate_ignores_password_fields(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (admin, _) = create_test_user(&pool).await;
    make_admin(&pool, admin).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, admin, "active").await;
    let (status, body) = elevate(
        &app,
        &dev.access_token,
        json!({"password": "whatever", "mfa_code": "123456", "reason": null}),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (403, Some("STEP_UP_REQUIRED"))
    );
}

#[sqlx::test]
async fn oidc_reauth_route_is_gone(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (admin, _) = create_test_user(&pool).await;
    make_admin(&pool, admin).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, admin, "active").await;
    let req = TestApp::request(Method::POST, "/auth/reauth/oidc/authorize/google")
        .header("Authorization", format!("Bearer {}", dev.access_token))
        .header("Content-Type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    assert_eq!(app.oneshot(req).await.status(), 404);
}

#[sqlx::test]
async fn reauth_methods_are_passkey_and_device(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (_, probation) = create_probation_session_with_token(&pool, &app.config, user).await;
    let get = |token: String| {
        let app = &app;
        async move {
            let req = TestApp::request(Method::GET, "/auth/reauth/methods")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap();
            let resp = app.oneshot(req).await;
            assert_eq!(resp.status(), 200);
            body_to_json(resp).await
        }
    };
    assert_eq!(
        get(probation.clone()).await,
        json!({"passkey": false, "device": false})
    );
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    assert_eq!(
        get(dev.access_token.clone()).await,
        json!({"passkey": false, "device": true})
    );
}
