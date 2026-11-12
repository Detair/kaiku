//! Process lifecycle: the production startup and SIGTERM paths.
//!
//! Spawns the built `vc-server` binary against this test's database and the
//! test Valkey, then sends SIGTERM like a container runtime does.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use fred::prelude::*;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;
use vc_server::config::Config;
use vc_server::permissions::GuildPermissions;

use super::helpers::{
    add_guild_member, create_guild_with_default_role, create_test_user, create_voice_channel,
    generate_access_token, WsClient,
};

/// The log line of the graceful shutdown path.
const SHUTDOWN_LINE: &str = "Received shutdown signal";

/// Valkey database for the spawned servers: their startup clears every
/// screen-share counter, which must not hit the in-process tests (database 0).
const SERVER_VALKEY_DB: u8 = 7;

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

fn server_redis_url(config: &Config) -> String {
    format!(
        "{}/{SERVER_VALKEY_DB}",
        config.redis_url.trim_end_matches('/')
    )
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind a free port")
        .local_addr()
        .expect("local addr")
        .port()
}

/// A spawned server process.
struct Server {
    child: Child,
    port: u16,
    logs: Receiver<String>,
}

impl Server {
    /// Start the binary and wait until `/health` answers.
    async fn start(pool: &PgPool) -> Self {
        let test_config = Config::default_for_test();
        let port = free_port();
        let mut child = Command::new(env!("CARGO_BIN_EXE_vc-server"))
            // No `.env` from the repository: run outside it with an explicit env.
            .current_dir(std::env::temp_dir())
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("DATABASE_URL", database_url_for(pool))
            .env("REDIS_URL", server_redis_url(&test_config))
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
        let (log_tx, logs) = std::sync::mpsc::channel::<String>();
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

        let mut server = Self { child, port, logs };
        let http = reqwest::Client::new();
        let started = Instant::now();
        loop {
            if let Ok(resp) = http
                .get(format!("http://127.0.0.1:{port}/health"))
                .send()
                .await
            {
                if resp.status().is_success() {
                    return server;
                }
            }
            if let Some(status) = server.child.try_wait().expect("poll child") {
                let logs: Vec<String> = server.logs.try_iter().collect();
                panic!("server exited early with {status}: {logs:#?}");
            }
            assert!(
                started.elapsed() < Duration::from_mins(1),
                "server did not become healthy in time"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    fn sigterm(&self) {
        let killed = Command::new("kill")
            .args(["-TERM", &self.child.id().to_string()])
            .status()
            .expect("run kill");
        assert!(killed.success(), "kill -TERM failed");
    }

    /// Wait for the process to exit within `bound`; returns the elapsed time
    /// and the collected log lines. Panics on a timeout or a failed exit.
    async fn exit_within(mut self, sent: Instant, bound: Duration) -> (Duration, Vec<String>) {
        let status = loop {
            if let Some(status) = self.child.try_wait().expect("poll child") {
                break status;
            }
            if sent.elapsed() > bound {
                let _ = self.child.kill();
                let _ = self.child.wait();
                let logs: Vec<String> = self.logs.try_iter().collect();
                panic!("server still running {bound:?} after SIGTERM: {logs:#?}");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        let elapsed = sent.elapsed();
        // Give the reader threads a moment to drain the pipes.
        let mut logs = Vec::new();
        while let Ok(line) = self.logs.recv_timeout(Duration::from_millis(500)) {
            logs.push(line);
        }
        assert!(
            status.success(),
            "server did not exit cleanly after SIGTERM ({status}): {logs:#?}"
        );
        (elapsed, logs)
    }

    async fn ws(&self, token: &str) -> WsClient {
        let (mut ws, _) =
            tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{}/ws", self.port))
                .await
                .expect("WebSocket connect");
        let auth = json!({ "type": "authenticate", "token": token }).to_string();
        ws.send(Message::Text(auth.into()))
            .await
            .expect("auth frame");
        next_of(&mut ws, "ready").await;
        ws
    }
}

async fn next_of(ws: &mut WsClient, event_type: &str) -> Value {
    let wait = async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(text))) => {
                    let value: Value = serde_json::from_str(&text).expect("JSON");
                    if value["type"] == event_type {
                        return value;
                    }
                }
                Some(Ok(_)) => {}
                other => panic!("socket ended waiting for {event_type}: {other:?}"),
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(15), wait)
        .await
        .unwrap_or_else(|_| panic!("no {event_type}"))
}

/// Read until the Close frame; returns the event types seen and the close
/// code.
async fn read_until_close(ws: &mut WsClient) -> (Vec<Value>, Option<CloseCode>) {
    let mut events = Vec::new();
    let wait = async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(text))) => {
                    events.push(serde_json::from_str(&text).expect("JSON"));
                }
                Some(Ok(Message::Close(frame))) => return frame.map(|f| f.code),
                Some(Ok(_)) => {}
                Some(Err(e)) => panic!("socket error before close: {e}"),
                None => return None,
            }
        }
    };
    let code = tokio::time::timeout(Duration::from_secs(10), wait)
        .await
        .expect("no close frame");
    (events, code)
}

