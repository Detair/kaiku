//! `passkeys` table, queries, L1a's strong-authenticator hooks and the
//! RP-ID pin (LP Task 4).

use sqlx::PgPool;
use uuid::Uuid;
use vc_server::auth::ctx::AuthCtx;
use vc_server::auth::passkeys::queries::{self, InsertError, NewPasskey, PasskeyState};
use vc_server::auth::passkeys::rp_pin::{check_rp_id, RpPinOutcome, RP_ID_SETTING};
use vc_server::auth::strong::{self, AuthenticatorRef};

use crate::helpers::passkeys::{passkey_session, seed_passkey};
use crate::helpers::{create_test_user, TestApp};

async fn app(pool: PgPool) -> TestApp {
    TestApp::with_passkeys(pool).await
}

#[sqlx::test]
async fn insert_and_find_by_credential_id(pool: PgPool) {
    let app = app(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (_, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let found = queries::find_by_credential_id(&app.pool, &row.credential_id)
        .await
        .unwrap()
        .expect("found");
    assert_eq!(found.id, row.id);
    assert_eq!(found.user_id, user);
    assert_eq!(found.state, PasskeyState::Active);
    assert_eq!(found.label, "Test passkey");
    assert!(found.ready_at.is_none());
    assert_eq!(found.id.get_version_num(), 7);
    assert!(queries::find_by_credential_id(&app.pool, b"nope")
        .await
        .unwrap()
        .is_none());
}

#[sqlx::test]
async fn duplicate_credential_is_reported(pool: PgPool) {
    let app = app(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (_, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let verified = vc_server::auth::passkeys::rp::VerifiedRegistration {
        passkey: row.credential.0.clone(),
        aaguid: None,
        backup_eligible: false,
        backup_state: false,
        transports: vec![],
    };
    let mut conn = app.pool.acquire().await.unwrap();
    let err = queries::insert(
        &mut conn,
        &NewPasskey {
            user_id: user,
            verified: &verified,
            label: "again",
            state: PasskeyState::Active,
            ready_at: None,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, InsertError::Duplicate), "{err:?}");
}

#[sqlx::test]
async fn revoke_only_active(pool: PgPool) {
    let app = app(pool).await;
    let ctx = AuthCtx::from(&app.state);
    let (user, _) = create_test_user(&app.pool).await;
    let (_, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let mut tx = app.pool.begin().await.unwrap();
    let revoked = queries::revoke(&mut tx, &ctx, user, row.id, "removed_by_owner")
        .await
        .unwrap();
    assert_eq!(revoked.map(|r| r.state), Some(PasskeyState::Revoked));
    assert!(queries::revoke(&mut tx, &ctx, user, row.id, "again")
        .await
        .unwrap()
        .is_none());
    tx.commit().await.unwrap();
    let (at, reason): (Option<chrono::DateTime<chrono::Utc>>, Option<String>) =
        sqlx::query_as("SELECT revoked_at, revoke_reason FROM passkeys WHERE id = $1")
            .bind(row.id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert!(at.is_some());
    assert_eq!(reason.as_deref(), Some("removed_by_owner"));
    // Another user's passkey is never revoked.
    let (other, _) = create_test_user(&app.pool).await;
    let (_, theirs) = seed_passkey(&app, other, PasskeyState::Active).await;
    let mut conn = app.pool.acquire().await.unwrap();
    assert!(queries::revoke(&mut conn, &ctx, user, theirs.id, "x")
        .await
        .unwrap()
        .is_none());
}

async fn raw_insert(pool: &PgPool, user: Uuid, state: &str, ready: bool) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO passkeys (id, user_id, credential_id, credential, label, backup_eligible,
                               backup_state, state, ready_at)
         VALUES ($1, $2, $3, '{}', 'x', false, false, $4,
                 CASE WHEN $5 THEN NOW() + INTERVAL '1 day' END)",
    )
    .bind(Uuid::new_v4())
    .bind(user)
    .bind(Uuid::new_v4().as_bytes().to_vec())
    .bind(state)
    .bind(ready)
    .execute(pool)
    .await
    .map(|_| ())
}

#[sqlx::test]
async fn state_check_rejects_unknown_values(pool: PgPool) {
    let app = app(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    for bad in ["pending_activation", "ACTIVE", ""] {
        assert!(
            raw_insert(&app.pool, user, bad, false).await.is_err(),
            "{bad}"
        );
    }
    raw_insert(&app.pool, user, "active", false).await.unwrap();
}

#[sqlx::test]
async fn pending_needs_ready_at(pool: PgPool) {
    let app = app(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    assert!(raw_insert(&app.pool, user, "pending", false).await.is_err());
    assert!(raw_insert(&app.pool, user, "active", true).await.is_err());
    raw_insert(&app.pool, user, "pending", true).await.unwrap();
}

#[sqlx::test]
async fn list_live_includes_pending(pool: PgPool) {
    let app = app(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (_, active) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, pending) = seed_passkey(&app, user, PasskeyState::Pending).await;
    let (_, _revoked) = seed_passkey(&app, user, PasskeyState::Revoked).await;
    let mut live: Vec<Uuid> = queries::list_live(&app.pool, user)
        .await
        .unwrap()
        .iter()
        .map(|r| r.id)
        .collect();
    live.sort();
    let mut want = vec![active.id, pending.id];
    want.sort();
    assert_eq!(live, want);
    let act: Vec<Uuid> = queries::list_active(&app.pool, user)
        .await
        .unwrap()
        .iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(act, vec![active.id]);
    assert_eq!(queries::active_count(&app.pool, user).await.unwrap(), 1);
    assert!(pending.ready_at.is_some());
}

#[sqlx::test]
async fn user_handle_is_stable(pool: PgPool) {
    let app = app(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let a = queries::user_handle(&app.pool, user).await.unwrap();
    let b = queries::user_handle(&app.pool, user).await.unwrap();
    assert_eq!(a, b);
    assert_ne!(a, user);
    assert_eq!(a.get_version_num(), 4);
    assert_eq!(
        queries::user_id_for_handle(&app.pool, a).await.unwrap(),
        Some(user)
    );
    assert_eq!(
        queries::user_id_for_handle(&app.pool, Uuid::new_v4())
            .await
            .unwrap(),
        None
    );
}

#[sqlx::test]
async fn user_delete_cascades(pool: PgPool) {
    let app = app(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (_, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    crate::helpers::delete_user(&app.pool, user).await;
    assert!(
        queries::find_by_credential_id(&app.pool, &row.credential_id)
            .await
            .unwrap()
            .is_none()
    );
}

#[sqlx::test]
async fn strong_authenticator_count_includes_passkeys(pool: PgPool) {
    let app = app(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let mut conn = app.pool.acquire().await.unwrap();
    assert_eq!(
        strong::active_strong_count(&mut conn, user).await.unwrap(),
        0
    );
    seed_passkey(&app, user, PasskeyState::Active).await;
    seed_passkey(&app, user, PasskeyState::Pending).await;
    seed_passkey(&app, user, PasskeyState::Revoked).await;
    assert_eq!(
        strong::active_strong_count(&mut conn, user).await.unwrap(),
        1
    );
    crate::helpers::approvals::KeyDeviceFixture::insert(&app.pool, &app.config, user, "active")
        .await;
    assert_eq!(
        strong::active_strong_count(&mut conn, user).await.unwrap(),
        2
    );
}

#[sqlx::test]
async fn passkey_is_active_only_when_active(pool: PgPool) {
    let app = app(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let mut conn = app.pool.acquire().await.unwrap();
    for (state, want) in [
        (PasskeyState::Active, true),
        (PasskeyState::Pending, false),
        (PasskeyState::Revoked, false),
    ] {
        let (_, row) = seed_passkey(&app, user, state).await;
        let a = AuthenticatorRef::passkey(row.id);
        assert_eq!(
            strong::is_active(&mut conn, a).await.unwrap(),
            want,
            "{state:?}"
        );
        assert_eq!(queries::is_active(&mut conn, row.id).await.unwrap(), want);
    }
    let unknown = AuthenticatorRef::passkey(Uuid::new_v4());
    assert!(!strong::is_active(&mut conn, unknown).await.unwrap());
}

#[sqlx::test]
async fn activating_a_passkey_marks_first_activity(pool: PgPool) {
    let app = app(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    sqlx::query("UPDATE users SET first_activity_at = NULL WHERE id = $1")
        .bind(user)
        .execute(&app.pool)
        .await
        .unwrap();
    let (_, pending) = seed_passkey(&app, user, PasskeyState::Pending).await;
    let first: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT first_activity_at FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert!(first.is_none(), "a pending passkey is no activity");
    let mut conn = app.pool.acquire().await.unwrap();
    assert!(queries::activate(&mut conn, user, pending.id)
        .await
        .unwrap());
    assert!(!queries::activate(&mut conn, user, pending.id)
        .await
        .unwrap());
    let (state, ready_at, first): (
        String,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<chrono::DateTime<chrono::Utc>>,
    ) = sqlx::query_as(
        "SELECT p.state, p.ready_at, u.first_activity_at
               FROM passkeys p JOIN users u ON u.id = p.user_id WHERE p.id = $1",
    )
    .bind(pending.id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(state, "active");
    assert!(ready_at.is_none());
    assert!(first.is_some());
    // An active insert marks it too.
    let (other, _) = create_test_user(&app.pool).await;
    sqlx::query("UPDATE users SET first_activity_at = NULL WHERE id = $1")
        .bind(other)
        .execute(&app.pool)
        .await
        .unwrap();
    seed_passkey(&app, other, PasskeyState::Active).await;
    let first: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT first_activity_at FROM users WHERE id = $1")
            .bind(other)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert!(first.is_some());
}

#[sqlx::test]
async fn revoking_a_passkey_ends_its_sessions(pool: PgPool) {
    let app = app(pool).await;
    let ctx = AuthCtx::from(&app.state);
    let (user, _) = create_test_user(&app.pool).await;
    let (_, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (sid, _) = passkey_session(&app.pool, &app.config, user, row.id).await;
    let (other_sid, _) =
        crate::helpers::create_session_with_token(&app.pool, &app.config, user).await;
    // An open grant produced by this passkey, on another session.
    let grant = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO step_up_grants (id, session_id, user_id, action, target_hash, target, via,
                                     authenticator_type, authenticator_id, expires_at)
         VALUES ($1, $2, $3, 'passkey.add', $4, '', 'passkey', 'passkey', $5,
                 NOW() + INTERVAL '200 seconds')",
    )
    .bind(grant)
    .bind(other_sid)
    .bind(user)
    .bind(vec![0u8; 32])
    .bind(row.id)
    .execute(&app.pool)
    .await
    .unwrap();
    let mut tx = app.pool.begin().await.unwrap();
    queries::revoke(&mut tx, &ctx, user, row.id, "removed_by_owner")
        .await
        .unwrap()
        .expect("revoked");
    tx.commit().await.unwrap();
    let (ended, reason): (Option<chrono::DateTime<chrono::Utc>>, Option<String>) =
        sqlx::query_as("SELECT ended_at, end_reason FROM sessions WHERE id = $1")
            .bind(sid)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert!(ended.is_some());
    assert_eq!(reason.as_deref(), Some("authenticator_revoked"));
    let other_ended: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT ended_at FROM sessions WHERE id = $1")
            .bind(other_sid)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert!(other_ended.is_none());
    let voided: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT voided_at FROM step_up_grants WHERE id = $1")
            .bind(grant)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert!(voided.is_some());
}

#[sqlx::test]
async fn session_authenticator_shows_passkey_label(pool: PgPool) {
    let app = app(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (_, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let mut conn = app.pool.acquire().await.unwrap();
    assert_eq!(
        strong::authenticator_label(&mut conn, AuthenticatorRef::passkey(row.id))
            .await
            .unwrap()
            .as_deref(),
        Some("Test passkey")
    );
    queries::rename(&app.pool, user, row.id, "Laptop")
        .await
        .unwrap()
        .expect("renamed");
    assert_eq!(
        strong::authenticator_label(&mut conn, AuthenticatorRef::passkey(row.id))
            .await
            .unwrap()
            .as_deref(),
        Some("Laptop")
    );
}

async fn pinned(pool: &PgPool) -> Option<String> {
    sqlx::query_scalar::<_, serde_json::Value>("SELECT value FROM system_settings WHERE key = $1")
        .bind(RP_ID_SETTING)
        .fetch_optional(pool)
        .await
        .unwrap()
        .and_then(|v| v.as_str().map(str::to_owned))
}

#[sqlx::test]
async fn first_start_pins_rp_id(pool: PgPool) {
    let app = app(pool).await;
    let ctx = AuthCtx::from(&app.state);
    assert_eq!(
        check_rp_id(&ctx, "chat.example.com", None).await.unwrap(),
        RpPinOutcome::Pinned
    );
    assert_eq!(pinned(&app.pool).await.as_deref(), Some("chat.example.com"));
}

#[sqlx::test]
async fn same_rp_id_is_unchanged(pool: PgPool) {
    let app = app(pool).await;
    let ctx = AuthCtx::from(&app.state);
    check_rp_id(&ctx, "chat.example.com", None).await.unwrap();
    assert_eq!(
        check_rp_id(&ctx, "chat.example.com", None).await.unwrap(),
        RpPinOutcome::Unchanged
    );
}

#[sqlx::test]
async fn changed_rp_id_without_passkeys_moves_pin(pool: PgPool) {
    let app = app(pool).await;
    let ctx = AuthCtx::from(&app.state);
    check_rp_id(&ctx, "chat.example.com", None).await.unwrap();
    let (user, _) = create_test_user(&app.pool).await;
    seed_passkey(&app, user, PasskeyState::Revoked).await;
    assert_eq!(
        check_rp_id(&ctx, "talk.example.org", None).await.unwrap(),
        RpPinOutcome::Moved {
            from: "chat.example.com".into(),
            revoked: 0
        }
    );
    assert_eq!(pinned(&app.pool).await.as_deref(), Some("talk.example.org"));
}

#[sqlx::test]
async fn changed_rp_id_with_passkeys_refuses_start(pool: PgPool) {
    let app = app(pool).await;
    let ctx = AuthCtx::from(&app.state);
    check_rp_id(&ctx, "chat.example.com", None).await.unwrap();
    let (user, _) = create_test_user(&app.pool).await;
    seed_passkey(&app, user, PasskeyState::Active).await;
    seed_passkey(&app, user, PasskeyState::Active).await;
    let err = check_rp_id(&ctx, "talk.example.org", None)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("chat.example.com"), "{err}");
    assert!(err.contains("talk.example.org"), "{err}");
    assert!(err.contains(" 2 passkeys"), "{err}");
    assert!(
        err.contains("PASSKEY_ALLOW_RP_ID_CHANGE=talk.example.org"),
        "{err}"
    );
    assert_eq!(pinned(&app.pool).await.as_deref(), Some("chat.example.com"));
    assert_eq!(queries::active_count(&app.pool, user).await.unwrap(), 2);
}

#[sqlx::test]
async fn allowed_change_revokes_and_moves_pin(pool: PgPool) {
    let app = app(pool).await;
    let ctx = AuthCtx::from(&app.state);
    check_rp_id(&ctx, "chat.example.com", None).await.unwrap();
    let (user, _) = create_test_user(&app.pool).await;
    let (_, row) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (_, pending) = seed_passkey(&app, user, PasskeyState::Pending).await;
    let (sid, _) = passkey_session(&app.pool, &app.config, user, row.id).await;
    assert_eq!(
        check_rp_id(&ctx, "talk.example.org", Some("talk.example.org"))
            .await
            .unwrap(),
        RpPinOutcome::Moved {
            from: "chat.example.com".into(),
            revoked: 2
        }
    );
    assert_eq!(pinned(&app.pool).await.as_deref(), Some("talk.example.org"));
    for id in [row.id, pending.id] {
        let (state, reason): (String, Option<String>) =
            sqlx::query_as("SELECT state, revoke_reason FROM passkeys WHERE id = $1")
                .bind(id)
                .fetch_one(&app.pool)
                .await
                .unwrap();
        assert_eq!(state, "revoked");
        assert_eq!(reason.as_deref(), Some("rp_id_changed"));
    }
    let ended: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT ended_at FROM sessions WHERE id = $1")
            .bind(sid)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert!(ended.is_some());
}

#[sqlx::test]
async fn allow_value_for_another_host_does_not_apply(pool: PgPool) {
    let app = app(pool).await;
    let ctx = AuthCtx::from(&app.state);
    check_rp_id(&ctx, "chat.example.com", None).await.unwrap();
    let (user, _) = create_test_user(&app.pool).await;
    seed_passkey(&app, user, PasskeyState::Active).await;
    assert!(
        check_rp_id(&ctx, "talk.example.org", Some("other.example.net"))
            .await
            .is_err()
    );
    assert_eq!(pinned(&app.pool).await.as_deref(), Some("chat.example.com"));
    assert_eq!(queries::active_count(&app.pool, user).await.unwrap(), 1);
}
