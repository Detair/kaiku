//! Desktop passkey sign-in and step-up through the system browser (F1 rev 4,
//! C29 steps 1–7). There is no registration command: the desktop app never
//! adds a passkey (B.1 rev 3).
//!
//! 1. Bind the loopback listener (`127.0.0.1:0`) and draw the callback path and a verifier — before
//!    anything else.
//! 2. Start the flow (`X-Kaiku-Client: desktop`, bearer for `step_up`).
//! 3. Open the ceremony URL in the system browser (or the chosen one).
//! 4. Wait for the hand-off POST on the loopback.
//! 5. Redeem code + verifier; check `purpose` (and the account for `step_up`). Sign-in tokens go to
//!    the session manager and never reach the webview.
//!
//! Errors are bare server codes or `PASSKEY_TIMEOUT`, `PASSKEY_CANCELLED`,
//! `PASSKEY_FAILED`, `PASSKEY_BROWSER_FAILED`. Verifier, code and callback
//! path are never logged.

use std::time::Duration;

use serde_json::{json, Value};
use tauri::{command, State};
use tokio::time::Instant;
use tracing::info;

use super::loopback_post::{self, HandBack, FLOW_LISTEN_MAX};
use super::oidc_loopback::{new_secret, s256_challenge, READ_DEADLINE};
use super::reauth::{registry, run_cancellable, Attempt, CANCELLED};
use crate::session::{AuthResult, SessionError, SessionManager, TokenResponse};
use crate::AppState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Purpose {
    /// With the user name typed in the app, if any (C29 addition,
    /// coordinator 2026-11-03): the server refuses another account's passkey.
    SignIn(Option<String>),
    StepUp,
}

impl Purpose {
    const fn as_str(&self) -> &'static str {
        match self {
            Self::SignIn(_) => "sign_in",
            Self::StepUp => "step_up",
        }
    }
}

/// A bare `UPPER_SNAKE` server code, else `PASSKEY_FAILED` (nothing else
/// from the loopback or the server reaches the webview).
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

#[derive(Debug)]
pub enum FlowResult {
    SignedIn(AuthResult),
    Granted(Value),
}

const FAILED: &str = "PASSKEY_FAILED";

fn code_of(e: &SessionError) -> String {
    match e {
        // A banned account is `Server(403, "ACCOUNT_BANNED")` (bare codes).
        SessionError::Server(_, code) => bare_code(code),
        SessionError::ServerMismatch => "SESSION_SERVER_MISMATCH".to_string(),
        _ => FAILED.to_string(),
    }
}

/// The server's ceremony URL, only if it is this server's own ceremony page
/// for this flow: same origin as `server_url`, `https` (or `http` on a
/// loopback host), path `/auth/passkeys/desktop/<flow_id>`, nothing else.
/// The URL goes to the OS or onto a browser's command line, so anything else
/// (an argument, a file, another scheme or site) is refused.
fn ceremony_url(server_url: &str, raw: &str, flow_id: &str) -> Option<String> {
    let url = url::Url::parse(raw).ok()?;
    let server = url::Url::parse(server_url).ok()?;
    let loopback = matches!(
        url.host(),
        Some(url::Host::Domain("localhost") | url::Host::Ipv4(_) | url::Host::Ipv6(_))
    ) && url.host().is_some_and(|h| match h {
        url::Host::Ipv4(ip) => ip.is_loopback(),
        url::Host::Ipv6(ip) => ip.is_loopback(),
        url::Host::Domain(d) => d == "localhost",
    });
    let scheme_ok = url.scheme() == "https" || (url.scheme() == "http" && loopback);
    let expected_path = format!("/auth/passkeys/desktop/{flow_id}");
    let ok = scheme_ok
        && url.origin() == server.origin()
        && url.path() == expected_path
        && url.query().is_none()
        && url.fragment().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && uuid_like(flow_id);
    ok.then(|| url.as_str().to_string())
}

