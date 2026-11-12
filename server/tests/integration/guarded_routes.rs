//! Routes behind the "full session" guard (spec B.4 probation rules, C27
//! "What each may do"). L1a guards `/api/admin/*`; L1b Task 16 adds the
//! own-account routes and the step-up guard to this file.
//!
//! Run with: `cargo test --test integration guarded_routes`

use axum::body::Body;
use axum::http::Method;
use sqlx::PgPool;

use super::helpers::login::peer_request;
use super::helpers::{
    body_to_json, create_probation_session_with_token, create_session_with_token, create_test_user,
    make_admin, TestApp,
};

async fn call(app: &TestApp, method: Method, uri: &str, token: &str) -> (u16, serde_json::Value) {
    let req = peer_request(method, uri)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    (status, body_to_json(resp).await)
}

/// Review round 1 (Argon B1, Codex M5): C27 puts `/api/admin/*` in the
/// "full" row; a probation session of an admin (every pre-upgrade session,
/// QR and password/OIDC sign-ins) reads and changes nothing there.
/// `/api/admin/status` stays open to any session.
#[sqlx::test]
async fn admin_routes_need_full(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (admin, _) = create_test_user(&pool).await;
    make_admin(&pool, admin).await;
    let (target, _) = create_test_user(&pool).await;
    let (_, probation) = create_probation_session_with_token(&pool, &app.config, admin).await;

    let reads = [
        "/api/admin/users".to_string(),
        "/api/admin/users/export".to_string(),
        format!("/api/admin/users/{target}/details"),
        "/api/admin/audit-log".to_string(),
        "/api/admin/stats".to_string(),
        "/api/admin/guilds".to_string(),
        "/api/admin/diagnostics".to_string(),
    ];
    for uri in &reads {
        let (status, body) = call(&app, Method::GET, uri, &probation).await;
        assert_eq!(
            (status, body["error"].as_str()),
            (403, Some("FULL_SESSION_REQUIRED")),
            "GET {uri}: {body}"
        );
    }
    let (status, body) = call(&app, Method::DELETE, "/api/admin/elevate", &probation).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (403, Some("FULL_SESSION_REQUIRED"))
    );

    // `/status` answers any session.
    let (status, body) = call(&app, Method::GET, "/api/admin/status", &probation).await;
    assert_eq!(status, 200, "{body}");

    // A full session of the same admin reads the list.
    let (_, full) = create_session_with_token(&pool, &app.config, admin).await;
    let (status, body) = call(&app, Method::GET, "/api/admin/users", &full).await;
    assert_eq!(status, 200, "{body}");

    // A non-admin is told it is no admin, whatever its assurance.
    let (plain, _) = create_test_user(&pool).await;
    let (_, plain_probation) = create_probation_session_with_token(&pool, &app.config, plain).await;
    let (status, body) = call(&app, Method::GET, "/api/admin/users", &plain_probation).await;
    assert_eq!((status, body["error"].as_str()), (403, Some("NOT_ADMIN")));
}

/// An elevation row on a probation session (one from before the upgrade)
/// opens no elevated route.
#[sqlx::test]
async fn probation_elevation_opens_nothing(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (admin, _) = create_test_user(&pool).await;
    make_admin(&pool, admin).await;
    let (target, _) = create_test_user(&pool).await;
    let (sid, probation) = create_probation_session_with_token(&pool, &app.config, admin).await;
    sqlx::query(
        "INSERT INTO elevated_sessions (user_id, session_id, ip_address, expires_at, reason)
         VALUES ($1, $2, '127.0.0.1', NOW() + INTERVAL '15 minutes', 'legacy')",
    )
    .bind(admin)
    .bind(sid)
    .execute(&pool)
    .await
    .unwrap();
    let req = peer_request(Method::POST, &format!("/api/admin/users/{target}/ban"))
        .header("Authorization", format!("Bearer {probation}"))
        .header("Content-Type", "application/json")
        .body(Body::from(r#"{"reason":"x"}"#))
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    let body = body_to_json(resp).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (403, Some("FULL_SESSION_REQUIRED")),
        "{body}"
    );
    let banned: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM global_bans WHERE user_id = $1)")
            .bind(target)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!banned);
}

// ============================================================================
// L1b Task 16: own-account routes behind the guards (C27)
// ============================================================================

use serde_json::{json, Value};
use uuid::Uuid;
use vc_server::auth::step_up::StepUpAction;

use super::helpers::approvals::KeyDeviceFixture;
use super::helpers::login::device_grant;
use super::helpers::{body_to_json_lenient, create_bot_application};

async fn send(app: &TestApp, method: Method, uri: &str, token: &str, body: Value) -> (u16, Value) {
    let req = peer_request(method, uri)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    (status, body_to_json_lenient(resp).await)
}

