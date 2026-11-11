//! Process shutdown: SIGTERM stops the server gracefully.
//!
//! Spawns the built `vc-server` binary against this test's database and the
//! test Valkey, then sends SIGTERM like a container runtime does.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use sqlx::PgPool;
use vc_server::config::Config;

/// The log line of the graceful shutdown path.
const SHUTDOWN_LINE: &str = "Received shutdown signal";

/// `DATABASE_URL` pointing at the database `pool` is connected to.
fn database_url_for(pool: &PgPool) -> String {
    let base = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for tests");
    let mut url = reqwest::Url::parse(&base).expect("DATABASE_URL is a URL");
    let db = pool
        .connect_options()
        .get_database()
        .expect("test pool names its database")
        .to_string();
    url.set_path(&db);
    url.to_string()
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind a free port")
        .local_addr()
        .expect("local addr")
        .port()
}

#[cfg(unix)]
#[sqlx::test]
async fn sigterm_stops_server_within_five_seconds(pool: PgPool) {
    let test_config = Config::default_for_test();
    let port = free_port();
    let workdir = std::env::temp_dir();
    let mut child = Command::new(env!("CARGO_BIN_EXE_vc-server"))
        // No `.env` from the repository: run outside it with an explicit env.
        .current_dir(&workdir)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("DATABASE_URL", database_url_for(&pool))
        .env("REDIS_URL", &test_config.redis_url)
        .env("JWT_PRIVATE_KEY", &test_config.jwt_private_key)
        .env("JWT_PUBLIC_KEY", &test_config.jwt_public_key)
        .env("PUBLIC_URL", "https://chat.example.com")
        .env("BIND_ADDRESS", format!("127.0.0.1:{port}"))
        .env("RUST_LOG", "vc_server=info")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn vc-server");

    // Collect both output streams on threads.
    let (log_tx, log_rx) = std::sync::mpsc::channel::<String>();
    for stream in [
        Box::new(child.stdout.take().expect("stdout")) as Box<dyn std::io::Read + Send>,
        Box::new(child.stderr.take().expect("stderr")),
    ] {
        let tx = log_tx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                let _ = tx.send(line);
            }
        });
    }
    drop(log_tx);

    // Wait until the server answers.
    let http = reqwest::Client::new();
    let started = Instant::now();
    loop {
        if let Ok(resp) = http
            .get(format!("http://127.0.0.1:{port}/health"))
            .send()
            .await
        {
            if resp.status().is_success() {
                break;
            }
        }
        if let Some(status) = child.try_wait().expect("poll child") {
            let logs: Vec<String> = log_rx.try_iter().collect();
            panic!("server exited early with {status}: {logs:#?}");
        }
        assert!(
            started.elapsed() < Duration::from_mins(1),
            "server did not become healthy in time"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let pid = child.id().to_string();
    let killed = Command::new("kill")
        .args(["-TERM", &pid])
        .status()
        .expect("run kill");
    assert!(killed.success(), "kill -TERM failed");

    let sent = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll child") {
            break status;
        }
        if sent.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            let _ = child.wait();
            let logs: Vec<String> = log_rx.try_iter().collect();
            panic!("server still running 5 s after SIGTERM: {logs:#?}");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let elapsed = sent.elapsed();

    // Give the reader threads a moment to drain the pipes.
    let mut logs = Vec::new();
    while let Ok(line) = log_rx.recv_timeout(Duration::from_millis(500)) {
        logs.push(line);
    }
    assert!(
        status.success(),
        "server did not exit cleanly after SIGTERM ({status}): {logs:#?}"
    );
    assert!(
        logs.iter().any(|l| l.contains(SHUTDOWN_LINE)),
        "shutdown line missing: {logs:#?}"
    );
    assert!(elapsed < Duration::from_secs(5), "took {elapsed:?}");
}
