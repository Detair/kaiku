//! Push subscriptions belong to the session that registered them.
//!
//! A chat-only (probation) session may overwrite or delete only its own rows,
//! rows without an owner, and rows whose owning session is no longer live; it
//! holds at most two rows and never prunes another live session's rows. When a
//! session ends, its rows go in the same transaction, whatever the reason. A
//! key phone's row (the install of an active or pending key device) belongs to
//! the device instead: no session end touches it, only a full session may
//! register, overwrite or delete it, and it goes with the device's revocation.
//! A device-key sign-in takes over the rows of its install.
//!
//! Run with: `cargo test --test integration push_ownership`

use axum::body::Body;
use axum::http::Method;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::approvals::secrets::b64u_decode;
use vc_server::approvals::signing::Decision;
use vc_server::auth::ctx::AuthCtx;
use vc_server::auth::revocation::{end_sessions_tx, EndReason};
use vc_server::auth::step_up::StepUpAction;

use super::helpers::approvals::KeyDeviceFixture;
use super::helpers::login::{device_grant, peer_request};
use super::helpers::migrations::{apply_from, apply_until};
use super::helpers::{
    body_to_json_lenient, create_probation_session_with_token, create_session_with_token,
    create_test_user, wait_for_lock_waiters, TestApp,
};

const PUSH_OWNER: i64 = 20_261_127_000_200;

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

/// `POST /api/me/push-subscriptions` with a `UnifiedPush` endpoint.
async fn register(
    app: &TestApp,
    token: &str,
    endpoint: &str,
    device: Option<Uuid>,
) -> (u16, Value) {
    let mut body = json!({
        "provider": "unifiedpush",
        "endpoint": format!("https://push.example.com/{endpoint}"),
    });
    if let Some(d) = device {
        body["device_id"] = json!(d);
    }
    send(app, Method::POST, "/api/me/push-subscriptions", token, body).await
}

async fn delete_device(app: &TestApp, token: &str, device: Uuid) -> u16 {
    send(
        app,
        Method::DELETE,
        &format!("/api/me/push-subscriptions/device/{device}"),
        token,
        json!({}),
    )
    .await
    .0
}

/// `(endpoint suffix, owning session)` of the user's row for `device`.
async fn row(pool: &PgPool, user: Uuid, device: Uuid) -> Option<(String, Option<Uuid>)> {
    sqlx::query_as::<_, (String, Option<Uuid>)>(
        "SELECT replace(endpoint, 'https://push.example.com/', ''), session_id
           FROM push_subscriptions WHERE user_id = $1 AND device_id = $2",
    )
    .bind(user)
    .bind(device)
    .fetch_optional(pool)
    .await
    .unwrap()
}

