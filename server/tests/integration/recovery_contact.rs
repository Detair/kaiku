//! The recovery contact (C28, Part C, SM3 rev 5).
//!
//! Run with: `cargo test --test integration recovery_contact`

use axum::body::Body;
use axum::http::Method;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::auth::step_up::StepUpAction;
use vc_server::security_events::{record, Actor, EventType, NewSecurityEvent};

use super::helpers::approvals::KeyDeviceFixture;
use super::helpers::login::{device_grant, peer_request};
use super::helpers::{
    body_to_json_lenient, create_probation_session_with_token, create_test_user, make_admin,
    TestApp,
};

async fn call(app: &TestApp, method: Method, uri: &str, token: &str, body: Value) -> (u16, Value) {
    let req = peer_request(method, uri)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    (status, body_to_json_lenient(resp).await)
}

/// Name `contact` for the account of `dev` (grant + PUT).
async fn name(app: &TestApp, dev: &KeyDeviceFixture, contact: Uuid) -> (u16, Value) {
    device_grant(
        app,
        dev,
        StepUpAction::RecoveryContactSet,
        &contact.to_string(),
    )
    .await;
    call(
        app,
        Method::PUT,
        "/api/me/recovery-contact",
        &dev.access_token,
        json!({"user_id": contact}),
    )
    .await
}

async fn confirm(app: &TestApp, contact: &KeyDeviceFixture, account: Uuid) -> u16 {
    call(
        app,
        Method::POST,
        &format!("/api/me/recovery-contact-for/{account}/confirm"),
        &contact.access_token,
        json!({}),
    )
    .await
    .0
}

#[sqlx::test]
async fn naming_needs_full_and_grant(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (contact, contact_name) = create_test_user(&pool).await;
    let (_, probation) = create_probation_session_with_token(&pool, &app.config, user).await;
    let body = json!({"user_id": contact});
    let (s, b) = call(
        &app,
        Method::PUT,
        "/api/me/recovery-contact",
        &probation,
        body.clone(),
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (403, Some("FULL_SESSION_REQUIRED"))
    );
    let (s, b) = call(
        &app,
        Method::PUT,
        "/api/me/recovery-contact",
        &dev.access_token,
        body,
    )
    .await;
    assert_eq!((s, b["error"].as_str()), (403, Some("STEP_UP_REQUIRED")));
    let (s, b) = name(&app, &dev, contact).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(b["username"], contact_name);
    assert!(b["confirmed_at"].is_null() && b["counts_from"].is_null());
    // Removing takes the grant for "".
    let (s, _) = call(
        &app,
        Method::DELETE,
        "/api/me/recovery-contact",
        &dev.access_token,
        json!({}),
    )
    .await;
    assert_eq!(s, 403);
    device_grant(&app, &dev, StepUpAction::RecoveryContactSet, "").await;
    let (s, _) = call(
        &app,
        Method::DELETE,
        "/api/me/recovery-contact",
        &dev.access_token,
        json!({}),
    )
    .await;
    assert_eq!(s, 204);
}

#[sqlx::test]
async fn invalid_contacts_are_refused(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (admin, _) = create_test_user(&pool).await;
    make_admin(&pool, admin).await;
    for bad in [
        admin,
        user,
        vc_server::system_account::SYSTEM_USER_ID,
        Uuid::new_v4(),
    ] {
        let (s, b) = name(&app, &dev, bad).await;
        assert_eq!(
            (s, b["error"].as_str()),
            (400, Some("RECOVERY_CONTACT_INVALID")),
            "{bad}"
        );
    }
}

#[sqlx::test]
async fn contact_confirms_from_a_full_session(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (contact, _) = create_test_user(&pool).await;
    let cdev = KeyDeviceFixture::insert(&pool, &app.config, contact, "active").await;
    name(&app, &dev, contact).await;
    let (_, probation) = create_probation_session_with_token(&pool, &app.config, contact).await;
    let (s, b) = call(
        &app,
        Method::POST,
        &format!("/api/me/recovery-contact-for/{user}/confirm"),
        &probation,
        json!({}),
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (403, Some("FULL_SESSION_REQUIRED"))
    );
    let (s, list) = call(
        &app,
        Method::GET,
        "/api/me/recovery-contact-for",
        &probation,
        json!({}),
    )
    .await;
    assert_eq!(s, 200);
    assert_eq!(list["accounts"][0]["user_id"], user.to_string());
    assert!(list["accounts"][0]["confirmed_at"].is_null());
    assert_eq!(confirm(&app, &cdev, user).await, 204);
    // Another account cannot confirm for someone who did not name it.
    let (other, _) = create_test_user(&pool).await;
    let odev = KeyDeviceFixture::insert(&pool, &app.config, other, "active").await;
    assert_eq!(confirm(&app, &odev, user).await, 404);
}

#[sqlx::test]
async fn a_new_contact_needs_a_new_confirmation(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (a, _) = create_test_user(&pool).await;
    let adev = KeyDeviceFixture::insert(&pool, &app.config, a, "active").await;
    let (b, _) = create_test_user(&pool).await;
    name(&app, &dev, a).await;
    assert_eq!(confirm(&app, &adev, user).await, 204);
    let (s, c) = name(&app, &dev, b).await;
    assert_eq!(s, 200);
    assert!(c["confirmed_at"].is_null(), "{c}");
    let types: Vec<String> = sqlx::query_scalar(
        "SELECT event_type FROM security_events WHERE subject_id = $1
          AND event_type LIKE 'recovery_contact_%' ORDER BY created_at",
    )
    .bind(user)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        types,
        vec![
            "recovery_contact_named",
            "recovery_contact_confirmed",
            "recovery_contact_changed"
        ]
    );
}