fn uuid_like(s: &str) -> bool {
    s.len() == 36 && s.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

/// The shared desktop flow; `open` launches the browser (stubbed in tests),
/// `listen_cap` bounds the wait (the flow's own expiry is the other bound).
#[allow(clippy::too_many_arguments)]
pub async fn run_flow(
    session: &SessionManager,
    server_url: &str,
    purpose: Purpose,
    action: Option<&str>,
    target: Option<&str>,
    attempt: &Attempt,
    open: impl FnOnce(&str) -> Result<(), String>,
    listen_cap: Duration,
) -> Result<FlowResult, String> {
    let flow = async {
        // 1. Listener first (C29 step 1).
        let (listener, cb) = loopback_post::bind()
            .await
            .map_err(|_| FAILED.to_string())?;
        let verifier = new_secret();
        let username = match &purpose {
            Purpose::SignIn(name) => name.as_deref().map(str::trim).filter(|n| !n.is_empty()),
            Purpose::StepUp => None,
        };
        // The session this step-up belongs to (a sign-in in between replaces it).
        let identity = session.identity_generation();
        let body = json!({
            "username": username,
            "purpose": purpose.as_str(),
            "action": action,
            "target": target,
            "loopback_port": cb.port,
            "callback_path": cb.callback_path,
            "client_challenge": s256_challenge(&verifier),
        });
        attempt.check()?;
        // 2. Start.
        let started = session
            .start_passkey_flow(server_url, &body, purpose == Purpose::StepUp)
            .await
            .map_err(|e| code_of(&e))?;
        let flow_id = started["flow_id"].as_str().ok_or(FAILED)?.to_string();
        let url = ceremony_url(server_url, started["url"].as_str().ok_or(FAILED)?, &flow_id)
            .ok_or(FAILED)?;
        let expires_in = started["expires_at"]
            .as_str()
            .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
            .map_or(FLOW_LISTEN_MAX, |t| {
                (t.with_timezone(&chrono::Utc) - chrono::Utc::now())
                    .to_std()
                    .unwrap_or_default()
            });
        let deadline = Instant::now() + expires_in.min(listen_cap);
        // 3. Browser.
        attempt.check()?;
        open(&url).map_err(|_| "PASSKEY_BROWSER_FAILED".to_string())?;
        info!(
            purpose = purpose.as_str(),
            "Opened the browser for a passkey"
        );
        // 4. Hand-off.
        let handed =
            loopback_post::wait_for_post(&listener, &cb, &flow_id, deadline, READ_DEADLINE)
                .await
                .map_err(|_| "PASSKEY_TIMEOUT".to_string())?;
        drop(listener);
        let code = match handed {
            HandBack::Code(code) => code,
            HandBack::Error(error) => return Err(bare_code(&error)),
        };
        // 5. Redeem and check what came back.
        attempt.check()?;
        let redeemed = session
            .redeem_passkey_flow(
                server_url,
                &flow_id,
                &code,
                &verifier,
                purpose == Purpose::StepUp,
            )
            .await
            .map_err(|e| code_of(&e))?;
        if redeemed["purpose"].as_str() != Some(purpose.as_str()) {
            return Err(FAILED.to_string());
        }
        match purpose {
            Purpose::SignIn(_) => {
                let expected = redeemed["user_id"].as_str().ok_or(FAILED)?.to_string();
                let tokens: TokenResponse =
                    serde_json::from_value(redeemed).map_err(|_| FAILED.to_string())?;
                // Only the account the server redeemed for (checked before
                // anything is stored).
                let result = session
                    .adopt_tokens_for(server_url, tokens, Some(&expected))
                    .await
                    .map_err(|e| code_of(&e))?;
                Ok(FlowResult::SignedIn(result))
            }
            Purpose::StepUp => {
                let me = session.current_user_id();
                if me.is_none()
                    || redeemed["user_id"].as_str() != me.as_deref()
                    || session.identity_generation() != identity
                    || redeemed["grant"]["action"].as_str() != action
                    || redeemed["grant"]["target"].as_str() != Some(target.unwrap_or(""))
                {
                    return Err(FAILED.to_string());
                }
                Ok(FlowResult::Granted(redeemed["grant"].clone()))
            }
        }
    };
    run_cancellable(attempt, flow).await.map_err(|e| {
        if e == CANCELLED {
            "PASSKEY_CANCELLED".to_string()
        } else {
            e
        }
    })
}

async fn with_attempt<T>(
    attempt_id: &str,
    run: impl AsyncFnOnce(&Attempt) -> Result<T, String>,
) -> Result<T, String> {
    let attempt = registry().register(attempt_id)?;
    let result = run(&attempt).await;
    registry().finish(attempt_id);
    result
}

/// Sign in with a passkey in the browser; returns the session result
/// without tokens.
#[command]
pub async fn passkey_sign_in(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    server_url: String,
    username: Option<String>,
    attempt_id: String,
) -> Result<AuthResult, String> {
    let browser = super::settings::load_settings(&app_handle)
        .await
        .map(|s| s.passkey_browser)
        .unwrap_or_default();
    with_attempt(&attempt_id, async |attempt| {
        let result = run_flow(
            &state.session,
            server_url.trim_end_matches('/'),
            Purpose::SignIn(username.clone()),
            None,
            None,
            attempt,
            |url| super::auth::open_browser_with(&app_handle, url, browser),
            FLOW_LISTEN_MAX,
        )
        .await?;
        match result {
            FlowResult::SignedIn(r) => Ok(r),
            FlowResult::Granted(_) => Err(FAILED.to_string()),
        }
    })
    .await
}

/// Confirm `action` on `target` with a passkey in the browser; returns the
/// `StepUpGrant` JSON.
#[command]
pub async fn passkey_step_up(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    server_url: String,
    action: String,
    target: Option<String>,
    attempt_id: String,
) -> Result<Value, String> {
    let browser = super::settings::load_settings(&app_handle)
        .await
        .map(|s| s.passkey_browser)
        .unwrap_or_default();
    with_attempt(&attempt_id, async |attempt| {
        let result = run_flow(
            &state.session,
            server_url.trim_end_matches('/'),
            Purpose::StepUp,
            Some(&action),
            Some(target.as_deref().unwrap_or("")),
            attempt,
            |url| super::auth::open_browser_with(&app_handle, url, browser),
            FLOW_LISTEN_MAX,
        )
        .await?;
        match result {
            FlowResult::Granted(g) => Ok(g),
            FlowResult::SignedIn(_) => Err(FAILED.to_string()),
        }
    })
    .await
}

/// Cancel a running (or about to start) passkey attempt.
#[command]
pub fn passkey_cancel(attempt_id: String) {
    registry().cancel(&attempt_id);
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
    use crate::commands::settings::PasskeyBrowser;
    use crate::session::store::MemoryTokenStore;
    use crate::AuthState;

    const FLOW_ID: &str = "4f1c5c43-6a0e-4f9a-9a3e-1f2d3c4b5a69";
    const USER: &str = "0192f0c0-0000-7000-8000-000000000001";

    fn manager() -> SessionManager {
        SessionManager::new(
            reqwest::Client::new(),
            Arc::new(MemoryTokenStore::new()),
            Arc::new(RwLock::new(AuthState::default())),
        )
    }

    /// Answers the start, checks the listener is already bound, and (after
    /// a moment) posts `field` to the loopback like the hand-off page.
    struct StartResponder {
        field: Option<(&'static str, &'static str)>,
        /// Replaces the ceremony URL the server answers with.
        url: Option<String>,
        /// The mock server's own origin.
        origin: String,
        seen: Arc<Mutex<Option<Value>>>,
        bound: Arc<AtomicBool>,
    }

    impl Respond for StartResponder {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            let port = u16::try_from(body["loopback_port"].as_u64().unwrap()).unwrap();
            let path = body["callback_path"].as_str().unwrap().to_string();
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                self.bound.store(true, Ordering::SeqCst);
            }
            *self.seen.lock().unwrap() = Some(json!({
                "body": body,
                "client": request.headers.get("x-kaiku-client").map(|v| v.to_str().unwrap().to_string()),
                "bearer": request.headers.get("authorization").is_some(),
            }));
            if let Some((name, value)) = self.field {
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    let form = format!("flow_id={FLOW_ID}&{name}={value}");
                    let req = format!(
                        "POST /callback/{path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{form}",
                        form.len()
                    );
                    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
                    s.write_all(req.as_bytes()).unwrap();
                    let mut out = Vec::new();
                    let _ = s.read_to_end(&mut out);
                });
            }
            let expires = (chrono::Utc::now() + chrono::Duration::minutes(10)).to_rfc3339();
            let origin = &self.origin;
            let ceremony = self.url.as_ref().map_or_else(
                || format!("{origin}/auth/passkeys/desktop/{FLOW_ID}"),
                |u| u.replace("OWN", origin),
            );
            ResponseTemplate::new(201).set_body_json(json!({
                "flow_id": FLOW_ID,
                "url": ceremony,
                "expires_at": expires,
            }))
        }
    }

    struct Fixture {
        server: MockServer,
        seen: Arc<Mutex<Option<Value>>>,
        bound: Arc<AtomicBool>,
    }

    async fn fixture(
        field: Option<(&'static str, &'static str)>,
        redeemed: Option<Value>,
    ) -> Fixture {
        fixture_with_url(field, redeemed, None).await
    }

    async fn fixture_with_url(
        field: Option<(&'static str, &'static str)>,
        redeemed: Option<Value>,
        url: Option<String>,
    ) -> Fixture {
        let server = MockServer::start().await;
        let seen = Arc::new(Mutex::new(None));
        let bound = Arc::new(AtomicBool::new(false));
        Mock::given(method("POST"))
            .and(path("/auth/passkeys/desktop/flows"))
            .respond_with(StartResponder {
                field,
                url,
                origin: server.uri(),
                seen: Arc::clone(&seen),
                bound: Arc::clone(&bound),
            })
            .mount(&server)
            .await;
        if let Some(r) = redeemed {
            Mock::given(method("POST"))
                .and(path("/auth/passkeys/desktop/redeem"))
                .and(header("x-kaiku-client", "desktop"))
                .respond_with(ResponseTemplate::new(200).set_body_json(r))
                .mount(&server)
                .await;
        }
        Mock::given(method("GET"))
            .and(path("/auth/me"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": USER, "username": "alice", "display_name": "Alice", "email": null,
                "avatar_url": null, "status": "online", "mfa_enabled": false
            })))
            .mount(&server)
            .await;
        Fixture {
            server,
            seen,
            bound,
        }
    }

    fn sign_in_answer() -> Value {
        json!({
            "purpose": "sign_in", "user_id": USER,
            "access_token": "at-1", "refresh_token": "rt-1", "expires_in": 900,
            "token_type": "Bearer", "setup_required": false, "password_change_required": false,
            "session_id": "0192f0c0-0000-7000-8000-0000000000aa", "assurance": "full",
            "absolute_expires_at": "2027-01-01T00:00:00Z"
        })
    }

    /// A manager signed in at `f`'s server (the step-up's own server).
    async fn signed_in_at(f: &Fixture) -> SessionManager {
        let m = manager();
        let tokens: TokenResponse = serde_json::from_value(sign_in_answer()).unwrap();
        m.adopt_tokens(&f.server.uri(), tokens).await.unwrap();
        m
    }

    async fn sign_in(f: &Fixture, m: &SessionManager) -> Result<FlowResult, String> {
        run_flow(
            m,
            &f.server.uri(),
            Purpose::SignIn(None),
            None,
            None,
            &Attempt::default(),
            |_| Ok(()),
            Duration::from_secs(5),
        )
        .await
    }

    #[tokio::test]
    async fn listener_is_bound_before_the_start_request() {
        let f = fixture(Some(("code", "c-1")), Some(sign_in_answer())).await;
        sign_in(&f, &manager()).await.unwrap();
        assert!(f.bound.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn start_request_carries_header_port_path_and_challenge() {
        let f = fixture(Some(("code", "c-1")), Some(sign_in_answer())).await;
        let m = manager();
        sign_in(&f, &m).await.unwrap();
        let seen = f.seen.lock().unwrap().clone().unwrap();
        assert_eq!(seen["client"], "desktop");
        assert_eq!(seen["bearer"], false, "sign_in sends no bearer");
        let body = &seen["body"];
        assert_eq!(body["purpose"], "sign_in");
        assert!(body["loopback_port"].as_u64().unwrap() > 0);
        assert_eq!(body["callback_path"].as_str().unwrap().len(), 43);
        assert_eq!(body["client_challenge"].as_str().unwrap().len(), 43);
        // step_up carries the session's bearer.
        let f2 = fixture(Some(("error", "PASSKEY_CANCELLED")), None).await;
        let m = signed_in_at(&f2).await;
        let r = run_flow(
            &m,
            &f2.server.uri(),
            Purpose::StepUp,
            Some("admin.elevate"),
            Some(""),
            &Attempt::default(),
            |_| Ok(()),
            Duration::from_secs(5),
        )
        .await;
        assert_eq!(r.unwrap_err(), "PASSKEY_CANCELLED");
        let seen = f2.seen.lock().unwrap().clone().unwrap();
        assert_eq!(seen["bearer"], true);
        assert_eq!(seen["body"]["action"], "admin.elevate");
    }

    #[tokio::test]
    async fn redeem_verifier_matches_the_start_challenge() {
        let f = fixture(Some(("code", "c-1")), Some(sign_in_answer())).await;
        sign_in(&f, &manager()).await.unwrap();
        let requests = f.server.received_requests().await.unwrap();
        let start: Value = serde_json::from_slice(
            &requests
                .iter()
                .find(|r| r.url.path().ends_with("/flows"))
                .unwrap()
                .body,
        )
        .unwrap();
        let redeem: Value = serde_json::from_slice(
            &requests
                .iter()
                .find(|r| r.url.path().ends_with("/redeem"))
                .unwrap()
                .body,
        )
        .unwrap();
        assert_eq!(redeem["flow_id"], FLOW_ID);
        assert_eq!(redeem["code"], "c-1");
        assert_eq!(
            s256_challenge(redeem["verifier"].as_str().unwrap()),
            start["client_challenge"].as_str().unwrap()
        );
    }

    #[tokio::test]
    async fn error_post_returns_bare_code() {
        let f = fixture(Some(("error", "PASSKEY_USER_MISMATCH")), None).await;
        assert_eq!(
            sign_in(&f, &manager()).await.unwrap_err(),
            "PASSKEY_USER_MISMATCH"
        );
    }

    #[tokio::test]
    async fn redeem_checks_purpose_and_user() {
        let mut wrong = sign_in_answer();
        wrong["purpose"] = json!("step_up");
        let f = fixture(Some(("code", "c-1")), Some(wrong)).await;
        assert_eq!(sign_in(&f, &manager()).await.unwrap_err(), "PASSKEY_FAILED");
        // A step-up for another account.
        let f2 = fixture(
            Some(("code", "c-2")),
            Some(json!({"purpose": "step_up", "user_id": "0192f0c0-0000-7000-8000-00000000ffff",
                        "grant": {"action": "admin.elevate", "target": "", "via": "passkey", "expires_at": "2027-01-01T00:00:00Z"}})),
        )
        .await;
        let m = signed_in_at(&f2).await;
        let r = run_flow(
            &m,
            &f2.server.uri(),
            Purpose::StepUp,
            Some("admin.elevate"),
            Some(""),
            &Attempt::default(),
            |_| Ok(()),
            Duration::from_secs(5),
        )
        .await;
        assert_eq!(r.unwrap_err(), "PASSKEY_FAILED");
    }

    #[tokio::test]
    async fn step_up_returns_the_grant() {
        let grant = json!({"action": "admin.elevate", "target": "", "via": "passkey", "expires_at": "2027-01-01T00:00:00Z"});
        let f2 = fixture(
            Some(("code", "c-2")),
            Some(json!({"purpose": "step_up", "user_id": USER, "grant": grant})),
        )
        .await;
        let m = signed_in_at(&f2).await;
        let r = run_flow(
            &m,
            &f2.server.uri(),
            Purpose::StepUp,
            Some("admin.elevate"),
            Some(""),
            &Attempt::default(),
            |_| Ok(()),
            Duration::from_secs(5),
        )
        .await;
        match r.unwrap() {
            FlowResult::Granted(g) => assert_eq!(g, grant),
            FlowResult::SignedIn(_) => panic!("expected a grant"),
        }
    }

    #[tokio::test]
    async fn step_up_never_sends_the_bearer_to_another_server() {
        let m = manager();
        let home = fixture(Some(("code", "c-1")), Some(sign_in_answer())).await;
        sign_in(&home, &m).await.unwrap();
        // Another server (a user-editable URL, a stale field): nothing is sent.
        let other = fixture(
            Some(("code", "c-2")),
            Some(json!({"purpose": "step_up", "user_id": USER, "grant": {}})),
        )
        .await;
        let r = run_flow(
            &m,
            &other.server.uri(),
            Purpose::StepUp,
            Some("admin.elevate"),
            Some(""),
            &Attempt::default(),
            |_| Ok(()),
            Duration::from_secs(5),
        )
        .await;
        assert_eq!(r.unwrap_err(), "SESSION_SERVER_MISMATCH");
        let e = m
            .redeem_passkey_flow(&other.server.uri(), FLOW_ID, "c", "v", true)
            .await
            .unwrap_err();
        assert!(matches!(e, SessionError::ServerMismatch), "{e:?}");
        assert!(other.server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn only_the_servers_own_ceremony_url_is_opened() {
        // Review I2: the server-supplied URL is launched by the OS or passed
        // to a browser's command line, so only its own ceremony page goes.
        for bad in [
            "--gpu-launcher=/bin/sh".to_string(),
            "file:///etc/passwd".to_string(),
            "javascript:alert(1)".to_string(),
            format!("https://evil.example.com/auth/passkeys/desktop/{FLOW_ID}"),
            "OWN/auth/passkeys/desktop/another".to_string(),
            format!("OWN/auth/passkeys/desktop/{FLOW_ID}?x=1"),
            format!("OWN/elsewhere/{FLOW_ID}"),
        ] {
            let f = fixture_with_url(
                Some(("code", "c-1")),
                Some(sign_in_answer()),
                Some(bad.clone()),
            )
            .await;
            let opened = AtomicBool::new(false);
            let r = run_flow(
                &manager(),
                &f.server.uri(),
                Purpose::SignIn(None),
                None,
                None,
                &Attempt::default(),
                |_| {
                    opened.store(true, Ordering::SeqCst);
                    Ok(())
                },
                Duration::from_secs(5),
            )
            .await;
            assert_eq!(r.unwrap_err(), "PASSKEY_FAILED", "{bad}");
            assert!(!opened.load(Ordering::SeqCst), "{bad}");
        }
    }

    #[tokio::test]
    async fn sign_in_sends_the_typed_user_name() {
        // C29 addition (coordinator, 2026-11-03).
        let f = fixture(Some(("code", "c-1")), Some(sign_in_answer())).await;
        run_flow(
            &manager(),
            &f.server.uri(),
            Purpose::SignIn(Some("alice".into())),
            None,
            None,
            &Attempt::default(),
            |_| Ok(()),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        let seen = f.seen.lock().unwrap().clone().unwrap();
        assert_eq!(seen["body"]["username"], "alice");
        let f = fixture(Some(("code", "c-1")), Some(sign_in_answer())).await;
        sign_in(&f, &manager()).await.unwrap();
        assert!(f.seen.lock().unwrap().clone().unwrap()["body"]["username"].is_null());
    }

    #[tokio::test]
    async fn loopback_error_codes_are_filtered() {
        // Review Fable m4: only a bare UPPER_SNAKE code reaches the webview.
        let f = fixture(Some(("error", "lower%3Cb%3E")), None).await;
        assert_eq!(sign_in(&f, &manager()).await.unwrap_err(), "PASSKEY_FAILED");
    }

    #[tokio::test]
    async fn sign_in_redeemed_for_another_account_is_refused() {
        // Review Argon M1: the redeemed user_id must be the adopted account.
        let mut answer = sign_in_answer();
        answer["user_id"] = json!("0192f0c0-0000-7000-8000-00000000ffff");
        let f = fixture(Some(("code", "c-1")), Some(answer)).await;
        let m = manager();
        assert_eq!(sign_in(&f, &m).await.unwrap_err(), "PASSKEY_FAILED");
        assert!(m.auth_info().is_none(), "nothing adopted");
    }

    #[tokio::test]
    async fn step_up_grant_must_match_the_request() {
        // Review Argon minor 1.
        let other = json!({"action": "account.delete", "target": "", "via": "passkey", "expires_at": "2027-01-01T00:00:00Z"});
        let f = fixture(
            Some(("code", "c-2")),
            Some(json!({"purpose": "step_up", "user_id": USER, "grant": other})),
        )
        .await;
        let m = signed_in_at(&f).await;
        let r = run_flow(
            &m,
            &f.server.uri(),
            Purpose::StepUp,
            Some("admin.elevate"),
            Some(""),
            &Attempt::default(),
            |_| Ok(()),
            Duration::from_secs(5),
        )
        .await;
        assert_eq!(r.unwrap_err(), "PASSKEY_FAILED");
    }

    #[tokio::test]
    async fn sign_in_tokens_stay_in_rust() {
        let f = fixture(Some(("code", "c-1")), Some(sign_in_answer())).await;
        let m = manager();
        let FlowResult::SignedIn(result) = sign_in(&f, &m).await.unwrap() else {
            panic!("expected a session");
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(!json.contains("at-1") && !json.contains("rt-1"), "{json}");
        assert_eq!(m.auth_info().unwrap().access_token, "at-1");
    }

    #[tokio::test]
    async fn cancel_before_browser_never_opens_it() {
        let f = fixture(None, None).await;
        let attempt = Attempt::default();
        let id = "cancel-test-attempt";
        let registered = registry().register(id).unwrap();
        registry().cancel(id);
        assert!(registered.is_cancelled());
        let opened = AtomicBool::new(false);
        let r = run_flow(
            &manager(),
            &f.server.uri(),
            Purpose::SignIn(None),
            None,
            None,
            &registered,
            |_| {
                opened.store(true, Ordering::SeqCst);
                Ok(())
            },
            Duration::from_secs(5),
        )
        .await;
        registry().finish(id);
        drop(attempt);
        assert_eq!(r.unwrap_err(), "PASSKEY_CANCELLED");
        assert!(!opened.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn timeout_is_reported() {
        let f = fixture(None, None).await;
        let r = run_flow(
            &manager(),
            &f.server.uri(),
            Purpose::SignIn(None),
            None,
            None,
            &Attempt::default(),
            |_| Ok(()),
            Duration::from_millis(200),
        )
        .await;
        assert_eq!(r.unwrap_err(), "PASSKEY_TIMEOUT");
    }

    #[tokio::test]
    async fn browser_failure_is_reported() {
        let f = fixture(None, None).await;
        let r = run_flow(
            &manager(),
            &f.server.uri(),
            Purpose::SignIn(None),
            None,
            None,
            &Attempt::default(),
            |_| Err("no browser".into()),
            Duration::from_secs(5),
        )
        .await;
        assert_eq!(r.unwrap_err(), "PASSKEY_BROWSER_FAILED");
    }

    #[test]
    #[allow(deprecated)]
    fn browser_setting_selects_the_program() {
        use tauri_plugin_shell::open::Program;

        use super::super::auth::browser_program;
        assert!(browser_program(PasskeyBrowser::System).is_none());
        assert!(matches!(
            browser_program(PasskeyBrowser::Chromium),
            Some(Program::Chromium)
        ));
        assert!(matches!(
            browser_program(PasskeyBrowser::Chrome),
            Some(Program::Chrome)
        ));
        assert!(matches!(
            browser_program(PasskeyBrowser::Firefox),
            Some(Program::Firefox)
        ));
        if cfg!(target_os = "macos") {
            assert_eq!(PasskeyBrowser::Safari.validated(), PasskeyBrowser::Safari);
        } else {
            assert_eq!(PasskeyBrowser::Safari.validated(), PasskeyBrowser::System);
            assert!(browser_program(PasskeyBrowser::Safari).is_none());
        }
        let s: crate::commands::settings::Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.passkey_browser, PasskeyBrowser::System);
        let s: crate::commands::settings::Settings =
            serde_json::from_str(r#"{"passkey_browser": "chromium"}"#).unwrap();
        assert_eq!(s.passkey_browser, PasskeyBrowser::Chromium);
    }
}

#[cfg(test)]
mod ceremony_url_tests {
    #[test]
    fn own_ceremony_url_passes() {
        let id = "4f1c5c43-6a0e-4f9a-9a3e-1f2d3c4b5a69";
        let u = format!("http://127.0.0.1:4100/auth/passkeys/desktop/{id}");
        assert_eq!(
            super::ceremony_url("http://127.0.0.1:4100", &u, id),
            Some(u.clone())
        );
        let h = format!("https://chat.example.com/auth/passkeys/desktop/{id}");
        assert_eq!(
            super::ceremony_url("https://chat.example.com", &h, id),
            Some(h.clone())
        );
        assert_eq!(
            super::ceremony_url("http://chat.example.com", &h.replace("https", "http"), id),
            None
        );
    }
}