async fn rows_of_session(pool: &PgPool, sid: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM push_subscriptions WHERE session_id = $1")
        .bind(sid)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn rows_of_user(pool: &PgPool, user: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM push_subscriptions WHERE user_id = $1")
        .bind(user)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn insert_row(pool: &PgPool, user: Uuid, endpoint: &str, device: Uuid, owner: Option<Uuid>) {
    sqlx::query(
        "INSERT INTO push_subscriptions (user_id, provider, endpoint, device_id, session_id)
         VALUES ($1, 'unifiedpush', $2, $3, $4)",
    )
    .bind(user)
    .bind(format!("https://push.example.com/{endpoint}"))
    .bind(device)
    .bind(owner)
    .execute(pool)
    .await
    .unwrap();
}

async fn end_session(app: &TestApp, user: Uuid, sid: Uuid, reason: EndReason) {
    let ctx = AuthCtx::from(&app.state);
    let mut tx = app.pool.begin().await.unwrap();
    let ended = end_sessions_tx(&mut tx, &ctx, user, &[sid], reason)
        .await
        .unwrap();
    assert_eq!(ended, vec![sid]);
    tx.commit().await.unwrap();
}

fn assert_full_required(status: u16, body: &Value) {
    assert_eq!(
        (status, body["error"].as_str()),
        (403, Some("FULL_SESSION_REQUIRED")),
        "{body}"
    );
}

// ----------------------------------------------------------------------------
// Device-key sign-in (as the Android app does it after a restart)
// ----------------------------------------------------------------------------

async fn public(app: &TestApp, path: &str, body: Value) -> (u16, Value) {
    let req = peer_request(Method::POST, path)
        .header("Content-Type", "application/json")
        .header("X-Kaiku-Client", "android")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    (status, body_to_json_lenient(resp).await)
}

/// Sign in with the device key; returns `(session id, access token)`.
async fn device_sign_in(app: &TestApp, dev: &KeyDeviceFixture) -> (Uuid, String) {
    let (s, c) = public(
        app,
        "/auth/device-login/challenge",
        json!({"user_id": dev.user_id, "device_id": dev.device_id}),
    )
    .await;
    assert_eq!(s, 201, "{c}");
    let prefix = b64u_decode(c["signing_prefix"].as_str().unwrap()).unwrap();
    let sig = dev.sign(&prefix, Decision::Approve, b"");
    let (s, auth) = public(
        app,
        "/auth/device-login",
        json!({"challenge_id": c["challenge_id"], "signature": sig}),
    )
    .await;
    assert_eq!(s, 200, "{auth}");
    let token = auth["access_token"].as_str().unwrap().to_string();
    let sid: Uuid = sqlx::query_scalar(
        "SELECT id FROM sessions WHERE user_id = $1 AND authenticator_id = $2
            AND ended_at IS NULL",
    )
    .bind(dev.user_id)
    .bind(dev.row_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    (sid, token)
}

// ============================================================================
// Migration
// ============================================================================

async fn session_row(pool: &PgPool, user: Uuid, device_row: Option<Uuid>, live: bool) -> Uuid {
    let id = Uuid::now_v7();
    let (assurance, methods, kind) = match device_row {
        Some(_) => ("full", "{device_key}", Some("key_device")),
        None => ("probation", "{qr}", None),
    };
    sqlx::query(
        "INSERT INTO sessions (id, user_id, token_hash, expires_at, assurance, auth_methods,
                               client_class, authenticator_type, authenticator_id,
                               absolute_expires_at, ended_at, end_reason)
         VALUES ($1, $2, $3, NOW() + INTERVAL '7 days', $4, $5::text[], 'android', $6, $7,
                 NOW() + INTERVAL '90 days',
                 CASE WHEN $8 THEN NULL ELSE NOW() END,
                 CASE WHEN $8 THEN NULL ELSE 'logout' END)",
    )
    .bind(id)
    .bind(user)
    .bind(format!("hash-{id}"))
    .bind(assurance)
    .bind(methods)
    .bind(kind)
    .bind(device_row)
    .bind(live)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn key_device(pool: &PgPool, user: Uuid, install: Uuid, state: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO approval_devices
            (id, user_id, device_id, public_key, public_key_hash, uv_class, label, state,
             enrolled_via, activated_at, revoked_at, revoke_reason)
         VALUES ($1, $2, $3, '\\x00', $4, 'per_use', 'Pixel', $5, 'cli', NOW(),
                 CASE WHEN $5 = 'revoked' THEN NOW() END,
                 CASE WHEN $5 = 'revoked' THEN 'owner' END)",
    )
    .bind(id)
    .bind(user)
    .bind(install)
    .bind(vec![7u8; 32])
    .bind(state)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn plain_user(pool: &PgPool, name: &str) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO users (username, display_name, password_hash) VALUES ($1, $1, 'x')
         RETURNING id",
    )
    .bind(name)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn old_row(pool: &PgPool, user: Uuid, endpoint: &str, device: Option<Uuid>) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO push_subscriptions (user_id, provider, endpoint, device_id)
         VALUES ($1, 'fcm', $2, $3) RETURNING id",
    )
    .bind(user)
    .bind(endpoint)
    .bind(device)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn owner_of(pool: &PgPool, row: Uuid) -> Option<Uuid> {
    sqlx::query_scalar("SELECT session_id FROM push_subscriptions WHERE id = $1")
        .bind(row)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Rows of an install that is an active key device of the same account
/// belong to that device's newest live session; every other row has no
/// owner. No row is lost.
#[sqlx::test(migrations = false)]
async fn push_owner_backfill_key_device_rows(pool: PgPool) {
    apply_until(&pool, PUSH_OWNER).await;
    let anna = plain_user(&pool, "anna").await;
    let ben = plain_user(&pool, "ben").await;

    // Anna's key phone: an older (ended) and a newer live device session.
    let phone = Uuid::new_v4();
    let phone_key = key_device(&pool, anna, phone, "active").await;
    let _ended = session_row(&pool, anna, Some(phone_key), false).await;
    let live = session_row(&pool, anna, Some(phone_key), true).await;
    let phone_row = old_row(&pool, anna, "tok-anna-phone", Some(phone)).await;

    // An install without a key device (chat-only sign-in on a tablet).
    let tablet = Uuid::new_v4();
    let _chat_only = session_row(&pool, anna, None, true).await;
    let tablet_row = old_row(&pool, anna, "tok-anna-tablet", Some(tablet)).await;

    // A row from before installs had ids.
    let legacy_row = old_row(&pool, anna, "tok-anna-legacy", None).await;

    // Two accounts on one install: Ben's key device lives there, Anna's not.
    let shared = Uuid::new_v4();
    let ben_key = key_device(&pool, ben, shared, "active").await;
    let ben_live = session_row(&pool, ben, Some(ben_key), true).await;
    let ben_row = old_row(&pool, ben, "tok-shared-ben", Some(shared)).await;
    let anna_shared_row = old_row(&pool, anna, "tok-shared-anna", Some(shared)).await;

    // A phone whose key device was removed (its session ended with it).
    let removed = Uuid::new_v4();
    let removed_key = key_device(&pool, anna, removed, "revoked").await;
    let _gone = session_row(&pool, anna, Some(removed_key), false).await;
    let removed_row = old_row(&pool, anna, "tok-anna-removed", Some(removed)).await;

    // A key phone that is signed out (no live session).
    let signed_out = Uuid::new_v4();
    let signed_out_key = key_device(&pool, ben, signed_out, "active").await;
    let _logged_out = session_row(&pool, ben, Some(signed_out_key), false).await;
    let signed_out_row = old_row(&pool, ben, "tok-ben-signed-out", Some(signed_out)).await;

    apply_from(&pool, PUSH_OWNER).await;

    assert_eq!(owner_of(&pool, phone_row).await, Some(live));
    assert_eq!(owner_of(&pool, ben_row).await, Some(ben_live));
    for orphan in [
        tablet_row,
        legacy_row,
        anna_shared_row,
        removed_row,
        signed_out_row,
    ] {
        assert_eq!(owner_of(&pool, orphan).await, None, "row {orphan}");
    }
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM push_subscriptions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(total, 7, "the migration removes no subscription");
}

// ============================================================================
// Ownership on registration and deletion
// ============================================================================

#[sqlx::test]
async fn registration_records_the_registering_session(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (sid, token) = create_probation_session_with_token(&pool, &app.config, user).await;
    let device = Uuid::new_v4();
    let (s, b) = register(&app, &token, "tablet", Some(device)).await;
    assert_eq!(s, 201, "{b}");
    assert_eq!(
        row(&pool, user, device).await,
        Some(("tablet".into(), Some(sid)))
    );
    // The answer is unchanged (no owner on the wire).
    assert!(b.get("session_id").is_none(), "{b}");

    // Without a device id the endpoint row is owned too.
    let (s, b) = register(&app, &token, "legacy", None).await;
    assert_eq!(s, 201, "{b}");
    assert_eq!(rows_of_session(&pool, sid).await, 2);
}

#[sqlx::test]
async fn probation_upsert_cannot_take_live_rows_device_id(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let phone = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (s, b) = register(&app, &phone.access_token, "phone", Some(phone.device_id)).await;
    assert_eq!(s, 201, "{b}");

    let (_, chat_only) = create_probation_session_with_token(&pool, &app.config, user).await;
    let (s, b) = register(&app, &chat_only, "elsewhere", Some(phone.device_id)).await;
    assert_full_required(s, &b);
    assert_eq!(
        row(&pool, user, phone.device_id).await,
        Some(("phone".into(), phone.session_id)),
        "the phone's row is unchanged"
    );
}

#[sqlx::test]
async fn probation_upsert_cannot_take_live_rows_endpoint(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let phone = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (s, b) = register(&app, &phone.access_token, "phone", Some(phone.device_id)).await;
    assert_eq!(s, 201, "{b}");

    let (_, chat_only) = create_probation_session_with_token(&pool, &app.config, user).await;
    // The same endpoint under another install id would delete the phone's row,
    let (s, b) = register(&app, &chat_only, "phone", Some(Uuid::new_v4())).await;
    assert_full_required(s, &b);
    // and without an install id it would overwrite it.
    let (s, b) = register(&app, &chat_only, "phone", None).await;
    assert_full_required(s, &b);
    assert_eq!(
        row(&pool, user, phone.device_id).await,
        Some(("phone".into(), phone.session_id))
    );
    assert_eq!(rows_of_user(&pool, user).await, 1);
}

#[sqlx::test]
async fn full_session_may_take_any_row(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (_, chat_only) = create_probation_session_with_token(&pool, &app.config, user).await;
    let device = Uuid::new_v4();
    let (s, b) = register(&app, &chat_only, "tablet", Some(device)).await;
    assert_eq!(s, 201, "{b}");

    let (full_sid, full) = create_session_with_token(&pool, &app.config, user).await;
    let (s, b) = register(&app, &full, "tablet-2", Some(device)).await;
    assert_eq!(s, 201, "{b}");
    assert_eq!(
        row(&pool, user, device).await,
        Some(("tablet-2".into(), Some(full_sid)))
    );
}

#[sqlx::test]
async fn probation_delete_device_skips_foreign_row(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let phone = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    register(&app, &phone.access_token, "phone", Some(phone.device_id)).await;
    let (_, chat_only) = create_probation_session_with_token(&pool, &app.config, user).await;

    // Another live session's row: skipped, still 204.
    assert_eq!(delete_device(&app, &chat_only, phone.device_id).await, 204);
    assert!(row(&pool, user, phone.device_id).await.is_some());

    // Its own row, a row without owner and one of an ended session: deleted.
    let own = Uuid::new_v4();
    register(&app, &chat_only, "own", Some(own)).await;
    assert_eq!(delete_device(&app, &chat_only, own).await, 204);
    assert!(row(&pool, user, own).await.is_none());

    let orphan = Uuid::new_v4();
    insert_row(&pool, user, "orphan", orphan, None).await;
    assert_eq!(delete_device(&app, &chat_only, orphan).await, 204);
    assert!(row(&pool, user, orphan).await.is_none());

    let (ended_sid, _) = create_probation_session_with_token(&pool, &app.config, user).await;
    end_session(&app, user, ended_sid, EndReason::Logout).await;
    let left = Uuid::new_v4();
    insert_row(&pool, user, "left", left, Some(ended_sid)).await;
    assert_eq!(delete_device(&app, &chat_only, left).await, 204);
    assert!(row(&pool, user, left).await.is_none());

    // A full session deletes the phone's row (the app's own sign-out).
    assert_eq!(
        delete_device(&app, &phone.access_token, phone.device_id).await,
        204
    );
    assert!(row(&pool, user, phone.device_id).await.is_none());
}

#[sqlx::test]
async fn orphan_row_reclaimed_by_next_registration(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let device = Uuid::new_v4();
    insert_row(&pool, user, "before-upgrade", device, None).await;
    let (sid, chat_only) = create_probation_session_with_token(&pool, &app.config, user).await;
    let (s, b) = register(&app, &chat_only, "after-upgrade", Some(device)).await;
    assert_eq!(s, 201, "{b}");
    assert_eq!(
        row(&pool, user, device).await,
        Some(("after-upgrade".into(), Some(sid)))
    );

    // A row whose session lapsed is free as well.
    let (old_sid, _) = create_probation_session_with_token(&pool, &app.config, user).await;
    sqlx::query(
        "UPDATE sessions SET created_at = NOW() - INTERVAL '2 days',
                auth_time = NOW() - INTERVAL '2 days',
                expires_at = NOW() - INTERVAL '1 minute' WHERE id = $1",
    )
    .bind(old_sid)
    .execute(&pool)
    .await
    .unwrap();
    let lapsed = Uuid::new_v4();
    insert_row(&pool, user, "lapsed", lapsed, Some(old_sid)).await;
    let (s, b) = register(&app, &chat_only, "reclaimed", Some(lapsed)).await;
    assert_eq!(s, 201, "{b}");
    assert_eq!(
        row(&pool, user, lapsed).await,
        Some(("reclaimed".into(), Some(sid)))
    );
}

#[sqlx::test]
async fn chat_only_session_holds_at_most_two_rows(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (sid, chat_only) = create_probation_session_with_token(&pool, &app.config, user).await;
    let devices: Vec<Uuid> = (0..4).map(|_| Uuid::new_v4()).collect();
    for (i, d) in devices.iter().enumerate() {
        let (s, b) = register(&app, &chat_only, &format!("d{i}"), Some(*d)).await;
        assert_eq!(s, 201, "{b}");
        assert!(rows_of_session(&pool, sid).await <= 2);
    }
    // The newest two are kept.
    assert!(row(&pool, user, devices[3]).await.is_some());
    assert!(row(&pool, user, devices[2]).await.is_some());
    assert_eq!(rows_of_session(&pool, sid).await, 2);
}

#[sqlx::test]
async fn probation_cap_never_evicts_foreign_rows(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (full_sid, full) = create_session_with_token(&pool, &app.config, user).await;
    for i in 0..9 {
        let (s, b) = register(&app, &full, &format!("f{i}"), Some(Uuid::new_v4())).await;
        assert_eq!(s, 201, "{b}");
    }
    // An old row without owner: the only one the cap may take.
    let orphan = Uuid::new_v4();
    insert_row(&pool, user, "orphan", orphan, None).await;
    sqlx::query(
        "UPDATE push_subscriptions SET created_at = NOW() - INTERVAL '1 day' WHERE device_id = $1",
    )
    .bind(orphan)
    .execute(&pool)
    .await
    .unwrap();
    // The full session's rows are the least recently used ones.
    sqlx::query(
        "UPDATE push_subscriptions SET created_at = NOW() - INTERVAL '2 days'
          WHERE session_id = $1",
    )
    .bind(full_sid)
    .execute(&pool)
    .await
    .unwrap();

    let (sid, chat_only) = create_probation_session_with_token(&pool, &app.config, user).await;
    for i in 0..3 {
        let (s, b) = register(&app, &chat_only, &format!("c{i}"), Some(Uuid::new_v4())).await;
        assert_eq!(s, 201, "{b}");
        assert_eq!(
            rows_of_session(&pool, full_sid).await,
            9,
            "round {i}: another live session's rows are never pruned"
        );
    }
    assert!(row(&pool, user, orphan).await.is_none(), "the orphan went");
    assert!(rows_of_session(&pool, sid).await >= 1);
    assert_eq!(rows_of_user(&pool, user).await, 10, "the cap still holds");
}

// ============================================================================
// Session end and device-key sign-in
// ============================================================================

#[sqlx::test]
async fn revoked_session_drops_its_rows(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let phone = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    register(&app, &phone.access_token, "phone", Some(phone.device_id)).await;
    let (intruder_sid, intruder) =
        create_probation_session_with_token(&pool, &app.config, user).await;
    let intruder_install = Uuid::new_v4();
    let (s, b) = register(&app, &intruder, "intruder", Some(intruder_install)).await;
    assert_eq!(s, 201, "{b}");

    // The owner ends the intruder's session from the phone.
    device_grant(
        &app,
        &phone,
        StepUpAction::SessionsEnd,
        &intruder_sid.to_string(),
    )
    .await;
    let (s, b) = send(
        &app,
        Method::DELETE,
        &format!("/auth/sessions/{intruder_sid}"),
        &phone.access_token,
        json!({}),
    )
    .await;
    assert_eq!(s, 204, "{b}");
    assert!(row(&pool, user, intruder_install).await.is_none());
    assert!(
        row(&pool, user, phone.device_id).await.is_some(),
        "the phone keeps its row"
    );
}

/// No end reason keeps a session's rows: every reason drops them in the
/// transaction that ends the session (a chat-only session's rows here).
#[sqlx::test]
async fn every_end_reason_drops_a_sessions_rows(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let all = [
        EndReason::Logout,
        EndReason::Superseded,
        EndReason::Limit,
        EndReason::AbsoluteCap,
        EndReason::Revoked,
        EndReason::OthersRevoked,
        EndReason::PasswordChanged,
        EndReason::Ban,
        EndReason::AccountDeleted,
        EndReason::RefreshReuse,
        EndReason::AuthenticatorRevoked,
        EndReason::Repudiated,
        EndReason::RequestCompleted,
        EndReason::Upgrade,
        EndReason::Cli,
    ];
    for reason in all {
        let (sid, token) = create_probation_session_with_token(&pool, &app.config, user).await;
        let device = Uuid::new_v4();
        let (s, b) = register(&app, &token, reason.as_str(), Some(device)).await;
        assert_eq!(s, 201, "{b}");
        let (s, b) = register(&app, &token, &format!("{}-legacy", reason.as_str()), None).await;
        assert_eq!(s, 201, "{b}");
        end_session(&app, user, sid, reason).await;
        assert_eq!(rows_of_session(&pool, sid).await, 0, "{}", reason.as_str());
    }
}

#[sqlx::test]
async fn superseded_session_rows_follow_device_login(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let phone = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (s, b) = register(&app, &phone.access_token, "phone", Some(phone.device_id)).await;
    assert_eq!(s, 201, "{b}");

    // The app signs in again with its key; it does not register again while
    // its local record is fresh.
    let (new_sid, _) = device_sign_in(&app, &phone).await;
    let reason: Option<String> =
        sqlx::query_scalar("SELECT end_reason FROM sessions WHERE id = $1")
            .bind(phone.session_id.unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(reason.as_deref(), Some("superseded"));
    assert_eq!(
        row(&pool, user, phone.device_id).await,
        Some(("phone".into(), Some(new_sid))),
        "the row stays and belongs to the new session"
    );

    // So a chat-only session still cannot take it.
    let (_, chat_only) = create_probation_session_with_token(&pool, &app.config, user).await;
    let (s, b) = register(&app, &chat_only, "elsewhere", Some(phone.device_id)).await;
    assert_full_required(s, &b);
}

/// A row of the phone's install that another session owns (one written
/// before the install became a key device) is not taken over by the
/// device-key sign-in; the phone's own full registration replaces it.
#[sqlx::test]
async fn device_login_does_not_adopt_another_sessions_row(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let phone = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (other_sid, _) = create_probation_session_with_token(&pool, &app.config, user).await;
    insert_row(&pool, user, "earlier", phone.device_id, Some(other_sid)).await;

    let (new_sid, token) = device_sign_in(&app, &phone).await;
    assert_eq!(
        row(&pool, user, phone.device_id).await,
        Some(("earlier".into(), Some(other_sid))),
        "the row stays with the session that registered it"
    );
    let (s, b) = register(&app, &token, "phone", Some(phone.device_id)).await;
    assert_eq!(s, 201, "{b}");
    assert_eq!(
        row(&pool, user, phone.device_id).await,
        Some(("phone".into(), Some(new_sid)))
    );
}

/// A key phone keeps its subscription over a sign-out (it belongs to the key
/// device) and its next device-key sign-in takes the row over. A row its
/// session registered under another install id goes with the session.
#[sqlx::test]
async fn key_phone_row_survives_sign_out_and_is_adopted(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let phone = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (s, b) = register(&app, &phone.access_token, "phone", Some(phone.device_id)).await;
    assert_eq!(s, 201, "{b}");
    let elsewhere = Uuid::new_v4();
    let (s, b) = register(&app, &phone.access_token, "elsewhere", Some(elsewhere)).await;
    assert_eq!(s, 201, "{b}");

    let (s, b) = send(
        &app,
        Method::POST,
        "/auth/logout",
        &phone.access_token,
        json!({}),
    )
    .await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(
        row(&pool, user, phone.device_id).await,
        Some(("phone".into(), phone.session_id)),
        "the key phone's own row stays"
    );
    assert!(
        row(&pool, user, elsewhere).await.is_none(),
        "a row of another install goes"
    );

    let (new_sid, _) = device_sign_in(&app, &phone).await;
    assert_eq!(
        row(&pool, user, phone.device_id).await,
        Some(("phone".into(), Some(new_sid)))
    );
}

/// Any other session's rows are gone after its sign-out, also when the app
/// did not delete them itself.
#[sqlx::test]
async fn sign_out_removes_the_sessions_rows(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;

    // A chat-only app that deletes its own row first.
    let (_, tablet) = create_probation_session_with_token(&pool, &app.config, user).await;
    let install = Uuid::new_v4();
    register(&app, &tablet, "tablet", Some(install)).await;
    assert_eq!(delete_device(&app, &tablet, install).await, 204);
    let (s, b) = send(&app, Method::POST, "/auth/logout", &tablet, json!({})).await;
    assert_eq!(s, 200, "{b}");
    assert!(row(&pool, user, install).await.is_none());

    // A chat-only session that signs out without deleting anything.
    let (sid, chat_only) = create_probation_session_with_token(&pool, &app.config, user).await;
    let other = Uuid::new_v4();
    let (s, b) = register(&app, &chat_only, "chat-only", Some(other)).await;
    assert_eq!(s, 201, "{b}");
    let (s, b) = register(&app, &chat_only, "legacy", None).await;
    assert_eq!(s, 201, "{b}");
    let (s, b) = send(&app, Method::POST, "/auth/logout", &chat_only, json!({})).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(rows_of_session(&pool, sid).await, 0, "nothing stays behind");

    // A full session that is not a key phone (a passkey sign-in, say).
    let (full_sid, full) = create_session_with_token(&pool, &app.config, user).await;
    let pc = Uuid::new_v4();
    let (s, b) = register(&app, &full, "pc", Some(pc)).await;
    assert_eq!(s, 201, "{b}");
    let (s, b) = send(&app, Method::POST, "/auth/logout", &full, json!({})).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(rows_of_session(&pool, full_sid).await, 0);
}

/// A key device revoked for cause ("that was not me", an account request)
/// loses its install's rows in the revoking transaction; the paths that send
/// the removed phone its last notice delete them afterwards.
#[sqlx::test]
async fn repudiated_device_loses_rows_in_the_transaction(pool: PgPool) {
    use vc_server::approvals::devices::{revoke_tx, RevokeReason};
    use vc_server::security_events::Actor;

    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let ctx = AuthCtx::from(&app.state);
    for (reason, dropped) in [
        (RevokeReason::Repudiated, true),
        (RevokeReason::Request, true),
        (RevokeReason::Owner, false),
        (RevokeReason::Admin, false),
        (RevokeReason::Cli, false),
        (RevokeReason::KeyInvalidated, false),
    ] {
        let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
        let (s, b) = register(&app, &dev.access_token, "dev", Some(dev.device_id)).await;
        assert_eq!(s, 201, "{b}");
        let mut tx = pool.begin().await.unwrap();
        revoke_tx(&mut tx, &ctx, dev.row_id, reason, Actor::User(user))
            .await
            .unwrap();
        let left: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM push_subscriptions WHERE user_id = $1 AND device_id = $2",
        )
        .bind(user)
        .bind(dev.device_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(left == 0, dropped, "{}", reason.as_str());
    }
}

// ============================================================================
// Two requests at once
// ============================================================================

#[sqlx::test]
async fn chat_only_upsert_racing_full_upsert(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (full_sid, full) = create_session_with_token(&pool, &app.config, user).await;
    let (_, chat_only) = create_probation_session_with_token(&pool, &app.config, user).await;
    for round in 0..20 {
        let device = Uuid::new_v4();
        let (a, b) = tokio::join!(
            register(&app, &full, "full", Some(device)),
            register(&app, &chat_only, "chat", Some(device)),
        );
        assert_eq!(a.0, 201, "round {round}: {}", a.1);
        assert!(b.0 == 201 || b.0 == 403, "round {round}: {}", b.1);
        // Whatever the order, the full session's registration stands.
        assert_eq!(
            row(&pool, user, device).await,
            Some(("full".into(), Some(full_sid))),
            "round {round}"
        );
    }
}

#[sqlx::test]
async fn session_ended_while_registering_leaves_no_row(pool: PgPool) {
    let app = std::sync::Arc::new(TestApp::with_pool(pool.clone()).await);
    let (user, _) = create_test_user(&pool).await;
    let (sid, token) = create_probation_session_with_token(&pool, &app.config, user).await;
    let device = Uuid::new_v4();

    // The session row is being ended (updated, not committed yet; the
    // denylist is not written here, so the request passes the session check
    // on the committed row, as one that arrived a moment earlier would) ...
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("UPDATE sessions SET ended_at = NOW(), end_reason = 'revoked' WHERE id = $1")
        .bind(sid)
        .execute(&mut *tx)
        .await
        .unwrap();
    // ... while its registration arrives.
    let registration = {
        let app = std::sync::Arc::clone(&app);
        let token = token.clone();
        tokio::spawn(async move { register(&app, &token, "late", Some(device)).await })
    };
    wait_for_lock_waiters(&pool, 1).await;
    tx.commit().await.unwrap();
    let (s, b) = registration.await.unwrap();
    assert_eq!(
        (s, b["error"].as_str()),
        (401, Some("INVALID_TOKEN")),
        "{b}"
    );
    assert!(row(&pool, user, device).await.is_none());

    // The other order: the registration commits first, then the end removes it.
    for round in 0..10 {
        let (sid, token) = create_probation_session_with_token(&pool, &app.config, user).await;
        let device = Uuid::new_v4();
        let ender = {
            let app = std::sync::Arc::clone(&app);
            tokio::spawn(async move { end_session(&app, user, sid, EndReason::Revoked).await })
        };
        let _ = register(&app, &token, "racing", Some(device)).await;
        ender.await.unwrap();
        assert!(
            row(&pool, user, device).await.is_none(),
            "round {round}: a revoked session left a row"
        );
    }
}

// ============================================================================
// A key phone's row belongs to its key device
// ============================================================================

/// The phone's install id as a chat-only session reads it from the device list.
async fn install_from_device_list(app: &TestApp, token: &str, phone: &KeyDeviceFixture) -> Uuid {
    let (s, b) = send(app, Method::GET, "/api/approval-devices", token, json!({})).await;
    assert_eq!(s, 200, "{b}");
    let list = b
        .as_array()
        .cloned()
        .unwrap_or_else(|| b["devices"].as_array().cloned().unwrap_or_default());
    let entry = list
        .iter()
        .find(|d| d["id"] == phone.row_id.to_string())
        .unwrap_or_else(|| panic!("phone in the list: {b}"));
    entry["device_id"].as_str().unwrap().parse().unwrap()
}

/// Every way a chat-only session could reach the phone's row: refused or
/// skipped, the row unchanged.
async fn chat_only_cannot_reach(
    app: &TestApp,
    pool: &PgPool,
    user: Uuid,
    phone: &KeyDeviceFixture,
    state: &str,
) {
    let before = row(pool, user, phone.device_id).await;
    assert!(before.is_some(), "{state}: the phone has a row");
    let (_, chat_only) = create_probation_session_with_token(pool, &app.config, user).await;
    let install = install_from_device_list(app, &chat_only, phone).await;
    assert_eq!(install, phone.device_id);

    let (s, b) = register(app, &chat_only, "hijack", Some(install)).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (403, Some("FULL_SESSION_REQUIRED")),
        "{state}: {b}"
    );
    let endpoint = before.clone().unwrap().0;
    let (s, b) = register(app, &chat_only, &endpoint, Some(Uuid::new_v4())).await;
    assert_eq!(s, 403, "{state}, same endpoint: {b}");
    let (s, b) = register(app, &chat_only, &endpoint, None).await;
    assert_eq!(s, 403, "{state}, same endpoint without install id: {b}");
    assert_eq!(
        delete_device(app, &chat_only, install).await,
        204,
        "{state}"
    );
    assert_eq!(
        row(pool, user, phone.device_id).await,
        before,
        "{state}: row unchanged"
    );
}

#[sqlx::test]
async fn chat_only_cannot_take_a_key_phones_row_in_any_state(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    for state in [
        "signed in",
        "signed out",
        "lapsed",
        "lifetime cap",
        "session limit",
        "after the upgrade",
    ] {
        let (user, _) = create_test_user(&pool).await;
        let phone = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
        let sid = phone.session_id.unwrap();
        if state == "after the upgrade" {
            insert_row(&pool, user, "phone", phone.device_id, None).await;
        } else {
            let (s, b) = register(&app, &phone.access_token, "phone", Some(phone.device_id)).await;
            assert_eq!(s, 201, "{b}");
        }
        match state {
            "signed out" => {
                let (s, b) = send(
                    &app,
                    Method::POST,
                    "/auth/logout",
                    &phone.access_token,
                    json!({}),
                )
                .await;
                assert_eq!(s, 200, "{b}");
            }
            "lapsed" => {
                sqlx::query(
                    "UPDATE sessions SET created_at = NOW() - INTERVAL '9 days',
                            auth_time = NOW() - INTERVAL '9 days',
                            expires_at = NOW() - INTERVAL '1 day' WHERE id = $1",
                )
                .bind(sid)
                .execute(&pool)
                .await
                .unwrap();
            }
            "lifetime cap" => end_session(&app, user, sid, EndReason::AbsoluteCap).await,
            "session limit" => end_session(&app, user, sid, EndReason::Limit).await,
            _ => {}
        }
        chat_only_cannot_reach(&app, &pool, user, &phone, state).await;
    }
}

#[sqlx::test]
async fn chat_only_cannot_pre_register_a_key_phones_install(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let phone = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (_, chat_only) = create_probation_session_with_token(&pool, &app.config, user).await;
    let install = install_from_device_list(&app, &chat_only, &phone).await;
    let (s, b) = register(&app, &chat_only, "first", Some(install)).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (403, Some("FULL_SESSION_REQUIRED")),
        "{b}"
    );
    assert!(
        row(&pool, user, install).await.is_none(),
        "nothing was stored"
    );
}

#[sqlx::test]
async fn chat_only_pruning_never_takes_a_key_phones_row(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let phone = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    register(&app, &phone.access_token, "phone", Some(phone.device_id)).await;
    // The phone signed out: its row's owner is no longer live, and it is the
    // least recently used row of the account.
    send(
        &app,
        Method::POST,
        "/auth/logout",
        &phone.access_token,
        json!({}),
    )
    .await;
    sqlx::query(
        "UPDATE push_subscriptions SET created_at = NOW() - INTERVAL '3 days' WHERE device_id = $1",
    )
    .bind(phone.device_id)
    .execute(&pool)
    .await
    .unwrap();
    for i in 0..9 {
        insert_row(&pool, user, &format!("old{i}"), Uuid::new_v4(), None).await;
    }
    let (_, chat_only) = create_probation_session_with_token(&pool, &app.config, user).await;
    for i in 0..3 {
        let (s, b) = register(&app, &chat_only, &format!("c{i}"), Some(Uuid::new_v4())).await;
        assert_eq!(s, 201, "{b}");
        assert!(
            row(&pool, user, phone.device_id).await.is_some(),
            "round {i}"
        );
    }
    assert_eq!(rows_of_user(&pool, user).await, 10);
}

#[sqlx::test]
async fn pending_key_device_install_is_protected_revoked_is_not(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let pending = KeyDeviceFixture::insert(&pool, &app.config, user, "pending").await;
    let revoked = KeyDeviceFixture::insert(&pool, &app.config, user, "revoked").await;
    let (_, chat_only) = create_probation_session_with_token(&pool, &app.config, user).await;
    let (s, b) = register(&app, &chat_only, "pending", Some(pending.device_id)).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (403, Some("FULL_SESSION_REQUIRED")),
        "{b}"
    );
    let (s, b) = register(&app, &chat_only, "revoked", Some(revoked.device_id)).await;
    assert_eq!(s, 201, "{b}");
}

