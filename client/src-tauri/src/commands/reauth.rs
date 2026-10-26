//! OIDC re-authentication for sensitive actions (desktop).
//!
//! Separate from the login and identity-link flows: it asks the server for a
//! provider URL (bearer-authenticated, with a loopback `redirect_uri`), opens
//! the system browser and waits for the server's redirect to `/callback`. The
//! callback carries no tokens; `reauth=elevated` is a hint only and the caller
//! confirms elevation with the server afterwards.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{command, State};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tracing::info;
use url::Url;

use crate::AppState;

const CALLBACK_PATH: &str = "/callback";
const CALLBACK_TIMEOUT: Duration = Duration::from_mins(1);
/// A connection that sends nothing for this long is dropped.
const READ_DEADLINE: Duration = Duration::from_secs(2);
const MAX_REQUEST_LINE: usize = 4096;

/// One re-authentication attempt: a persistent cancelled flag (checked before
/// every side effect) plus a wake-up for the waiting flow.
#[derive(Default)]
pub struct Attempt {
    cancelled: AtomicBool,
    wake: Notify,
}

impl Attempt {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.wake.notify_one();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// `Err` once cancelled; call right before each side effect.
    fn check(&self) -> Result<(), String> {
        if self.is_cancelled() {
            Err(CANCELLED.to_string())
        } else {
            Ok(())
        }
    }
}

const CANCELLED: &str = "REAUTH_CANCELLED";
/// How many unmatched / finished ids are remembered (bounded on purpose).
const REMEMBERED_IDS: usize = 32;

/// Running attempts by id. A cancel for an unknown id is remembered only if it
/// might still register (IPC gap) and only in a small bounded list; a cancel
/// for a finished id is ignored.
#[derive(Default)]
struct Registry {
    running: HashMap<String, Arc<Attempt>>,
    early_cancels: VecDeque<String>,
    finished: VecDeque<String>,
}

fn remember(list: &mut VecDeque<String>, id: &str) {
    if list.len() >= REMEMBERED_IDS {
        list.pop_front();
    }
    list.push_back(id.to_string());
}

impl Registry {
    fn register(&mut self, id: &str) -> Result<Arc<Attempt>, String> {
        if self.running.contains_key(id) {
            return Err("REAUTH_FAILED".to_string());
        }
        let attempt = Arc::new(Attempt::default());
        if let Some(pos) = self.early_cancels.iter().position(|e| e == id) {
            self.early_cancels.remove(pos);
            attempt.cancel();
        }
        self.running.insert(id.to_string(), Arc::clone(&attempt));
        Ok(attempt)
    }

    fn cancel(&mut self, id: &str) {
        if let Some(attempt) = self.running.get(id) {
            attempt.cancel();
        } else if !self.finished.iter().any(|f| f == id) {
            remember(&mut self.early_cancels, id);
        }
    }

    fn finish(&mut self, id: &str) {
        if self.running.remove(id).is_some() {
            remember(&mut self.finished, id);
        }
    }
}

fn registry() -> std::sync::MutexGuard<'static, Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Run `flow` unless the attempt is cancelled. Cancellation is checked first
/// and polled first, so a ready flow cannot win against a pending cancel; a
/// cancelled flow is dropped where it stands (no browser, no listener). The
/// flag is checked once more on the way out, so a cancel that lands while the
/// flow itself is being polled still wins: a cancelled attempt never succeeds.
async fn run_cancellable<T>(
    attempt: &Attempt,
    flow: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    attempt.check()?;
    let result = tokio::select! {
        biased;
        () = attempt.wake.notified() => Err(CANCELLED.to_string()),
        result = flow => result,
    };
    attempt.check()?;
    result
}

/// Cancel a running (or about to start) re-authentication attempt.
#[command]
pub fn oidc_reauth_cancel(attempt_id: String) {
    registry().cancel(&attempt_id);
}

/// Result of the loopback callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReauthCallback {
    /// `?reauth=elevated&expires_at=...`
    Elevated { expires_at: Option<String> },
    /// `?reauth_error=CODE`
    Error(String),
}

