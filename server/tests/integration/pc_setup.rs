//! "Set up this PC": the attempt table, the `pc_setup` enrolment variant and
//! the challenge kind's schema (spec Step 5, SM2 and SM5).
//!
//! Run with: `cargo test --test integration pc_setup`

use sqlx::PgPool;
use uuid::Uuid;
use vc_server::approvals::pc_setup::AttemptState;

use super::helpers::{create_test_user, insert_session_row};

/// A probation session of `user_id`, as a PC without a key would have.
async fn probation_session(pool: &PgPool, user_id: Uuid) -> Uuid {
    let sid = Uuid::now_v7();
    insert_session_row(pool, sid, user_id, false).await;
    sid
}

/// Insert an attempt row of `session_id` in `state`; the error is the DB's.
async fn insert_attempt(
    pool: &PgPool,
    user_id: Uuid,
    session_id: Uuid,
    state: &str,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO pc_setup_attempts (id, user_id, session_id, state, qr_token_hash,
                                        qr_rotates_at, expires_at)
         VALUES ($1, $2, $3, $4, $5, NOW() + INTERVAL '60 seconds', NOW() + INTERVAL '10 minutes')",
    )
    .bind(id)
    .bind(user_id)
    .bind(session_id)
    .bind(state)
    .bind(Uuid::new_v4().as_bytes().repeat(2))
    .execute(pool)
    .await?;
    Ok(id)
}

/// Insert an enrolment row; `variant`, `issuer_kind`, the token hash and the
/// authenticator type are the knobs the CHECKs look at.
async fn insert_enrollment(
    pool: &PgPool,
    user_id: Uuid,
    variant: &str,
    issuer_kind: &str,
    token_hash: Option<Vec<u8>>,
    authenticator_type: &str,
    (issuer_session, issuer_device): (Option<Uuid>, Option<Uuid>),
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO enrollments (id, variant, subject_id, issuer_kind, issuer_session_id,
                                  issuer_device_id, setup_token_hash, nonce, authenticator_type,
                                  state, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'issued', NOW() + INTERVAL '10 minutes')",
    )
    .bind(Uuid::new_v4())
    .bind(variant)
    .bind(user_id)
    .bind(issuer_kind)
    .bind(issuer_session)
    .bind(issuer_device)
    .bind(token_hash)
    .bind(vec![7u8; 32])
    .bind(authenticator_type)
    .execute(pool)
    .await
    .map(|_| ())
}

fn is_check_violation(e: &sqlx::Error) -> bool {
    e.as_database_error()
        .and_then(|d| d.code())
        .is_some_and(|c| c == "23514")
}

fn is_unique_violation(e: &sqlx::Error) -> bool {
    e.as_database_error()
        .is_some_and(|d| d.is_unique_violation())
}

#[sqlx::test]
async fn schema_allows_pc_setup_enrolment_without_a_token(pool: PgPool) {
    let (user, _) = create_test_user(&pool).await;
    let sid = probation_session(&pool, user).await;
    let device = Uuid::new_v4();

    // A passkey enrolment issued by an attempt: no setup token at all.
    insert_enrollment(
        &pool,
        user,
        "pc_setup",
        "pc_setup",
        None,
        "passkey",
        (Some(sid), Some(device)),
    )
    .await
    .expect("pc_setup enrolment without a token");

    // Every other variant still needs its token ...
    let e = insert_enrollment(
        &pool,
        user,
        "own",
        "key_device",
        None,
        "key_device",
        (Some(sid), Some(device)),
    )
    .await
    .unwrap_err();
    assert!(is_check_violation(&e), "{e:?}");
    // ... and a pc_setup enrolment can only add a passkey,
    let e = insert_enrollment(
        &pool,
        user,
        "pc_setup",
        "pc_setup",
        None,
        "key_device",
        (Some(sid), Some(device)),
    )
    .await
    .unwrap_err();
    assert!(is_check_violation(&e), "{e:?}");
    // ... records both the session and the approving device,
    let e = insert_enrollment(
        &pool,
        user,
        "pc_setup",
        "pc_setup",
        None,
        "passkey",
        (Some(sid), None),
    )
    .await
    .unwrap_err();
    assert!(is_check_violation(&e), "{e:?}");
    // ... and the variant and the issuer kind belong together.
    let e = insert_enrollment(
        &pool,
        user,
        "pc_setup",
        "session",
        None,
        "passkey",
        (Some(sid), Some(device)),
    )
    .await
    .unwrap_err();
    assert!(is_check_violation(&e), "{e:?}");
    let e = insert_enrollment(
        &pool,
        user,
        "own",
        "pc_setup",
        Some(vec![1u8; 32]),
        "passkey",
        (Some(sid), Some(device)),
    )
    .await
    .unwrap_err();
    assert!(is_check_violation(&e), "{e:?}");
    // The token column stays unique where present.
    insert_enrollment(
        &pool,
        user,
        "own",
        "key_device",
        Some(vec![9u8; 32]),
        "key_device",
        (Some(sid), Some(device)),
    )
    .await
    .expect("own with a token");
    let e = insert_enrollment(
        &pool,
        user,
        "own",
        "key_device",
        Some(vec![9u8; 32]),
        "key_device",
        (Some(sid), Some(device)),
    )
    .await
    .unwrap_err();
    assert!(is_unique_violation(&e), "{e:?}");
}

