//! WebSocket Connection Manager
//!
//! Owns the desktop's realtime connection: token-aware (re)connects with
//! backoff, a ping/silence watchdog and one event path to the webview.
//!
//! - Before every attempt the current access token is read from the [`SessionManager`]; a token
//!   that expires within 60 s is refreshed first. A handshake rejected with HTTP 401/403 (or a
//!   server close with code 1008) triggers one refresh and an immediate retry; if a fresh token is
//!   rejected as well, `ws:auth_rejected` is emitted once and the loop backs off without further
//!   refreshes. An ended session stops the loop.
//! - Every inbound text frame is forwarded unchanged as `ws:server_event` (parsed JSON), so the
//!   webview handles native and browser events with the same code and unknown event types are never
//!   dropped.
//! - Status events: `ws:connecting`, `ws:connected`, `ws:reconnecting` (attempt number, emitted
//!   when the connection is lost or an attempt fails) and `ws:disconnected` (the loop ended by
//!   itself, e.g. the session is gone). An explicit [`WebSocketManager::disconnect`] emits nothing.

use std::sync::{Arc, Once};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, watch, Notify};
use tokio::task::JoinHandle;
use tokio::time::{interval_at, sleep, sleep_until, timeout, Instant, MissedTickBehavior};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, tungstenite};
use tracing::{debug, error, info, warn};
use vc_common::protocol::PcType;

use crate::session::{AuthInfo, SessionError, SessionManager};

/// Refresh the access token before connecting when it expires within this.
const REFRESH_MARGIN: Duration = Duration::from_mins(1);
/// Give up on a TCP/TLS/WebSocket handshake after this long.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
/// Outgoing messages buffered for the socket task.
const OUTBOUND_QUEUE: usize = 100;

/// Default `pc_type` for backward compatibility with old servers.
const fn default_pc_type() -> PcType {
    PcType::Publisher
}

/// Client events sent to the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientEvent {
    Ping,
    Subscribe {
        channel_id: String,
    },
    Unsubscribe {
        channel_id: String,
    },
    Typing {
        channel_id: String,
    },
    StopTyping {
        channel_id: String,
    },
    VoiceJoin {
        channel_id: String,
    },
    VoiceLeave {
        channel_id: String,
    },
    VoicePublisherOffer {
        channel_id: String,
        sdp: String,
    },
    VoiceSubscriberAnswer {
        channel_id: String,
        sdp: String,
    },
    VoiceIceCandidate {
        channel_id: String,
        candidate: String,
        #[serde(default = "default_pc_type")]
        pc_type: PcType,
    },
    VoiceMute {
        channel_id: String,
    },
    VoiceUnmute {
        channel_id: String,
    },
    SetActivity {
        activity: Option<Value>,
    },
    SetCustomStatus {
        custom_status: Option<Value>,
    },
}

/// A message queued for the server.
#[derive(Debug, Clone)]
pub enum Outbound {
    /// A typed event from Rust code (voice signalling, typing, …).
    Event(ClientEvent),
    /// A JSON object from the webview, already checked by [`validate_raw`].
    Raw(String),
}

impl Outbound {
    fn into_text(self) -> Option<String> {
        match self {
            Self::Event(event) => serde_json::to_string(&event)
                .map_err(|e| error!("Failed to serialize client event: {}", e))
                .ok(),
            Self::Raw(text) => Some(text),
        }
    }
}

/// Connection status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting { attempt: u32 },
}

/// Watchdog timings.
#[derive(Debug, Clone, Copy)]
pub struct WsTimings {
    /// Send `ClientEvent::Ping` this often while connected.
    pub ping_every: Duration,
    /// Drop and reconnect when no frame arrived for this long.
    pub silence_limit: Duration,
}

impl Default for WsTimings {
    fn default() -> Self {
        Self {
            ping_every: Duration::from_secs(20),
            silence_limit: Duration::from_secs(45),
        }
    }
}

/// Where status and server events go: the webview in the app, a recorder in tests.
pub trait WsEvents: Send + Sync + 'static {
    fn emit(&self, event: &str, payload: Value);
}

impl WsEvents for AppHandle {
    fn emit(&self, event: &str, payload: Value) {
        if let Err(e) = Emitter::emit(self, event, payload) {
            warn!(error = %e, event, "Could not deliver WebSocket event to the webview");
        }
    }
}