/// One guarded route: how to call it and which grant opens it.
struct Guarded {
    method: Method,
    uri: String,
    body: Value,
    action: StepUpAction,
    target: String,
}

/// The step-up routes of C27 for `user` (a session id `other` to end and an
/// identity id to unlink).
fn guarded_routes(other: Uuid, identity: Uuid) -> Vec<Guarded> {
    let g = |method: Method, uri: &str, body: Value, action, target: &str| Guarded {
        method,
        uri: uri.to_string(),
        body,
        action,
        target: target.to_string(),
    };
    vec![
        g(
            Method::POST,
            "/auth/me/password",
            json!({"new_password": "zq8#vT4p-guarded"}),
            StepUpAction::PasswordSet,
            "",
        ),
        g(
            Method::DELETE,
            "/auth/me/password",
            json!({}),
            StepUpAction::PasswordRemove,
            "",
        ),
        g(
            Method::POST,
            "/auth/mfa/setup",
            json!({}),
            StepUpAction::TotpSet,
            "",
        ),
        g(
            Method::POST,
            "/auth/mfa/disable",
            json!({}),
            StepUpAction::TotpRemove,
            "",
        ),
        g(
            Method::POST,
            "/auth/recovery-codes",
            json!({}),
            StepUpAction::RecoveryCodesCreate,
            "",
        ),
        g(
            Method::DELETE,
            &format!("/auth/me/identities/{identity}"),
            json!({}),
            StepUpAction::IdentityUnlink,
            &identity.to_string(),
        ),
        g(
            Method::DELETE,
            &format!("/auth/sessions/{other}"),
            json!({}),
            StepUpAction::SessionsEnd,
            &other.to_string(),
        ),
        g(
            Method::DELETE,
            "/auth/sessions",
            json!({}),
            StepUpAction::SessionsEnd,
            "others",
        ),
        g(
            Method::POST,
            "/api/me/delete-account",
            json!({"confirm": "DELETE"}),
            StepUpAction::AccountDelete,
            "",
        ),
    ]
}

async fn link_identity(pool: &PgPool, user: Uuid) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO user_identities (user_id, provider_slug, subject)
         VALUES ($1, 'testidp', $2) RETURNING id",
    )
    .bind(user)
    .bind(Uuid::new_v4().to_string())
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test]
async fn probation_gets_full_session_required(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (other, _) = create_session_with_token(&pool, &app.config, user).await;
    let identity = link_identity(&pool, user).await;
    let (_, probation) = create_probation_session_with_token(&pool, &app.config, user).await;
    for r in guarded_routes(other, identity) {
        let (status, body) = send(&app, r.method.clone(), &r.uri, &probation, r.body).await;
        assert_eq!(
            (status, body["error"].as_str()),
            (403, Some("FULL_SESSION_REQUIRED")),
            "{} {}: {body}",
            r.method,
            r.uri
        );
    }
    let (status, body) = send(
        &app,
        Method::GET,
        "/auth/me/identities/authorize/anything",
        &probation,
        json!({}),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (403, Some("FULL_SESSION_REQUIRED"))
    );
}

#[sqlx::test]
async fn full_without_grant_gets_step_up_required(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (other, _) = create_session_with_token(&pool, &app.config, user).await;
    let identity = link_identity(&pool, user).await;
    for r in guarded_routes(other, identity) {
        let (status, body) = send(&app, r.method.clone(), &r.uri, &dev.access_token, r.body).await;
        assert_eq!(
            (status, body["error"].as_str()),
            (403, Some("STEP_UP_REQUIRED")),
            "{} {}: {body}",
            r.method,
            r.uri
        );
    }
    let (status, body) = send(
        &app,
        Method::GET,
        "/auth/me/identities/authorize/anything",
        &dev.access_token,
        json!({}),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (403, Some("STEP_UP_REQUIRED"))
    );
}

/// Each route opens with its own grant (one account per route: deleting the
/// account ends its sessions).
#[sqlx::test]
async fn each_route_opens_with_its_grant(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let template = guarded_routes(Uuid::nil(), Uuid::nil());
    for i in 0..template.len() {
        let (user, _) = create_test_user(&pool).await;
        let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
        let (other, _) = create_session_with_token(&pool, &app.config, user).await;
        let identity = link_identity(&pool, user).await;
        // TOTP on, so `/auth/mfa/disable` has something to turn off.
        sqlx::query("UPDATE users SET mfa_secret = 'encrypted' WHERE id = $1")
            .bind(user)
            .execute(&pool)
            .await
            .unwrap();
        let r = guarded_routes(other, identity).remove(i);
        device_grant(&app, &dev, r.action, &r.target).await;
        let (status, body) = send(&app, r.method.clone(), &r.uri, &dev.access_token, r.body).await;
        assert!(
            (200..300).contains(&status),
            "{} {} with its grant: {status} {body}",
            r.method,
            r.uri
        );
    }
}

