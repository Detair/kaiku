//! Admin actions on an account's sign-in (C27 "Admin setup", F6 rev 4).
//!
//! Run with: `cargo test --test integration admin_setup`

use axum::body::Body;
use axum::http::Method;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use super::helpers::approvals::KeyDeviceFixture;
use super::helpers::{
    body_to_json_lenient, create_session_with_token, create_test_user, grant_step_up,
    insert_session_row, make_admin, TestApp,
};

async fn call(app: &TestApp, method: Method, uri: &str, token: &str, body: Value) -> (u16, Value) {
    let req = TestApp::request(method, uri)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    (status, body_to_json_lenient(resp).await)
}

/// A system admin with a full session.
async fn admin(app: &TestApp) -> (Uuid, Uuid, String) {
    let (id, _) = create_test_user(&app.pool).await;
    make_admin(&app.pool, id).await;
    let (sid, token) = create_session_with_token(&app.pool, &app.config, id).await;
    (id, sid, token)
}

async fn events(pool: &PgPool, user: Uuid) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT event_type FROM security_events WHERE subject_id = $1 ORDER BY created_at, id",
    )
    .bind(user)
    .fetch_all(pool)
    .await
    .unwrap()
}

#[sqlx::test]
async fn may_approve_and_factor_toggles_are_audited(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (_, sid, token) = admin(&app).await;
    let (user, name) = create_test_user(&pool).await;
    let uri = format!("/api/admin/users/{user}/may-approve");

    // Without the grant: refused, nothing changes.
    let (s, b) = call(
        &app,
        Method::PUT,
        &uri,
        &token,
        json!({"may_approve": false}),
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (403, Some("STEP_UP_REQUIRED")),
        "{b}"
    );
    grant_step_up(&pool, sid, "admin.may_approve", &user.to_string()).await;
    let (s, b) = call(
        &app,
        Method::PUT,
        &uri,
        &token,
        json!({"may_approve": false}),
    )
    .await;
    assert_eq!((s, b["may_approve"].as_bool()), (200, Some(false)), "{b}");
    let may: bool = sqlx::query_scalar("SELECT may_approve FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!may);
    // One grant per action.
    let (s, _) = call(
        &app,
        Method::PUT,
        &uri,
        &token,
        json!({"may_approve": true}),
    )
    .await;
    assert_eq!(s, 403);

    // Mark the password unsafe, then a throttled TOTP allowed again.
    sqlx::query(
        "UPDATE users SET password_hash = 'x', mfa_secret = 'y', totp_state = 'throttled' WHERE id = $1",
    )
    .bind(user)
    .execute(&pool)
    .await
    .unwrap();
    let uri = format!("/api/admin/users/{user}/factors");
    grant_step_up(&pool, sid, "admin.factor.set_state", &user.to_string()).await;
    let (s, b) = call(
        &app,
        Method::PUT,
        &uri,
        &token,
        json!({"password": "compromised", "totp": "active"}),
    )
    .await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(b, json!({"password": "compromised", "totp": "active"}));
    // A compromised password is not allowed again by an admin (B.1).
    grant_step_up(&pool, sid, "admin.factor.set_state", &user.to_string()).await;
    let (s, b) = call(
        &app,
        Method::PUT,
        &uri,
        &token,
        json!({"password": "active"}),
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (400, Some("VALIDATION_ERROR")),
        "{b}"
    );

    let ev = events(&pool, user).await;
    assert!(ev.contains(&"password_compromised".to_string()), "{ev:?}");
    let audit: Vec<String> = sqlx::query_scalar(
        "SELECT action FROM system_audit_log
          WHERE target_id = $1 AND action LIKE 'admin.users.%' ORDER BY created_at",
    )
    .bind(user)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        audit,
        ["admin.users.may_approve", "admin.users.factors"],
        "{name}"
    );
}

