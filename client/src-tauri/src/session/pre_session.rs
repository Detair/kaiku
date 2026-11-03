//! Pre-session secrets on the desktop (C27): the account-request secret
//! (SM3) and the registration ticket (F8).
//!
//! Native clients get these secrets in response bodies (`request_secret`,
//! `registration_ticket`). They stay here: the start answers handed to the
//! webview have them removed, and [`SessionManager::pre_session_request`]
//! adds the matching header (`X-Kaiku-Request-Secret`, `X-Kaiku-Ticket`) to an
//! allowlisted set of request-client and ticket routes. The request secret is
//! kept in the OS keyring (a request waits 24 h or more, across restarts) and
//! deleted when the request finishes or ends; the ticket lives in memory for
//! its 30 minutes.

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};
use tracing::warn;
use zeroize::Zeroizing;

use super::attempts::{code_error, json_body, DESKTOP_CLIENT};
use super::{network, normalize_server_url, RegisterRequest, SessionError, SessionManager};

/// Which secret a pre-session request carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreSessionKind {
    /// The account-request secret (request client, SM3).
    Request,
    /// The registration ticket (F8).
    Ticket,
}

/// A registration ticket lives 30 minutes (C27).
pub const TICKET_TTL: Duration = Duration::from_mins(30);
/// An OIDC factor's verifier is used within the 60-s life of its code.
const OIDC_FACTOR_TTL: Duration = Duration::from_mins(2);

/// Written and removed once before a request starts, to know the keyring works.
const KEYRING_PROBE_ENTRY: &str = "account_request_probe";

/// Keyring entry naming the server of the open request.
pub const REQUEST_SERVER_ENTRY: &str = "account_request_server";

/// Keyring entry of the request secret for `base`.
pub fn request_entry(base: &str) -> String {
    format!("account_request:{base}")
}

/// Request states after which the secret is worthless.
const TERMINAL_REQUEST_STATES: &[&str] =
    &["completed", "cancelled", "withdrawn", "expired", "void"];

struct Held {
    base: String,
    secret: Zeroizing<String>,
    at: Instant,
}

/// Secrets held for the pre-session flows.
#[derive(Default)]
pub struct PreSessionSecrets {
    ticket: Mutex<Option<Held>>,
    oidc_verifier: Mutex<Option<Held>>,
}

impl std::fmt::Debug for PreSessionSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreSessionSecrets")
            .field("ticket", &"[REDACTED]")
            .field("oidc_verifier", &"[REDACTED]")
            .finish()
    }
}

fn take_fresh(slot: &Mutex<Option<Held>>, base: &str, ttl: Duration) -> Option<Zeroizing<String>> {
    let mut held = slot.lock().unwrap_or_else(PoisonError::into_inner);
    match held.take() {
        Some(h) if h.base == base && h.at.elapsed() < ttl => Some(h.secret),
        _ => None,
    }
}

fn is_uuid(segment: &str) -> bool {
    segment.len() == 36 && uuid::Uuid::parse_str(segment).is_ok()
}

/// The only routes a pre-session secret is sent to (exact method + path, no
/// query). `None` refuses without a request.
pub fn allowed(kind: PreSessionKind, method: &str, path: &str) -> Option<reqwest::Method> {
    if path.contains(['?', '#']) {
        return None;
    }
    let rest = match kind {
        PreSessionKind::Request => path.strip_prefix("/auth/account-requests/current")?,
        PreSessionKind::Ticket => path.strip_prefix("/auth/registration")?,
    };
    let segments: Vec<&str> = if rest.is_empty() {
        Vec::new()
    } else {
        rest.strip_prefix('/')?.split('/').collect()
    };
    let ok = match (kind, method, segments.as_slice()) {
        (PreSessionKind::Request, "GET" | "DELETE", [])
        | (PreSessionKind::Request, "PUT", ["password" | "recovery-contact"])
        | (PreSessionKind::Request, "POST", ["totp" | "finish"] | ["totp", "verify"])
        | (PreSessionKind::Ticket, "GET", [])
        | (PreSessionKind::Ticket, "POST", ["totp"] | ["totp", "verify"])
        | (_, "POST", ["enrollments"]) => true,
        (_, "GET" | "DELETE", ["enrollments", id]) => is_uuid(id),
        (_, "POST", ["enrollments", id, "confirm"]) => is_uuid(id),
        _ => false,
    };
    if !ok {
        return None;
    }
    reqwest::Method::from_bytes(method.as_bytes()).ok()
}

