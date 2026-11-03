//! List, rename and remove passkeys (LP Task 8; C29, SM4, B.7).

use axum::body::Body;
use axum::http::Method;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::approvals::secrets::b64u_encode;
use vc_server::auth::passkeys::queries::{self, PasskeyState};
use vc_server::auth::step_up::{StepUpAction, StepUpTarget};

use crate::helpers::approvals::KeyDeviceFixture;
use crate::helpers::login::{peer_request, TEST_ORIGIN};
use crate::helpers::passkeys::{passkey_session, seed_passkey};
use crate::helpers::{body_to_json, create_session_with_token, create_test_user, TestApp};
use crate::passkeys_login::sign_in_with;
use crate::passkeys_step_up::{auth_user, step_up_with};

async fn send(
    app: &TestApp,
    method: Method,
    path: &str,
    token: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let req = peer_request(method, path)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json");
    let req = match body {
        Some(b) => req.body(Body::from(b.to_string())),
        None => req.body(Body::empty()),
    }
    .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    let bytes = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .unwrap()
        .to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn delete(app: &TestApp, token: &str, id: Uuid) -> (u16, Value) {
    send(
        app,
        Method::DELETE,
        &format!("/auth/passkeys/{id}"),
        token,
        None,
    )
    .await
}

#[sqlx::test]
async fn list_shows_own_live_passkeys_only(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (other, _) = create_test_user(&app.pool).await;
    let (_, active) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, pending) = seed_passkey(&app, user, PasskeyState::Pending).await;
    seed_passkey(&app, user, PasskeyState::Revoked).await;
    seed_passkey(&app, other, PasskeyState::Active).await;
    let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
    let (s, b) = send(&app, Method::GET, "/auth/passkeys", &token, None).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(b["rp_id"], "chat.example.com");
    let handle = queries::user_handle(&app.pool, user).await.unwrap();
    assert_eq!(b["user_handle"], b64u_encode(handle.as_bytes()));
    let list = b["passkeys"].as_array().unwrap();
    let ids: Vec<&str> = list.iter().map(|p| p["id"].as_str().unwrap()).collect();
    assert_eq!(ids.len(), 2, "{b}");
    assert!(ids.contains(&active.id.to_string().as_str()));
    assert!(ids.contains(&pending.id.to_string().as_str()));
    let p = list.iter().find(|p| p["state"] == "pending").unwrap();
    assert!(p["ready_at"].is_string());
    for p in list {
        assert!(
            p.get("credential").is_none() && p.get("public_key").is_none(),
            "{p}"
        );
        for key in [
            "label",
            "created_at",
            "backup_eligible",
            "backup_state",
            "transports",
        ] {
            assert!(p.get(key).is_some(), "{key}: {p}");
        }
    }
    // Any session may list (probation too).
    let (_, probation) =
        crate::helpers::create_probation_session_with_token(&app.pool, &app.config, user).await;
    assert_eq!(
        send(&app, Method::GET, "/auth/passkeys", &probation, None)
            .await
            .0,
        200
    );
}