/// Install the process-wide rustls crypto provider (idempotent). Both `ring`
/// and `aws-lc-rs` are compiled into this crate's graph, so rustls cannot
/// choose one on its own and a `wss://` handshake would panic without this.
pub fn install_crypto_provider() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        // Err only when another provider was installed first: that one is used.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Delay before retry number `attempt` (1-based): exponential from 1 s,
/// capped at 30 s, with ±20 % jitter (`jitter` in `-0.5..=0.5`). Same curve as
/// `nextBackoffMs` in `client/src/stores/websocket/connection.ts`.
pub fn next_backoff(attempt: u32, jitter: f64) -> Duration {
    const BASE_MS: u64 = 1_000;
    const MAX_MS: u64 = 30_000;
    const SPREAD: f64 = 0.4;
    let exponent = attempt.max(1) - 1;
    let base = BASE_MS
        .saturating_mul(1u64.checked_shl(exponent).unwrap_or(u64::MAX))
        .min(MAX_MS);
    let factor = jitter.clamp(-0.5, 0.5).mul_add(SPREAD, 1.0);
    // `base` ≤ 30 000 and factor in 0.8..=1.2: exact in f64, never negative.
    let millis = (base as f64 * factor).round() as u64;
    Duration::from_millis(millis)
}

/// The server refused the handshake for an auth reason: 401 (token) or 403
/// (C11: e.g. a pending password change with header auth). Handled like a
/// 1008 close.
pub fn is_auth_rejection(err: &tungstenite::Error) -> bool {
    matches!(err, tungstenite::Error::Http(response) if matches!(response.status().as_u16(), 401 | 403))
}

/// A server close that means "auth problem" (C11: 1008); 1011/1013 and the
/// rest are ordinary drops that reconnect with backoff.
pub fn close_needs_refresh(code: CloseCode) -> bool {
    code == CloseCode::Policy
}

/// Check a message from the webview: a JSON object with a string `type`.
pub fn validate_raw(message: &str) -> Result<String, String> {
    let value: Value =
        serde_json::from_str(message).map_err(|e| format!("Invalid JSON message: {e}"))?;
    match value.get("type") {
        Some(Value::String(_)) if value.is_object() => Ok(message.to_string()),
        _ => Err("Message must be a JSON object with a string \"type\"".to_string()),
    }
}

/// WebSocket manager state.
pub struct WebSocketManager {
    tx: mpsc::Sender<Outbound>,
    status: watch::Receiver<ConnectionStatus>,
    wake: Arc<Notify>,
    task: JoinHandle<()>,
}

impl WebSocketManager {
    /// Start the connection loop for the current session.
    pub fn connect(
        app: AppHandle,
        session: Arc<SessionManager>,
        timings: WsTimings,
    ) -> Result<Self, String> {
        if session.auth_info().is_none() {
            return Err("Not authenticated".to_string());
        }
        // The crypto provider is installed at app setup (lib.rs).
        Ok(Self::start(Arc::new(app), session, timings, None))
    }

    /// Spawn the loop. `ws_url` overrides the URL derived from the session.
    pub(crate) fn start(
        events: Arc<dyn WsEvents>,
        session: Arc<SessionManager>,
        timings: WsTimings,
        ws_url: Option<String>,
    ) -> Self {
        let (tx, rx) = mpsc::channel(OUTBOUND_QUEUE);
        let (status_tx, status) = watch::channel(ConnectionStatus::Connecting);
        let wake = Arc::new(Notify::new());
        let conn = Connection {
            events,
            session,
            timings,
            ws_url,
            outbound: rx,
            status: status_tx,
            wake: wake.clone(),
        };
        let task = tokio::spawn(conn.run());
        Self {
            tx,
            status,
            wake,
            task,
        }
    }

    /// Queue an event for the server. Fails while not connected (like the
    /// browser socket), so nothing stale is sent after a reconnect.
    pub async fn send(&self, event: ClientEvent) -> Result<(), String> {
        self.queue(Outbound::Event(event))
    }

    /// Queue a JSON message from the webview (see [`validate_raw`]).
    pub fn send_raw(&self, message: &str) -> Result<(), String> {
        self.queue(Outbound::Raw(validate_raw(message)?))
    }