/// Error string for a failed server call: `HTTP_<status>` or, when the server
/// named a code, `HTTP_<status>:<code>` so the status is never lost.
fn http_error(status: u16, code: Option<&str>) -> String {
    match code {
        Some(code) => format!("HTTP_{status}:{code}"),
        None => format!("HTTP_{status}"),
    }
}

/// What the webview gets back on success.
#[derive(Debug, Serialize)]
pub struct ReauthOutcome {
    pub expires_at: Option<String>,
}

#[derive(Deserialize)]
struct AuthorizeResponse {
    url: String,
}

/// Parse the request target of a loopback request. Returns `None` unless the
/// raw path is exactly `/callback` (checked before any normalisation) and the
/// query carries a recognised result.
pub fn parse_callback_target(target: &str) -> Option<ReauthCallback> {
    let (raw_path, query) = target.split_once('?')?;
    if raw_path != CALLBACK_PATH {
        return None;
    }
    let parsed = Url::parse(&format!("http://127.0.0.1/?{query}")).ok()?;
    let mut elevated = false;
    let mut expires_at = None;
    let mut error = None;
    for (k, v) in parsed.query_pairs() {
        match k.as_ref() {
            "reauth" if v == "elevated" => elevated = true,
            "expires_at" => expires_at = Some(v.to_string()),
            "reauth_error" => error = Some(v.to_string()),
            _ => {}
        }
    }
    if let Some(code) = error {
        Some(ReauthCallback::Error(code))
    } else if elevated {
        Some(ReauthCallback::Elevated { expires_at })
    } else {
        None
    }
}

/// Extract the target of a complete `GET <target> HTTP/1.x` request line.
fn request_target(line: &str) -> Option<&str> {
    let mut parts = line.split(' ');
    let (method, target, version) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || method != "GET" || !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
        return None;
    }
    target.starts_with('/').then_some(target)
}

/// Read the request line of one connection within `deadline`; `None` when it
/// is incomplete, too long, or the peer is idle.
async fn read_request_line(
    stream: &mut tokio::net::TcpStream,
    deadline: Duration,
) -> Option<String> {
    let read = async {
        let mut buf = Vec::with_capacity(512);
        let mut chunk = [0u8; 512];
        while buf.len() < MAX_REQUEST_LINE {
            let n = stream.read(&mut chunk).await.ok()?;
            if n == 0 {
                return None;
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(end) = buf.windows(2).position(|w| w == b"\r\n") {
                return String::from_utf8(buf[..end].to_vec()).ok();
            }
        }
        None
    };
    tokio::time::timeout(deadline, read).await.ok().flatten()
}

const PAGE_HEADERS: &str = "Content-Type: text/html\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n";

async fn respond(stream: &mut tokio::net::TcpStream, ok: bool) {
    let html = if ok {
        "<html><body><h2>Confirmed</h2><p>You can close this window and return to the app.</p></body></html>"
    } else {
        "<html><body><h2>Could not confirm</h2><p>You can close this window and return to the app.</p></body></html>"
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\n{PAGE_HEADERS}Content-Length: {}\r\n\r\n{html}",
        html.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.flush().await;
}

/// Wait for the callback on `listener`. Everything that is not a complete
/// `GET /callback?...` with a recognised result is answered and ignored, so a
/// stray or idle connection cannot end or stall the flow.
pub async fn wait_for_callback(
    listener: &TcpListener,
    timeout: Duration,
    read_deadline: Duration,
    attempt: &Attempt,
) -> Result<ReauthCallback, String> {
    let accept = async {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                continue;
            };
            let line = read_request_line(&mut stream, read_deadline).await;
            let result = line
                .as_deref()
                .and_then(request_target)
                .and_then(parse_callback_target);
            if let Some(result) = result {
                // Cancelled meanwhile: the page must not claim a confirmation
                // and the result is dropped.
                if attempt.is_cancelled() {
                    respond(&mut stream, false).await;
                    return Err(CANCELLED.to_string());
                }
                let ok = matches!(result, ReauthCallback::Elevated { .. });
                respond(&mut stream, ok).await;
                return Ok(result);
            }
            let _ = stream
                .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                .await;
            let _ = stream.flush().await;
        }
    };
    let result = tokio::time::timeout(timeout, accept)
        .await
        .map_err(|_| "REAUTH_TIMEOUT".to_string())??;
    attempt.check()?;
    Ok(result)
}

