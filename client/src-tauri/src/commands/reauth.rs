//! OIDC re-authentication for sensitive actions (desktop).
//!
//! Separate from the login and identity-link flows: it asks the server for a
//! provider URL (bearer-authenticated, with a loopback `redirect_uri`), opens
//! the system browser and waits for the server's redirect to `/callback`. The
//! callback carries no tokens; `reauth=elevated` is a hint only and the caller
//! confirms elevation with the server afterwards.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{command, State};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tracing::info;
use url::Url;

use crate::AppState;

const CALLBACK_PATH: &str = "/callback";
const CALLBACK_TIMEOUT: Duration = Duration::from_mins(1);
/// A connection that sends nothing for this long is dropped.
const READ_DEADLINE: Duration = Duration::from_secs(2);
const MAX_REQUEST_LINE: usize = 4096;

/// Result of the loopback callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReauthCallback {
    /// `?reauth=elevated&expires_at=...`
    Elevated { expires_at: Option<String> },
    /// `?reauth_error=CODE`
    Error(String),
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
    if parts.next().is_some() || method != "GET" || !version.starts_with("HTTP/1.") {
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
                let ok = matches!(result, ReauthCallback::Elevated { .. });
                respond(&mut stream, ok).await;
                return result;
            }
            let _ = stream
                .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                .await;
            let _ = stream.flush().await;
        }
    };
    tokio::time::timeout(timeout, accept)
        .await
        .map_err(|_| "REAUTH_TIMEOUT".to_string())
}

/// Re-authenticate with an OIDC provider (desktop).
///
/// Errors are short codes for the webview's shared error mapping: the server's
/// error code, `HTTP_<status>` when it sent none, `REAUTH_TIMEOUT`, or
/// `REAUTH_FAILED`.
#[command]
pub async fn oidc_reauth(
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
        return Err(code.unwrap_or_else(|| format!("HTTP_{status}")));
    }
    let AuthorizeResponse { url } = response
        .json()
        .await
        .map_err(|_| "REAUTH_FAILED".to_string())?;

    #[allow(deprecated)] // tauri-plugin-shell::open is deprecated in favor of tauri-plugin-opener
    {
        use tauri_plugin_shell::ShellExt;
        app_handle
            .shell()
            .open(&url, None)
            .map_err(|_| "REAUTH_FAILED".to_string())?;
    }
    info!("Opened browser for re-authentication, waiting for callback");

    match wait_for_callback(&listener, CALLBACK_TIMEOUT, READ_DEADLINE).await? {
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
        let result = wait_for_callback(&listener, LONG, LONG).await;
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
        let result = wait_for_callback(&listener, LONG, LONG).await;
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
        let result = wait_for_callback(&listener, LONG, SHORT).await;
        assert_eq!(result, Ok(ReauthCallback::Elevated { expires_at: None }));
        client.await.unwrap();
    }

    #[tokio::test]
    async fn listener_times_out() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let result = wait_for_callback(&listener, Duration::from_millis(50), SHORT).await;
        assert_eq!(result, Err("REAUTH_TIMEOUT".to_string()));
    }
}
