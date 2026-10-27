//! Loopback listener and secrets for the desktop OIDC flows.
//!
//! The desktop binds `127.0.0.1:0`, puts a random per-flow value in its return
//! address (`http://127.0.0.1:PORT/callback?flow=<value>`) and waits for the
//! server's redirect there. Only a complete `GET /callback?...` request line
//! (HTTP/1.0 or 1.1, raw path checked before any normalisation) that carries
//! exactly that per-flow value is accepted; everything else is answered with
//! 204 and ignored, so a stray or idle local connection can neither end nor
//! stall the flow. Login additionally sends an S256 challenge of a random
//! verifier and receives only a one-time code, redeemed with the verifier.

use std::collections::HashMap;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use url::Url;

pub const CALLBACK_PATH: &str = "/callback";
/// The user has this long to finish signing in at the provider.
pub const CALLBACK_TIMEOUT: Duration = Duration::from_mins(1);
/// A connection that has not sent its request line after this long is dropped.
pub const READ_DEADLINE: Duration = Duration::from_secs(2);
const MAX_REQUEST_LINE: usize = 4096;
/// Query key of the per-flow value in the return address.
const FLOW_PARAM: &str = "flow";
/// Pause after a failed `accept()` (e.g. out of file descriptors), so a
/// persistent error does not spin until the overall timeout.
const ACCEPT_BACKOFF: Duration = Duration::from_millis(50);

/// Source of loopback connections (the bound listener; a stub in tests).
pub trait Acceptor {
    fn accept_stream(&self)
        -> impl std::future::Future<Output = std::io::Result<TcpStream>> + Send;
}

impl Acceptor for TcpListener {
    async fn accept_stream(&self) -> std::io::Result<TcpStream> {
        self.accept().await.map(|(stream, _)| stream)
    }
}

/// Length-checked comparison that does not stop at the first difference.
fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// A fresh random secret: 32 bytes, unpadded base64url (43 characters).
/// Used for the per-flow value and the login verifier.
pub fn new_secret() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// S256 challenge of `verifier`: unpadded base64url SHA-256.
pub fn s256_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// The loopback return address for `port` carrying the per-flow value.
pub fn return_address(port: u16, flow: &str) -> String {
    format!("http://127.0.0.1:{port}{CALLBACK_PATH}?{FLOW_PARAM}={flow}")
}

/// Extract the target of a complete `GET <target> HTTP/1.x` request line.
pub fn request_target(line: &str) -> Option<&str> {
    let mut parts = line.split(' ');
    let (method, target, version) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || method != "GET" || !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
        return None;
    }
    target.starts_with('/').then_some(target)
}

/// Query parameters of a request target, or `None` unless the raw path is
/// exactly `/callback` and the query carries exactly one `flow` equal to
/// `expected_flow`. The returned map does not contain `flow`.
pub fn callback_params(target: &str, expected_flow: &str) -> Option<HashMap<String, String>> {
    let (raw_path, query) = target.split_once('?')?;
    if raw_path != CALLBACK_PATH {
        return None;
    }
    let parsed = Url::parse(&format!("http://127.0.0.1/?{query}")).ok()?;
    let mut flows = 0;
    let mut flow_ok = false;
    let mut params = HashMap::new();
    for (k, v) in parsed.query_pairs() {
        if k == FLOW_PARAM {
            flows += 1;
            flow_ok = constant_time_eq(&v, expected_flow);
        } else {
            params.insert(k.into_owned(), v.into_owned());
        }
    }
    (flows == 1 && flow_ok).then_some(params)
}

/// Result of the login loopback callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginCallback {
    /// `?code=` — one-time hand-off code to redeem with the verifier.
    Code(String),
    /// `?error=CODE` — generic server error code.
    Error(String),
}

/// Recognise a login hand-back; `None` keeps waiting.
pub fn parse_login_callback(params: &HashMap<String, String>) -> Option<LoginCallback> {
    if let Some(code) = params.get("error") {
        return Some(LoginCallback::Error(code.clone()));
    }
    params
        .get("code")
        .filter(|c| !c.is_empty())
        .map(|c| LoginCallback::Code(c.clone()))
}

/// Result of the identity-link loopback callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkCallback {
    /// `?linked=<slug>`
    Linked,
    /// `?link_error=CODE` (or a generic `?error=CODE`)
    Error(String),
}

/// Recognise a link hand-back; `None` keeps waiting.
pub fn parse_link_callback(params: &HashMap<String, String>) -> Option<LinkCallback> {
    if let Some(code) = params.get("link_error").or_else(|| params.get("error")) {
        return Some(LinkCallback::Error(code.clone()));
    }
    params
        .contains_key("linked")
        .then_some(LinkCallback::Linked)
}

