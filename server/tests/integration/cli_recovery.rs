//! `kaiku-admin` recovery commands (F6 item 4; L1b Task 28).
//!
//! Run with: `cargo test --test integration cli_recovery`

use sqlx::PgPool;
use uuid::Uuid;
use vc_server::cli::{run, CliCtx, CliIo, Command};

use super::account_requests::{admin_with_device, bootstrap_user, start_bootstrap};
use super::helpers::approvals::KeyDeviceFixture;
use super::helpers::{create_test_user, make_admin, TestApp};

#[derive(Default)]
struct Out(Vec<String>);

impl CliIo for Out {
    fn print(&mut self, s: &str) {
        self.0.push(s.to_string());
    }

    async fn read_line(&mut self, _prompt: &str) -> std::io::Result<String> {
        Err(std::io::ErrorKind::UnexpectedEof.into())
    }
}

fn cli_ctx(app: &TestApp) -> CliCtx {
    CliCtx {
        db: app.pool.clone(),
        redis: app.state.redis.clone(),
        config: (*app.config).clone(),
    }
}

async fn cli(app: &TestApp, args: &[&str]) -> anyhow::Result<Vec<String>> {
    let args: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    let cmd = Command::parse(&args).map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut out = Out::default();
    run(&cli_ctx(app), cmd, &mut out).await?;
    Ok(out.0)
}

async fn audit_actions(pool: &PgPool, user: Uuid) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT action FROM system_audit_log WHERE target_id = $1 AND action LIKE 'cli.%'
          ORDER BY created_at",
    )
    .bind(user)
    .fetch_all(pool)
    .await
    .unwrap()
}

#[sqlx::test]
async fn cli_resets_throttles_and_compromised_factors(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = create_test_user(&pool).await;
    sqlx::query(
        "UPDATE users SET password_hash = 'x', mfa_secret = 'y', password_state = 'compromised',
                totp_state = 'throttled'
          WHERE id = $1",
    )
    .bind(user)
    .execute(&pool)
    .await
    .unwrap();
    cli(&app, &["factor", "reset", &name, "password"])
        .await
        .unwrap();
    cli(&app, &["throttle", "reset", &name]).await.unwrap();
    let (pw, totp): (String, String) =
        sqlx::query_as("SELECT password_state, totp_state FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((pw.as_str(), totp.as_str()), ("active", "active"));
    assert_eq!(
        audit_actions(&pool, user).await,
        ["cli.factor.reset", "cli.throttle.reset"]
    );
}

#[sqlx::test]
async fn cli_does_immediate_actions_on_admin_accounts(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (admin, name) = create_test_user(&pool).await;
    make_admin(&pool, admin).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, admin, "active").await;
    let (contact, contact_name) = create_test_user(&pool).await;
    cli(&app, &["may-approve", &name, "off"]).await.unwrap();
    cli(&app, &["recovery-contact", &name, &contact_name])
        .await
        .unwrap();
    cli(&app, &["sessions", "end", &name, "--all"])
        .await
        .unwrap();
    cli(&app, &["devices", "remove", &name, &dev.row_id.to_string()])
        .await
        .unwrap();
    cli(&app, &["admin", "demote", &name]).await.unwrap();
    let (may, rc, is_admin): (bool, Option<Uuid>, bool) = sqlx::query_as(
        "SELECT may_approve, recovery_contact_id,
                EXISTS (SELECT 1 FROM system_admins WHERE user_id = u.id)
           FROM users u WHERE id = $1",
    )
    .bind(admin)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!may);
    assert_eq!(rc, Some(contact));
    assert!(!is_admin);
    let events: Vec<String> = sqlx::query_scalar(
        "SELECT event_type FROM security_events WHERE subject_id = $1 AND actor_kind = 'cli'
          ORDER BY created_at",
    )
    .bind(admin)
    .fetch_all(&pool)
    .await
    .unwrap();
    for t in [
        "recovery_contact_named",
        "sessions_ended",
        "authenticator_removed",
        "admin_role_removed",
    ] {
        assert!(events.contains(&t.to_string()), "{t}: {events:?}");
    }
}

