//! Desktop OIDC delivery through LP's loopback POST (C29 step 8, C27).
//!
//! The desktop sign-in (`GET /auth/oidc/authorize/{provider}`) and the
//! account request's OIDC factor (`POST
//! /auth/account-requests/oidc/authorize/{provider_id}`) start the provider
//! flow with `loopback_port`, `callback_path` and `client_challenge` instead
//! of a loopback `redirect_uri`, and get `{url, flow_id}`. The server's
//! callback answers with the hand-off page of the passkey flows, which
//! form-POSTs `flow_id` and `code` (or `error`) to LP's listener
//! ([`loopback_post`]). The code is redeemed with the verifier: the sign-in at
//! `POST /auth/oidc/desktop/exchange` (a `LoginStep`, the verifier becomes the
//! attempt's collection secret), the request factor as `{oidc_code}` with the
//! verifier kept in Rust until the request start sends it.
//!
//! 1. Bind the listener (`127.0.0.1:0`, it draws `callback_path`) before anything else, then the
//!    verifier.
//! 2. Start; only an `https` provider URL (or `http` on a loopback host) is opened, in the browser
//!    chosen under "Browser for passkey sign-in" (LP).
//! 3. Wait for the flow's hand-off POST, at most [`OIDC_LISTEN_MAX`].
//!
//! Errors are bare codes: the server's start or hand-off code (`[A-Z0-9_]`,
//! else [`FAILED`]), [`TIMEOUT`], [`BROWSER_FAILED`], or `Connection failed:
//! …`. Verifier, code and callback path are never logged.

use std::future::Future;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::time::Instant;
use tracing::info;
use url::Url;
use zeroize::Zeroizing;

use super::loopback_post::{self, HandBack, FLOW_LISTEN_MAX};
use super::oidc_loopback::{new_secret, s256_challenge, READ_DEADLINE};
use super::reauth::{run_cancellable, Attempt, CANCELLED as ATTEMPT_CANCELLED};
use crate::session::SessionManager;

/// Anything that is not a server code (malformed answer, unsafe URL, no listener).
pub const FAILED: &str = "OIDC_FAILED";
/// No hand-off within [`OIDC_LISTEN_MAX`].
pub const TIMEOUT: &str = "OIDC_TIMEOUT";
/// The chosen browser did not start (LP's code; its text names the setting).
pub const BROWSER_FAILED: &str = "PASSKEY_BROWSER_FAILED";
/// The webview cancelled the flow (`oidc_cancel`); nothing more happens.
pub const CANCELLED: &str = "OIDC_CANCELLED";

/// How long the app waits for the provider sign-in in the browser (the
/// server keeps an OIDC flow 10 minutes; a provider sign-in with its own
/// second factor takes a few).
pub const OIDC_LISTEN_MAX: Duration = Duration::from_mins(5);

/// What a start sends next to the provider: the listener and the challenge.
pub struct LoopbackStart {
    pub port: u16,
    /// The listener's per-flow path: a secret (Codex m6), wiped on drop.
    pub callback_path: Zeroizing<String>,
    pub client_challenge: String,
}

impl std::fmt::Debug for LoopbackStart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoopbackStart")
            .field("port", &self.port)
            .field("callback_path", &"[REDACTED]")
            .field("client_challenge", &self.client_challenge)
            .finish()
    }
}

/// The hand-off: the one-time code and the verifier that redeems it.
pub struct Delivered {
    pub code: Zeroizing<String>,
    pub verifier: Zeroizing<String>,
}

impl std::fmt::Debug for Delivered {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Delivered([REDACTED])")
    }
}

/// A bare `UPPER_SNAKE` code, else [`FAILED`] (nothing else reaches the webview).
fn bare_code(code: &str) -> String {
    if !code.is_empty()
        && code.len() <= 64
        && code
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b == b'_' || b.is_ascii_digit())
    {
        code.to_string()
    } else {
        FAILED.to_string()
    }
}