    fn queue(&self, item: Outbound) -> Result<(), String> {
        if *self.status.borrow() != ConnectionStatus::Connected {
            return Err("WebSocket not connected".to_string());
        }
        self.tx.try_send(item).map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => "WebSocket send queue is full".to_string(),
            mpsc::error::TrySendError::Closed(_) => "WebSocket not connected".to_string(),
        })
    }

    /// Get the current connection status.
    pub fn status(&self) -> ConnectionStatus {
        self.status.borrow().clone()
    }

    /// Resolves once the first attempt has an outcome: `Ok` when connected,
    /// `Err` when it failed (the loop keeps retrying) or the loop ended.
    pub fn first_outcome(&self) -> impl std::future::Future<Output = Result<(), String>> + 'static {
        let mut status = self.status.clone();
        async move {
            let outcome = status
                .wait_for(|s| *s != ConnectionStatus::Connecting)
                .await
                .map(|s| s.clone());
            match outcome {
                Ok(ConnectionStatus::Connected) => Ok(()),
                Ok(ConnectionStatus::Reconnecting { .. }) => {
                    Err("WebSocket connection failed; retrying".to_string())
                }
                _ => Err("WebSocket connection ended".to_string()),
            }
        }
    }

    /// Skip the current backoff wait and try to connect now.
    pub fn reconnect_now(&self) {
        self.wake.notify_waiters();
    }

    /// Stop the loop and drop the socket. Emits no event: the caller knows.
    pub fn disconnect(&self) {
        self.task.abort();
    }
}

impl Drop for WebSocketManager {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// How one connected session ended.
enum Ended {
    /// Lost (error, close, silence): reconnect with backoff.
    Lost,
    /// Server closed with 1008: refresh the token, then reconnect.
    AuthProblem,
    /// The manager is gone: stop without reporting anything.
    Shutdown,
}

/// Where a streak of auth rejections stands; reset when a connection ends
/// normally.
#[derive(Clone, Copy)]
enum AuthRetry {
    /// No rejection yet: the next one refreshes and retries at once.
    Fresh,
    /// Refreshed once for this streak.
    Refreshed,
    /// A fresh token was rejected too and `ws:auth_rejected` was emitted.
    Reported,
}

/// What to do before the next attempt.
enum Next {
    Backoff,
    Immediately,
    Stop,
}

/// The connection loop's state.
struct Connection {
    events: Arc<dyn WsEvents>,
    session: Arc<SessionManager>,
    timings: WsTimings,
    ws_url: Option<String>,
    outbound: mpsc::Receiver<Outbound>,
    status: watch::Sender<ConnectionStatus>,
    wake: Arc<Notify>,
}

impl Connection {
    fn set_status(&self, status: ConnectionStatus) {
        let (name, payload) = match &status {
            ConnectionStatus::Disconnected => ("ws:disconnected", Value::Null),
            ConnectionStatus::Connecting => ("ws:connecting", Value::Null),
            ConnectionStatus::Connected => ("ws:connected", Value::Null),
            ConnectionStatus::Reconnecting { attempt } => {
                ("ws:reconnecting", Value::from(*attempt))
            }
        };
        self.status.send_replace(status);
        self.events.emit(name, payload);
    }

    async fn run(mut self) {
        let mut attempt = 0u32;
        let mut auth = AuthRetry::Fresh;
        self.set_status(ConnectionStatus::Connecting);

        loop {
            // `true` once this round's failure was reported as ws:reconnecting.
            let mut reported = false;
            let next = match self.attempt().await {
                Attempt::Connected(ws) => {
                    let ended = self.serve(*ws).await;
                    if matches!(ended, Ended::Shutdown) {
                        return;
                    }
                    // The live connection is gone: say so before refreshing or
                    // retrying, so the webview recovers (re-subscribes) on the
                    // next ws:connected and nothing is queued for a dead socket.
                    // The backoff counts from 1 again after a live connection.
                    attempt = 1;
                    self.set_status(ConnectionStatus::Reconnecting { attempt });
                    reported = true;
                    match ended {
                        Ended::AuthProblem => self.after_rejection(&mut auth).await,
                        Ended::Lost | Ended::Shutdown => {
                            auth = AuthRetry::Fresh;
                            Next::Backoff
                        }
                    }
                }
                Attempt::Failed => Next::Backoff,
                Attempt::Rejected => self.after_rejection(&mut auth).await,
                Attempt::NoSession => Next::Stop,
            };

            match next {
                Next::Stop => {
                    info!("WebSocket loop stopped: no session");
                    self.set_status(ConnectionStatus::Disconnected);
                    return;
                }
                Next::Immediately => {}
                Next::Backoff => {
                    if !reported {
                        attempt = attempt.saturating_add(1);
                        self.set_status(ConnectionStatus::Reconnecting { attempt });
                    }
                    let delay = next_backoff(attempt, rand::random::<f64>() - 0.5);
                    info!("Reconnecting in {:?} (attempt {})", delay, attempt);
                    tokio::select! {
                        () = sleep(delay) => {}
                        () = self.wake.notified() => info!("Reconnect requested"),
                    }
                }
            }
        }
    }