#[sqlx::test]
async fn rename_changes_label(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (_, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
    let path = format!("/auth/passkeys/{}", row.id);
    let (s, b) = send(
        &app,
        Method::PATCH,
        &path,
        &token,
        Some(json!({"label": "  Work laptop "})),
    )
    .await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(b["label"], "Work laptop");
    let (s, b) = send(
        &app,
        Method::PATCH,
        &path,
        &token,
        Some(json!({"label": ""})),
    )
    .await;
    assert_eq!((s, b["error"].as_str()), (400, Some("VALIDATION_ERROR")));
    let audited: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM system_audit_log WHERE action = 'auth.passkey.renamed' AND target_id = $1",
    )
    .bind(row.id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(audited, 1);
}

#[sqlx::test]
async fn rename_other_users_passkey_is_404(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (other, _) = create_test_user(&app.pool).await;
    let (_, theirs) = seed_passkey(&app, other, PasskeyState::Active).await;
    let (_, revoked) = seed_passkey(&app, user, PasskeyState::Revoked).await;
    let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
    for id in [theirs.id, revoked.id] {
        let (s, _) = send(
            &app,
            Method::PATCH,
            &format!("/auth/passkeys/{id}"),
            &token,
            Some(json!({"label": "x"})),
        )
        .await;
        assert_eq!(s, 404);
    }
}

#[sqlx::test]
async fn remove_needs_step_up_for_that_passkey(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, a) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, b) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
    let (s, r) = delete(&app, &token, b.id).await;
    assert_eq!((s, r["error"].as_str()), (403, Some("STEP_UP_REQUIRED")));
    // A grant for another passkey does not count.
    step_up_with(
        &app,
        &token,
        &mut auth,
        &a,
        user,
        "passkey.remove",
        &a.id.to_string(),
    )
    .await;
    let (s, _) = delete(&app, &token, b.id).await;
    assert_eq!(s, 403);
    step_up_with(
        &app,
        &token,
        &mut auth,
        &a,
        user,
        "passkey.remove",
        &b.id.to_string(),
    )
    .await;
    let (s, r) = delete(&app, &token, b.id).await;
    assert_eq!(s, 200, "{r}");
    let row = queries::find_own(&app.pool, user, b.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.state, PasskeyState::Revoked);
    let ev: Vec<(String, Value)> =
        sqlx::query_as("SELECT event_type, details FROM security_events WHERE subject_id = $1")
            .bind(user)
            .fetch_all(&app.pool)
            .await
            .unwrap();
    assert!(
        ev.iter().any(|(t, d)| t == "authenticator_removed"
            && d["type"] == "passkey"
            && d["by"] == "self"),
        "{ev:?}"
    );
    assert!(
        ev.iter()
            .any(|(t, d)| t == "step_up_action" && d["action"] == "passkey.remove"),
        "{ev:?}"
    );
}

#[sqlx::test]
async fn last_passkey_cannot_be_removed(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (sid, token) = create_session_with_token(&app.pool, &app.config, user).await;
    step_up_with(
        &app,
        &token,
        &mut auth,
        &row,
        user,
        "passkey.remove",
        &row.id.to_string(),
    )
    .await;
    let (s, r) = delete(&app, &token, row.id).await;
    assert_eq!(
        (s, r["error"].as_str()),
        (409, Some("LAST_STRONG_AUTHENTICATOR")),
        "{r}"
    );
    // The grant is still usable.
    let mut tx = app.pool.begin().await.unwrap();
    vc_server::auth::step_up::consume_tx(
        &mut tx,
        &auth_user(user, sid),
        StepUpAction::PasskeyRemove,
        &StepUpTarget::Id(row.id),
    )
    .await
    .unwrap();
}

#[sqlx::test]
async fn concurrent_removals_keep_one_passkey(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth_a, a) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, b) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
    step_up_with(
        &app,
        &token,
        &mut auth_a,
        &a,
        user,
        "passkey.remove",
        &a.id.to_string(),
    )
    .await;
    step_up_with(
        &app,
        &token,
        &mut auth_a,
        &a,
        user,
        "passkey.remove",
        &b.id.to_string(),
    )
    .await;
    let ((sa, _), (sb, _)) = tokio::join!(delete(&app, &token, a.id), delete(&app, &token, b.id));
    let mut s = vec![sa, sb];
    s.sort_unstable();
    assert_eq!(s, vec![200, 409]);
    assert_eq!(queries::active_count(&app.pool, user).await.unwrap(), 1);
}