#[sqlx::test]
async fn schema_allows_the_pc_setup_challenge_kind_and_requester(pool: PgPool) {
    let (user, _) = create_test_user(&pool).await;
    sqlx::query(
        "INSERT INTO approvals (id, kind, approver_id, subject_id, device_id, requester_kind,
                                requester_id, nonce, context_hash, context, display, state,
                                expires_at)
         VALUES ($1, 'pc_setup', $2, $2, $3, 'pc_setup_attempt', $4, $5, $5, '{}', '{}',
                 'issued', NOW() + INTERVAL '2 minutes')",
    )
    .bind(Uuid::new_v4())
    .bind(user)
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .bind(vec![3u8; 32])
    .execute(&pool)
    .await
    .expect("pc_setup challenge for an attempt");
}

#[sqlx::test]
async fn one_live_attempt_per_session(pool: PgPool) {
    let (user, _) = create_test_user(&pool).await;
    let sid = probation_session(&pool, user).await;
    let other = probation_session(&pool, user).await;

    let first = insert_attempt(&pool, user, sid, "waiting").await.unwrap();
    // A second live attempt of the same session is refused in every live state ...
    for state in ["waiting", "claimed", "code", "enrolled"] {
        let e = insert_attempt(&pool, user, sid, state).await.unwrap_err();
        assert!(is_unique_violation(&e), "{state}: {e:?}");
    }
    // ... another session of the account is independent ...
    insert_attempt(&pool, user, other, "waiting").await.unwrap();
    // ... and a finished attempt makes room for the next one.
    for state in AttemptState::ALL.into_iter().filter(|s| s.is_terminal()) {
        sqlx::query("UPDATE pc_setup_attempts SET state = $2 WHERE id = $1")
            .bind(first)
            .bind(state.as_str())
            .execute(&pool)
            .await
            .unwrap();
        let next = insert_attempt(&pool, user, sid, "waiting").await.unwrap();
        sqlx::query("UPDATE pc_setup_attempts SET state = 'cancelled' WHERE id = $1")
            .bind(next)
            .execute(&pool)
            .await
            .unwrap();
    }
    // The table refuses a state SM5 does not know and an attempt that outlives 10 minutes.
    let e = insert_attempt(&pool, user, sid, "approved")
        .await
        .unwrap_err();
    assert!(is_check_violation(&e), "{e:?}");
    let e = sqlx::query(
        "INSERT INTO pc_setup_attempts (id, user_id, session_id, state, qr_token_hash, qr_rotates_at, expires_at)
         VALUES ($1, $2, $3, 'cancelled', $4, NOW(), NOW() + INTERVAL '11 minutes')",
    )
    .bind(Uuid::new_v4())
    .bind(user)
    .bind(sid)
    .bind(vec![5u8; 32])
    .execute(&pool)
    .await
    .unwrap_err();
    assert!(is_check_violation(&e), "{e:?}");
}

#[sqlx::test]
async fn deleting_the_session_row_leaves_the_attempt_alone(pool: PgPool) {
    let (user, _) = create_test_user(&pool).await;
    let sid = probation_session(&pool, user).await;
    let id = insert_attempt(&pool, user, sid, "waiting").await.unwrap();
    // Logout and refresh delete the session row without the account lock.
    sqlx::query("DELETE FROM sessions WHERE id = $1")
        .bind(sid)
        .execute(&pool)
        .await
        .unwrap();
    let (state, session): (String, Uuid) =
        sqlx::query_as("SELECT state, session_id FROM pc_setup_attempts WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("attempt row survives");
    assert_eq!((state.as_str(), session), ("waiting", sid));
}