#[sqlx::test]
async fn grant_for_another_action_or_target_is_refused(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (a, _) = create_session_with_token(&pool, &app.config, user).await;
    let (b, _) = create_session_with_token(&pool, &app.config, user).await;

    // A grant to end session A does not end session B.
    device_grant(&app, &dev, StepUpAction::SessionsEnd, &a.to_string()).await;
    let (status, body) = send(
        &app,
        Method::DELETE,
        &format!("/auth/sessions/{b}"),
        &dev.access_token,
        json!({}),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (403, Some("STEP_UP_REQUIRED"))
    );
    // A grant for another action opens nothing else.
    device_grant(&app, &dev, StepUpAction::PasswordRemove, "").await;
    let (status, body) = send(
        &app,
        Method::POST,
        "/auth/me/password",
        &dev.access_token,
        json!({"new_password": "zq8#vT4p-guarded"}),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (403, Some("STEP_UP_REQUIRED"))
    );
    // The right one works.
    let (status, body) = send(
        &app,
        Method::DELETE,
        &format!("/auth/sessions/{a}"),
        &dev.access_token,
        json!({}),
    )
    .await;
    assert_eq!(status, 204, "{body}");
}

#[sqlx::test]
async fn grant_is_consumed_by_the_action(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    create_session_with_token(&pool, &app.config, user).await;
    device_grant(&app, &dev, StepUpAction::SessionsEnd, "others").await;
    let (status, body) = send(
        &app,
        Method::DELETE,
        "/auth/sessions",
        &dev.access_token,
        json!({}),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert!(body["revoked_count"].as_i64().unwrap() >= 1, "{body}");
    let (status, body) = send(
        &app,
        Method::DELETE,
        "/auth/sessions",
        &dev.access_token,
        json!({}),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (403, Some("STEP_UP_REQUIRED"))
    );
    // The action is in the security log.
    let details: Vec<Value> = sqlx::query_scalar(
        "SELECT details FROM security_events WHERE subject_id = $1 AND event_type = 'step_up_action'",
    )
    .bind(user)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        details,
        vec![json!({"action": "sessions.end", "target_label": "all other sessions"})]
    );
}

#[sqlx::test]
async fn probation_keeps_chat_profile_settings_log_and_logout(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let guild = super::helpers::create_guild(&pool, user).await;
    let channel = super::helpers::create_channel(&pool, guild, "general").await;
    let (_, probation) = create_probation_session_with_token(&pool, &app.config, user).await;

    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/api/messages/channel/{channel}"),
        &probation,
        json!({"content": "hello from probation"}),
    )
    .await;
    assert!((200..300).contains(&status), "send: {status} {body}");
    let (status, body) = send(
        &app,
        Method::POST,
        "/auth/me",
        &probation,
        json!({"display_name": "Still Me"}),
    )
    .await;
    assert!((200..300).contains(&status), "profile: {status} {body}");
    let (status, body) = send(
        &app,
        Method::GET,
        "/api/me/preferences",
        &probation,
        json!({}),
    )
    .await;
    assert!((200..300).contains(&status), "settings: {status} {body}");
    let (status, body) = send(
        &app,
        Method::GET,
        "/api/me/security-events",
        &probation,
        json!({}),
    )
    .await;
    assert!((200..300).contains(&status), "log: {status} {body}");
    let (status, body) = send(
        &app,
        Method::GET,
        "/auth/recovery-codes/count",
        &probation,
        json!({}),
    )
    .await;
    assert!((200..300).contains(&status), "count: {status} {body}");
    let (status, body) = send(&app, Method::POST, "/auth/logout", &probation, json!({})).await;
    assert!((200..300).contains(&status), "logout: {status} {body}");
}

#[sqlx::test]
async fn long_lived_tokens_need_full(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (app_id, _, _) = create_bot_application(&pool, user).await;
    let guild = super::helpers::create_guild(&pool, user).await;
    let channel = super::helpers::create_channel(&pool, guild, "hooks").await;
    let (_, probation) = create_probation_session_with_token(&pool, &app.config, user).await;
    for (uri, body) in [
        (format!("/api/applications/{app_id}/bot"), json!({})),
        (format!("/api/applications/{app_id}/reset-token"), json!({})),
        (
            format!("/api/channels/{channel}/webhooks"),
            json!({"name": "ci"}),
        ),
    ] {
        let (status, resp) = send(&app, Method::POST, &uri, &probation, body).await;
        assert_eq!(
            (status, resp["error"].as_str()),
            (403, Some("FULL_SESSION_REQUIRED")),
            "{uri}: {resp}"
        );
    }
    // A full session creates the webhook (the guild owner may).
    let (_, full) = create_session_with_token(&pool, &app.config, user).await;
    let (status, resp) = send(
        &app,
        Method::POST,
        &format!("/api/channels/{channel}/webhooks"),
        &full,
        json!({"name": "ci"}),
    )
    .await;
    assert!((200..300).contains(&status), "{status} {resp}");
}