#[cfg(unix)]
#[sqlx::test]
async fn sigterm_stops_server_within_five_seconds(pool: PgPool) {
    let server = Server::start(&pool).await;
    server.sigterm();
    let (elapsed, logs) = server
        .exit_within(Instant::now(), Duration::from_secs(5))
        .await;
    assert!(
        logs.iter().any(|l| l.contains(SHUTDOWN_LINE)),
        "shutdown line missing: {logs:#?}"
    );
    assert!(
        logs.iter().any(|l| l.contains("Ingestion workers drained")),
        "telemetry drain ran into its timeout: {logs:#?}"
    );
    assert!(elapsed < Duration::from_secs(5), "took {elapsed:?}");
}

/// Open, authenticated sockets in a voice room: on SIGTERM every socket gets
/// Close 1012 at once (telling the sockets is the first thing the shutdown
/// does), both leave the room through the normal path (sessions finalized),
/// and the process exits 0 within the bound. Whether a socket still hears
/// `voice_user_left` for the other before its own close frame is not
/// promised: the clients reconnect and rejoin (C32).
#[cfg(unix)]
#[sqlx::test]
async fn sigterm_closes_sockets_with_1012_and_ends_voice_sessions(pool: PgPool) {
    let config = Config::default_for_test();
    let (owner, _) = create_test_user(&pool).await;
    let (member, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(
        &pool,
        owner,
        GuildPermissions::VIEW_CHANNEL | GuildPermissions::VOICE_CONNECT,
    )
    .await;
    add_guild_member(&pool, guild, member).await;
    let voice = create_voice_channel(&pool, guild, "voice").await;

    let server = Server::start(&pool).await;
    let mut sockets = Vec::new();
    for user in [owner, member] {
        let mut ws = server.ws(&generate_access_token(&config, user)).await;
        ws.send(Message::Text(
            json!({ "type": "voice_join", "channel_id": voice })
                .to_string()
                .into(),
        ))
        .await
        .expect("voice_join");
        next_of(&mut ws, "voice_room_state").await;
        sockets.push(ws);
    }

    let sent = Instant::now();
    server.sigterm();
    let mut left = Vec::new();
    for ws in &mut sockets {
        let (events, code) = read_until_close(ws).await;
        assert_eq!(
            code,
            Some(CloseCode::Restart),
            "close code (events {events:?})"
        );
        left.extend(
            events
                .into_iter()
                .filter(|e| e["type"] == "voice_user_left")
                .map(|e| e["user_id"].as_str().unwrap_or_default().to_string()),
        );
    }
    assert!(left.len() <= 1, "voice_user_left seen: {left:?}");
    assert!(
        sent.elapsed() < Duration::from_secs(2),
        "close frames took {:?}",
        sent.elapsed()
    );
    let (elapsed, logs) = server.exit_within(sent, Duration::from_secs(8)).await;
    assert!(elapsed < Duration::from_secs(8), "took {elapsed:?}");
    assert!(
        logs.iter().any(|l| l.contains("WebSockets closed")),
        "sockets not drained: {logs:#?}"
    );
    // Both left through the normal leave path (session finalized).
    let finalized: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM connection_sessions WHERE channel_id = $1 AND user_id = ANY($2)",
    )
    .bind(voice)
    .bind([owner, member])
    .fetch_one(&pool)
    .await
    .expect("count sessions");
    assert_eq!(finalized, 2);
}

/// The production startup clears screen-share slot counters left by a
/// previous process (single node: rooms start empty).
#[cfg(unix)]
#[sqlx::test]
async fn startup_clears_stale_screen_share_counters(pool: PgPool) {
    let config = Config::default_for_test();
    let redis = vc_server::db::create_redis_client(&server_redis_url(&config))
        .await
        .expect("connect test Valkey");
    let stale = format!(
        "{}{}",
        vc_server::voice::screen_share::SCREEN_SHARE_KEY_PREFIX,
        Uuid::new_v4()
    );
    let _: () = redis
        .set(&stale, 2, None, None, false)
        .await
        .expect("seed counter");

    let server = Server::start(&pool).await;
    let exists: i64 = redis.exists(&stale).await.expect("exists");
    server.sigterm();
    let _ = server
        .exit_within(Instant::now(), Duration::from_secs(8))
        .await;
    redis.quit().await.expect("quit");
    assert_eq!(exists, 0, "stale screen-share counter survived startup");
}
