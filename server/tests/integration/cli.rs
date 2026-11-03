//! `kaiku-admin`, the host CLI (spec F6 item 4; plan Task 13).
//!
//! Run with: `cargo test --test integration cli`

use std::time::Duration;

use futures::StreamExt;
use serde_json::Value;
use sqlx::PgPool;
use tokio::sync::mpsc;
use uuid::Uuid;
use vc_server::cli::{run, CliCtx, CliIo, Command};
use vc_server::system_account::SYSTEM_USER_ID;

use super::enrollments::{claim_with, good, Phone};
use super::helpers::approvals::KeyDeviceFixture;
use super::helpers::{
    create_session_with_token, create_test_user, make_admin, next_ws_event, spawn_test_server,
    ws_connect, TestApp,
};

/// A terminal driven by the test: every printed line goes to `printed`,
/// every read takes the next line from `answers`.
struct ScriptedIo {
    out: Vec<String>,
    printed: mpsc::UnboundedSender<String>,
    answers: mpsc::UnboundedReceiver<String>,
}

impl CliIo for ScriptedIo {
    fn print(&mut self, s: &str) {
        self.out.push(s.to_string());
        let _ = self.printed.send(s.to_string());
    }

    async fn read_line(&mut self, _prompt: &str) -> std::io::Result<String> {
        self.answers
            .recv()
            .await
            .ok_or_else(|| std::io::ErrorKind::UnexpectedEof.into())
    }
}

fn scripted() -> (
    ScriptedIo,
    mpsc::UnboundedReceiver<String>,
    mpsc::UnboundedSender<String>,
) {
    let (printed, printed_rx) = mpsc::unbounded_channel();
    let (answer_tx, answers) = mpsc::unbounded_channel();
    (
        ScriptedIo {
            out: Vec::new(),
            printed,
            answers,
        },
        printed_rx,
        answer_tx,
    )
}

fn cli_ctx(app: &TestApp) -> CliCtx {
    CliCtx {
        db: app.pool.clone(),
        redis: app.state.redis.clone(),
        config: (*app.config).clone(),
    }
}

/// The new phone: waits for the printed setup link, claims it as `label`
/// and types `typed(code)` once per entry.
async fn phone_claims(
    app: &TestApp,
    mut printed: mpsc::UnboundedReceiver<String>,
    answers: mpsc::UnboundedSender<String>,
    label: &str,
    typed: impl Fn(&str) -> Vec<String>,
) -> Value {
    let token = loop {
        let line = printed.recv().await.expect("the CLI prints the setup link");
        if let Some(i) = line.find("/setup#v=1&t=") {
            let rest = &line[i + "/setup#v=1&t=".len()..];
            break rest.split_whitespace().next().unwrap().to_string();
        }
    };
    let (status, claim) = claim_with(app, &token, &Phone::new(), label, good(), "per_use").await;
    assert_eq!(status, 201, "{claim}");
    for line in typed(claim["code"].as_str().unwrap()) {
        answers.send(line).unwrap();
    }
    claim
}