#[sqlx::test]
async fn password_change_uses_the_blocklist_and_starts_a_generation(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let generation = |pool: PgPool| async move {
        sqlx::query_scalar::<_, i64>("SELECT password_generation FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap()
    };
    let g1 = generation(pool.clone()).await;
    device_grant(&app, &dev, StepUpAction::PasswordSet, "").await;
    let (status, body) = send(
        &app,
        Method::POST,
        "/auth/me/password",
        &dev.access_token,
        json!({"new_password": "iloveyou"}),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str(), body["reason"].as_str()),
        (400, Some("PASSWORD_REJECTED"), Some("breached")),
        "{body}"
    );
    // The refusal did not spend the grant; `current_password` is ignored.
    let (status, body) = send(
        &app,
        Method::POST,
        "/auth/me/password",
        &dev.access_token,
        json!({"new_password": "zq8#vT4p-guarded", "current_password": "wrong"}),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(generation(pool.clone()).await, g1 + 1);
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(vc_server::auth::verify_password("zq8#vT4p-guarded", &hash).unwrap());
    let types: Vec<String> = sqlx::query_scalar(
        "SELECT event_type FROM security_events WHERE subject_id = $1 ORDER BY created_at",
    )
    .bind(user)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(types.contains(&"password_changed".to_string()), "{types:?}");
    assert!(types.contains(&"step_up_action".to_string()), "{types:?}");
}

#[sqlx::test]
async fn removing_the_password_without_a_strong_authenticator_is_refused(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    device_grant(&app, &dev, StepUpAction::PasswordRemove, "").await;
    // The device stops being active (without the revocation cascade): the
    // account would be left with nothing to sign in with.
    sqlx::query("UPDATE approval_devices SET state = 'pending' WHERE id = $1")
        .bind(dev.row_id)
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = send(
        &app,
        Method::DELETE,
        "/auth/me/password",
        &dev.access_token,
        json!({}),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (409, Some("LAST_STRONG_AUTHENTICATOR")),
        "{body}"
    );
    sqlx::query("UPDATE approval_devices SET state = 'active' WHERE id = $1")
        .bind(dev.row_id)
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = send(
        &app,
        Method::DELETE,
        "/auth/me/password",
        &dev.access_token,
        json!({}),
    )
    .await;
    assert_eq!(status, 204, "{body}");
    let hash: Option<String> = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(hash.is_none());
}

/// L1a review item: `AdminSubscribe` trusts the cached elevation only for a
/// full session (the HTTP admin guard's row check).
#[sqlx::test]
async fn probation_admin_cannot_subscribe_to_admin_events(pool: PgPool) {
    use vc_server::ws::{OutboundMsg, ServerEvent};
    let app = TestApp::with_pool(pool.clone()).await;
    let (admin, _) = create_test_user(&pool).await;
    make_admin(&pool, admin).await;
    let (sid, _) = create_probation_session_with_token(&pool, &app.config, admin).await;
    sqlx::query(
        "INSERT INTO elevated_sessions (user_id, session_id, ip_address, expires_at, reason)
         VALUES ($1, $2, '127.0.0.1', NOW() + INTERVAL '15 minutes', 'legacy')",
    )
    .bind(admin)
    .bind(sid)
    .execute(&pool)
    .await
    .unwrap();
    vc_server::admin::cache_elevated_status(&app.state.redis, sid, true, 60).await;

    let (tx, mut rx) = tokio::sync::mpsc::channel(10);
    let subscribed =
        vc_server::ws::access::SocketAccess::for_session(admin, Some(sid), app.state.db.clone());
    let mut msg_state = vc_server::ws::ClientMessageState::default();
    vc_server::ws::handle_client_message(
        r#"{"type":"admin_subscribe"}"#,
        admin,
        &app.state,
        &tx,
        &subscribed,
        &mut msg_state,
    )
    .await
    .unwrap();
    assert!(!subscribed.admin_subscribed());
    match tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .unwrap()
        .unwrap()
    {
        OutboundMsg::Event(ServerEvent::Error { code, .. }) => {
            assert_eq!(code, "ADMIN_NOT_ELEVATED")
        }
        other => panic!("unexpected {other:?}"),
    }
}