async fn fcm_tokens_pushed(mock: &super::helpers::FcmMock) -> Vec<String> {
    mock.received
        .lock()
        .unwrap()
        .iter()
        .filter_map(|m| m["message"]["token"].as_str().map(str::to_string))
        .collect()
}

/// The key phone's row is no session's to lose: ending its session from a
/// PC ("end all other sessions", "end this session"), a password change or a
/// sign-out leaves it, and the phone still gets a wake afterwards.
#[sqlx::test]
async fn key_phone_row_survives_every_session_end(pool: PgPool) {
    use super::helpers::{spawn_fcm_mock, test_service_account, FcmMockResponse};
    let mock = spawn_fcm_mock(FcmMockResponse::Ok).await;
    let mut config = vc_server::config::Config::default_for_test();
    config.fcm_service_account = Some(test_service_account(mock.token_uri()));
    config.fcm_api_base.clone_from(&mock.base_url);
    let app = TestApp::with_pool_and_config(pool.clone(), config).await;
    let (user, _) = create_test_user(&pool).await;
    let phone = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let pc = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let token = "fcm-key-phone-token:abcdefghijklmnop_0123";
    let (s, b) = send(
        &app,
        Method::POST,
        "/api/me/push-subscriptions",
        &phone.access_token,
        json!({"provider": "fcm", "endpoint": token, "device_id": phone.device_id}),
    )
    .await;
    assert_eq!(s, 201, "{b}");
    let stored = || async {
        sqlx::query_scalar::<_, String>(
            "SELECT endpoint FROM push_subscriptions WHERE user_id = $1 AND device_id = $2",
        )
        .bind(user)
        .bind(phone.device_id)
        .fetch_optional(&pool)
        .await
        .unwrap()
    };

    // "End all other sessions" from the PC, then the phone signs in again.
    device_grant(&app, &pc, StepUpAction::SessionsEnd, "others").await;
    let (s, b) = send(
        &app,
        Method::DELETE,
        "/auth/sessions",
        &pc.access_token,
        json!({}),
    )
    .await;
    assert_eq!(s, 200, "{b}");
    assert!(b["revoked_count"].as_u64() >= Some(1), "{b}");
    assert_eq!(
        stored().await.as_deref(),
        Some(token),
        "after end-all-others"
    );
    let (phone_sid, phone_token) = device_sign_in(&app, &phone).await;
    assert_eq!(
        stored().await.as_deref(),
        Some(token),
        "after the device-key sign-in"
    );

    // "End this session" for the phone's session.
    device_grant(&app, &pc, StepUpAction::SessionsEnd, &phone_sid.to_string()).await;
    let (s, b) = send(
        &app,
        Method::DELETE,
        &format!("/auth/sessions/{phone_sid}"),
        &pc.access_token,
        json!({}),
    )
    .await;
    assert_eq!(s, 204, "{b}");
    assert_eq!(
        stored().await.as_deref(),
        Some(token),
        "after end-this-session"
    );
    drop(phone_token);

    // A password change ending the session, the remaining reasons, a sign-out.
    let (sid, _) = device_sign_in(&app, &phone).await;
    end_session(&app, user, sid, EndReason::PasswordChanged).await;
    let (sid, _) = device_sign_in(&app, &phone).await;
    end_session(&app, user, sid, EndReason::RefreshReuse).await;
    let (_, t) = device_sign_in(&app, &phone).await;
    let (s, b) = send(&app, Method::POST, "/auth/logout", &t, json!({})).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(stored().await.as_deref(), Some(token), "after every end");

    vc_server::push::deliver(
        &app.state,
        user,
        &vc_server::push::WakeSignal::dm(Uuid::now_v7(), 1),
    )
    .await
    .unwrap();
    assert!(
        fcm_tokens_pushed(&mock).await.iter().any(|t| t == token),
        "the phone still gets a wake"
    );
}

