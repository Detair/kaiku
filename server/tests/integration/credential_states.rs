//! Credential states, factor generations and recovery codes (spec B.1, B.7
//! rule 2; plan Task 15).
//!
//! Run with: `cargo test --test integration credential_states`

use axum::body::Body;
use axum::http::Method;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::auth::ctx::AuthCtx;
use vc_server::auth::factors::{self, ExitPath, Factor, FactorRef, FactorState, StateCause};
use vc_server::auth::recovery_codes;
use vc_server::auth::step_up::StepUpAction;
use vc_server::security_events::Actor;

use super::helpers::approvals::KeyDeviceFixture;
use super::helpers::login::{device_grant, peer_request};
use super::helpers::{
    body_to_json_lenient, create_probation_session_with_token, create_test_user, TestApp,
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

async fn generations(pool: &PgPool, user: Uuid) -> (i64, i64, i64) {
    sqlx::query_as(
        "SELECT password_generation, totp_generation, recovery_codes_generation
           FROM users WHERE id = $1",
    )
    .bind(user)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn events_of(pool: &PgPool, user: Uuid, ty: &str) -> Vec<(String, Value)> {
    sqlx::query_as(
        "SELECT severity, details FROM security_events
          WHERE subject_id = $1 AND event_type = $2 ORDER BY created_at",
    )
    .bind(user)
    .bind(ty)
    .fetch_all(pool)
    .await
    .unwrap()
}

#[sqlx::test]
async fn ten_codes_with_65_bits(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let ctx = AuthCtx::from(&app.state);
    let mut tx = pool.begin().await.unwrap();
    let codes = recovery_codes::replace_tx(&mut tx, &ctx, user)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(codes.len(), 10);
    let alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    for c in &codes {
        let parts: Vec<&str> = c.split('-').collect();
        assert_eq!(
            parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
            vec![5, 5, 3],
            "{c}"
        );
        assert!(
            parts.concat().chars().all(|ch| alphabet.contains(ch)),
            "{c}"
        );
    }
    let unique: std::collections::HashSet<_> = codes.iter().collect();
    assert_eq!(unique.len(), 10);

    let (_, _, generation) = generations(&pool, user).await;
    let rows: Vec<(i64, String)> =
        sqlx::query_as("SELECT generation, code_hash FROM recovery_codes WHERE user_id = $1")
            .bind(user)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(rows.len(), 10);
    for (g, hash) in rows {
        assert_eq!(g, generation);
        assert!(hash.starts_with("$argon2id$"), "{hash}");
    }
    assert_eq!(recovery_codes::left(&pool, user).await.unwrap(), 10);
}

#[sqlx::test]
async fn code_is_single_use_and_normalised(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let ctx = AuthCtx::from(&app.state);
    let mut tx = pool.begin().await.unwrap();
    let codes = recovery_codes::replace_tx(&mut tx, &ctx, user)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let typed = codes[3].to_lowercase().replace('-', " ");
    let mut tx = pool.begin().await.unwrap();
    assert!(recovery_codes::consume_tx(&mut tx, user, &typed)
        .await
        .unwrap());
    tx.commit().await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    assert!(!recovery_codes::consume_tx(&mut tx, user, &codes[3])
        .await
        .unwrap());
    assert!(
        !recovery_codes::consume_tx(&mut tx, user, "AAAAA-AAAAA-AAA")
            .await
            .unwrap()
    );
    tx.commit().await.unwrap();
    assert_eq!(recovery_codes::left(&pool, user).await.unwrap(), 9);
}

#[sqlx::test]
async fn new_set_replaces_the_old_generation(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let ctx = AuthCtx::from(&app.state);
    let mut tx = pool.begin().await.unwrap();
    let old = recovery_codes::replace_tx(&mut tx, &ctx, user)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (_, _, g1) = generations(&pool, user).await;

    let mut tx = pool.begin().await.unwrap();
    let new = recovery_codes::replace_tx(&mut tx, &ctx, user)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (_, _, g2) = generations(&pool, user).await;
    assert_eq!(g2, g1 + 1);

    let mut tx = pool.begin().await.unwrap();
    assert!(!recovery_codes::consume_tx(&mut tx, user, &old[0])
        .await
        .unwrap());
    assert!(recovery_codes::consume_tx(&mut tx, user, &new[0])
        .await
        .unwrap());
    tx.commit().await.unwrap();
    assert_eq!(recovery_codes::left(&pool, user).await.unwrap(), 9);
}

#[sqlx::test]
async fn compromise_starts_a_generation_and_records_an_event(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let ctx = AuthCtx::from(&app.state);
    let (g, _, _) = generations(&pool, user).await;

    let mut tx = pool.begin().await.unwrap();
    let changed = factors::mark_compromised_tx(
        &mut tx,
        &ctx,
        user,
        FactorRef {
            factor: Factor::Password,
            generation: g,
        },
        StateCause::Report,
        Actor::User(user),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert!(changed);

    let mut conn = pool.acquire().await.unwrap();
    let states = factors::states(&mut conn, user).await.unwrap();
    assert_eq!(states.password.state, FactorState::Compromised);
    assert_eq!(states.password.generation, g + 1);
    let at: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT password_compromised_at FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(at.is_some());
    let ev = events_of(&pool, user, "password_compromised").await;
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].0, "high");
    assert_eq!(ev[0].1, json!({"cause": "report"}));
}

#[sqlx::test]
async fn compromise_of_an_old_generation_changes_nothing(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let ctx = AuthCtx::from(&app.state);
    let (g, tg, _) = generations(&pool, user).await;

    // The password was replaced since the attempt used it.
    let mut tx = pool.begin().await.unwrap();
    let g2 = factors::replace_tx(&mut tx, &ctx, user, Factor::Password)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(g2, g + 1);

    for (factor, generation) in [(Factor::Password, g), (Factor::Totp, tg - 1)] {
        let mut tx = pool.begin().await.unwrap();
        let changed = factors::mark_compromised_tx(
            &mut tx,
            &ctx,
            user,
            FactorRef { factor, generation },
            StateCause::Cancel,
            Actor::System,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        assert!(!changed, "{factor:?}");
    }
    let mut conn = pool.acquire().await.unwrap();
    let states = factors::states(&mut conn, user).await.unwrap();
    assert_eq!(states.password.state, FactorState::Active);
    assert_eq!(states.password.generation, g2);
    assert!(events_of(&pool, user, "password_compromised")
        .await
        .is_empty());
}

#[sqlx::test]
async fn throttle_starts_no_generation(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let ctx = AuthCtx::from(&app.state);
    let before = generations(&pool, user).await;

    for f in [Factor::Password, Factor::Totp] {
        let mut tx = pool.begin().await.unwrap();
        factors::throttle_tx(&mut tx, &ctx, user, f).await.unwrap();
        // A second throttle records nothing new.
        factors::throttle_tx(&mut tx, &ctx, user, f).await.unwrap();
        tx.commit().await.unwrap();
    }
    assert_eq!(generations(&pool, user).await, before);
    let mut conn = pool.acquire().await.unwrap();
    let states = factors::states(&mut conn, user).await.unwrap();
    assert_eq!(states.password.state, FactorState::Throttled);
    assert_eq!(states.totp.state, FactorState::Throttled);
    let ev = events_of(&pool, user, "password_throttled").await;
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0], ("high".to_string(), json!({"cause": "guessing"})));
    assert_eq!(events_of(&pool, user, "totp_throttled").await.len(), 1);
}