#[sqlx::test]
async fn cli_cancels_a_request(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    admin_with_device(&app).await;
    let (user, name, totp) = bootstrap_user(&app).await;
    let (s, b) = start_bootstrap(&app, user, &name, &totp).await;
    assert_eq!(s, 201, "{b}");
    let id = b["request"]["id"].as_str().unwrap().to_string();
    cli(&app, &["request", "cancel", &id]).await.unwrap();
    let (state, by): (String, serde_json::Value) =
        sqlx::query_as("SELECT state, cancelled_by FROM account_requests WHERE id = $1::uuid")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(state, "cancelled");
    assert_eq!(by["kind"], "cli");
    // A second cancel fails.
    assert!(cli(&app, &["request", "cancel", &id]).await.is_err());
}

#[sqlx::test]
async fn every_cli_action_is_audited_and_notified(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (other_admin, _) = create_test_user(&pool).await;
    make_admin(&pool, other_admin).await;
    let (user, name) = create_test_user(&pool).await;
    let (_, contact_name) = create_test_user(&pool).await;
    sqlx::query(
        "UPDATE users SET password_hash = 'x', mfa_secret = 'y', password_state = 'compromised',
                totp_state = 'throttled' WHERE id = $1",
    )
    .bind(user)
    .execute(&pool)
    .await
    .unwrap();
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    cli(&app, &["recovery-contact", &name, &contact_name])
        .await
        .unwrap();
    cli(&app, &["factor", "reset", &name, "password"])
        .await
        .unwrap();
    cli(&app, &["throttle", "reset", &name]).await.unwrap();
    cli(&app, &["may-approve", &name, "off"]).await.unwrap();
    cli(&app, &["sessions", "end", &name, "--all"])
        .await
        .unwrap();
    cli(&app, &["devices", "remove", &name, &dev.row_id.to_string()])
        .await
        .unwrap();
    cli(&app, &["recovery-contact", &name, "--none"])
        .await
        .unwrap();
    assert_eq!(
        audit_actions(&pool, user).await,
        [
            "cli.recovery_contact",
            "cli.factor.reset",
            "cli.throttle.reset",
            "cli.may_approve",
            "cli.sessions.end",
            "cli.devices.remove",
            "cli.recovery_contact"
        ]
    );
    let events: Vec<(String, serde_json::Value)> = sqlx::query_as(
        "SELECT event_type, details FROM security_events
          WHERE subject_id = $1 AND actor_kind = 'cli' ORDER BY created_at, id",
    )
    .bind(user)
    .fetch_all(&pool)
    .await
    .unwrap();
    let types: Vec<&str> = events.iter().map(|(t, _)| t.as_str()).collect();
    for t in [
        "recovery_contact_named",
        "factor_restored",
        "throttle_reset",
        "may_approve_changed",
        "sessions_ended",
        "authenticator_removed",
        "recovery_contact_changed",
    ] {
        assert!(types.contains(&t), "{t}: {types:?}");
    }
    let restored = &events
        .iter()
        .find(|(t, _)| t == "factor_restored")
        .unwrap()
        .1;
    assert_eq!(
        *restored,
        serde_json::json!({"factor": "password", "by": "cli"})
    );
    let may = &events
        .iter()
        .find(|(t, _)| t == "may_approve_changed")
        .unwrap()
        .1;
    assert_eq!(*may, serde_json::json!({"may_approve": false, "by": "cli"}));
    // High events reach every admin (the sweep delivers what is pending).
    vc_server::security_events::notices::deliver_pending(&app.state)
        .await
        .unwrap();
    for t in ["factor_restored", "may_approve_changed", "sessions_ended"] {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM security_notices
              WHERE subject_id = $1 AND recipient_id = $2 AND event_type = $3",
        )
        .bind(user)
        .bind(other_admin)
        .bind(t)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(n, 1, "{t} to the other admin");
    }
}