// ============================================================================
// Registrations nobody else can reach any more
// ============================================================================

async fn lapse_long_ago(pool: &PgPool, sid: Uuid) {
    sqlx::query(
        "UPDATE sessions SET created_at = NOW() - INTERVAL '60 days',
                auth_time = NOW() - INTERVAL '60 days',
                expires_at = NOW() - INTERVAL '40 days',
                absolute_expires_at = NOW() - INTERVAL '40 days' WHERE id = $1",
    )
    .bind(sid)
    .execute(pool)
    .await
    .unwrap();
}

/// "End all other sessions" removes every other registration of the account,
/// whoever owns it: a lapsed sign-in's, one without owner from before the
/// upgrade, one of a session already cleaned up. A key phone's registration
/// and the acting session's own stay.
#[sqlx::test]
async fn end_all_others_removes_every_other_registration(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let phone = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let pc = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (s, b) = register(&app, &phone.access_token, "phone", Some(phone.device_id)).await;
    assert_eq!(s, 201, "{b}");
    let acting = Uuid::new_v4();
    let (s, b) = register(&app, &pc.access_token, "acting", Some(acting)).await;
    assert_eq!(s, 201, "{b}");

    // A chat-only sign-in registers, then lapses without an end.
    let (lapsed_sid, chat_only) =
        create_probation_session_with_token(&pool, &app.config, user).await;
    let lapsed = Uuid::new_v4();
    let (s, b) = register(&app, &chat_only, "lapsed", Some(lapsed)).await;
    assert_eq!(s, 201, "{b}");
    sqlx::query(
        "UPDATE sessions SET created_at = NOW() - INTERVAL '9 days',
                auth_time = NOW() - INTERVAL '9 days',
                expires_at = NOW() - INTERVAL '1 day' WHERE id = $1",
    )
    .bind(lapsed_sid)
    .execute(&pool)
    .await
    .unwrap();
    // One without owner (from before the upgrade), one of a session that is
    // already cleaned up.
    let ownerless = Uuid::new_v4();
    insert_row(&pool, user, "ownerless", ownerless, None).await;
    let cleaned = Uuid::new_v4();
    insert_row(&pool, user, "cleaned", cleaned, Some(Uuid::new_v4())).await;

    device_grant(&app, &pc, StepUpAction::SessionsEnd, "others").await;
    let (s, b) = send(
        &app,
        Method::DELETE,
        "/auth/sessions",
        &pc.access_token,
        json!({}),
    )
    .await;
    assert_eq!(s, 200, "{b}");

    for (device, what) in [
        (lapsed, "lapsed"),
        (ownerless, "ownerless"),
        (cleaned, "cleaned up"),
    ] {
        assert!(row(&pool, user, device).await.is_none(), "{what} row gone");
    }
    assert!(
        row(&pool, user, phone.device_id).await.is_some(),
        "key phone row stays"
    );
    assert_eq!(
        row(&pool, user, acting).await,
        Some(("acting".into(), pc.session_id)),
        "the acting session's own row stays"
    );
}