/// Re-authenticate with an OIDC provider (desktop).
///
/// Errors are short codes for the webview's shared error mapping: the server's
/// `HTTP_<status>` / `HTTP_<status>:<code>` for a failed server call, `REAUTH_TIMEOUT`, or
/// `REAUTH_FAILED`.
#[command]
pub async fn oidc_reauth(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    server_url: String,
    provider_id: String,
    reason: Option<String>,
    mfa_code: Option<String>,
    attempt_id: String,
) -> Result<ReauthOutcome, String> {
    let attempt = registry().register(&attempt_id)?;
    let result = run_cancellable(
        &attempt,
        reauth_flow(
            &attempt,
            state,
            app_handle,
            server_url,
            provider_id,
            reason,
            mfa_code,
        ),
    )
    .await;
    registry().finish(&attempt_id);
    result
}

async fn reauth_flow(
    attempt: &Attempt,
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    server_url: String,
    provider_id: String,
    reason: Option<String>,
    mfa_code: Option<String>,
) -> Result<ReauthOutcome, String> {
    let access_token = {
        let auth = state.auth.read().await;
        auth.access_token.clone()
    }
    .ok_or("REAUTH_SESSION_INVALID")?;

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| "REAUTH_FAILED".to_string())?;
    let port = listener
        .local_addr()
        .map_err(|_| "REAUTH_FAILED".to_string())?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}{CALLBACK_PATH}");

    let mut endpoint =
        Url::parse(server_url.trim_end_matches('/')).map_err(|_| "REAUTH_FAILED".to_string())?;
    endpoint
        .path_segments_mut()
        .map_err(|()| "REAUTH_FAILED".to_string())?
        .extend(&["auth", "reauth", "oidc", "authorize", &provider_id]);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| "REAUTH_FAILED".to_string())?;
    let response = client
        .post(endpoint)
        .bearer_auth(&access_token)
        .json(&serde_json::json!({
            "redirect_uri": redirect_uri,
            "reason": reason,
            "mfa_code": mfa_code,
        }))
        .send()
        .await
        .map_err(|_| "REAUTH_FAILED".to_string())?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let code = response
            .json::<serde_json::Value>()
            .await
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string));
        return Err(http_error(status, code.as_deref()));
    }
    let AuthorizeResponse { url } = response
        .json()
        .await
        .map_err(|_| "REAUTH_FAILED".to_string())?;

    // Cancelled while the server was answering: do not open the browser.
    attempt.check()?;

    #[allow(deprecated)] // tauri-plugin-shell::open is deprecated in favor of tauri-plugin-opener
    {
        use tauri_plugin_shell::ShellExt;
        app_handle
            .shell()
            .open(&url, None)
            .map_err(|_| "REAUTH_FAILED".to_string())?;
    }
    info!("Opened browser for re-authentication, waiting for callback");

    attempt.check()?;
    match wait_for_callback(&listener, CALLBACK_TIMEOUT, READ_DEADLINE, attempt).await? {
        ReauthCallback::Elevated { expires_at } => Ok(ReauthOutcome { expires_at }),
        ReauthCallback::Error(code) => Err(code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_elevated() {
        assert_eq!(
            parse_callback_target("/callback?reauth=elevated&expires_at=2026-10-25T10%3A00%3A00Z"),
            Some(ReauthCallback::Elevated {
                expires_at: Some("2026-10-25T10:00:00Z".into())
            })
        );
    }

    #[test]
    fn parses_error_code() {
        assert_eq!(
            parse_callback_target("/callback?reauth_error=REAUTH_STALE"),
            Some(ReauthCallback::Error("REAUTH_STALE".into()))
        );
    }

    #[test]
    fn error_wins_over_elevated() {
        assert_eq!(
            parse_callback_target("/callback?reauth=elevated&reauth_error=REAUTH_FAILED"),
            Some(ReauthCallback::Error("REAUTH_FAILED".into()))
        );
    }

    #[test]
    fn rejects_other_paths() {
        assert_eq!(parse_callback_target("/favicon.ico"), None);
        assert_eq!(
            parse_callback_target("/callback/extra?reauth=elevated"),
            None
        );
        assert_eq!(parse_callback_target("/callbackx?reauth=elevated"), None);
        assert_eq!(
            parse_callback_target("/x/../callback?reauth=elevated"),
            None
        );
        assert_eq!(parse_callback_target("//callback?reauth=elevated"), None);
        assert_eq!(parse_callback_target("/"), None);
    }

    #[test]
    fn no_recognised_result_is_ignored() {
        assert_eq!(parse_callback_target("/callback"), None);
        assert_eq!(parse_callback_target("/callback?reauth=nope"), None);
    }

    #[test]
    fn only_http_1_0_and_1_1_are_accepted() {
        assert!(request_target("GET /callback?a=1 HTTP/1.0").is_some());
        assert_eq!(request_target("GET /callback HTTP/1."), None);
        assert_eq!(request_target("GET /callback HTTP/1.garbage"), None);
        assert_eq!(request_target("GET /callback HTTP/1.2"), None);
    }

    #[tokio::test]
    async fn pre_cancelled_ready_flow_never_runs() {
        let mut reg = Registry::default();
        reg.cancel("a1"); // arrives before the attempt registers
        let attempt = reg.register("a1").unwrap();
        let ran = AtomicBool::new(false);
        let result = run_cancellable(&attempt, async {
            ran.store(true, Ordering::SeqCst);
            Ok::<_, String>(1)
        })
        .await;
        assert_eq!(result, Err(CANCELLED.to_string()));
        assert!(!ran.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn cancel_during_flow_drops_it() {
        let mut reg = Registry::default();
        let attempt = reg.register("a2").unwrap();
        let waker = Arc::clone(&attempt);
        let canceller = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            waker.cancel();
        });
        let result: Result<(), String> = run_cancellable(&attempt, std::future::pending()).await;
        assert_eq!(result, Err(CANCELLED.to_string()));
        canceller.await.unwrap();
    }

    #[test]
    fn side_effect_check_fails_after_cancel() {
        let mut reg = Registry::default();
        let attempt = reg.register("a3").unwrap();
        assert!(attempt.check().is_ok());
        reg.cancel("a3");
        assert_eq!(attempt.check(), Err(CANCELLED.to_string()));
    }

    #[test]
    fn late_cancel_after_finish_leaves_no_entry() {
        let mut reg = Registry::default();
        reg.register("a4").unwrap();
        reg.finish("a4");
        reg.cancel("a4");
        assert!(reg.running.is_empty());
        assert!(reg.early_cancels.is_empty());
    }

    #[test]
    fn unknown_cancels_are_bounded() {
        let mut reg = Registry::default();
        for i in 0..1000 {
            reg.cancel(&format!("unknown-{i}"));
        }
        assert_eq!(reg.early_cancels.len(), REMEMBERED_IDS);
        for i in 0..1000 {
            let id = format!("done-{i}");
            reg.register(&id).unwrap();
            reg.finish(&id);
        }
        assert_eq!(reg.finished.len(), REMEMBERED_IDS);
        assert!(reg.running.is_empty());
    }

    #[test]
    fn duplicate_attempt_id_is_rejected() {
        let mut reg = Registry::default();
        let first = reg.register("dup").unwrap();
        assert!(reg.register("dup").is_err());
        // The first attempt keeps its own state and can still be cancelled.
        reg.cancel("dup");
        assert!(first.is_cancelled());
        reg.finish("dup");
        assert!(reg.register("dup").is_ok());
    }

    #[test]
    fn http_errors_keep_status_and_code() {
        assert_eq!(
            http_error(503, Some("service_unavailable")),
            "HTTP_503:service_unavailable"
        );
        assert_eq!(http_error(429, None), "HTTP_429");
    }

    #[tokio::test]
    async fn cancel_during_the_flow_poll_never_yields_its_result() {
        // The cancel lands while the flow itself is being polled and the flow
        // completes in that same poll: the result must still be discarded.
        let attempt = Attempt::default();
        let result = run_cancellable(&attempt, async {
            attempt.cancel();
            Ok::<_, String>(7)
        })
        .await;
        assert_eq!(result, Err(CANCELLED.to_string()));
    }

    #[tokio::test]
    async fn cancelled_attempt_ignores_an_arriving_callback() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let attempt = Attempt::default();
        attempt.cancel();
        let client = tokio::spawn(async move { get(port, "/callback?reauth=elevated").await });
        let result = wait_for_callback(&listener, LONG, LONG, &attempt).await;
        assert_eq!(result, Err(CANCELLED.to_string()));
        let page = client.await.unwrap();
        assert!(!page.contains("<h2>Confirmed</h2>"));
    }

    #[tokio::test]
    async fn finished_flow_wins_without_cancel() {
        let attempt = Attempt::default();
        let result = run_cancellable(&attempt, async { Ok::<_, String>(7) }).await;
        assert_eq!(result, Ok(7));
    }

    #[test]
    fn request_line_must_be_a_complete_get() {
        assert_eq!(
            request_target("GET /callback?a=1 HTTP/1.1"),
            Some("/callback?a=1")
        );
        assert_eq!(request_target("POST /callback HTTP/1.1"), None);
        assert_eq!(request_target("GET /callback"), None);
        assert_eq!(request_target("GET /callback HTTP/2"), None);
        assert_eq!(request_target("GET callback HTTP/1.1"), None);
        assert_eq!(request_target("GET /a b HTTP/1.1"), None);
    }

    async fn send(port: u16, raw: &str) -> String {
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        s.write_all(raw.as_bytes()).await.unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        out
    }

    async fn get(port: u16, target: &str) -> String {
        send(port, &format!("GET {target} HTTP/1.1\r\nHost: x\r\n\r\n")).await
    }

    const LONG: Duration = Duration::from_secs(5);
    const SHORT: Duration = Duration::from_millis(150);

    #[tokio::test]
    async fn listener_ignores_other_paths_and_sets_headers() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = tokio::spawn(async move {
            let other = get(port, "/favicon.ico").await;
            let ok = get(port, "/callback?reauth=elevated").await;
            (other, ok)
        });
        let result = wait_for_callback(&listener, LONG, LONG, &Attempt::default()).await;
        assert_eq!(result, Ok(ReauthCallback::Elevated { expires_at: None }));
        let (other, ok) = client.await.unwrap();
        assert!(other.starts_with("HTTP/1.1 204"));
        assert!(ok.contains("Cache-Control: no-store"));
        assert!(ok.contains("Referrer-Policy: no-referrer"));
    }

    #[tokio::test]
    async fn garbage_and_empty_callbacks_do_not_end_the_flow() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = tokio::spawn(async move {
            send(port, "garbage\r\n\r\n").await;
            get(port, "/callback").await;
            send(port, "POST /callback?reauth=elevated HTTP/1.1\r\n\r\n").await;
            get(port, "/callback?reauth_error=REAUTH_STALE").await;
        });
        let result = wait_for_callback(&listener, LONG, LONG, &Attempt::default()).await;
        assert_eq!(result, Ok(ReauthCallback::Error("REAUTH_STALE".into())));
        client.await.unwrap();
    }

    #[tokio::test]
    async fn idle_connection_does_not_stall_the_flow() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = tokio::spawn(async move {
            // Connects and sends nothing; keep it open past the read deadline.
            let idle = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            let ok = get(port, "/callback?reauth=elevated").await;
            drop(idle);
            ok
        });
        let result = wait_for_callback(&listener, LONG, SHORT, &Attempt::default()).await;
        assert_eq!(result, Ok(ReauthCallback::Elevated { expires_at: None }));
        client.await.unwrap();
    }

    #[tokio::test]
    async fn listener_times_out() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let result = wait_for_callback(
            &listener,
            Duration::from_millis(50),
            SHORT,
            &Attempt::default(),
        )
        .await;
        assert_eq!(result, Err("REAUTH_TIMEOUT".to_string()));
    }
}