#[test]
fn parse_commands_and_usage() {
    let p = |a: &[&str]| Command::parse(&a.iter().map(|s| (*s).to_string()).collect::<Vec<_>>());
    let id = Uuid::new_v4();
    assert_eq!(
        p(&["enroll", "alice"]).unwrap(),
        Command::Enroll {
            username: "alice".into()
        }
    );
    assert_eq!(
        p(&["devices", "list", "alice"]).unwrap(),
        Command::DevicesList {
            username: "alice".into()
        }
    );
    assert_eq!(
        p(&["devices", "remove", "alice", &id.to_string()]).unwrap(),
        Command::DevicesRemove {
            username: "alice".into(),
            device: id
        }
    );
    assert_eq!(
        p(&["sessions", "list", "alice"]).unwrap(),
        Command::SessionsList {
            username: "alice".into()
        }
    );
    assert_eq!(
        p(&["sessions", "end", "alice", &id.to_string()]).unwrap(),
        Command::SessionsEnd {
            username: "alice".into(),
            session: Some(id)
        }
    );
    assert_eq!(
        p(&["sessions", "end", "alice", "--all"]).unwrap(),
        Command::SessionsEnd {
            username: "alice".into(),
            session: None
        }
    );
    for bad in [
        &[][..],
        &["enroll"][..],
        &["enroll", "a", "b"][..],
        &["devices", "remove", "alice", "not-a-uuid"][..],
        &["sessions", "end", "alice"][..],
        &["frobnicate"][..],
    ] {
        let e = p(bad).unwrap_err();
        assert!(e.to_string().contains("Usage"), "{bad:?}: {e}");
    }

    // The binary: a usage error exits 2 before touching any service.
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_kaiku-admin"))
        .env_clear()
        .output()
        .expect("run kaiku-admin");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("Usage"));
}

#[sqlx::test]
async fn cli_enrolment_activates_at_once(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, username) = create_test_user(&pool).await;
    let (admin, _) = create_test_user(&pool).await;
    make_admin(&pool, admin).await;
    let ctx = cli_ctx(&app);
    let (mut io, printed, answers) = scripted();
    let cmd = Command::Enroll {
        username: username.clone(),
    };
    let (result, claim) = tokio::join!(
        run(&ctx, cmd, &mut io),
        phone_claims(&app, printed, answers, "Pixel 9", |code| vec![format!(
            "{} {}",
            &code[..3],
            &code[3..]
        )]),
    );
    result.expect("enrolment succeeds");
    let out = io.out.join("\n");
    assert!(
        out.contains(&format!(
            "A device calling itself 'Pixel 9' wants to become {username}'s key"
        )),
        "{out}"
    );
    assert!(
        !out.contains(claim["claim_secret"].as_str().unwrap()),
        "never prints tokens beyond the setup link"
    );

    let (state, via): (String, String) =
        sqlx::query_as("SELECT state, enrolled_via FROM approval_devices WHERE user_id = $1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((state.as_str(), via.as_str()), ("active", "cli"));
    let actor: String = sqlx::query_scalar(
        "SELECT actor_kind FROM security_events
          WHERE subject_id = $1 AND event_type = 'authenticator_added'",
    )
    .bind(user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(actor, "cli");
    let audit: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT actor_id, action FROM system_audit_log WHERE target_id = $1 ORDER BY created_at",
    )
    .bind(user)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(audit.iter().all(|(a, _)| *a == SYSTEM_USER_ID), "{audit:?}");
    assert!(audit.iter().any(|(_, a)| a == "cli.enroll"), "{audit:?}");
    assert!(
        audit
            .iter()
            .any(|(_, a)| a == "security.authenticator_added"),
        "{audit:?}"
    );

    vc_server::auth::timers::run_once(&app.state).await.unwrap();
    for recipient in [user, admin] {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM security_notices
              WHERE recipient_id = $1 AND subject_id = $2 AND event_type = 'authenticator_added'",
        )
        .bind(recipient)
        .bind(user)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(n, 1, "notice for {recipient}");
    }
}

#[sqlx::test]
async fn cli_works_on_admin_accounts(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (admin, username) = create_test_user(&pool).await;
    make_admin(&pool, admin).await;
    let ctx = cli_ctx(&app);
    let (mut io, printed, answers) = scripted();
    let (result, _) = tokio::join!(
        run(&ctx, Command::Enroll { username }, &mut io),
        phone_claims(&app, printed, answers, "Admin phone", |code| vec![
            code.to_string()
        ]),
    );
    result.expect("enrolment succeeds");
    let state: String = sqlx::query_scalar("SELECT state FROM approval_devices WHERE user_id = $1")
        .bind(admin)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "active");
}