/// The routine that removes expired session rows takes their registrations
/// with them, never a key phone's.
#[sqlx::test]
async fn cleanup_removes_a_lapsed_sessions_registrations(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let phone = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (s, b) = register(&app, &phone.access_token, "phone", Some(phone.device_id)).await;
    assert_eq!(s, 201, "{b}");
    let (lapsed_sid, chat_only) =
        create_probation_session_with_token(&pool, &app.config, user).await;
    let lapsed = Uuid::new_v4();
    let (s, b) = register(&app, &chat_only, "lapsed", Some(lapsed)).await;
    assert_eq!(s, 201, "{b}");
    let (live_sid, live) = create_probation_session_with_token(&pool, &app.config, user).await;
    let other = Uuid::new_v4();
    let (s, b) = register(&app, &live, "live", Some(other)).await;
    assert_eq!(s, 201, "{b}");

    // Both the chat-only sign-in and the key phone's session lapsed long ago.
    lapse_long_ago(&pool, lapsed_sid).await;
    lapse_long_ago(&pool, phone.session_id.unwrap()).await;
    let removed = vc_server::db::cleanup_expired_sessions(&pool)
        .await
        .unwrap();
    assert!(removed >= 2, "{removed}");

    assert!(
        row(&pool, user, lapsed).await.is_none(),
        "the lapsed sign-in's row went"
    );
    assert!(
        row(&pool, user, phone.device_id).await.is_some(),
        "a key phone's row stays"
    );
    assert_eq!(
        row(&pool, user, other).await,
        Some(("live".into(), Some(live_sid))),
        "a live sign-in's row stays"
    );
}

