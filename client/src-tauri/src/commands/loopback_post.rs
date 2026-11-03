//! Loopback listener for the desktop passkey hand-off (C29 step 5).
//!
//! The app binds `127.0.0.1:0` (the literal address) **before** it starts a
//! flow and draws a per-flow `callback_path`. The server's hand-off page in
//! the system browser form-POSTs `flow_id` and `code` (or `error`) to
//! `http://127.0.0.1:P/callback/<callback_path>`. Only exactly that request,
//! with `Host: 127.0.0.1:P`, a form body within the size and time bounds and
//! the flow's own `flow_id`, gets 200 "You can close this page". Anything
//! else gets an empty 204: nothing is echoed, logged or shown, and the
//! listener keeps waiting until the flow's deadline. The callback path is
//! compared in constant time. Secrets (code, callback path) are never logged.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::Instant;
use zeroize::Zeroizing;

use super::oidc_loopback::{Acceptor, CallbackTimeout};

/// The listener never waits longer than a flow lives (10 minutes).
pub const FLOW_LISTEN_MAX: Duration = Duration::from_mins(10);
/// Request line, headers and body together.
pub const MAX_REQUEST_BYTES: usize = 8 * 1024;
/// Connections served at the same time (each bounded by its read deadline).
const MAX_CONNECTIONS: usize = 16;
/// Pause after a failed `accept()`.
const ACCEPT_BACKOFF: Duration = Duration::from_millis(50);

/// Where the hand-off page posts.
#[derive(Clone, PartialEq, Eq)]
pub struct LoopbackCallback {
    pub port: u16,
    /// 32 random bytes, b64u (43 characters); wiped when dropped.
    pub callback_path: Zeroizing<String>,
}

impl std::fmt::Debug for LoopbackCallback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoopbackCallback")
            .field("port", &self.port)
            .field("callback_path", &"[REDACTED]")
            .finish()
    }
}

/// What the browser handed back.
#[derive(Clone, PartialEq, Eq)]
pub enum HandBack {
    Code(String),
    Error(String),
}

impl std::fmt::Debug for HandBack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Code(_) => f.write_str("Code([REDACTED])"),
            Self::Error(e) => f.debug_tuple("Error").field(e).finish(),
        }
    }
}

/// Bind `127.0.0.1:0` and draw the per-flow callback path.
pub async fn bind() -> std::io::Result<(TcpListener, LoopbackCallback)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    Ok((
        listener,
        LoopbackCallback {
            port,
            callback_path: Zeroizing::new(super::oidc_loopback::new_secret()),
        },
    ))
}

const CLOSE_PAGE: &str =
    "<!doctype html><html><head><meta charset=\"utf-8\"><title>Kaiku</title></head>\
<body><p>You can close this page and return to Kaiku.</p></body></html>";