    /// After an auth rejection (handshake 401/403 or a 1008 close). The first
    /// one refreshes the token and retries at once. If the server still
    /// rejects a fresh token, a new token will not help (e.g. a password
    /// change is required): report `ws:auth_rejected` once and keep backing
    /// off without refreshing; the regular expiry check still refreshes when
    /// the token actually runs out.
    async fn after_rejection(&self, auth: &mut AuthRetry) -> Next {
        match auth {
            AuthRetry::Fresh => {}
            AuthRetry::Refreshed => {
                warn!("WebSocket rejected a freshly refreshed token");
                self.events.emit("ws:auth_rejected", Value::Null);
                *auth = AuthRetry::Reported;
                return Next::Backoff;
            }
            AuthRetry::Reported => return Next::Backoff,
        }
        match self.session.refresh().await {
            Ok(_) => {
                *auth = AuthRetry::Refreshed;
                Next::Immediately
            }
            // The session manager published the expiry (auth:session_expired).
            Err(SessionError::Expired) => Next::Stop,
            Err(e) => {
                warn!(error = %e, "Token refresh after WebSocket rejection failed");
                Next::Backoff
            }
        }
    }

    /// The token to connect with, refreshed first when it is about to expire.
    async fn fresh_auth(&self) -> Result<AuthInfo, Attempt> {
        let info = self.session.auth_info().ok_or(Attempt::NoSession)?;
        if !expires_soon(&info) {
            return Ok(info);
        }
        match self.session.refresh().await {
            Ok(info) => Ok(info),
            Err(SessionError::Expired) => Err(Attempt::NoSession),
            Err(e) => {
                warn!(error = %e, "Token refresh before WebSocket connect failed");
                Err(Attempt::Failed)
            }
        }
    }

    async fn attempt(&self) -> Attempt {
        let info = match self.fresh_auth().await {
            Ok(info) => info,
            Err(outcome) => return outcome,
        };
        let url = self
            .ws_url
            .clone()
            .unwrap_or_else(|| build_ws_url(&info.server_url));
        info!("Connecting to WebSocket: {}", url);
        let request = match build_ws_request(&url, &info.access_token) {
            Ok(request) => request,
            Err(e) => {
                error!("Failed to build WebSocket request: {}", e);
                return Attempt::Failed;
            }
        };
        match timeout(HANDSHAKE_TIMEOUT, connect_async(request)).await {
            Ok(Ok((ws, _))) => Attempt::Connected(Box::new(ws)),
            Ok(Err(e)) if is_auth_rejection(&e) => {
                info!("WebSocket handshake rejected the token");
                Attempt::Rejected
            }
            Ok(Err(e)) => {
                warn!("Failed to connect: {}", e);
                Attempt::Failed
            }
            Err(_) => {
                warn!("WebSocket handshake timed out");
                Attempt::Failed
            }
        }
    }

    /// Pump frames both ways until the connection ends.
    async fn serve(&mut self, ws: WsStream) -> Ended {
        info!("WebSocket connected");
        // Drop anything queued for an older connection.
        while self.outbound.try_recv().is_ok() {}
        self.set_status(ConnectionStatus::Connected);

        let (mut write, mut read) = ws.split();
        let mut ping = interval_at(
            Instant::now() + self.timings.ping_every,
            self.timings.ping_every,
        );
        ping.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut last_frame = Instant::now();

        loop {
            tokio::select! {
                msg = read.next() => {
                    last_frame = Instant::now();
                    match msg {
                        Some(Ok(Message::Text(text))) => self.forward(&text),
                        Some(Ok(Message::Ping(data))) => {
                            if let Err(e) = write.send(Message::Pong(data)).await {
                                warn!("Failed to send pong: {}", e);
                                return Ended::Lost;
                            }
                        }
                        Some(Ok(Message::Close(frame))) => {
                            info!("Server closed connection: {:?}", frame);
                            return match frame {
                                Some(f) if close_needs_refresh(f.code) => Ended::AuthProblem,
                                _ => Ended::Lost,
                            };
                        }
                        Some(Ok(_)) => {}
                        Some(Err(e)) => {
                            warn!("WebSocket error: {}", e);
                            return Ended::Lost;
                        }
                        None => {
                            info!("WebSocket stream ended");
                            return Ended::Lost;
                        }
                    }
                }
                item = self.outbound.recv() => {
                    // `None`: every sender (the manager) is gone.
                    let Some(item) = item else { return Ended::Shutdown };
                    let Some(text) = item.into_text() else { continue };
                    debug!("Sending WebSocket message ({} bytes)", text.len());
                    if let Err(e) = write.send(Message::Text(text.into())).await {
                        warn!("Failed to send message: {}", e);
                        return Ended::Lost;
                    }
                }
                _ = ping.tick() => {
                    let text = Outbound::Event(ClientEvent::Ping).into_text().unwrap_or_default();
                    if let Err(e) = write.send(Message::Text(text.into())).await {
                        warn!("Failed to send ping: {}", e);
                        return Ended::Lost;
                    }
                }
                () = sleep_until(last_frame + self.timings.silence_limit) => {
                    warn!("No frame for {:?}, dropping connection", self.timings.silence_limit);
                    return Ended::Lost;
                }
            }
        }
    }