fn path_not_allowed() -> SessionError {
    SessionError::InvalidRequest("PATH_NOT_ALLOWED".to_string())
}

fn no_secret() -> SessionError {
    SessionError::Server(404, "NO_PRE_SESSION_SECRET".to_string())
}

impl SessionManager {
    fn forget_request_secret(&self, base: &str) {
        for entry in [request_entry(base), REQUEST_SERVER_ENTRY.to_string()] {
            if let Err(e) = self.store.clear_secret(&entry) {
                warn!(error = %e, "Could not remove the account-request secret from the OS keyring");
            }
        }
    }

    fn request_secret(&self) -> Result<(String, Zeroizing<String>), SessionError> {
        let base = self
            .store
            .load_secret(REQUEST_SERVER_ENTRY)
            .map_err(|e| SessionError::Network(e.to_string()))?
            .ok_or_else(no_secret)?;
        let secret = self
            .store
            .load_secret(&request_entry(&base))
            .map_err(|e| SessionError::Network(e.to_string()))?
            .ok_or_else(no_secret)?;
        Ok((base, Zeroizing::new(secret)))
    }

    fn ticket(&self) -> Result<(String, Zeroizing<String>), SessionError> {
        let held = self
            .pre_session
            .ticket
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        match held.as_ref() {
            Some(h) if h.at.elapsed() < TICKET_TTL => Ok((h.base.clone(), h.secret.clone())),
            _ => Err(no_secret()),
        }
    }