#[sqlx::test]
async fn wrong_code_three_times_fails(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, username) = create_test_user(&pool).await;
    let ctx = cli_ctx(&app);
    let (mut io, printed, answers) = scripted();
    let (result, claim) = tokio::join!(
        run(&ctx, Command::Enroll { username }, &mut io),
        phone_claims(&app, printed, answers, "Pixel 9", |code| {
            let wrong = if code == "000000" { "111111" } else { "000000" };
            vec![wrong.to_string(); 3]
        }),
    );
    assert!(result.is_err(), "three wrong codes fail the enrolment");
    let out = io.out.join("\n");
    assert!(
        out.contains("2 tries left") && out.contains("1 try left"),
        "{out}"
    );
    let state: String = sqlx::query_scalar("SELECT state FROM enrollments WHERE id = $1")
        .bind(Uuid::parse_str(claim["enrollment_id"].as_str().unwrap()).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "failed");
    let devices: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM approval_devices WHERE user_id = $1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(devices, 0);
}

#[sqlx::test]
async fn cli_removes_devices_and_ends_sessions(pool: PgPool) {
    use tokio_tungstenite::tungstenite::Message;
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (user, username) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let mut ws = ws_connect(&server, &dev.access_token).await;
    next_ws_event(&mut ws, "ready", Duration::from_secs(5)).await;
    let ctx = cli_ctx(&app);

    let (mut io, _printed, _answers) = scripted();
    run(
        &ctx,
        Command::DevicesList {
            username: username.clone(),
        },
        &mut io,
    )
    .await
    .unwrap();
    assert!(io.out.join("\n").contains(&dev.row_id.to_string()));

    run(
        &ctx,
        Command::DevicesRemove {
            username: username.clone(),
            device: dev.row_id,
        },
        &mut io,
    )
    .await
    .unwrap();
    let err = next_ws_event(&mut ws, "error", Duration::from_secs(2)).await;
    assert_eq!(err["code"], "SESSION_REVOKED");
    let closed = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Close(_))) | None => return,
                Some(Ok(_)) => {}
                Some(Err(_)) => return,
            }
        }
    })
    .await;
    assert!(
        closed.is_ok(),
        "the device's socket closes through the listener"
    );
    let (state, reason): (String, String) =
        sqlx::query_as("SELECT state, revoke_reason FROM approval_devices WHERE id = $1")
            .bind(dev.row_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((state.as_str(), reason.as_str()), ("revoked", "cli"));
    let by: Value = sqlx::query_scalar(
        "SELECT details FROM security_events
          WHERE subject_id = $1 AND event_type = 'authenticator_removed' AND actor_kind = 'cli'",
    )
    .bind(user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(by["by"], "cli");

    // Sessions: list, end one, end all.
    let (one, _) = create_session_with_token(&pool, &app.config, user).await;
    let (two, _) = create_session_with_token(&pool, &app.config, user).await;
    run(
        &ctx,
        Command::SessionsList {
            username: username.clone(),
        },
        &mut io,
    )
    .await
    .unwrap();
    let listed = io.out.join("\n");
    assert!(listed.contains(&one.to_string()) && listed.contains(&two.to_string()));
    run(
        &ctx,
        Command::SessionsEnd {
            username: username.clone(),
            session: Some(one),
        },
        &mut io,
    )
    .await
    .unwrap();
    let ended = |sid: Uuid| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, Option<String>>("SELECT end_reason FROM sessions WHERE id = $1")
                .bind(sid)
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };
    assert_eq!(ended(one).await.as_deref(), Some("cli"));
    assert_eq!(ended(two).await, None);
    run(
        &ctx,
        Command::SessionsEnd {
            username,
            session: None,
        },
        &mut io,
    )
    .await
    .unwrap();
    assert_eq!(ended(two).await.as_deref(), Some("cli"));
    let audited: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM system_audit_log
          WHERE target_id = $1 AND actor_id = $2 AND action LIKE 'cli.%'",
    )
    .bind(user)
    .bind(SYSTEM_USER_ID)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audited, 5, "every command is audited");
}