    /// Forward one server frame to the webview as `ws:server_event`.
    fn forward(&self, text: &str) {
        match serde_json::from_str::<Value>(text) {
            Ok(event) => self.events.emit("ws:server_event", event),
            Err(e) => warn!("Failed to parse server message: {}", e),
        }
    }
}

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Outcome of one connection attempt.
enum Attempt {
    Connected(Box<WsStream>),
    /// Transient failure: back off and retry.
    Failed,
    /// HTTP 401 from the handshake.
    Rejected,
    /// There is no session (signed out or expired): stop.
    NoSession,
}

fn expires_soon(info: &AuthInfo) -> bool {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    let margin_ms = u64::try_from(REFRESH_MARGIN.as_millis()).unwrap_or(u64::MAX);
    info.expires_at_ms <= now_ms.saturating_add(margin_ms)
}

/// Build the WebSocket URL (without token — token goes in protocol header).
fn build_ws_url(server_url: &str) -> String {
    let base = server_url
        .replace("http://", "ws://")
        .replace("https://", "wss://");
    format!("{}/ws", base.trim_end_matches('/'))
}

/// Build a WebSocket handshake request with the JWT in the Sec-WebSocket-Protocol header.
/// This avoids exposing the token in URL query parameters (which get logged by proxies).
fn build_ws_request(
    ws_url: &str,
    token: &str,
) -> Result<tungstenite::http::Request<()>, tungstenite::error::Error> {
    let mut request = ws_url.into_client_request()?;
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        format!("access_token.{token}, access_token")
            .parse()
            .map_err(|_| {
                tungstenite::error::Error::Url(tungstenite::error::UrlError::NoPathOrQuery)
            })?,
    );
    Ok(request)
}

#[cfg(test)]
// The handshake callbacks must return tungstenite's large `ErrorResponse`.
#[allow(clippy::result_large_err)]
mod tests {
    use std::sync::Mutex as StdMutex;

    use serde_json::json;
    use tokio::net::TcpListener;
    use tokio::sync::{mpsc, Notify, RwLock};
    use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
    use tokio_tungstenite::tungstenite::http::StatusCode;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::session::store::MemoryTokenStore;
    use crate::AuthState;

    /// Records every emitted event instead of sending it to a webview.
    #[derive(Default)]
    struct Recorder {
        events: StdMutex<Vec<(String, Value)>>,
        changed: Notify,
    }

    impl WsEvents for Recorder {
        fn emit(&self, event: &str, payload: Value) {
            self.events
                .lock()
                .unwrap()
                .push((event.to_string(), payload));
            self.changed.notify_waiters();
        }
    }

    impl Recorder {
        fn names(&self) -> Vec<String> {
            self.events
                .lock()
                .unwrap()
                .iter()
                .map(|(n, _)| n.clone())
                .collect()
        }

        /// Wait until `name` was recorded `count` times.
        async fn wait_count(&self, name: &str, count: usize) {
            loop {
                let notified = self.changed.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if self.names().iter().filter(|n| *n == name).count() >= count {
                    return;
                }
                notified.await;
            }
        }

        /// Wait until an event matching `pred` was recorded; returns its payload.
        async fn wait_for(&self, pred: impl Fn(&str, &Value) -> bool) -> Value {
            loop {
                let notified = self.changed.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if let Some((_, p)) = self.events.lock().unwrap().iter().find(|(n, p)| pred(n, p)) {
                    return p.clone();
                }
                notified.await;
            }
        }
    }

    fn tokens(access: &str, refresh: &str) -> Value {
        json!({
            "access_token": access,
            "refresh_token": refresh,
            "expires_in": 900,
            "token_type": "Bearer",
            "setup_required": false
        })
    }

