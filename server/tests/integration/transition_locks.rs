//! B.7 rule 1 (ordered account locks, re-checks under the lock), rule 2
//! (authenticator cascades) and the strong-authenticator helpers.
//!
//! Run with: `cargo test --test integration transition_locks`

use std::time::Duration;

use axum::body::Body;
use axum::http::Method;
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::approvals::cascade::on_authenticator_revoked;
use vc_server::approvals::transition::{lock_accounts, lock_scope, LockScope, TransitionError};
use vc_server::auth::ctx::AuthCtx;
use vc_server::auth::strong::{
    active_strong_count, authenticator_label, ensure_not_last_strong, is_active, AuthenticatorRef,
};
use vc_server::system_account::SYSTEM_USER_ID;

use super::helpers::approvals::{device_session, KeyDeviceFixture};
use super::helpers::{
    body_to_json, create_session_with_token, create_test_user, make_admin, TestApp,
};

#[sqlx::test]
async fn lock_accounts_never_deadlocks(pool: PgPool) {
    let (a, _) = create_test_user(&pool).await;
    let (b, _) = create_test_user(&pool).await;
    let started = std::time::Instant::now();
    let mut tasks = Vec::new();
    for i in 0..50 {
        let pool = pool.clone();
        let ids = if i % 2 == 0 { vec![a, b] } else { vec![b, a] };
        tasks.push(tokio::spawn(async move {
            let mut tx = pool.begin().await.unwrap();
            let locked = lock_accounts(&mut tx, &ids)
                .await
                .expect("no deadlock (40P01)");
            assert_eq!(locked.len(), 2);
            tokio::time::sleep(Duration::from_millis(50)).await;
            tx.commit().await.unwrap();
        }));
    }
    for t in tasks {
        t.await.unwrap();
    }
    assert!(
        started.elapsed() < Duration::from_secs(5) * 2,
        "finished in time"
    );
}

#[sqlx::test]
async fn lock_accounts_reports_state(pool: PgPool) {
    let (plain, _) = create_test_user(&pool).await;
    let (admin, _) = create_test_user(&pool).await;
    make_admin(&pool, admin).await;
    let (banned, _) = create_test_user(&pool).await;
    sqlx::query("INSERT INTO global_bans (user_id, banned_by, reason) VALUES ($1, $2, 'x')")
        .bind(banned)
        .bind(admin)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET may_approve = false WHERE id = $1")
        .bind(plain)
        .execute(&pool)
        .await
        .unwrap();
    let missing = Uuid::new_v4();
    let mut tx = pool.begin().await.unwrap();
    let m = lock_accounts(
        &mut tx,
        &[plain, admin, banned, SYSTEM_USER_ID, missing, plain],
    )
    .await
    .unwrap();
    assert_eq!(m.len(), 5, "deduplicated");
    assert!(m[&plain].exists && !m[&plain].banned && !m[&plain].is_admin && !m[&plain].may_approve);
    assert!(m[&admin].is_admin && m[&admin].may_approve);
    assert!(m[&banned].banned);
    assert!(m[&SYSTEM_USER_ID].is_system);
    assert!(!m[&missing].exists);
    let keys: Vec<Uuid> = m.keys().copied().collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted, "ascending id order");
}