#[sqlx::test]
async fn counts_from_is_confirmation_plus_delay(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (contact, _) = create_test_user(&pool).await;
    let cdev = KeyDeviceFixture::insert(&pool, &app.config, contact, "active").await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    name(&app, &dev, contact).await;
    confirm(&app, &cdev, user).await;
    let ctx = vc_server::auth::ctx::AuthCtx::from(&app.state);
    let mut conn = pool.acquire().await.unwrap();
    let c = vc_server::auth::recovery_contact::current(&mut conn, &ctx, user)
        .await
        .unwrap()
        .unwrap();
    let confirmed = c.confirmed_at.unwrap();
    assert_eq!(
        c.counts_from.unwrap() - confirmed,
        chrono::Duration::hours(24)
    );
}

/// A high event reaches the account, its confirmed contact and the admins;
/// the contact's and the admin's copies carry no `ip`.
#[sqlx::test]
async fn notices_reach_account_contact_and_admins(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (contact, _) = create_test_user(&pool).await;
    let cdev = KeyDeviceFixture::insert(&pool, &app.config, contact, "active").await;
    let (admin, _) = create_test_user(&pool).await;
    make_admin(&pool, admin).await;
    name(&app, &dev, contact).await;
    confirm(&app, &cdev, user).await;
    let mut tx = pool.begin().await.unwrap();
    let mut ev = NewSecurityEvent::new(
        user,
        EventType::PasswordCompromised,
        Actor::System,
        json!({"cause": "admin"}),
    );
    ev.ip = Some("203.0.113.7".parse().unwrap());
    let id = record(&mut tx, ev).await.unwrap();
    tx.commit().await.unwrap();
    vc_server::security_events::notices::deliver(&app.state, &[id])
        .await
        .unwrap();
    let recipients: Vec<Uuid> = sqlx::query_scalar(
        "SELECT n.recipient_id FROM security_notices n
           JOIN security_notice_events e ON e.notice_id = n.id
          WHERE e.event_id = $1 ORDER BY n.recipient_id",
    )
    .bind(id)
    .fetch_all(&pool)
    .await
    .unwrap();
    let mut want = vec![user, contact, admin];
    want.sort();
    assert_eq!(recipients, want);
}

#[sqlx::test]
async fn admin_sets_contacts_for_non_admins_only(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (admin, _) = create_test_user(&pool).await;
    make_admin(&pool, admin).await;
    let adev = KeyDeviceFixture::insert(&pool, &app.config, admin, "active").await;
    let (user, _) = create_test_user(&pool).await;
    let (contact, _) = create_test_user(&pool).await;
    device_grant(
        &app,
        &adev,
        StepUpAction::AdminRecoveryContactSet,
        &user.to_string(),
    )
    .await;
    let (s, b) = call(
        &app,
        Method::PUT,
        &format!("/api/admin/users/{user}/recovery-contact"),
        &adev.access_token,
        json!({"user_id": contact}),
    )
    .await;
    assert_eq!(s, 200, "{b}");
    let (other_admin, _) = create_test_user(&pool).await;
    make_admin(&pool, other_admin).await;
    device_grant(
        &app,
        &adev,
        StepUpAction::AdminRecoveryContactSet,
        &other_admin.to_string(),
    )
    .await;
    let (s, b) = call(
        &app,
        Method::PUT,
        &format!("/api/admin/users/{other_admin}/recovery-contact"),
        &adev.access_token,
        json!({"user_id": contact}),
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (403, Some("ADMIN_TARGET_IS_ADMIN"))
    );
}

#[sqlx::test]
async fn contact_copies_have_no_ip_or_place(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (contact, _) = create_test_user(&pool).await;
    let cdev = KeyDeviceFixture::insert(&pool, &app.config, contact, "active").await;
    name(&app, &dev, contact).await;
    confirm(&app, &cdev, user).await;
    let mut tx = pool.begin().await.unwrap();
    let mut ev = NewSecurityEvent::new(
        user,
        EventType::PasswordCompromised,
        Actor::System,
        json!({"cause": "admin"}),
    );
    ev.ip = Some("203.0.113.8".parse().unwrap());
    let id = record(&mut tx, ev).await.unwrap();
    tx.commit().await.unwrap();
    vc_server::security_events::notices::deliver(&app.state, &[id])
        .await
        .unwrap();
    let message: Uuid = sqlx::query_scalar(
        "SELECT n.message_id FROM security_notices n
           JOIN security_notice_events e ON e.notice_id = n.id
          WHERE e.event_id = $1 AND n.recipient_id = $2",
    )
    .bind(id)
    .bind(contact)
    .fetch_one(&pool)
    .await
    .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let notices = vc_server::security_events::notices::notices_by_message(
        &mut conn,
        contact,
        Some(&[message]),
        None,
    )
    .await
    .unwrap();
    let n = notices.get(&message).expect("the contact's notice");
    let text = serde_json::to_string(n).unwrap();
    assert!(!text.contains("203.0.113.8"), "{text}");
    assert!(n.events.iter().all(|e| e.ip.is_none() && e.place.is_none()));
}