fn uuid_like(s: &str) -> bool {
    s.len() == 36 && s.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

/// The provider URL, only if it is safe to hand to the OS or onto a
/// browser's command line: `https` (or `http` on a loopback host, for local
/// providers), no userinfo.
fn provider_url(raw: &str) -> Option<String> {
    let url = Url::parse(raw).ok()?;
    let loopback = match url.host()? {
        url::Host::Domain(d) => d == "localhost",
        url::Host::Ipv4(ip) => ip.is_loopback(),
        url::Host::Ipv6(ip) => ip.is_loopback(),
    };
    let scheme_ok = url.scheme() == "https" || (url.scheme() == "http" && loopback);
    (scheme_ok && url.username().is_empty() && url.password().is_none())
        .then(|| url.as_str().to_string())
}

/// `{server}/seg/ments` with each segment encoded.
pub fn endpoint(server_url: &str, segments: &[&str]) -> Result<Url, String> {
    let mut url = Url::parse(server_url).map_err(|_| FAILED.to_string())?;
    url.path_segments_mut()
        .map_err(|()| FAILED.to_string())?
        .pop_if_empty()
        .extend(segments);
    Ok(url)
}

/// Send a desktop start (`X-Kaiku-Client: desktop`, redirects not followed)
/// → the JSON answer; a refusal is the server's bare code.
pub async fn send_start(request: reqwest::RequestBuilder) -> Result<Value, String> {
    let response = request
        .header("x-kaiku-client", "desktop")
        .send()
        .await
        // Never the request URL: its query carries `callback_path` (Codex m6).
        .map_err(|e| format!("Connection failed: {}", e.without_url()))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        let code = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v.get("error").and_then(Value::as_str).map(bare_code));
        return Err(code.unwrap_or_else(|| FAILED.to_string()));
    }
    serde_json::from_str(&text).map_err(|_| FAILED.to_string())
}

/// A client for the starts: no redirects, a short timeout.
pub fn start_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| FAILED.to_string())
}

/// Run one delivery: bind, start, open, wait (see the module docs). A
/// cancelled `attempt` stops it before the start, before the browser opens
/// and after the hand-off.
pub async fn deliver<F, Fut>(
    attempt: &Attempt,
    start: F,
    open: impl FnOnce(&str) -> Result<(), String>,
    listen_cap: Duration,
) -> Result<Delivered, String>
where
    F: FnOnce(LoopbackStart) -> Fut,
    Fut: Future<Output = Result<Value, String>>,
{
    // 1. Listener first (C29 step 1); it draws the callback path.
    let (listener, cb) = loopback_post::bind()
        .await
        .map_err(|_| FAILED.to_string())?;
    let verifier = Zeroizing::new(new_secret());
    attempt.check()?;
    let answer = start(LoopbackStart {
        port: cb.port,
        callback_path: cb.callback_path.clone(),
        client_challenge: s256_challenge(&verifier),
    })
    .await?;
    let flow_id = answer["flow_id"]
        .as_str()
        .filter(|f| uuid_like(f))
        .ok_or(FAILED)?
        .to_string();
    let url = answer["url"]
        .as_str()
        .and_then(provider_url)
        .ok_or(FAILED)?;
    // 2. Browser (a flow the webview gave up meanwhile opens nothing).
    attempt.check()?;
    open(&url).map_err(|_| BROWSER_FAILED.to_string())?;
    info!("Opened the browser for a provider sign-in");
    // 3. Hand-off.
    let deadline = Instant::now() + listen_cap.min(FLOW_LISTEN_MAX);
    let handed = loopback_post::wait_for_post(&listener, &cb, &flow_id, deadline, READ_DEADLINE)
        .await
        .map_err(|_| TIMEOUT.to_string())?;
    drop(listener);
    attempt.check()?;
    match handed {
        HandBack::Code(code) => Ok(Delivered {
            code: Zeroizing::new(code),
            verifier,
        }),
        HandBack::Error(error) => Err(bare_code(&error)),
    }
}

/// Run `run` under the attempt `attempt_id` of the shared registry, so
/// `oidc_cancel` (or any cancel of that id) reaches it.
pub async fn with_attempt<T>(
    attempt_id: &str,
    run: impl AsyncFnOnce(&Attempt) -> Result<T, String>,
) -> Result<T, String> {
    let attempt = super::reauth::registry().register(attempt_id)?;
    let result = run(&attempt).await;
    super::reauth::registry().finish(attempt_id);
    result
}