#[sqlx::test]
async fn key_device_removal_counts_passkeys(pool: PgPool) {
    use crate::approvals_core::{answer, stepup_challenge};
    let app = TestApp::with_passkeys(pool).await;
    for passkey_first in [true, false] {
        let (user, _) = create_test_user(&app.pool).await;
        let (mut auth, row) = seed_passkey(&app, user, PasskeyState::Active).await;
        let dev = KeyDeviceFixture::insert(&app.pool, &app.config, user, "active").await;
        let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
        let device_path = format!("/api/approval-devices/{}", dev.row_id);
        if passkey_first {
            step_up_with(
                &app,
                &token,
                &mut auth,
                &row,
                user,
                "passkey.remove",
                &row.id.to_string(),
            )
            .await;
            let (s, r) = delete(&app, &token, row.id).await;
            assert_eq!(s, 200, "{r}");
            // The key device is now the last strong authenticator (C26 route).
            let c =
                stepup_challenge(&app, &dev, "key_device.remove", &dev.row_id.to_string()).await;
            assert_eq!(answer(&app, &dev, &c, &[]).await.0, 200);
            let (s, r) = send(&app, Method::DELETE, &device_path, &dev.access_token, None).await;
            assert_eq!(
                (s, r["error"].as_str()),
                (409, Some("LAST_STRONG_AUTHENTICATOR")),
                "{r}"
            );
        } else {
            step_up_with(
                &app,
                &token,
                &mut auth,
                &row,
                user,
                "key_device.remove",
                &dev.row_id.to_string(),
            )
            .await;
            step_up_with(
                &app,
                &token,
                &mut auth,
                &row,
                user,
                "passkey.remove",
                &row.id.to_string(),
            )
            .await;
            let (s, r) = send(&app, Method::DELETE, &device_path, &token, None).await;
            assert_eq!(s, 200, "{r}");
            let (s, r) = delete(&app, &token, row.id).await;
            assert_eq!(
                (s, r["error"].as_str()),
                (409, Some("LAST_STRONG_AUTHENTICATOR")),
                "{r}"
            );
        }
        let mut conn = app.pool.acquire().await.unwrap();
        assert_eq!(
            vc_server::auth::strong::active_strong_count(&mut conn, user)
                .await
                .unwrap(),
            1
        );
    }
}

#[sqlx::test]
async fn removing_a_passkey_ends_its_sessions(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth_a, a) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, b) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, b_token) = passkey_session(&app.pool, &app.config, user, b.id).await;
    let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
    step_up_with(
        &app,
        &token,
        &mut auth_a,
        &a,
        user,
        "passkey.remove",
        &b.id.to_string(),
    )
    .await;
    let (s, r) = delete(&app, &token, b.id).await;
    assert_eq!(s, 200, "{r}");
    let (s, _) = send(&app, Method::GET, "/auth/passkeys", &b_token, None).await;
    assert_eq!(s, 401, "the passkey's session ended");
    let (s, _) = send(&app, Method::GET, "/auth/passkeys", &token, None).await;
    assert_eq!(s, 200, "another session stays");
}

#[sqlx::test]
async fn removal_response_carries_signal_data(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth_a, a) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, b) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
    step_up_with(
        &app,
        &token,
        &mut auth_a,
        &a,
        user,
        "passkey.remove",
        &b.id.to_string(),
    )
    .await;
    let (s, r) = delete(&app, &token, b.id).await;
    assert_eq!(s, 200, "{r}");
    assert_eq!(r["rp_id"], "chat.example.com");
    assert_eq!(r["credential_id"], b64u_encode(&b.credential_id));
    assert_eq!(
        r["remaining_credential_ids"],
        json!([b64u_encode(&a.credential_id)])
    );
    let handle = queries::user_handle(&app.pool, user).await.unwrap();
    assert_eq!(r["user_handle"], b64u_encode(handle.as_bytes()));
}

#[sqlx::test]
async fn sign_in_is_not_a_step_up(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth, a) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, b) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (s, signed, _) = sign_in_with(
        &app.router,
        &app.pool,
        &mut auth,
        &a.credential_id,
        user,
        TEST_ORIGIN,
        true,
        None,
        None,
    )
    .await;
    assert_eq!(s, 200);
    let token = signed["access_token"].as_str().unwrap();
    let (s, r) = delete(&app, token, b.id).await;
    assert_eq!((s, r["error"].as_str()), (403, Some("STEP_UP_REQUIRED")));
}

#[sqlx::test]
async fn removed_passkey_cannot_sign_in(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (mut auth_a, a) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (mut auth_b, b) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, token) = create_session_with_token(&app.pool, &app.config, user).await;
    step_up_with(
        &app,
        &token,
        &mut auth_a,
        &a,
        user,
        "passkey.remove",
        &b.id.to_string(),
    )
    .await;
    assert_eq!(delete(&app, &token, b.id).await.0, 200);
    let (s, r, _) = sign_in_with(
        &app.router,
        &app.pool,
        &mut auth_b,
        &b.credential_id,
        user,
        TEST_ORIGIN,
        true,
        None,
        None,
    )
    .await;
    assert_eq!((s, r["error"].as_str()), (400, Some("PASSKEY_UNKNOWN")));
}