#[sqlx::test]
async fn transition_rechecks_under_the_lock(pool: PgPool) {
    let (a, _) = create_test_user(&pool).await;
    let (admin, _) = create_test_user(&pool).await;

    // The test holds A's row lock.
    let mut holder = pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM users WHERE id = $1 FOR UPDATE")
        .bind(a)
        .execute(&mut *holder)
        .await
        .unwrap();

    // A transition that needs A not banned waits for the lock …
    let pool2 = pool.clone();
    let transition = tokio::spawn(async move {
        let mut tx = pool2.begin().await.unwrap();
        let locked = lock_accounts(&mut tx, &[a]).await.unwrap();
        if locked[&a].banned {
            return Err(TransitionError::Precondition("account banned"));
        }
        sqlx::query("UPDATE users SET display_name = 'changed' WHERE id = $1")
            .bind(a)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        Ok(())
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!transition.is_finished(), "waits for the lock");

    // … the holder bans A and releases: the transition fails, no effect.
    sqlx::query("INSERT INTO global_bans (user_id, banned_by, reason) VALUES ($1, $2, 'x')")
        .bind(a)
        .bind(admin)
        .execute(&mut *holder)
        .await
        .unwrap();
    holder.commit().await.unwrap();
    let result = transition.await.unwrap();
    assert!(matches!(result, Err(TransitionError::Precondition(_))));
    let name: String = sqlx::query_scalar("SELECT display_name FROM users WHERE id = $1")
        .bind(a)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_ne!(name, "changed");
}

#[sqlx::test]
async fn lock_scope_without_recorded_routes_locks_subject_and_actors(pool: PgPool) {
    let (subject, _) = create_test_user(&pool).await;
    let (actor, _) = create_test_user(&pool).await;
    let mut tx = pool.begin().await.unwrap();
    let locked = lock_scope(
        &mut tx,
        &LockScope {
            subject,
            actors: vec![actor, subject],
            affected: Vec::new(),
        },
    )
    .await
    .unwrap();
    let mut want = vec![subject, actor];
    want.sort();
    assert_eq!(locked.keys().copied().collect::<Vec<_>>(), want);

    // Both rows are locked by this transaction.
    let other = sqlx::query("SELECT id FROM users WHERE id = $1 FOR UPDATE NOWAIT")
        .bind(actor)
        .execute(&pool)
        .await;
    assert!(other.is_err(), "the actor row is locked");
    tx.rollback().await.unwrap();
}

#[sqlx::test]
async fn revoking_an_authenticator_ends_its_sessions(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (second, _) = device_session(&pool, &app.config, user, dev.row_id).await;
    let (unrelated, unrelated_token) = create_session_with_token(&pool, &app.config, user).await;
    sqlx::query("UPDATE sessions SET authenticator_id = gen_random_uuid() WHERE id = $1")
        .bind(unrelated)
        .execute(&pool)
        .await
        .unwrap();

    let ctx = AuthCtx::from(&app.state);
    let mut tx = pool.begin().await.unwrap();
    let out = on_authenticator_revoked(
        &mut tx,
        &ctx,
        user,
        AuthenticatorRef::key_device(dev.row_id),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let mut ended = out.ended_sessions.clone();
    ended.sort();
    let mut want = vec![dev.session_id.unwrap(), second];
    want.sort();
    assert_eq!(ended, want);
    for sid in want {
        let reason: Option<String> =
            sqlx::query_scalar("SELECT end_reason FROM sessions WHERE id = $1")
                .bind(sid)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(reason.as_deref(), Some("authenticator_revoked"));
    }
    let req = TestApp::request(Method::GET, "/auth/me")
        .header("Authorization", format!("Bearer {unrelated_token}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.oneshot(req).await.status(), 200, "other sessions stay");
}

#[sqlx::test]
async fn last_strong_authenticator_is_protected(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let one = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    KeyDeviceFixture::insert(&pool, &app.config, user, "pending").await;
    let mut conn = pool.acquire().await.unwrap();
    assert_eq!(active_strong_count(&mut conn, user).await.unwrap(), 1);
    let err = ensure_not_last_strong(&mut conn, user, AuthenticatorRef::key_device(one.row_id))
        .await
        .unwrap_err();
    let resp = axum::response::IntoResponse::into_response(err);
    assert_eq!(resp.status(), 409);
    assert_eq!(
        body_to_json(resp).await["error"],
        "LAST_STRONG_AUTHENTICATOR"
    );

    let two = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    assert_eq!(active_strong_count(&mut conn, user).await.unwrap(), 2);
    ensure_not_last_strong(&mut conn, user, AuthenticatorRef::key_device(two.row_id))
        .await
        .expect("another active one remains");
    assert_eq!(
        authenticator_label(&mut conn, AuthenticatorRef::key_device(two.row_id))
            .await
            .unwrap()
            .as_deref(),
        Some("Pixel 8")
    );
}

#[sqlx::test]
async fn is_active_follows_device_state(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let mut conn = pool.acquire().await.unwrap();
    for (state, want) in [("pending", false), ("active", true), ("revoked", false)] {
        let d = KeyDeviceFixture::insert(&pool, &app.config, user, state).await;
        assert_eq!(
            is_active(&mut conn, AuthenticatorRef::key_device(d.row_id))
                .await
                .unwrap(),
            want,
            "{state}"
        );
    }
    assert!(
        !is_active(&mut conn, AuthenticatorRef::key_device(Uuid::new_v4()))
            .await
            .unwrap()
    );
}

#[sqlx::test]
async fn auth_me_reports_may_approve(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (_, token) = create_session_with_token(&pool, &app.config, user).await;
    let me = |token: String| {
        let app = &app;
        async move {
            let req = TestApp::request(Method::GET, "/auth/me")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap();
            body_to_json(app.oneshot(req).await).await
        }
    };
    assert_eq!(me(token.clone()).await["may_approve"], true);
    sqlx::query("UPDATE users SET may_approve = false WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(me(token).await["may_approve"], false);
}