/// The flow, dropped where it stands when the webview cancels (the
/// listener closes with it); a cancel is [`CANCELLED`].
async fn cancellable<T>(
    attempt: &Attempt,
    flow: impl Future<Output = Result<T, String>>,
) -> Result<T, String> {
    run_cancellable(attempt, flow).await.map_err(|e| {
        if e == ATTEMPT_CANCELLED {
            CANCELLED.to_string()
        } else {
            e
        }
    })
}

/// Desktop SSO sign-in (F3 variant): the delivery, then the exchange →
/// `LoginStep` JSON (the verifier stays in Rust under the attempt id).
pub async fn sign_in(
    session: &SessionManager,
    attempt: &Attempt,
    server_url: &str,
    provider_slug: &str,
    open: impl FnOnce(&str) -> Result<(), String>,
    listen_cap: Duration,
) -> Result<Value, String> {
    cancellable(attempt, async {
        let delivered =
            sign_in_delivery(attempt, server_url, provider_slug, open, listen_cap).await?;
        // A flow given up after the hand-off redeems nothing.
        attempt.check()?;
        session
            .exchange_oidc_code(server_url, &delivered.code, delivered.verifier)
            .await
            .map_err(|e| e.to_string())
    })
    .await
}

async fn sign_in_delivery(
    attempt: &Attempt,
    server_url: &str,
    provider_slug: &str,
    open: impl FnOnce(&str) -> Result<(), String>,
    listen_cap: Duration,
) -> Result<Delivered, String> {
    let client = start_client()?;
    deliver(
        attempt,
        |lb| async move {
            let mut url = endpoint(server_url, &["auth", "oidc", "authorize", provider_slug])?;
            url.query_pairs_mut()
                .append_pair("loopback_port", &lb.port.to_string())
                .append_pair("callback_path", &lb.callback_path)
                .append_pair("client_challenge", &lb.client_challenge);
            send_start(client.get(url.as_str())).await
        },
        open,
        listen_cap,
    )
    .await
}