/// B.1 exit table, applied to stored rows.
#[sqlx::test]
async fn exit_rules_follow_b1(pool: PgPool) {
    use ExitPath::{Admin, Cli, CompletedRequest, OtherMethodSignIn, Replacement};
    use FactorState::{Active, Compromised, Throttled};
    let _app = TestApp::with_pool(pool.clone()).await;
    let cases = [
        (Factor::Password, Throttled, OtherMethodSignIn, Active),
        (Factor::Password, Throttled, CompletedRequest, Active),
        (Factor::Password, Throttled, Admin, Active),
        (Factor::Password, Throttled, Cli, Active),
        (Factor::Totp, Throttled, OtherMethodSignIn, Throttled),
        (Factor::Totp, Throttled, Replacement, Active),
        (Factor::Totp, Throttled, CompletedRequest, Active),
        (Factor::Totp, Throttled, Admin, Active),
        (Factor::Totp, Throttled, Cli, Active),
        (
            Factor::Password,
            Compromised,
            OtherMethodSignIn,
            Compromised,
        ),
        (Factor::Password, Compromised, Admin, Compromised),
        (Factor::Password, Compromised, Replacement, Active),
        (Factor::Password, Compromised, CompletedRequest, Active),
        (Factor::Password, Compromised, Cli, Active),
        (Factor::Totp, Compromised, OtherMethodSignIn, Compromised),
        (Factor::Totp, Compromised, Admin, Compromised),
        (Factor::Totp, Compromised, Replacement, Active),
        (Factor::Totp, Compromised, CompletedRequest, Active),
        (Factor::Totp, Compromised, Cli, Active),
    ];
    for (factor, from, why, to) in cases {
        let (user, _) = create_test_user(&pool).await;
        let sql = if factor == Factor::Password {
            "UPDATE users SET password_state = $2 WHERE id = $1"
        } else {
            "UPDATE users SET totp_state = $2 WHERE id = $1"
        };
        let from_s = match from {
            Active => "active",
            Throttled => "throttled",
            Compromised => "compromised",
        };
        sqlx::query(sql)
            .bind(user)
            .bind(from_s)
            .execute(&pool)
            .await
            .unwrap();
        let before = generations(&pool, user).await;
        let mut tx = pool.begin().await.unwrap();
        factors::clear_throttle_tx(&mut tx, user, factor, why)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let mut conn = pool.acquire().await.unwrap();
        let s = factors::states(&mut conn, user).await.unwrap();
        let got = if factor == Factor::Password {
            s.password.state
        } else {
            s.totp.state
        };
        assert_eq!(got, to, "{factor:?} {from:?} via {why:?}");
        assert_eq!(
            generations(&pool, user).await,
            before,
            "exits start no generation"
        );
    }
}