/// Read the request line of one connection within `deadline`; `None` when it
/// is incomplete, too long, not UTF-8, or the peer is idle.
async fn read_request_line(stream: &mut TcpStream, deadline: Duration) -> Option<String> {
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

/// Headers of the result page: never cached, never sent as a referrer.
const PAGE_HEADERS: &str = "Content-Type: text/html\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n";

async fn respond_page(stream: &mut TcpStream, html: &str) {
    let response = format!(
        "HTTP/1.1 200 OK\r\n{PAGE_HEADERS}Content-Length: {}\r\n\r\n{html}",
        html.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.flush().await;
}

/// The overall wait ended without a recognised callback.
#[derive(Debug, PartialEq, Eq)]
pub struct CallbackTimeout;

/// Wait on `listener` for the flow's callback. `parse` turns the callback's
/// parameters into a result (or `None` to keep waiting); `page` is the HTML
/// shown in the browser for that result.
pub async fn wait_for_callback<T>(
    listener: &(impl Acceptor + Sync),
    expected_flow: &str,
    timeout: Duration,
    read_deadline: Duration,
    parse: fn(&HashMap<String, String>) -> Option<T>,
    page: fn(&T) -> &'static str,
) -> Result<T, CallbackTimeout> {
    let accept = async {
        loop {
            let Ok(mut stream) = listener.accept_stream().await else {
                // Also yields, so the overall timeout can fire.
                tokio::time::sleep(ACCEPT_BACKOFF).await;
                continue;
            };
            let line = read_request_line(&mut stream, read_deadline).await;
            let result = line
                .as_deref()
                .and_then(request_target)
                .and_then(|target| callback_params(target, expected_flow))
                .and_then(|params| parse(&params));
            if let Some(result) = result {
                respond_page(&mut stream, page(&result)).await;
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
        .map_err(|_| CallbackTimeout)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLOW: &str = "flow-value_0123456789abcdefghijklmnopqrstu";

    fn login(target: &str) -> Option<LoginCallback> {
        callback_params(target, FLOW).and_then(|p| parse_login_callback(&p))
    }

    #[test]
    fn challenge_is_s256_base64url() {
        // RFC 7636 appendix B.
        assert_eq!(
            s256_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn secrets_are_random_base64url() {
        let a = new_secret();
        let b = new_secret();
        assert_ne!(a, b);
        assert_eq!(a.len(), 43);
        assert!(a
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'));
        assert_eq!(s256_challenge(&a).len(), 43);
    }

    #[test]
    fn return_address_carries_the_flow_value() {
        assert_eq!(
            return_address(54321, FLOW),
            format!("http://127.0.0.1:54321/callback?flow={FLOW}")
        );
    }

    #[test]
    fn accepts_code_with_the_expected_flow_value() {
        assert_eq!(
            login(&format!("/callback?flow={FLOW}&code=abc")),
            Some(LoginCallback::Code("abc".into()))
        );
        assert_eq!(
            login(&format!("/callback?code=abc&flow={FLOW}")),
            Some(LoginCallback::Code("abc".into()))
        );
        assert_eq!(
            login(&format!("/callback?flow={FLOW}&error=OIDC_PROVIDER_ERROR")),
            Some(LoginCallback::Error("OIDC_PROVIDER_ERROR".into()))
        );
    }

    #[test]
    fn rejects_missing_or_wrong_flow_value() {
        assert_eq!(login("/callback?code=abc"), None);
        assert_eq!(login("/callback?flow=other&code=abc"), None);
        assert_eq!(login(&format!("/callback?flow={FLOW}x&code=abc")), None);
        assert_eq!(login("/callback?flow=&code=abc"), None);
        assert_eq!(
            login(&format!("/callback?flow={FLOW}&flow=other&code=abc")),
            None
        );
        assert_eq!(
            login(&format!("/callback?flow={FLOW}&flow={FLOW}&code=abc")),
            None
        );
    }

    #[test]
    fn rejects_other_paths() {
        for target in [
            format!("/favicon.ico?flow={FLOW}&code=abc"),
            format!("/callback/extra?flow={FLOW}&code=abc"),
            format!("/callbackx?flow={FLOW}&code=abc"),
            format!("/x/../callback?flow={FLOW}&code=abc"),
            format!("//callback?flow={FLOW}&code=abc"),
            format!("/%63allback?flow={FLOW}&code=abc"),
            "/callback".to_string(),
            "/".to_string(),
        ] {
            assert_eq!(login(&target), None, "{target}");
        }
    }

    #[test]
    fn tokens_in_the_callback_are_not_a_result() {
        // An old-style hand-back (tokens in the URL) is never adopted.
        assert_eq!(
            login(&format!(
                "/callback?flow={FLOW}&access_token=a&refresh_token=r&expires_in=900"
            )),
            None
        );
        assert_eq!(login(&format!("/callback?flow={FLOW}&code=")), None);
    }

    #[test]
    fn link_results() {
        let link = |t: &str| callback_params(t, FLOW).and_then(|p| parse_link_callback(&p));
        assert_eq!(
            link(&format!("/callback?flow={FLOW}&linked=google")),
            Some(LinkCallback::Linked)
        );
        assert_eq!(
            link(&format!(
                "/callback?flow={FLOW}&link_error=IDENTITY_ALREADY_LINKED"
            )),
            Some(LinkCallback::Error("IDENTITY_ALREADY_LINKED".into()))
        );
        assert_eq!(link("/callback?linked=google"), None);
        assert_eq!(link(&format!("/callback?flow={FLOW}")), None);
    }

    #[test]
    fn request_line_must_be_a_complete_get() {
        assert_eq!(
            request_target("GET /callback?a=1 HTTP/1.1"),
            Some("/callback?a=1")
        );
        assert!(request_target("GET /callback?a=1 HTTP/1.0").is_some());
        assert_eq!(request_target("POST /callback HTTP/1.1"), None);
        assert_eq!(request_target("GET /callback"), None);
        assert_eq!(request_target("GET /callback HTTP/2"), None);
        assert_eq!(request_target("GET /callback HTTP/1."), None);
        assert_eq!(request_target("GET /callback HTTP/1.2"), None);
        assert_eq!(request_target("GET callback HTTP/1.1"), None);
        assert_eq!(request_target("GET /a b HTTP/1.1"), None);
        assert_eq!(request_target("GET  /callback HTTP/1.1"), None);
        assert_eq!(request_target("get /callback HTTP/1.1"), None);
        assert_eq!(request_target(""), None);
    }

    async fn send(port: u16, raw: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
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

    async fn wait_login(
        listener: &TcpListener,
        timeout: Duration,
        deadline: Duration,
    ) -> Result<LoginCallback, CallbackTimeout> {
        wait_for_callback(
            listener,
            FLOW,
            timeout,
            deadline,
            parse_login_callback,
            |_| "<html><body>done</body></html>",
        )
        .await
    }

    #[tokio::test]
    async fn listener_ignores_strays_and_sets_headers() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = tokio::spawn(async move {
            let other = get(port, "/favicon.ico").await;
            // Injection without the per-flow value is ignored.
            let injected = get(port, "/callback?code=attacker").await;
            send(port, "garbage\r\n\r\n").await;
            send(
                port,
                &format!("POST /callback?flow={FLOW}&code=x HTTP/1.1\r\n\r\n"),
            )
            .await;
            let ok = get(port, &format!("/callback?flow={FLOW}&code=real")).await;
            (other, injected, ok)
        });
        let result = wait_login(&listener, LONG, LONG).await;
        assert_eq!(result, Ok(LoginCallback::Code("real".into())));
        let (other, injected, ok) = client.await.unwrap();
        assert!(other.starts_with("HTTP/1.1 204"));
        assert!(injected.starts_with("HTTP/1.1 204"));
        assert!(ok.starts_with("HTTP/1.1 200"));
        assert!(ok.contains("Cache-Control: no-store"));
        assert!(ok.contains("Referrer-Policy: no-referrer"));
    }

    #[tokio::test]
    async fn idle_connection_does_not_stall_the_flow() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = tokio::spawn(async move {
            let idle = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            let ok = get(port, &format!("/callback?flow={FLOW}&code=c")).await;
            drop(idle);
            ok
        });
        let result = wait_login(&listener, LONG, SHORT).await;
        assert_eq!(result, Ok(LoginCallback::Code("c".into())));
        client.await.unwrap();
    }

    /// An acceptor whose `accept()` always fails, counting the calls.
    #[derive(Default)]
    struct FailingAcceptor {
        calls: std::sync::atomic::AtomicUsize,
    }

    impl Acceptor for FailingAcceptor {
        async fn accept_stream(&self) -> std::io::Result<TcpStream> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(std::io::Error::other("EMFILE"))
        }
    }

    #[tokio::test]
    async fn persistent_accept_errors_back_off() {
        let acceptor = FailingAcceptor::default();
        let result = wait_for_callback(
            &acceptor,
            FLOW,
            Duration::from_millis(300),
            SHORT,
            parse_login_callback,
            |_| "",
        )
        .await;
        assert_eq!(result, Err(CallbackTimeout));
        let calls = acceptor.calls.load(std::sync::atomic::Ordering::SeqCst);
        assert!(calls <= 10, "accept() retried {calls} times in 300 ms");
    }

    #[test]
    fn flow_compare_is_exact() {
        assert!(constant_time_eq(FLOW, FLOW));
        assert!(!constant_time_eq(FLOW, &FLOW[1..]));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(constant_time_eq("", ""));
    }

    #[tokio::test]
    async fn listener_times_out() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let result = wait_login(&listener, Duration::from_millis(50), SHORT).await;
        assert_eq!(result, Err(CallbackTimeout));
    }
}