/// Read one request (head and body) within the size and time bounds.
async fn read_request(stream: &mut TcpStream, deadline: Duration) -> Option<Vec<u8>> {
    let read = async {
        let mut buf = Vec::with_capacity(1024);
        let mut chunk = [0u8; 1024];
        loop {
            if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = std::str::from_utf8(&buf[..end]).ok()?;
                let len = content_length(head)?;
                let total = end + 4 + len;
                if total > MAX_REQUEST_BYTES {
                    return None;
                }
                while buf.len() < total {
                    let n = stream.read(&mut chunk).await.ok()?;
                    if n == 0 {
                        return None;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                buf.truncate(total);
                return Some(buf);
            }
            if buf.len() >= MAX_REQUEST_BYTES {
                return None;
            }
            let n = stream.read(&mut chunk).await.ok()?;
            if n == 0 {
                return None;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
    };
    tokio::time::timeout(deadline, read).await.ok().flatten()
}

/// `Content-Length` of a head (`None` without one, or with chunked bodies).
fn content_length(head: &str) -> Option<usize> {
    let mut len = None;
    for line in head.split("\r\n").skip(1) {
        let (name, value) = line.split_once(':')?;
        let name = name.trim().to_ascii_lowercase();
        if name == "transfer-encoding" {
            return None;
        }
        if name == "content-length" {
            if len.is_some() {
                return None;
            }
            len = Some(value.trim().parse().ok()?);
        }
    }
    len
}

/// The single value of header `wanted`; `None` when it is missing, sent
/// twice, or a header line is malformed.
fn header<'a>(head: &'a str, wanted: &str) -> Option<&'a str> {
    let mut found = None;
    for line in head.split("\r\n").skip(1) {
        let (name, value) = line.split_once(':')?;
        if name.trim().eq_ignore_ascii_case(wanted) {
            if found.is_some() {
                return None;
            }
            found = Some(value.trim());
        }
    }
    found
}

/// The flow's hand-off, if `request` is exactly it (C29 step 5).
fn parse(request: &[u8], cb: &LoopbackCallback, flow_id: &str) -> Option<HandBack> {
    let end = request.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&request[..end]).ok()?;
    let body = &request[end + 4..];
    let line = head.split("\r\n").next()?;
    let target = line.strip_prefix("POST ")?.strip_suffix(" HTTP/1.1")?;
    let path = target.strip_prefix("/callback/")?;
    if !super::oidc_loopback::constant_time_eq(path, &cb.callback_path) {
        return None;
    }
    if header(head, "host")? != format!("127.0.0.1:{}", cb.port) {
        return None;
    }
    let ctype = header(head, "content-type")?;
    let media = ctype.split(';').next().unwrap_or("").trim();
    if !media.eq_ignore_ascii_case("application/x-www-form-urlencoded") {
        return None;
    }
    let (mut flow, mut code, mut error) = (None, None, None);
    for (k, v) in url::form_urlencoded::parse(body) {
        let slot = match k.as_ref() {
            "flow_id" => &mut flow,
            "code" => &mut code,
            "error" => &mut error,
            _ => return None,
        };
        if slot.replace(v.into_owned()).is_some() {
            return None;
        }
    }
    if flow.as_deref() != Some(flow_id) {
        return None;
    }
    match (code, error) {
        (Some(c), None) if !c.is_empty() => Some(HandBack::Code(c)),
        (None, Some(e)) if !e.is_empty() => Some(HandBack::Error(e)),
        _ => None,
    }
}

async fn reply(stream: &mut TcpStream, ok: bool) {
    let response = if ok {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\n\
             Referrer-Policy: no-referrer\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{CLOSE_PAGE}",
            CLOSE_PAGE.len()
        )
    } else {
        "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n".to_string()
    };
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.flush().await;
}

