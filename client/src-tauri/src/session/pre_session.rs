//! Pre-session secrets on the desktop (C27): the account-request secret
//! (SM3) and the registration ticket (F8).
//!
//! Native clients get these secrets in response bodies (`request_secret`,
//! `registration_ticket`). They stay here: the start answers handed to the
//! webview have them removed, and [`SessionManager::pre_session_request`]
//! adds the matching header (`X-Kaiku-Request-Secret`, `X-Kaiku-Ticket`) to an
//! allowlisted set of request-client and ticket routes. The request secret is
//! kept in the OS keyring (a request waits 24 h or more, across restarts) and
//! deleted when the request finishes, ends or is unknown to the server; the
//! ticket lives in memory for at most its 30 minutes, until something is
//! bound or the server no longer knows it.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};
use tracing::warn;
use zeroize::Zeroizing;

use super::attempts::{code_error, json_body, refuse_token_answer, DESKTOP_CLIENT};
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
    /// The OIDC verifier's one-time code: the verifier goes only into the
    /// start that names this code (Argon review, part 7).
    bound_to: Option<Zeroizing<String>>,
    /// Tells this secret from a later one in the same slot (expiry timer).
    seq: u64,
}

type Slot = Arc<Mutex<Option<Held>>>;

fn lock_slot(slot: &Mutex<Option<Held>>) -> std::sync::MutexGuard<'_, Option<Held>> {
    slot.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Secrets held for the pre-session flows.
#[derive(Default)]
pub struct PreSessionSecrets {
    ticket: Slot,
    oidc_verifier: Slot,
    next_seq: AtomicU64,
}

impl PreSessionSecrets {
    /// Hold `secret` in `slot`; a timer drops (and so zeroizes) it at
    /// `ttl`, even if nothing asks for it again (Codex review m7). Without a
    /// runtime (plain unit tests) the TTL is still checked on every read.
    fn hold(
        &self,
        slot: &Slot,
        base: String,
        secret: Zeroizing<String>,
        bound_to: Option<Zeroizing<String>>,
        ttl: Duration,
    ) {
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        *lock_slot(slot) = Some(Held {
            base,
            secret,
            at: Instant::now(),
            bound_to,
            seq,
        });
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let weak = Arc::downgrade(slot);
        let deadline = tokio::time::Instant::now() + ttl;
        runtime.spawn(async move {
            tokio::time::sleep_until(deadline).await;
            if let Some(slot) = weak.upgrade() {
                let mut held = lock_slot(&slot);
                if held.as_ref().is_some_and(|h| h.seq == seq) {
                    *held = None;
                }
            }
        });
    }
}

impl std::fmt::Debug for PreSessionSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreSessionSecrets")
            .field("ticket", &"[REDACTED]")
            .field("oidc_verifier", &"[REDACTED]")
            .finish()
    }
}