#[sqlx::test]
async fn immediate_actions_on_other_admins_are_refused(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (me, sid, token) = admin(&app).await;
    let (other, _) = create_test_user(&pool).await;
    make_admin(&pool, other).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, other, "active").await;
    for (action, target) in [
        ("admin.may_approve", other),
        ("admin.factor.set_state", other),
        ("admin.authenticator.remove", dev.row_id),
        ("admin.recovery_contact.set", other),
    ] {
        grant_step_up(&pool, sid, action, &target.to_string()).await;
    }
    let (contact, _) = create_test_user(&pool).await;
    for (m, uri, body) in [
        (
            Method::PUT,
            format!("/api/admin/users/{other}/may-approve"),
            json!({"may_approve": false}),
        ),
        (
            Method::PUT,
            format!("/api/admin/users/{other}/factors"),
            json!({"password": "compromised"}),
        ),
        (
            Method::POST,
            format!(
                "/api/admin/users/{other}/authenticators/{}/remove",
                dev.row_id
            ),
            json!({}),
        ),
        (
            Method::PUT,
            format!("/api/admin/users/{other}/recovery-contact"),
            json!({"user_id": contact}),
        ),
    ] {
        let (s, b) = call(&app, m, &uri, &token, body).await;
        assert_eq!(
            (s, b["error"].as_str()),
            (403, Some("ADMIN_TARGET_IS_ADMIN")),
            "{uri}: {b}"
        );
    }
    // The admin's own account is no "other admin".
    grant_step_up(&pool, sid, "admin.may_approve", &me.to_string()).await;
    let (s, b) = call(
        &app,
        Method::PUT,
        &format!("/api/admin/users/{me}/may-approve"),
        &token,
        json!({"may_approve": true}),
    )
    .await;
    assert_eq!(s, 200, "{b}");
}

#[sqlx::test]
async fn remove_device_now_notifies_the_removed_device_last(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (_, sid, token) = admin(&app).await;
    let (user, _) = create_test_user(&pool).await;
    // The account's only strong authenticator: no last-authenticator check.
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    sqlx::query(
        "INSERT INTO push_subscriptions (user_id, provider, endpoint, device_id)
         VALUES ($1, 'unifiedpush', 'https://push.invalid/x', $2)",
    )
    .bind(user)
    .bind(dev.device_id)
    .execute(&pool)
    .await
    .unwrap();
    let uri = format!(
        "/api/admin/users/{user}/authenticators/{}/remove",
        dev.row_id
    );
    let (s, _) = call(&app, Method::POST, &uri, &token, json!({})).await;
    assert_eq!(s, 403, "needs the grant");
    grant_step_up(
        &pool,
        sid,
        "admin.authenticator.remove",
        &dev.row_id.to_string(),
    )
    .await;
    let (s, b) = call(&app, Method::POST, &uri, &token, json!({})).await;
    assert_eq!(s, 204, "{b}");
    let (state, reason): (String, Option<String>) =
        sqlx::query_as("SELECT state, revoke_reason FROM approval_devices WHERE id = $1")
            .bind(dev.row_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        (state.as_str(), reason.as_deref()),
        ("revoked", Some("admin"))
    );
    // Its session ended with it.
    let live: bool = sqlx::query_scalar("SELECT ended_at IS NULL FROM sessions WHERE id = $1")
        .bind(dev.session_id.unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!live);
    let removed: Value = sqlx::query_scalar(
        "SELECT details FROM security_events
          WHERE subject_id = $1 AND event_type = 'authenticator_removed'",
    )
    .bind(user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(removed["by"], "admin");
    // The removed phone's last notice was addressed to it before its push
    // rows went (the delivery ran inline).
    let notice: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM security_notices
          WHERE subject_id = $1 AND event_type = 'authenticator_removed'",
    )
    .bind(user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        notice >= 1,
        "the notice was delivered before the push rows went"
    );
    let subs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM push_subscriptions WHERE user_id = $1 AND device_id = $2",
    )
    .bind(user)
    .bind(dev.device_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(subs, 0);
}

#[sqlx::test]
async fn admin_view_fields(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (_, _, token) = admin(&app).await;
    let (user, _) = create_test_user(&pool).await;
    KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    insert_session_row(&pool, Uuid::new_v4(), user, false).await;
    let (s, b) = call(
        &app,
        Method::GET,
        &format!("/api/admin/users/{user}/details"),
        &token,
        json!({}),
    )
    .await;
    assert_eq!(s, 200, "{b}");
    for field in [
        "is_admin",
        "may_approve",
        "key_devices",
        "passkeys",
        "password_state",
        "totp_state",
        "recovery_codes_left",
        "bootstrap_open",
        "recovery_contact",
        "open_request",
        "open_sessions",
    ] {
        assert!(b.get(field).is_some(), "{field}: {b}");
    }
    assert_eq!(b["is_admin"], false);
    assert_eq!(b["key_devices"].as_array().unwrap().len(), 1);
    assert_eq!(b["key_devices"][0]["state"], "active");
    assert_eq!(b["open_sessions"]["probation"].as_i64().unwrap() >= 1, true);
    let (s, list) = call(&app, Method::GET, "/api/admin/users", &token, json!({})).await;
    assert_eq!(s, 200, "{list}");
    let me = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["id"] == user.to_string())
        .unwrap();
    assert_eq!(me["is_admin"], false);
    assert!(me["may_approve"].is_boolean());
}