/// Wait for the flow's hand-off POST until `deadline` (capped at
/// [`FLOW_LISTEN_MAX`]). Up to [`MAX_CONNECTIONS`] connections are served at
/// once; a slow or idle one is dropped after `read_deadline`.
pub async fn wait_for_post(
    listener: &(impl Acceptor + Sync),
    cb: &LoopbackCallback,
    flow_id: &str,
    deadline: Instant,
    read_deadline: Duration,
) -> Result<HandBack, CallbackTimeout> {
    let deadline = deadline.min(Instant::now() + FLOW_LISTEN_MAX);
    // Each connection is served on its own task (bounded), so an idle or slow
    // one never delays the browser's POST (each still has `read_deadline`).
    let accept = async {
        let mut tasks: tokio::task::JoinSet<Option<HandBack>> = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                // At the cap, no accept: further connections wait in the
                // kernel's backlog until a slot frees (read deadline), so
                // the browser's one-shot POST is never refused.
                accepted = listener.accept_stream(), if tasks.len() < MAX_CONNECTIONS => {
                    let Ok(mut stream) = accepted else {
                        tokio::time::sleep(ACCEPT_BACKOFF).await;
                        continue;
                    };
                    let cb = cb.clone();
                    let flow_id = flow_id.to_string();
                    tasks.spawn(async move {
                        let request = read_request(&mut stream, read_deadline).await;
                        let result = request.as_deref().and_then(|r| parse(r, &cb, &flow_id));
                        reply(&mut stream, result.is_some()).await;
                        result
                    });
                }
                Some(done) = tasks.join_next(), if !tasks.is_empty() => {
                    if let Ok(Some(result)) = done {
                        tasks.abort_all();
                        return result;
                    }
                }
            }
        }
    };
    tokio::time::timeout_at(deadline, accept)
        .await
        .map_err(|_| CallbackTimeout)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLOW: &str = "0b9c2f6e-6a1e-4a43-9d0e-6d3f0a3c1b2a";
    const READ: Duration = Duration::from_millis(300);

    async fn raw(port: u16, request: &[u8]) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(request).await.unwrap();
        let mut out = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(2), s.read_to_end(&mut out)).await;
        String::from_utf8_lossy(&out).into_owned()
    }

    fn post(cb: &LoopbackCallback, path: &str, host: &str, ctype: &str, body: &str) -> Vec<u8> {
        let _ = cb;
        format!(
            "POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    fn good(cb: &LoopbackCallback, body: &str) -> Vec<u8> {
        post(
            cb,
            &format!("/callback/{}", cb.callback_path.as_str()),
            &format!("127.0.0.1:{}", cb.port),
            "application/x-www-form-urlencoded",
            body,
        )
    }

    fn spawn_wait(
        listener: TcpListener,
        cb: LoopbackCallback,
        deadline: Duration,
    ) -> tokio::task::JoinHandle<Result<HandBack, CallbackTimeout>> {
        tokio::spawn(async move {
            wait_for_post(&listener, &cb, FLOW, Instant::now() + deadline, READ).await
        })
    }

    #[tokio::test]
    async fn bind_uses_the_literal_loopback_address() {
        let (listener, cb) = bind().await.unwrap();
        let addr = listener.local_addr().unwrap();
        assert_eq!(addr.ip().to_string(), "127.0.0.1");
        assert_eq!(addr.port(), cb.port);
        assert_eq!(cb.callback_path.len(), 43);
        let (_, other) = bind().await.unwrap();
        assert_ne!(cb.callback_path, other.callback_path);
    }

    #[tokio::test]
    async fn listener_accepts_only_its_flow_post() {
        let (listener, cb) = bind().await.unwrap();
        let task = spawn_wait(listener, cb.clone(), Duration::from_secs(5));
        let reply = raw(
            cb.port,
            &good(&cb, &format!("flow_id={FLOW}&code=the-code")),
        )
        .await;
        assert!(reply.starts_with("HTTP/1.1 200 OK\r\n"), "{reply}");
        assert!(reply.contains("Content-Type: text/html; charset=utf-8"));
        assert!(reply.contains("Cache-Control: no-store"));
        assert!(reply.contains("You can close this page"));
        assert!(!reply.contains("the-code"), "nothing echoed");
        assert!(!reply.to_ascii_lowercase().contains("location:"));
        assert_eq!(task.await.unwrap(), Ok(HandBack::Code("the-code".into())));
    }

    #[tokio::test]
    async fn listener_answers_204_to_everything_else() {
        let (listener, cb) = bind().await.unwrap();
        let task = spawn_wait(listener, cb.clone(), Duration::from_secs(10));
        let host = format!("127.0.0.1:{}", cb.port);
        let path = format!("/callback/{}", cb.callback_path.as_str());
        let form = "application/x-www-form-urlencoded";
        let body = format!("flow_id={FLOW}&code=c");
        let mut other_path = cb.callback_path.as_str().to_owned();
        other_path.replace_range(
            0..1,
            if other_path.starts_with('A') {
                "B"
            } else {
                "A"
            },
        );
        let cases: Vec<Vec<u8>> = vec![
            format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n\r\n").into_bytes(),
            post(&cb, "/callback/nope", &host, form, &body),
            post(&cb, &format!("/callback/{other_path}"), &host, form, &body),
            post(&cb, &format!("{path}?x=1"), &host, form, &body),
            post(&cb, &path, &format!("localhost:{}", cb.port), form, &body),
            post(&cb, &path, &host, form, "flow_id=another&code=c"),
            post(&cb, &path, &host, form, &format!("flow_id={FLOW}")),
            post(
                &cb,
                &path,
                &host,
                form,
                &format!("flow_id={FLOW}&code=c&error=E"),
            ),
            post(&cb, &path, &host, form, ""),
            post(&cb, &path, &host, "text/plain", &body),
            post(
                &cb,
                &path,
                &host,
                form,
                &format!("flow_id={FLOW}&code={}", "x".repeat(MAX_REQUEST_BYTES)),
            ),
        ];
        for case in cases {
            let reply = raw(cb.port, &case).await;
            assert!(reply.starts_with("HTTP/1.1 204 No Content\r\n"), "{reply}");
            assert!(!reply.contains("flow_id") && !reply.contains("code="));
        }
        // A connection that never finishes its request does not stall it.
        let _idle = TcpStream::connect(("127.0.0.1", cb.port)).await.unwrap();
        let reply = raw(cb.port, &good(&cb, &body)).await;
        assert!(reply.starts_with("HTTP/1.1 200 OK"), "{reply}");
        assert_eq!(task.await.unwrap(), Ok(HandBack::Code("c".into())));
    }

    #[tokio::test]
    async fn idle_connections_never_delay_the_post() {
        // Review Argon M2: each connection is served on its own.
        let (listener, cb) = bind().await.unwrap();
        let cb2 = cb.clone();
        let task = tokio::spawn(async move {
            wait_for_post(
                &listener,
                &cb2,
                FLOW,
                Instant::now() + Duration::from_secs(20),
                Duration::from_secs(3),
            )
            .await
        });
        let mut idle = Vec::new();
        for _ in 0..5 {
            idle.push(TcpStream::connect(("127.0.0.1", cb.port)).await.unwrap());
        }
        let started = std::time::Instant::now();
        let reply = raw(cb.port, &good(&cb, &format!("flow_id={FLOW}&code=c"))).await;
        assert!(reply.starts_with("HTTP/1.1 200 OK"), "{reply}");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(task.await.unwrap(), Ok(HandBack::Code("c".into())));
        drop(idle);
    }

    #[tokio::test]
    async fn connections_over_the_cap_wait_for_a_slot() {
        // Review round 2 (Argon B R3): with every slot held by an idle
        // connection, the browser's one-shot POST waits in the backlog and
        // is served once a slot frees (read deadline), never refused.
        let (listener, cb) = bind().await.unwrap();
        let cb2 = cb.clone();
        let task = tokio::spawn(async move {
            wait_for_post(
                &listener,
                &cb2,
                FLOW,
                Instant::now() + Duration::from_secs(20),
                Duration::from_secs(1),
            )
            .await
        });
        let mut idle = Vec::new();
        for _ in 0..MAX_CONNECTIONS {
            idle.push(TcpStream::connect(("127.0.0.1", cb.port)).await.unwrap());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        let reply = raw(cb.port, &good(&cb, &format!("flow_id={FLOW}&code=c"))).await;
        assert!(reply.starts_with("HTTP/1.1 200 OK"), "{reply}");
        assert_eq!(task.await.unwrap(), Ok(HandBack::Code("c".into())));
        drop(idle);
    }

    #[tokio::test]
    async fn duplicate_host_or_content_type_is_refused() {
        // Review Argon m6.
        let (listener, cb) = bind().await.unwrap();
        let task = spawn_wait(listener, cb.clone(), Duration::from_secs(5));
        let body = format!("flow_id={FLOW}&code=c");
        let host = format!("127.0.0.1:{}", cb.port);
        let path = format!("/callback/{}", cb.callback_path.as_str());
        for extra in [
            format!("Host: {host}\r\n"),
            "Content-Type: text/plain\r\n".to_string(),
        ] {
            let req = format!(
                "POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/x-www-form-urlencoded\r\n{extra}Content-Length: {}\r\n\r\n{body}",
                body.len()
            );
            let reply = raw(cb.port, req.as_bytes()).await;
            assert!(reply.starts_with("HTTP/1.1 204"), "{reply}");
        }
        raw(cb.port, &good(&cb, &body)).await;
        assert_eq!(task.await.unwrap(), Ok(HandBack::Code("c".into())));
    }

    #[tokio::test]
    async fn error_post_is_handed_back() {
        let (listener, cb) = bind().await.unwrap();
        let task = spawn_wait(listener, cb.clone(), Duration::from_secs(5));
        raw(
            cb.port,
            &good(&cb, &format!("flow_id={FLOW}&error=PASSKEY_USER_MISMATCH")),
        )
        .await;
        assert_eq!(
            task.await.unwrap(),
            Ok(HandBack::Error("PASSKEY_USER_MISMATCH".into()))
        );
    }

    #[tokio::test]
    async fn listener_times_out() {
        let (listener, cb) = bind().await.unwrap();
        let task = spawn_wait(listener, cb, Duration::from_millis(200));
        assert_eq!(task.await.unwrap(), Err(CallbackTimeout));
    }

    #[test]
    fn debug_never_prints_secrets() {
        let cb = LoopbackCallback {
            port: 1,
            callback_path: Zeroizing::new("secret-path".into()),
        };
        assert!(!format!("{cb:?}").contains("secret-path"));
        assert!(!format!("{:?}", HandBack::Code("c0de".into())).contains("c0de"));
    }

    #[test]
    fn the_callback_path_is_wiped_when_dropped() {
        // Fable r2 R3: the path itself is zeroized, not only the copy the
        // start request gets.
        fn wiped_on_drop(_: &Zeroizing<String>) {}
        let cb = LoopbackCallback {
            port: 1,
            callback_path: Zeroizing::new("secret-path".into()),
        };
        wiped_on_drop(&cb.callback_path);
    }
}