/// Take the secret if it is for `base`, younger than `ttl` and (when it is
/// bound to a code) bound to `code`. A secret for another code stays.
fn take_fresh(
    slot: &Mutex<Option<Held>>,
    base: &str,
    ttl: Duration,
    code: Option<&str>,
) -> Option<Zeroizing<String>> {
    let mut held = lock_slot(slot);
    let expired = held.as_ref().is_some_and(|h| h.at.elapsed() >= ttl);
    let matches = held.as_ref().is_some_and(|h| {
        let bound = match (&h.bound_to, code) {
            (Some(b), Some(c)) => b.as_str() == c,
            (Some(_), None) => false,
            (None, _) => true,
        };
        h.base == base && !expired && bound
    });
    if !matches {
        if expired {
            *held = None;
        }
        return None;
    }
    held.take().map(|h| h.secret)
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

/// The route of the request or ticket itself (not one of its steps).
fn is_resource_route(kind: PreSessionKind, path: &str) -> bool {
    match kind {
        PreSessionKind::Request => path == "/auth/account-requests/current",
        PreSessionKind::Ticket => path == "/auth/registration",
    }
}

fn path_not_allowed() -> SessionError {
    SessionError::InvalidRequest("PATH_NOT_ALLOWED".to_string())
}

fn no_secret() -> SessionError {
    SessionError::Server(404, "NO_PRE_SESSION_SECRET".to_string())
}

fn keyring_unavailable() -> SessionError {
    SessionError::Server(503, "KEYRING_UNAVAILABLE".to_string())
}

/// A 2xx answer without the secret it must carry.
fn invalid_answer(field: &str) -> SessionError {
    warn!(field, "The server's answer lacks its pre-session secret");
    SessionError::Server(502, "INVALID_SERVER_RESPONSE".to_string())
}

/// Move a non-empty string field out of `answer` (the buffer itself goes
/// into `Zeroizing`, no copy stays behind).
fn take_secret(answer: &mut Value, field: &str) -> Option<Zeroizing<String>> {
    match answer.as_object_mut()?.remove(field)? {
        Value::String(s) if !s.is_empty() => Some(Zeroizing::new(s)),
        Value::String(s) => {
            drop(Zeroizing::new(s));
            None
        }
        _ => None,
    }
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
        // A keyring failure is a stable code, never the backend's own text.
        let keyring = |e: super::store::StoreError| {
            warn!(error = %e, "Could not read the account-request secret from the OS keyring");
            keyring_unavailable()
        };
        let base = self
            .store
            .load_secret(REQUEST_SERVER_ENTRY)
            .map_err(keyring)?
            .ok_or_else(no_secret)?;
        let secret = self
            .store
            .load_secret(&request_entry(&base))
            .map_err(keyring)?
            .ok_or_else(no_secret)?;
        Ok((base, Zeroizing::new(secret)))
    }

    fn ticket(&self) -> Result<(String, Zeroizing<String>), SessionError> {
        let held = lock_slot(&self.pre_session.ticket);
        match held.as_ref() {
            Some(h) if h.at.elapsed() < TICKET_TTL => Ok((h.base.clone(), h.secret.clone())),
            _ => Err(no_secret()),
        }
    }

    /// Keep the verifier of a request OIDC factor until the start that
    /// names its one-time `code` sends it (used by `request_oidc_factor`;
    /// single use, two minutes).
    pub fn keep_request_oidc_verifier(
        &self,
        server_url: &str,
        code: &str,
        verifier: Zeroizing<String>,
    ) -> Result<(), SessionError> {
        let base = normalize_server_url(server_url)?;
        self.pre_session.hold(
            &self.pre_session.oidc_verifier,
            base,
            verifier,
            Some(Zeroizing::new(code.to_string())),
            OIDC_FACTOR_TTL,
        );
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
        // A request whose secret cannot be kept could neither be followed
        // nor withdrawn (and would block a new one): check the keyring first,
        // before the single-use OIDC verifier is spent.
        if let Err(e) = self
            .store
            .save_secret(KEYRING_PROBE_ENTRY, "1")
            .and_then(|()| self.store.clear_secret(KEYRING_PROBE_ENTRY))
        {
            warn!(error = %e, "The OS keyring cannot keep an account-request secret");
            return Err(keyring_unavailable());
        }
        for key in ["first", "second"] {
            let factor = body.get(key);
            let is_oidc =
                factor.and_then(|f| f.get("type")).and_then(Value::as_str) == Some("oidc");
            let code = factor
                .and_then(|f| f.get("oidc_code"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            if is_oidc {
                if let Some(verifier) = take_fresh(
                    &self.pre_session.oidc_verifier,
                    &base,
                    OIDC_FACTOR_TTL,
                    code.as_deref(),
                ) {
                    body[key]["verifier"] = json!(verifier.as_str());
                }
            }
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
        refuse_token_answer(&answer)?;
        // Only one request is followed: an older secret (any server) goes,
        // also when this answer lacks one (Argon review, part 9 #2).
        if let Ok(Some(old_base)) = self.store.load_secret(REQUEST_SERVER_ENTRY) {
            self.forget_request_secret(&old_base);
        }
        let Some(secret) = take_secret(&mut answer, "request_secret") else {
            return Err(invalid_answer("request_secret"));
        };
        let saved = self
            .store
            .save_secret(&request_entry(&base), &secret)
            .and_then(|()| self.store.save_secret(REQUEST_SERVER_ENTRY, &base));
        if let Err(e) = saved {
            warn!(error = %e, "Could not keep the account-request secret in the OS keyring");
            return Err(keyring_unavailable());
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
        if !response.status().is_success() {
            // 409 included: `USER_EXISTS` / `EMAIL_TAKEN` reach the webview as codes.
            return Err(code_error(response).await);
        }
        let mut answer = json_body(response).await?;
        refuse_token_answer(&answer)?;
        // A new account replaces an older ticket, also when this answer
        // lacks one (Argon review, part 9 #2).
        *lock_slot(&self.pre_session.ticket) = None;
        let Some(ticket) = take_secret(&mut answer, "registration_ticket") else {
            return Err(invalid_answer("registration_ticket"));
        };
        self.pre_session
            .hold(&self.pre_session.ticket, base, ticket, None, TICKET_TTL);
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
            // The request or ticket itself is unknown, refused or gone: its
            // secret is worthless. Only the resource's own route decides
            // (a wrong code elsewhere is 401 too, and keeps the secret).
            let dead = matches!(response.status().as_u16(), 401 | 404 | 410);
            if dead && is_resource_route(kind, path) {
                self.forget(kind, &base);
            }
            return Err(code_error(response).await);
        }
        let answer = json_body(response).await?;
        refuse_token_answer(&answer)?;
        let state = answer.get("state").and_then(Value::as_str);
        let ended = match kind {
            PreSessionKind::Request => {
                let current = path == "/auth/account-requests/current";
                (current && method == "DELETE")
                    || path == "/auth/account-requests/current/finish"
                    || (current && state.is_some_and(|s| TERMINAL_REQUEST_STATES.contains(&s)))
            }
            // Something is bound: the account now signs in by F4, and the
            // ticket has no further use.
            PreSessionKind::Ticket => {
                path == "/auth/registration/totp/verify"
                    || (path.starts_with("/auth/registration/enrollments") && state == Some("done"))
            }
        };
        if ended {
            self.forget(kind, &base);
        }
        Ok(answer)
    }

    /// Drop the secret `kind` names (request secret for `base`, or the ticket).
    fn forget(&self, kind: PreSessionKind, base: &str) {
        match kind {
            PreSessionKind::Request => self.forget_request_secret(base),
            PreSessionKind::Ticket => *lock_slot(&self.pre_session.ticket) = None,
        }
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
                ResponseTemplate::new(201).set_body_json(
                    json!({"request": request("waiting"), "request_secret": "rs-1"}),
                ),
            )
            .mount(&server)
            .await;
        let store = Arc::new(MemoryTokenStore::new());
        let m = manager(&store);
        m.keep_request_oidc_verifier(&server.uri(), "c", Zeroizing::new("ver-1".into()))
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
        assert!(take_fresh(
            &m.pre_session.oidc_verifier,
            &server.uri(),
            OIDC_FACTOR_TTL,
            Some("c")
        )
        .is_none());
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

    async fn current_answers(server: &MockServer, status: u16, body: Value) {
        Mock::given(method("GET"))
            .and(path("/auth/account-requests/current"))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn a_dead_request_secret_is_forgotten() {
        for (status, code) in [
            (401, "INVALID_TOKEN"),
            (404, "NOT_FOUND"),
            (410, "ACCOUNT_REQUEST_ENDED"),
        ] {
            let server = MockServer::start().await;
            let store = Arc::new(MemoryTokenStore::new());
            let m = started(&server, &store).await;
            current_answers(&server, status, json!({"error": code, "message": "x"})).await;
            let err = m
                .pre_session_request(
                    PreSessionKind::Request,
                    "GET",
                    "/auth/account-requests/current",
                    None,
                )
                .await
                .unwrap_err();
            assert_eq!(err.to_string(), code);
            assert!(
                store
                    .load_secret(&request_entry(&server.uri()))
                    .unwrap()
                    .is_none(),
                "{status}"
            );
            assert!(store.load_secret(REQUEST_SERVER_ENTRY).unwrap().is_none());
        }
    }

    #[tokio::test]
    async fn a_refused_step_keeps_the_request_secret() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryTokenStore::new());
        let m = started(&server, &store).await;
        Mock::given(method("POST"))
            .and(path("/auth/account-requests/current/totp/verify"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({"error": "INVALID_MFA"})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!(
                "/auth/account-requests/current/enrollments/{ID}"
            )))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({"error": "NOT_FOUND"})))
            .mount(&server)
            .await;
        m.pre_session_request(
            PreSessionKind::Request,
            "POST",
            "/auth/account-requests/current/totp/verify",
            Some(json!({"code": "123456"})),
        )
        .await
        .unwrap_err();
        m.pre_session_request(
            PreSessionKind::Request,
            "GET",
            &format!("/auth/account-requests/current/enrollments/{ID}"),
            None,
        )
        .await
        .unwrap_err();
        assert!(store
            .load_secret(&request_entry(&server.uri()))
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn a_new_request_elsewhere_drops_the_old_secret() {
        let first = MockServer::start().await;
        let store = Arc::new(MemoryTokenStore::new());
        started(&first, &store).await;
        let second = MockServer::start().await;
        started(&second, &store).await;
        assert!(store
            .load_secret(&request_entry(&first.uri()))
            .unwrap()
            .is_none());
        assert_eq!(
            store.load_secret(REQUEST_SERVER_ENTRY).unwrap().as_deref(),
            Some(second.uri().as_str())
        );
    }

    #[tokio::test]
    async fn keyring_failure_keeps_the_oidc_verifier() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryTokenStore::new());
        let m = manager(&store);
        m.keep_request_oidc_verifier(&server.uri(), "c", Zeroizing::new("ver-1".into()))
            .unwrap();
        store.set_fail_save(true);
        let err = m
            .account_request_start(
                &server.uri(),
                json!({"username": "alice", "purpose": "bootstrap", "first": {"type": "oidc", "oidc_code": "c"}}),
            )
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "KEYRING_UNAVAILABLE");
        assert!(take_fresh(
            &m.pre_session.oidc_verifier,
            &server.uri(),
            OIDC_FACTOR_TTL,
            Some("c")
        )
        .is_some());
    }

    async fn registered(server: &MockServer) -> SessionManager {
        Mock::given(method("POST"))
            .and(path("/auth/register"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                "user_id": "u1", "username": "neu", "ticket_expires_at": "t", "registration_ticket": "tick-1"
            })))
            .mount(server)
            .await;
        let m = manager(&Arc::new(MemoryTokenStore::new()));
        m.register(RegisterRequest {
            server_url: server.uri(),
            username: "neu".into(),
            email: None,
            password: "pw-123456".into(),
            display_name: None,
        })
        .await
        .unwrap();
        assert!(m.has_ticket());
        m
    }

    #[tokio::test]
    async fn the_ticket_is_forgotten_once_something_is_bound() {
        let server = MockServer::start().await;
        let m = registered(&server).await;
        Mock::given(method("POST"))
            .and(path("/auth/registration/totp/verify"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"recovery_codes": ["A"]})),
            )
            .mount(&server)
            .await;
        m.pre_session_request(
            PreSessionKind::Ticket,
            "POST",
            "/auth/registration/totp/verify",
            Some(json!({"code": "123456"})),
        )
        .await
        .unwrap();
        assert!(!m.has_ticket());

        let server = MockServer::start().await;
        let m = registered(&server).await;
        Mock::given(method("GET"))
            .and(path(format!("/auth/registration/enrollments/{ID}")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"enrollment_id": ID, "state": "claimed"})),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(format!("/auth/registration/enrollments/{ID}/confirm")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"enrollment_id": ID, "state": "done"})),
            )
            .mount(&server)
            .await;
        let enrollment = format!("/auth/registration/enrollments/{ID}");
        m.pre_session_request(PreSessionKind::Ticket, "GET", &enrollment, None)
            .await
            .unwrap();
        assert!(m.has_ticket());
        m.pre_session_request(
            PreSessionKind::Ticket,
            "POST",
            &format!("{enrollment}/confirm"),
            None,
        )
        .await
        .unwrap();
        assert!(!m.has_ticket());
    }

    #[tokio::test]
    async fn a_dead_ticket_is_forgotten_but_a_wrong_code_keeps_it() {
        let server = MockServer::start().await;
        let m = registered(&server).await;
        Mock::given(method("POST"))
            .and(path("/auth/registration/totp/verify"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({"error": "INVALID_MFA"})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/auth/registration"))
            .respond_with(
                ResponseTemplate::new(410).set_body_json(json!({"error": "TICKET_EXPIRED"})),
            )
            .mount(&server)
            .await;
        m.pre_session_request(
            PreSessionKind::Ticket,
            "POST",
            "/auth/registration/totp/verify",
            Some(json!({"code": "000000"})),
        )
        .await
        .unwrap_err();
        assert!(m.has_ticket());
        m.pre_session_request(PreSessionKind::Ticket, "GET", "/auth/registration", None)
            .await
            .unwrap_err();
        assert!(!m.has_ticket());
    }

    /// Argon review: a 409 passes its server code (`USER_EXISTS`,
    /// `EMAIL_TAKEN`) so the webview words it; no English text from Rust.
    #[tokio::test]
    async fn register_conflict_passes_the_server_code() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/register"))
            .respond_with(
                ResponseTemplate::new(409)
                    .set_body_json(json!({"error": "EMAIL_TAKEN", "message": "Email taken"})),
            )
            .mount(&server)
            .await;
        let err = manager(&Arc::new(MemoryTokenStore::new()))
            .register(RegisterRequest {
                server_url: server.uri(),
                username: "neu".into(),
                email: Some("a@example.com".into()),
                password: "pw-123456".into(),
                display_name: None,
            })
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "EMAIL_TAKEN");
    }

    fn token_body() -> Value {
        json!({"access_token": "at-secret-1", "refresh_token": "rt-secret-1",
               "expires_in": 900, "token_type": "Bearer"})
    }

    #[tokio::test]
    async fn a_pre_l1b_token_answer_to_register_is_server_outdated() {
        // Fable review M1: the token pair never reaches the webview.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/register"))
            .respond_with(ResponseTemplate::new(200).set_body_json(token_body()))
            .mount(&server)
            .await;
        let m = manager(&Arc::new(MemoryTokenStore::new()));
        let err = m
            .register(RegisterRequest {
                server_url: server.uri(),
                username: "neu".into(),
                email: None,
                password: "pw-123456".into(),
                display_name: None,
            })
            .await
            .unwrap_err();
        let text = format!("{err} {err:?}");
        assert_eq!(err.to_string(), "SERVER_OUTDATED");
        assert!(
            !text.contains("at-secret-1") && !text.contains("rt-secret-1"),
            "{text}"
        );
        assert!(!m.has_ticket());
    }

    #[tokio::test]
    async fn a_token_answer_to_a_pre_session_request_is_refused() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryTokenStore::new());
        let m = started(&server, &store).await;
        Mock::given(method("POST"))
            .and(path("/auth/account-requests/current/finish"))
            .respond_with(ResponseTemplate::new(200).set_body_json(token_body()))
            .mount(&server)
            .await;
        let err = m
            .pre_session_request(
                PreSessionKind::Request,
                "POST",
                "/auth/account-requests/current/finish",
                None,
            )
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "SERVER_OUTDATED");
    }

    #[tokio::test]
    async fn a_start_without_request_secret_fails_and_drops_the_old_one() {
        // Argon review part 9 #2: never follow an older request silently.
        let server = MockServer::start().await;
        let store = Arc::new(MemoryTokenStore::new());
        let m = started(&server, &store).await;
        server.reset().await;
        Mock::given(method("POST"))
            .and(path("/auth/account-requests"))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(json!({"request": request("waiting")})),
            )
            .mount(&server)
            .await;
        let err = m
            .account_request_start(
                &server.uri(),
                json!({"username": "bob", "purpose": "recovery"}),
            )
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "INVALID_SERVER_RESPONSE");
        assert!(store.load_secret(REQUEST_SERVER_ENTRY).unwrap().is_none());
        assert!(store
            .load_secret(&request_entry(&server.uri()))
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn a_register_without_ticket_fails_and_drops_the_old_ticket() {
        let server = MockServer::start().await;
        let m = registered(&server).await;
        server.reset().await;
        Mock::given(method("POST"))
            .and(path("/auth/register"))
            .respond_with(ResponseTemplate::new(201).set_body_json(
                json!({"user_id": "u2", "username": "zwei", "ticket_expires_at": "t"}),
            ))
            .mount(&server)
            .await;
        let err = m
            .register(RegisterRequest {
                server_url: server.uri(),
                username: "zwei".into(),
                email: None,
                password: "pw-123456".into(),
                display_name: None,
            })
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "INVALID_SERVER_RESPONSE");
        assert!(!m.has_ticket());
    }

    #[tokio::test]
    async fn a_keyring_read_failure_is_a_stable_code() {
        // Argon review part 9 #5: no backend text reaches the webview.
        let store = Arc::new(MemoryTokenStore::new());
        store.set_fail_load_secret(true);
        let m = manager(&store);
        let err = m
            .pre_session_request(
                PreSessionKind::Request,
                "GET",
                "/auth/account-requests/current",
                None,
            )
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "KEYRING_UNAVAILABLE");
    }

    #[tokio::test]
    async fn the_oidc_verifier_goes_only_to_the_start_naming_its_code() {
        // Argon review part 7: a stale code gets no verifier, the newer one stays.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/account-requests"))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(
                    json!({"request": request("waiting"), "request_secret": "rs-1"}),
                ),
            )
            .mount(&server)
            .await;
        let store = Arc::new(MemoryTokenStore::new());
        let m = manager(&store);
        m.keep_request_oidc_verifier(&server.uri(), "new-code", Zeroizing::new("ver-new".into()))
            .unwrap();
        m.account_request_start(
            &server.uri(),
            json!({"username": "alice", "purpose": "bootstrap", "first": {"type": "oidc", "oidc_code": "old-code"}, "totp_code": "123456"}),
        )
        .await
        .unwrap();
        let sent: Value =
            serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
        assert!(sent["first"].get("verifier").is_none());
        assert!(take_fresh(
            &m.pre_session.oidc_verifier,
            &server.uri(),
            OIDC_FACTOR_TTL,
            Some("new-code")
        )
        .is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn held_secrets_are_dropped_at_their_deadline() {
        // Codex review m7: not only when read again.
        let s = PreSessionSecrets::default();
        s.hold(
            &s.ticket,
            "b".into(),
            Zeroizing::new("tick".into()),
            None,
            TICKET_TTL,
        );
        s.hold(
            &s.oidc_verifier,
            "b".into(),
            Zeroizing::new("ver".into()),
            Some(Zeroizing::new("c".into())),
            OIDC_FACTOR_TTL,
        );
        tokio::time::advance(OIDC_FACTOR_TTL + Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        assert!(lock_slot(&s.oidc_verifier).is_none());
        assert!(lock_slot(&s.ticket).is_some());
        tokio::time::advance(TICKET_TTL).await;
        tokio::task::yield_now().await;
        assert!(lock_slot(&s.ticket).is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn a_newer_secret_outlives_the_older_ones_timer() {
        let s = PreSessionSecrets::default();
        s.hold(
            &s.ticket,
            "b".into(),
            Zeroizing::new("old".into()),
            None,
            TICKET_TTL,
        );
        tokio::time::advance(TICKET_TTL - Duration::from_secs(5)).await;
        s.hold(
            &s.ticket,
            "b".into(),
            Zeroizing::new("new".into()),
            None,
            TICKET_TTL,
        );
        tokio::time::advance(Duration::from_secs(10)).await;
        tokio::task::yield_now().await;
        assert_eq!(
            lock_slot(&s.ticket).as_ref().map(|h| h.secret.as_str()),
            Some("new")
        );
    }

    #[test]
    fn debug_output_redacts_secrets() {
        let s = PreSessionSecrets::default();
        s.hold(
            &s.ticket,
            "b".into(),
            Zeroizing::new("tick-secret".into()),
            None,
            TICKET_TTL,
        );
        assert!(!format!("{s:?}").contains("tick-secret"));
    }
}