    /// Keep the verifier of a request OIDC factor until the start sends it
    /// (used by `request_oidc_factor`; single use, two minutes).
    pub fn keep_request_oidc_verifier(
        &self,
        server_url: &str,
        verifier: Zeroizing<String>,
    ) -> Result<(), SessionError> {
        let base = normalize_server_url(server_url)?;
        *self
            .pre_session
            .oidc_verifier
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Held {
            base,
            secret: verifier,
            at: Instant::now(),
        });
        Ok(())
    }

    /// Start an account request (recovery or bootstrap). The answer's
    /// `request_secret` goes to the keyring and is removed from the result. A
    /// factor of `type: "oidc"` gets the kept verifier added.
    pub async fn account_request_start(
        &self,
        server_url: &str,
        mut body: Value,
    ) -> Result<Value, SessionError> {
        let base = normalize_server_url(server_url)?;
        for key in ["first", "second"] {
            let is_oidc = body
                .get(key)
                .and_then(|f| f.get("type"))
                .and_then(Value::as_str)
                == Some("oidc");
            if is_oidc {
                if let Some(verifier) =
                    take_fresh(&self.pre_session.oidc_verifier, &base, OIDC_FACTOR_TTL)
                {
                    body[key]["verifier"] = json!(verifier.as_str());
                }
            }
        }
        // A request whose secret cannot be kept could neither be followed
        // nor withdrawn (and would block a new one): check the keyring first.
        if let Err(e) = self
            .store
            .save_secret(KEYRING_PROBE_ENTRY, "1")
            .and_then(|()| self.store.clear_secret(KEYRING_PROBE_ENTRY))
        {
            warn!(error = %e, "The OS keyring cannot keep an account-request secret");
            return Err(SessionError::Server(503, "KEYRING_UNAVAILABLE".to_string()));
        }
        let response = self
            .http
            .post(format!("{base}/auth/account-requests"))
            .header(DESKTOP_CLIENT.0, DESKTOP_CLIENT.1)
            .json(&body)
            .send()
            .await
            .map_err(|e| network(&e))?;
        if !response.status().is_success() {
            return Err(code_error(response).await);
        }
        let mut answer = json_body(response).await?;
        let secret = answer
            .as_object_mut()
            .and_then(|o| o.remove("request_secret"))
            .and_then(|v| v.as_str().map(|s| Zeroizing::new(s.to_string())));
        if let Some(secret) = secret {
            let saved = self
                .store
                .save_secret(&request_entry(&base), &secret)
                .and_then(|()| self.store.save_secret(REQUEST_SERVER_ENTRY, &base));
            if let Err(e) = saved {
                warn!(error = %e, "Could not keep the account-request secret in the OS keyring");
                return Err(SessionError::Server(503, "KEYRING_UNAVAILABLE".to_string()));
            }
        }
        Ok(answer)
    }

    /// `POST /auth/register` (F8): never a session. The ticket stays here.
    pub async fn register(&self, req: RegisterRequest) -> Result<Value, SessionError> {
        let base = normalize_server_url(&req.server_url)?;
        let response = self
            .http
            .post(format!("{base}/auth/register"))
            .header(DESKTOP_CLIENT.0, DESKTOP_CLIENT.1)
            .json(&json!({
                "username": req.username,
                "email": req.email,
                "password": req.password,
                "display_name": req.display_name,
            }))
            .send()
            .await
            .map_err(|e| network(&e))?;
        if response.status().as_u16() == 409 {
            return Err(SessionError::Server(
                409,
                "Username or email already exists".to_string(),
            ));
        }
        if !response.status().is_success() {
            return Err(code_error(response).await);
        }
        let mut answer = json_body(response).await?;
        if let Some(ticket) = answer
            .as_object_mut()
            .and_then(|o| o.remove("registration_ticket"))
            .and_then(|v| v.as_str().map(|s| Zeroizing::new(s.to_string())))
        {
            *self
                .pre_session
                .ticket
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = Some(Held {
                base,
                secret: ticket,
                at: Instant::now(),
            });
        }
        Ok(answer)
    }

    /// An allowlisted request-client or ticket call with its secret header.
    pub async fn pre_session_request(
        &self,
        kind: PreSessionKind,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, SessionError> {
        let verb = allowed(kind, method, path).ok_or_else(path_not_allowed)?;
        let (base, secret, header) = match kind {
            PreSessionKind::Request => {
                let (base, secret) = self.request_secret()?;
                (base, secret, "x-kaiku-request-secret")
            }
            PreSessionKind::Ticket => {
                let (base, secret) = self.ticket()?;
                (base, secret, "x-kaiku-ticket")
            }
        };
        let mut request = self
            .http
            .request(verb, format!("{base}{path}"))
            .header(DESKTOP_CLIENT.0, DESKTOP_CLIENT.1)
            .header(header, secret.as_str());
        if let Some(body) = &body {
            request = request.json(body);
        }
        let response = request.send().await.map_err(|e| network(&e))?;
        if !response.status().is_success() {
            return Err(code_error(response).await);
        }
        let answer = json_body(response).await?;
        if kind == PreSessionKind::Request {
            let current = path == "/auth/account-requests/current";
            let ended = (current && method == "DELETE")
                || path == "/auth/account-requests/current/finish"
                || (current
                    && answer
                        .get("state")
                        .and_then(Value::as_str)
                        .is_some_and(|s| TERMINAL_REQUEST_STATES.contains(&s)));
            if ended {
                self.forget_request_secret(&base);
            }
        }
        Ok(answer)
    }

    #[cfg(test)]
    pub fn has_ticket(&self) -> bool {
        self.ticket().is_ok()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use tokio::sync::RwLock;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::session::store::{MemoryTokenStore, TokenStore};
    use crate::AuthState;

    const ID: &str = "55555555-5555-4555-8555-555555555555";

    fn manager(store: &Arc<MemoryTokenStore>) -> SessionManager {
        SessionManager::new(
            reqwest::Client::new(),
            store.clone(),
            Arc::new(RwLock::new(AuthState::default())),
        )
    }

    fn request(state: &str) -> Value {
        json!({"id": "r1", "kind": "self", "purpose": "recovery", "state": state})
    }

    async fn started(server: &MockServer, store: &Arc<MemoryTokenStore>) -> SessionManager {
        Mock::given(method("POST"))
            .and(path("/auth/account-requests"))
            .and(header("x-kaiku-client", "desktop"))
            .respond_with(ResponseTemplate::new(201).set_body_json(
                json!({"request": request("waiting"), "request_secret": "req-secret-1"}),
            ))
            .mount(server)
            .await;
        let m = manager(store);
        let answer = m
            .account_request_start(
                &server.uri(),
                json!({"username": "alice", "purpose": "recovery"}),
            )
            .await
            .unwrap();
        assert!(answer.get("request_secret").is_none());
        m
    }

    #[tokio::test]
    async fn account_request_start_keeps_the_secret_out_of_the_result() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryTokenStore::new());
        let m = started(&server, &store).await;
        assert_eq!(
            store
                .load_secret(&request_entry(&server.uri()))
                .unwrap()
                .as_deref(),
            Some("req-secret-1")
        );
        drop(m);
    }

    #[tokio::test]
    async fn request_oidc_factor_keeps_the_verifier_and_start_adds_it() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/account-requests"))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(json!({"request": request("waiting")})),
            )
            .mount(&server)
            .await;
        let store = Arc::new(MemoryTokenStore::new());
        let m = manager(&store);
        m.keep_request_oidc_verifier(&server.uri(), Zeroizing::new("ver-1".into()))
            .unwrap();
        m.account_request_start(
            &server.uri(),
            json!({"username": "alice", "purpose": "bootstrap", "first": {"type": "oidc", "oidc_code": "c"}, "totp_code": "123456"}),
        )
        .await
        .unwrap();
        let sent: Value =
            serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
        assert_eq!(sent["first"]["verifier"], "ver-1");
        // Single use.
        assert!(take_fresh(&m.pre_session.oidc_verifier, &server.uri(), OIDC_FACTOR_TTL).is_none());
    }

    #[tokio::test]
    async fn keyring_failure_refuses_before_starting_a_request() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/account-requests"))
            .respond_with(
                ResponseTemplate::new(201)
                    .set_body_json(json!({"request": request("waiting"), "request_secret": "s"})),
            )
            .expect(0)
            .mount(&server)
            .await;
        let store = Arc::new(MemoryTokenStore::new());
        store.set_fail_save(true);
        let err = manager(&store)
            .account_request_start(&server.uri(), json!({"username": "alice"}))
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "KEYRING_UNAVAILABLE");
    }

    #[tokio::test]
    async fn pre_session_request_adds_the_secret_header() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryTokenStore::new());
        let m = started(&server, &store).await;
        Mock::given(method("GET"))
            .and(path("/auth/account-requests/current"))
            .and(header("x-kaiku-request-secret", "req-secret-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(request("ready")))
            .expect(1)
            .mount(&server)
            .await;
        let r = m
            .pre_session_request(
                PreSessionKind::Request,
                "GET",
                "/auth/account-requests/current",
                None,
            )
            .await
            .unwrap();
        assert_eq!(r["state"], "ready");
        // Still kept: the request is open.
        assert!(store
            .load_secret(&request_entry(&server.uri()))
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn terminal_request_or_finish_deletes_the_secret() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryTokenStore::new());
        let m = started(&server, &store).await;
        Mock::given(method("POST"))
            .and(path("/auth/account-requests/current/finish"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    json!({"request": request("completed"), "recovery_codes": ["A"]}),
                ),
            )
            .mount(&server)
            .await;
        m.pre_session_request(
            PreSessionKind::Request,
            "POST",
            "/auth/account-requests/current/finish",
            None,
        )
        .await
        .unwrap();
        assert!(store
            .load_secret(&request_entry(&server.uri()))
            .unwrap()
            .is_none());

        let server = MockServer::start().await;
        let store = Arc::new(MemoryTokenStore::new());
        let m = started(&server, &store).await;
        Mock::given(method("GET"))
            .and(path("/auth/account-requests/current"))
            .respond_with(ResponseTemplate::new(200).set_body_json(request("void")))
            .mount(&server)
            .await;
        m.pre_session_request(
            PreSessionKind::Request,
            "GET",
            "/auth/account-requests/current",
            None,
        )
        .await
        .unwrap();
        assert!(store.load_secret(REQUEST_SERVER_ENTRY).unwrap().is_none());
    }

    #[tokio::test]
    async fn pre_session_request_refuses_paths_outside_the_allowlist() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryTokenStore::new());
        let m = started(&server, &store).await;
        for (kind, verb, p) in [
            (PreSessionKind::Request, "GET", "/auth/me"),
            (
                PreSessionKind::Request,
                "GET",
                "/auth/account-requests/current/../../me",
            ),
            (PreSessionKind::Request, "POST", "/auth/account-requests"),
            (
                PreSessionKind::Request,
                "GET",
                "/auth/account-requests/current?x=1",
            ),
            (
                PreSessionKind::Request,
                "POST",
                "/auth/account-requests/current",
            ),
            (
                PreSessionKind::Request,
                "GET",
                "/auth/account-requests/current/enrollments/not-a-uuid",
            ),
            (PreSessionKind::Request, "GET", "/auth/registration"),
            (
                PreSessionKind::Ticket,
                "GET",
                "/auth/account-requests/current",
            ),
            (PreSessionKind::Ticket, "DELETE", "/auth/registration"),
            (PreSessionKind::Ticket, "GET", "/api/admin/users"),
        ] {
            let err = m
                .pre_session_request(kind, verb, p, None)
                .await
                .unwrap_err();
            assert_eq!(err.to_string(), "PATH_NOT_ALLOWED", "{verb} {p}");
        }
        // Only the start reached the server.
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
        assert!(allowed(
            PreSessionKind::Request,
            "POST",
            &format!("/auth/account-requests/current/enrollments/{ID}/confirm")
        )
        .is_some());
        assert!(allowed(
            PreSessionKind::Ticket,
            "POST",
            "/auth/registration/totp/verify"
        )
        .is_some());
        assert!(allowed(
            PreSessionKind::Ticket,
            "DELETE",
            &format!("/auth/registration/enrollments/{ID}")
        )
        .is_some());
    }

    #[tokio::test]
    async fn register_keeps_the_ticket_and_uses_it() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/register"))
            .and(header("x-kaiku-client", "desktop"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                "user_id": "u1", "username": "neu", "ticket_expires_at": "t", "registration_ticket": "tick-1"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/auth/registration"))
            .and(header("x-kaiku-ticket", "tick-1"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"expires_at": "t", "bound": false})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let store = Arc::new(MemoryTokenStore::new());
        let m = manager(&store);
        let answer = m
            .register(RegisterRequest {
                server_url: server.uri(),
                username: "neu".into(),
                email: None,
                password: "pw-123456".into(),
                display_name: None,
            })
            .await
            .unwrap();
        assert!(answer.get("registration_ticket").is_none());
        assert_eq!(answer["user_id"], "u1");
        assert!(m.has_ticket());
        assert!(m.auth_info().is_none());
        let status = m
            .pre_session_request(PreSessionKind::Ticket, "GET", "/auth/registration", None)
            .await
            .unwrap();
        assert_eq!(status["bound"], false);
    }

    #[test]
    fn debug_output_redacts_secrets() {
        let s = PreSessionSecrets::default();
        *s.ticket.lock().unwrap() = Some(Held {
            base: "b".into(),
            secret: Zeroizing::new("tick-secret".into()),
            at: Instant::now(),
        });
        assert!(!format!("{s:?}").contains("tick-secret"));
    }
}