#[sqlx::test]
async fn backup_code_routes_are_gone(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let token = super::helpers::generate_access_token(&app.config, user);
    let (status, _) = call(
        &app,
        Method::POST,
        "/auth/mfa/backup-codes",
        &token,
        json!({}),
    )
    .await;
    assert_eq!(status, 404);
    let (status, _) = call(
        &app,
        Method::GET,
        "/auth/mfa/backup-codes/count",
        &token,
        json!({}),
    )
    .await;
    assert_eq!(status, 404);
}

/// `POST /auth/recovery-codes`: full + `recovery_codes.create`, event
/// `recovery_codes_created`; `GET /auth/recovery-codes/count` from any session.
#[sqlx::test]
async fn recovery_code_routes_are_guarded(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (_, probation) = create_probation_session_with_token(&pool, &app.config, user).await;

    let (status, body) = call(
        &app,
        Method::POST,
        "/auth/recovery-codes",
        &probation,
        json!({}),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (403, Some("FULL_SESSION_REQUIRED"))
    );
    let (status, body) = call(
        &app,
        Method::POST,
        "/auth/recovery-codes",
        &dev.access_token,
        json!({}),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (403, Some("STEP_UP_REQUIRED"))
    );

    device_grant(&app, &dev, StepUpAction::RecoveryCodesCreate, "").await;
    let (status, body) = call(
        &app,
        Method::POST,
        "/auth/recovery-codes",
        &dev.access_token,
        json!({}),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["recovery_codes"].as_array().map(Vec::len), Some(10));
    assert_eq!(
        events_of(&pool, user, "recovery_codes_created").await.len(),
        1
    );

    let (status, body) = call(
        &app,
        Method::GET,
        "/auth/recovery-codes/count",
        &probation,
        json!({}),
    )
    .await;
    assert_eq!((status, body["left"].as_i64()), (200, Some(10)), "{body}");
}