/// The OIDC factor of an account request (C27) → `{oidc_code}`; the
/// verifier is kept for `account_request_start` (single use).
pub async fn request_factor(
    session: &SessionManager,
    attempt: &Attempt,
    server_url: &str,
    provider_id: &str,
    open: impl FnOnce(&str) -> Result<(), String>,
    listen_cap: Duration,
) -> Result<Value, String> {
    let client = start_client()?;
    let flow = async {
        let delivered = deliver(
            attempt,
            |lb| async move {
                let url = endpoint(
                    server_url,
                    &["auth", "account-requests", "oidc", "authorize", provider_id],
                )?;
                send_start(client.post(url.as_str()).json(&json!({
                    "loopback_port": lb.port,
                    "callback_path": lb.callback_path.as_str(),
                    "client_challenge": lb.client_challenge,
                })))
                .await
            },
            open,
            listen_cap,
        )
        .await?;
        // A flow given up after the hand-off keeps no verifier (a newer
        // factor's one stays).
        attempt.check()?;
        // Bound to its code: only the start naming this code gets it.
        session
            .keep_request_oidc_verifier(server_url, &delivered.code, delivered.verifier)
            .map_err(|e| e.to_string())?;
        Ok(json!({ "oidc_code": delivered.code.as_str() }))
    };
    cancellable(attempt, flow).await
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    use tokio::sync::RwLock;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    use super::*;
    use crate::session::store::MemoryTokenStore;
    use crate::AuthState;

    const FLOW_ID: &str = "7c9e6679-7425-40de-944b-e07fc1f90ae7";
    const ATTEMPT: &str = "44444444-4444-4444-8444-444444444444";

    fn manager() -> SessionManager {
        SessionManager::new(
            reqwest::Client::new(),
            Arc::new(MemoryTokenStore::new()),
            Arc::new(RwLock::new(AuthState::default())),
        )
    }

    /// What the start saw.
    #[derive(Default)]
    struct Seen {
        query: Vec<(String, String)>,
        body: Option<Value>,
        client: Option<String>,
        bound: bool,
    }

    /// Answers a start like L1b, records it, and (after a moment) posts
    /// `field` to the loopback like the hand-off page.
    struct StartResponder {
        field: Option<(&'static str, &'static str)>,
        url: String,
        seen: Arc<Mutex<Seen>>,
    }

    impl Respond for StartResponder {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            let query: Vec<(String, String)> = request
                .url
                .query_pairs()
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
            let body: Option<Value> = serde_json::from_slice(&request.body).ok();
            let lookup = |k: &str| {
                body.as_ref()
                    .and_then(|b| {
                        b.get(k)
                            .map(|v| v.to_string().trim_matches('"').to_string())
                    })
                    .or_else(|| query.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()))
            };
            let port: u16 = lookup("loopback_port").unwrap().parse().unwrap();
            let cb_path = lookup("callback_path").unwrap();
            let bound = std::net::TcpStream::connect(("127.0.0.1", port)).is_ok();
            *self.seen.lock().unwrap() = Seen {
                query,
                body: body.clone(),
                client: request
                    .headers
                    .get("x-kaiku-client")
                    .map(|v| v.to_str().unwrap().to_string()),
                bound,
            };
            if let Some((name, value)) = self.field {
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(100));
                    let form = format!("flow_id={FLOW_ID}&{name}={value}");
                    let req = format!(
                        "POST /callback/{cb_path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{form}",
                        form.len()
                    );
                    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
                    s.write_all(req.as_bytes()).unwrap();
                    let mut out = Vec::new();
                    let _ = s.read_to_end(&mut out);
                });
            }
            ResponseTemplate::new(200).set_body_json(json!({ "url": self.url, "flow_id": FLOW_ID }))
        }
    }

    async fn mount_start(
        server: &MockServer,
        verb: &str,
        route: &str,
        field: Option<(&'static str, &'static str)>,
        url: &str,
    ) -> Arc<Mutex<Seen>> {
        let seen = Arc::new(Mutex::new(Seen::default()));
        Mock::given(method(verb))
            .and(path(route))
            .respond_with(StartResponder {
                field,
                url: url.to_string(),
                seen: seen.clone(),
            })
            .mount(server)
            .await;
        seen
    }

    fn query_value<'a>(seen: &'a Seen, name: &str) -> Option<&'a str> {
        seen.query
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    #[tokio::test]
    async fn oidc_start_sends_loopback_port_and_callback_path_not_redirect_uri() {
        let server = MockServer::start().await;
        let seen = mount_start(
            &server,
            "GET",
            "/auth/oidc/authorize/kc",
            Some(("code", "one-time")),
            "https://idp.example.com/auth?state=x",
        )
        .await;
        Mock::given(method("POST"))
            .and(path("/auth/oidc/desktop/exchange"))
            .respond_with(ResponseTemplate::new(202).set_body_json(json!({
                "status": "confirmation_required",
                "attempt": {"attempt_id": ATTEMPT, "kind": "oidc", "state": "open",
                            "expires_at": "2099-01-01T00:00:00Z", "qr": null, "totp_available": false}
            })))
            .mount(&server)
            .await;
        let m = manager();
        sign_in(
            &m,
            &Attempt::default(),
            &server.uri(),
            "kc",
            |_| Ok(()),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        let seen = seen.lock().unwrap();
        assert!(seen.bound, "the listener is bound before the start");
        assert_eq!(seen.client.as_deref(), Some("desktop"));
        assert!(query_value(&seen, "redirect_uri").is_none());
        assert_eq!(query_value(&seen, "callback_path").unwrap().len(), 43);
        assert_eq!(query_value(&seen, "client_challenge").unwrap().len(), 43);
        assert!(query_value(&seen, "loopback_port")
            .unwrap()
            .parse::<u16>()
            .is_ok());
    }

    #[tokio::test]
    async fn oidc_opens_the_chosen_browser_and_waits_on_lp_loopback_post() {
        let server = MockServer::start().await;
        let seen = mount_start(
            &server,
            "GET",
            "/auth/oidc/authorize/kc",
            Some(("code", "one-time")),
            "https://idp.example.com/auth?state=x",
        )
        .await;
        Mock::given(method("POST"))
            .and(path("/auth/oidc/desktop/exchange"))
            .and(header("x-kaiku-client", "desktop"))
            .respond_with(ResponseTemplate::new(202).set_body_json(json!({
                "status": "totp_required",
                "attempt": {"attempt_id": ATTEMPT, "kind": "oidc", "state": "open",
                            "expires_at": "2099-01-01T00:00:00Z", "qr": null, "totp_available": true}
            })))
            .mount(&server)
            .await;
        let m = manager();
        let opened = Arc::new(Mutex::new(None::<String>));
        let o = opened.clone();
        let step = sign_in(
            &m,
            &Attempt::default(),
            &server.uri(),
            "kc",
            move |url| {
                *o.lock().unwrap() = Some(url.to_string());
                Ok(())
            },
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert_eq!(step["status"], "totp_required");
        assert_eq!(
            opened.lock().unwrap().as_deref(),
            Some("https://idp.example.com/auth?state=x")
        );
        // The exchange sent the hand-off code and the verifier behind the
        // challenge; the verifier stays as the attempt's collection secret.
        let exchange = server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .find(|r| r.url.path() == "/auth/oidc/desktop/exchange")
            .unwrap();
        let body: Value = serde_json::from_slice(&exchange.body).unwrap();
        assert_eq!(body["code"], "one-time");
        let verifier = body["verifier"].as_str().unwrap();
        let challenge = query_value(&seen.lock().unwrap(), "client_challenge")
            .unwrap()
            .to_string();
        assert_eq!(s256_challenge(verifier), challenge);
        assert_eq!(
            m.attempt_verifiers().get(ATTEMPT).unwrap().1.as_str(),
            verifier
        );
    }

    #[tokio::test]
    async fn a_browser_that_does_not_start_says_so() {
        let server = MockServer::start().await;
        mount_start(
            &server,
            "GET",
            "/auth/oidc/authorize/kc",
            None,
            "https://idp.example.com/a",
        )
        .await;
        let err = sign_in(
            &manager(),
            &Attempt::default(),
            &server.uri(),
            "kc",
            |_| Err("no browser".into()),
            Duration::from_secs(5),
        )
        .await
        .unwrap_err();
        assert_eq!(err, BROWSER_FAILED);
    }

    #[tokio::test]
    async fn only_a_safe_provider_url_is_opened() {
        for bad in [
            "http://idp.example.com/auth",
            "javascript:alert(1)",
            "https://user:pw@idp.example.com/auth",
            "file:///etc/passwd",
        ] {
            let server = MockServer::start().await;
            mount_start(&server, "GET", "/auth/oidc/authorize/kc", None, bad).await;
            let opened = AtomicBool::new(false);
            let err = sign_in(
                &manager(),
                &Attempt::default(),
                &server.uri(),
                "kc",
                |_| {
                    opened.store(true, Ordering::SeqCst);
                    Ok(())
                },
                Duration::from_secs(5),
            )
            .await
            .unwrap_err();
            assert_eq!(err, FAILED, "{bad}");
            assert!(!opened.load(Ordering::SeqCst), "{bad} must not be opened");
        }
    }

    #[tokio::test]
    async fn a_hand_off_error_is_its_bare_code() {
        let server = MockServer::start().await;
        mount_start(
            &server,
            "GET",
            "/auth/oidc/authorize/kc",
            Some(("error", "OIDC_PROVIDER_ERROR")),
            "https://idp.example.com/a",
        )
        .await;
        let err = sign_in(
            &manager(),
            &Attempt::default(),
            &server.uri(),
            "kc",
            |_| Ok(()),
            Duration::from_secs(5),
        )
        .await
        .unwrap_err();
        assert_eq!(err, "OIDC_PROVIDER_ERROR");

        let server = MockServer::start().await;
        mount_start(
            &server,
            "GET",
            "/auth/oidc/authorize/kc",
            Some(("error", "%3Cscript%3E")),
            "https://idp.example.com/a",
        )
        .await;
        let err = sign_in(
            &manager(),
            &Attempt::default(),
            &server.uri(),
            "kc",
            |_| Ok(()),
            Duration::from_secs(5),
        )
        .await
        .unwrap_err();
        assert_eq!(err, FAILED);
    }

    #[tokio::test]
    async fn no_hand_off_in_time_is_a_timeout() {
        let server = MockServer::start().await;
        mount_start(
            &server,
            "GET",
            "/auth/oidc/authorize/kc",
            None,
            "https://idp.example.com/a",
        )
        .await;
        let err = sign_in(
            &manager(),
            &Attempt::default(),
            &server.uri(),
            "kc",
            |_| Ok(()),
            Duration::from_millis(200),
        )
        .await
        .unwrap_err();
        assert_eq!(err, TIMEOUT);
    }

    #[tokio::test]
    async fn a_refused_start_is_the_servers_code() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/auth/oidc/authorize/kc"))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_json(json!({"error": "OIDC_CLIENT_OUTDATED", "message": "x"})),
            )
            .mount(&server)
            .await;
        let err = sign_in(
            &manager(),
            &Attempt::default(),
            &server.uri(),
            "kc",
            |_| Ok(()),
            Duration::from_secs(5),
        )
        .await
        .unwrap_err();
        assert_eq!(err, "OIDC_CLIENT_OUTDATED");
    }

    #[tokio::test]
    async fn a_start_redirect_is_not_followed() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(307).insert_header("location", "https://idp.example.com/a"),
            )
            .mount(&server)
            .await;
        let err = sign_in(
            &manager(),
            &Attempt::default(),
            &server.uri(),
            "kc",
            |_| Ok(()),
            Duration::from_secs(5),
        )
        .await
        .unwrap_err();
        assert_eq!(err, FAILED);
    }

    #[tokio::test]
    async fn request_oidc_factor_posts_the_loopback_and_keeps_the_verifier() {
        let server = MockServer::start().await;
        let seen = mount_start(
            &server,
            "POST",
            "/auth/account-requests/oidc/authorize/kc",
            Some(("code", "factor-code")),
            "https://idp.example.com/auth",
        )
        .await;
        Mock::given(method("POST"))
            .and(path("/auth/account-requests"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({"request": {
                "id": ATTEMPT, "purpose": "recovery", "state": "waiting",
                "ready_at": "2099-01-01T00:00:00Z", "expires_at": "2099-01-02T00:00:00Z"
            }, "request_secret": "rs"})))
            .mount(&server)
            .await;
        let m = manager();
        let answer = request_factor(
            &m,
            &Attempt::default(),
            &server.uri(),
            "kc",
            |_| Ok(()),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        // Only the code reaches the webview.
        assert_eq!(answer, json!({"oidc_code": "factor-code"}));
        let (challenge, client, bound) = {
            let seen = seen.lock().unwrap();
            let body = seen.body.clone().unwrap();
            assert!(body.get("redirect_uri").is_none());
            assert_eq!(body["callback_path"].as_str().unwrap().len(), 43);
            (
                body["client_challenge"].as_str().unwrap().to_string(),
                seen.client.clone(),
                seen.bound,
            )
        };
        assert!(bound);
        assert_eq!(client.as_deref(), Some("desktop"));
        // The request start adds the kept verifier to the OIDC factor.
        let _ = m
            .account_request_start(
                &server.uri(),
                json!({"username": "alice", "purpose": "recovery",
                       "first": {"type": "oidc", "oidc_code": "factor-code"}}),
            )
            .await;
        let start = server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .find(|r| r.url.path() == "/auth/account-requests")
            .unwrap();
        let sent: Value = serde_json::from_slice(&start.body).unwrap();
        assert_eq!(
            s256_challenge(sent["first"]["verifier"].as_str().unwrap()),
            challenge
        );
    }

    #[tokio::test]
    async fn a_connection_failure_names_no_callback_path() {
        // Codex review m6: the request URL (with `callback_path`) never
        // crosses IPC in an error.
        let err = sign_in(
            &manager(),
            &Attempt::default(),
            "http://127.0.0.1:1",
            "kc",
            |_| Ok(()),
            Duration::from_secs(5),
        )
        .await
        .unwrap_err();
        assert!(err.starts_with("Connection failed"), "{err}");
        for leak in [
            "callback_path",
            "loopback_port",
            "client_challenge",
            "127.0.0.1:1/",
        ] {
            assert!(!err.contains(leak), "{err}");
        }
    }

    #[tokio::test]
    async fn a_cancelled_flow_opens_no_browser() {
        // Argon review part 7 #2: a flow the webview gave up during the start
        // opens nothing and keeps nothing.
        let server = MockServer::start().await;
        mount_start(
            &server,
            "GET",
            "/auth/oidc/authorize/kc",
            None,
            "https://idp.example.com/a",
        )
        .await;
        let id = "oidc-test-cancel-before-open";
        let attempt = crate::commands::reauth::registry().register(id).unwrap();
        crate::commands::reauth::registry().cancel(id);
        let opened = AtomicBool::new(false);
        let err = sign_in(
            &manager(),
            &attempt,
            &server.uri(),
            "kc",
            |_| {
                opened.store(true, Ordering::SeqCst);
                Ok(())
            },
            Duration::from_secs(5),
        )
        .await
        .unwrap_err();
        crate::commands::reauth::registry().finish(id);
        assert_eq!(err, CANCELLED);
        assert!(!opened.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn a_flow_cancelled_while_the_browser_is_open_redeems_nothing() {
        let server = MockServer::start().await;
        mount_start(
            &server,
            "GET",
            "/auth/oidc/authorize/kc",
            Some(("code", "late-code")),
            "https://idp.example.com/a",
        )
        .await;
        let id = "oidc-test-cancel-while-open";
        let attempt = crate::commands::reauth::registry().register(id).unwrap();
        let err = sign_in(
            &manager(),
            &attempt,
            &server.uri(),
            "kc",
            |_| {
                // The user gives up while the provider page is open.
                crate::commands::reauth::registry().cancel(id);
                Ok(())
            },
            Duration::from_secs(5),
        )
        .await
        .unwrap_err();
        crate::commands::reauth::registry().finish(id);
        assert_eq!(err, CANCELLED);
        tokio::time::sleep(Duration::from_millis(300)).await;
        let exchanged = server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .any(|r| r.url.path() == "/auth/oidc/desktop/exchange");
        assert!(!exchanged);
    }

    #[tokio::test]
    async fn a_cancelled_request_factor_keeps_no_verifier() {
        let server = MockServer::start().await;
        mount_start(
            &server,
            "POST",
            "/auth/account-requests/oidc/authorize/kc",
            Some(("code", "factor-late")),
            "https://idp.example.com/auth",
        )
        .await;
        let id = "oidc-test-cancel-factor";
        let attempt = crate::commands::reauth::registry().register(id).unwrap();
        let m = manager();
        let err = request_factor(
            &m,
            &attempt,
            &server.uri(),
            "kc",
            |_| {
                crate::commands::reauth::registry().cancel(id);
                Ok(())
            },
            Duration::from_secs(5),
        )
        .await
        .unwrap_err();
        crate::commands::reauth::registry().finish(id);
        assert_eq!(err, CANCELLED);
        Mock::given(method("POST"))
            .and(path("/auth/account-requests"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({"request": {
                "id": ATTEMPT, "purpose": "recovery", "state": "waiting"
            }, "request_secret": "rs"})))
            .mount(&server)
            .await;
        let _ = m
            .account_request_start(
                &server.uri(),
                json!({"username": "a", "purpose": "recovery", "first": {"type": "oidc", "oidc_code": "factor-late"}}),
            )
            .await;
        let start = server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .find(|r| r.url.path() == "/auth/account-requests")
            .unwrap();
        let sent: Value = serde_json::from_slice(&start.body).unwrap();
        assert!(sent["first"].get("verifier").is_none());
    }

    #[test]
    fn debug_output_redacts_secrets() {
        let d = Delivered {
            code: Zeroizing::new("code-secret".into()),
            verifier: Zeroizing::new("verifier-secret".into()),
        };
        let s = LoopbackStart {
            port: 1234,
            callback_path: Zeroizing::new("path-secret".into()),
            client_challenge: "c".into(),
        };
        let out = format!("{d:?} {s:?}");
        assert!(!out.contains("secret"), "{out}");
    }
}