/// A pending key device's install is protected on every path an active one
/// is: register by install id or by endpoint, delete by install, and the
/// per-account pruning. (A waiting phone normally has no session; its row is
/// staged here without owner.)
#[sqlx::test]
async fn pending_key_device_install_is_protected_on_every_path(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let pending = KeyDeviceFixture::insert(&pool, &app.config, user, "pending").await;
    insert_row(&pool, user, "waiting-phone", pending.device_id, None).await;
    sqlx::query(
        "UPDATE push_subscriptions SET created_at = NOW() - INTERVAL '3 days' WHERE device_id = $1",
    )
    .bind(pending.device_id)
    .execute(&pool)
    .await
    .unwrap();
    let before = row(&pool, user, pending.device_id).await;

    let (_, chat_only) = create_probation_session_with_token(&pool, &app.config, user).await;
    let (s, b) = register(&app, &chat_only, "hijack", Some(pending.device_id)).await;
    assert_eq!(s, 403, "by install id: {b}");
    let (s, b) = register(&app, &chat_only, "waiting-phone", Some(Uuid::new_v4())).await;
    assert_eq!(s, 403, "by endpoint: {b}");
    let (s, b) = register(&app, &chat_only, "waiting-phone", None).await;
    assert_eq!(s, 403, "by endpoint without install id: {b}");
    assert_eq!(
        delete_device(&app, &chat_only, pending.device_id).await,
        204
    );
    assert_eq!(row(&pool, user, pending.device_id).await, before);

    // Fill the account up: the waiting phone's row is the oldest.
    for i in 0..9 {
        insert_row(&pool, user, &format!("old{i}"), Uuid::new_v4(), None).await;
    }
    for i in 0..2 {
        let (s, b) = register(&app, &chat_only, &format!("c{i}"), Some(Uuid::new_v4())).await;
        assert_eq!(s, 201, "{b}");
        assert_eq!(
            row(&pool, user, pending.device_id).await,
            before,
            "pruning, round {i}"
        );
    }
}

/// The two-rows-per-session pruning of a chat-only caller never removes a key
/// phone's row, also one that session itself registered before the install
/// became a key device (active or pending).
#[sqlx::test]
async fn chat_only_session_pruning_never_takes_a_key_phones_row(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    for state in ["active", "pending"] {
        let (user, _) = create_test_user(&pool).await;
        let (sid, chat_only) = create_probation_session_with_token(&pool, &app.config, user).await;
        let phone = KeyDeviceFixture::insert(&pool, &app.config, user, state).await;
        insert_row(&pool, user, "phone", phone.device_id, Some(sid)).await;
        sqlx::query(
            "UPDATE push_subscriptions SET created_at = NOW() - INTERVAL '3 days'
              WHERE device_id = $1",
        )
        .bind(phone.device_id)
        .execute(&pool)
        .await
        .unwrap();
        for i in 0..3 {
            let (s, b) = register(&app, &chat_only, &format!("c{i}"), Some(Uuid::new_v4())).await;
            assert_eq!(s, 201, "{state}: {b}");
        }
        assert_eq!(
            row(&pool, user, phone.device_id).await,
            Some(("phone".into(), Some(sid))),
            "{state}: the key phone's row stays"
        );
    }
}