    /// A signed-in session whose access token is `access`; `/auth/refresh`
    /// (if mounted by the caller) rotates it.
    async fn session_with(server: &MockServer, access: &str) -> Arc<SessionManager> {
        Mock::given(method("POST"))
            .and(path("/auth/login"))
            .respond_with(ResponseTemplate::new(200).set_body_json(tokens(access, "rt-1")))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path("/auth/me"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "0192f0c0-0000-7000-8000-000000000001",
                "username": "alice",
                "display_name": "Alice",
                "email": null,
                "avatar_url": null,
                "status": "online",
                "mfa_enabled": false
            })))
            .mount(server)
            .await;
        let session = Arc::new(SessionManager::new(
            reqwest::Client::new(),
            Arc::new(MemoryTokenStore::new()),
            Arc::new(RwLock::new(AuthState::default())),
        ));
        session
            .login(&server.uri(), "alice", "pw", None)
            .await
            .unwrap();
        session
    }

    fn token_of(req: &Request) -> String {
        req.headers()
            .get("Sec-WebSocket-Protocol")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .find_map(|p| p.strip_prefix("access_token."))
            .unwrap_or_default()
            .to_string()
    }

    fn accept_protocol(mut resp: Response) -> Response {
        resp.headers_mut()
            .insert("Sec-WebSocket-Protocol", "access_token".parse().unwrap());
        resp
    }

    async fn within<T>(fut: impl std::future::Future<Output = T>) -> T {
        tokio::time::timeout(Duration::from_secs(10), fut)
            .await
            .expect("timed out")
    }

    #[test]
    fn next_backoff_caps_at_30s() {
        assert_eq!(next_backoff(10, 0.0), Duration::from_secs(30));
        assert_eq!(next_backoff(1, 0.0), Duration::from_secs(1));
        assert_eq!(next_backoff(2, 0.0), Duration::from_secs(2));
        // ±20 % jitter like nextBackoffMs (jitter in -0.5..0.5).
        assert_eq!(next_backoff(1, 0.5), Duration::from_millis(1200));
        assert_eq!(next_backoff(1, -0.5), Duration::from_millis(800));
    }

    #[tokio::test]
    async fn wss_scheme_is_supported() {
        let err = connect_async("wss://127.0.0.1:9/ws")
            .await
            .expect_err("nothing listens on port 9");
        assert!(
            !matches!(
                err,
                tungstenite::Error::Url(tungstenite::error::UrlError::TlsFeatureNotEnabled)
            ),
            "wss:// must be supported, got {err:?}"
        );
    }

    #[tokio::test]
    async fn wss_handshake_has_a_crypto_provider() {
        // A peer that accepts TCP and hangs up: the TLS setup must run (and
        // fail with an error) instead of panicking for lack of a provider.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let _ = listener.accept().await;
        });
        install_crypto_provider();
        let result = connect_async(format!("wss://127.0.0.1:{port}/ws")).await;
        assert!(result.is_err());
    }

    #[test]
    fn ws_send_rejects_non_object_json() {
        assert!(validate_raw("[]").is_err());
        assert!(validate_raw("{\"x\":1}").is_err());
        assert!(validate_raw("{\"type\":1}").is_err());
        assert!(validate_raw("not json").is_err());
        assert!(validate_raw(r#"{"type":"typing","channel_id":"c"}"#).is_ok());
    }

    #[test]
    fn auth_rejection_detected_for_http_401_and_403() {
        let response = |status: u16| {
            tungstenite::Error::Http(Box::new(
                tungstenite::http::Response::builder()
                    .status(status)
                    .body(None)
                    .unwrap(),
            ))
        };
        assert!(is_auth_rejection(&response(401)));
        assert!(is_auth_rejection(&response(403)));
        assert!(!is_auth_rejection(&response(500)));
        assert!(!is_auth_rejection(&tungstenite::Error::ConnectionClosed));
    }

    #[test]
    fn close_code_1008_is_an_auth_problem() {
        use tungstenite::protocol::frame::coding::CloseCode;
        assert!(close_needs_refresh(CloseCode::Policy));
        assert!(!close_needs_refresh(CloseCode::Error)); // 1011
        assert!(!close_needs_refresh(CloseCode::Again)); // 1013
        assert!(!close_needs_refresh(CloseCode::Normal));
    }

    #[tokio::test]
    async fn reconnect_uses_refreshed_token() {
        let http = MockServer::start().await;
        let session = session_with(&http, "old").await;
        Mock::given(method("POST"))
            .and(path("/auth/refresh"))
            .respond_with(ResponseTemplate::new(200).set_body_json(tokens("new", "rt-2")))
            .expect(1)
            .mount(&http)
            .await;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (seen_tx, mut seen_rx) = mpsc::unbounded_channel::<String>();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let seen_tx = seen_tx.clone();
                tokio::spawn(async move {
                    let cb = |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
                        let token = token_of(req);
                        let _ = seen_tx.send(token.clone());
                        if token == "new" {
                            Ok(accept_protocol(resp))
                        } else {
                            let mut err = ErrorResponse::new(None);
                            *err.status_mut() = StatusCode::UNAUTHORIZED;
                            Err(err)
                        }
                    };
                    if let Ok(mut ws) = tokio_tungstenite::accept_hdr_async(stream, cb).await {
                        while ws.next().await.is_some() {}
                    }
                });
            }
        });

        let events = Arc::new(Recorder::default());
        let _manager = WebSocketManager::start(
            events.clone(),
            session.clone(),
            WsTimings::default(),
            Some(format!("ws://127.0.0.1:{port}/ws")),
        );

        within(events.wait_for(|n, _| n == "ws:connected")).await;
        assert_eq!(within(seen_rx.recv()).await.as_deref(), Some("old"));
        assert_eq!(within(seen_rx.recv()).await.as_deref(), Some("new"));
        assert_eq!(session.auth_info().unwrap().access_token, "new");
        // Retried at once after the refresh: no backoff in between.
        assert!(!events.names().iter().any(|n| n == "ws:reconnecting"));
    }

    #[tokio::test]
    async fn policy_close_refreshes_and_recovers() {
        use tungstenite::protocol::frame::coding::CloseCode;
        use tungstenite::protocol::CloseFrame;

        let http = MockServer::start().await;
        let session = session_with(&http, "old").await;
        Mock::given(method("POST"))
            .and(path("/auth/refresh"))
            .respond_with(ResponseTemplate::new(200).set_body_json(tokens("new", "rt-2")))
            .expect(1)
            .mount(&http)
            .await;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (seen_tx, mut seen_rx) = mpsc::unbounded_channel::<String>();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let seen_tx = seen_tx.clone();
                tokio::spawn(async move {
                    let token = Arc::new(StdMutex::new(String::new()));
                    let seen = token.clone();
                    let cb =
                        move |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
                            *seen.lock().unwrap() = token_of(req);
                            Ok(accept_protocol(resp))
                        };
                    let Ok(mut ws) = tokio_tungstenite::accept_hdr_async(stream, cb).await else {
                        return;
                    };
                    let token = token.lock().unwrap().clone();
                    let _ = seen_tx.send(token.clone());
                    if token == "old" {
                        let _ = ws
                            .close(Some(CloseFrame {
                                code: CloseCode::Policy,
                                reason: "".into(),
                            }))
                            .await;
                    }
                    while ws.next().await.is_some() {}
                });
            }
        });

        let events = Arc::new(Recorder::default());
        let manager = WebSocketManager::start(
            events.clone(),
            session.clone(),
            WsTimings::default(),
            Some(format!("ws://127.0.0.1:{port}/ws")),
        );

        assert_eq!(within(seen_rx.recv()).await.as_deref(), Some("old"));
        assert_eq!(within(seen_rx.recv()).await.as_deref(), Some("new"));
        within(events.wait_count("ws:connected", 2)).await;
        let names: Vec<String> = events
            .names()
            .into_iter()
            .filter(|n| n != "ws:server_event")
            .collect();
        assert_eq!(
            names,
            [
                "ws:connecting",
                "ws:connected",
                "ws:reconnecting",
                "ws:connected"
            ]
        );
        assert_eq!(manager.status(), ConnectionStatus::Connected);
    }

    /// A server that rejects every handshake with `status`: exactly one
    /// refresh across several attempts, then `ws:auth_rejected`.
    async fn persistent_rejection_refreshes_once(status: StatusCode) {
        let http = MockServer::start().await;
        let session = session_with(&http, "old").await;
        Mock::given(method("POST"))
            .and(path("/auth/refresh"))
            .respond_with(ResponseTemplate::new(200).set_body_json(tokens("new", "rt-2")))
            .expect(1)
            .mount(&http)
            .await;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (seen_tx, mut seen_rx) = mpsc::unbounded_channel::<()>();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let _ = seen_tx.send(());
                tokio::spawn(async move {
                    let cb = move |_: &Request, _: Response| -> Result<Response, ErrorResponse> {
                        let mut err = ErrorResponse::new(None);
                        *err.status_mut() = status;
                        Err(err)
                    };
                    let _ = tokio_tungstenite::accept_hdr_async(stream, cb).await;
                });
            }
        });

        let events = Arc::new(Recorder::default());
        let manager = WebSocketManager::start(
            events.clone(),
            session.clone(),
            WsTimings::default(),
            Some(format!("ws://127.0.0.1:{port}/ws")),
        );

        for _ in 0..4 {
            within(seen_rx.recv()).await;
        }
        drop(manager);
        let names = events.names();
        assert_eq!(
            names.iter().filter(|n| *n == "ws:auth_rejected").count(),
            1,
            "{names:?}"
        );
        assert!(!names.iter().any(|n| n == "ws:connected"), "{names:?}");
        assert_eq!(session.auth_info().unwrap().access_token, "new");
        // `.expect(1)` on /auth/refresh is verified when `http` drops.
    }

    #[tokio::test]
    async fn persistent_401_refreshes_once() {
        persistent_rejection_refreshes_once(StatusCode::UNAUTHORIZED).await;
    }

    #[tokio::test]
    async fn persistent_403_refreshes_once() {
        persistent_rejection_refreshes_once(StatusCode::FORBIDDEN).await;
    }

    #[tokio::test]
    async fn silent_server_triggers_watchdog_reconnect() {
        let http = MockServer::start().await;
        let session = session_with(&http, "tok").await;
        tokio::time::pause();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (conn_tx, mut conn_rx) = mpsc::unbounded_channel::<tokio::time::Instant>();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let conn_tx = conn_tx.clone();
                tokio::spawn(async move {
                    let cb = |_: &Request, resp: Response| -> Result<Response, ErrorResponse> {
                        Ok(accept_protocol(resp))
                    };
                    if let Ok(ws) = tokio_tungstenite::accept_hdr_async(stream, cb).await {
                        let _ = conn_tx.send(tokio::time::Instant::now());
                        // Never send anything, never read: a dead peer.
                        std::future::pending::<()>().await;
                        drop(ws);
                    }
                });
            }
        });

        let timings = WsTimings {
            ping_every: Duration::from_secs(20),
            silence_limit: Duration::from_secs(45),
        };
        let events = Arc::new(Recorder::default());
        let _manager = WebSocketManager::start(
            events.clone(),
            session,
            timings,
            Some(format!("ws://127.0.0.1:{port}/ws")),
        );

        let first = conn_rx.recv().await.unwrap();
        let second = conn_rx.recv().await.unwrap();
        assert!(
            second.duration_since(first) >= timings.silence_limit,
            "reconnected after {:?}",
            second.duration_since(first)
        );
        let names = events.names();
        assert!(names.iter().any(|n| n == "ws:reconnecting"), "{names:?}");
    }

    #[tokio::test]
    async fn unknown_event_types_are_forwarded() {
        let http = MockServer::start().await;
        let session = session_with(&http, "tok").await;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let cb = |_: &Request, resp: Response| -> Result<Response, ErrorResponse> {
                Ok(accept_protocol(resp))
            };
            let mut ws = tokio_tungstenite::accept_hdr_async(stream, cb)
                .await
                .unwrap();
            ws.send(Message::Text(
                json!({"type": "forum_post_created", "post": {"id": "p1"}})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
            while ws.next().await.is_some() {}
        });

        let events = Arc::new(Recorder::default());
        let _manager = WebSocketManager::start(
            events.clone(),
            session,
            WsTimings::default(),
            Some(format!("ws://127.0.0.1:{port}/ws")),
        );

        let payload = within(events.wait_for(|n, _| n == "ws:server_event")).await;
        assert_eq!(payload["type"], "forum_post_created");
        assert_eq!(payload["post"]["id"], "p1");
    }

    #[tokio::test]
    async fn expired_session_stops_the_loop() {
        let http = MockServer::start().await;
        let session = session_with(&http, "old").await;
        Mock::given(method("POST"))
            .and(path("/auth/refresh"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&http)
            .await;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                tokio::spawn(async move {
                    let cb = |_: &Request, _: Response| -> Result<Response, ErrorResponse> {
                        let mut err = ErrorResponse::new(None);
                        *err.status_mut() = StatusCode::UNAUTHORIZED;
                        Err(err)
                    };
                    let _ = tokio_tungstenite::accept_hdr_async(stream, cb).await;
                });
            }
        });

        let events = Arc::new(Recorder::default());
        let manager = WebSocketManager::start(
            events.clone(),
            session.clone(),
            WsTimings::default(),
            Some(format!("ws://127.0.0.1:{port}/ws")),
        );

        within(events.wait_for(|n, _| n == "ws:disconnected")).await;
        assert!(session.auth_info().is_none());
        assert!(matches!(manager.status(), ConnectionStatus::Disconnected));
        // The session manager publishes the expiry itself (auth:session_expired).
        assert!(!events.names().iter().any(|n| n == "auth:session_expired"));
    }
}
