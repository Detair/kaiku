//! Login attempts (C27, SM1) on the desktop.
//!
//! The PC side of "Sign in with your phone", of password and OIDC sign-ins
//! that wait for a phone confirmation, and of the TOTP step. The collection
//! secret is a verifier drawn here: the server sees only `client_challenge =
//! S256(verifier)` at creation and the verifier in the body of every later
//! attempt call. Verifiers never leave Rust; the webview only names the
//! attempt id. A verifier is forgotten on any terminal state, on collect and
//! after ten minutes (the attempt's maximum life).

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use zeroize::Zeroizing;

use super::{
    invalid_response, network, normalize_server_url, AuthResult, SessionError, SessionManager,
    TokenResponse,
};
use crate::commands::oidc_loopback::{new_secret, s256_challenge};

/// Header the server requires on every desktop session-creating call (C27).
pub const DESKTOP_CLIENT: (&str, &str) = ("x-kaiku-client", "desktop");

/// An attempt lives at most ten minutes (C27); its verifier no longer.
pub const ATTEMPT_TTL: Duration = Duration::from_mins(10);
/// At most this many verifiers are kept; the oldest goes first.
pub const MAX_ATTEMPTS: usize = 8;

/// Attempt states after which the attempt can never yield a session.
const TERMINAL_STATES: &[&str] = &["consumed", "denied", "reported", "cancelled", "expired"];

struct Entry {
    base: String,
    verifier: Zeroizing<String>,
    created: Instant,
}

/// Verifiers of the attempts this app started, by attempt id.
#[derive(Default)]
pub struct AttemptVerifiers {
    entries: Mutex<HashMap<String, Entry>>,
}

impl std::fmt::Debug for AttemptVerifiers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let len = self
            .entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len();
        f.debug_struct("AttemptVerifiers")
            .field("entries", &len)
            .field("verifiers", &"[REDACTED]")
            .finish()
    }
}

impl AttemptVerifiers {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Keep `verifier` for `attempt_id` on server `base`.
    pub fn insert(&self, attempt_id: &str, base: &str, verifier: Zeroizing<String>) {
        self.prune(Instant::now());
        let mut entries = self.lock();
        while entries.len() >= MAX_ATTEMPTS {
            let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, e)| e.created)
                .map(|(id, _)| id.clone())
            else {
                break;
            };
            entries.remove(&oldest);
        }
        entries.insert(
            attempt_id.to_string(),
            Entry {
                base: base.to_string(),
                verifier,
                created: Instant::now(),
            },
        );
    }

    /// Server base and verifier of a live attempt.
    pub fn get(&self, attempt_id: &str) -> Option<(String, Zeroizing<String>)> {
        self.prune(Instant::now());
        self.lock()
            .get(attempt_id)
            .map(|e| (e.base.clone(), e.verifier.clone()))
    }

    pub fn remove(&self, attempt_id: &str) {
        self.lock().remove(attempt_id);
    }

    /// Drop entries older than [`ATTEMPT_TTL`] at `now`.
    pub fn prune(&self, now: Instant) {
        self.lock()
            .retain(|_, e| now.saturating_duration_since(e.created) < ATTEMPT_TTL);
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    #[cfg(test)]
    fn backdate(&self, attempt_id: &str, by: Duration) {
        if let Some(e) = self.lock().get_mut(attempt_id) {
            e.created = e.created.checked_sub(by).unwrap_or(e.created);
        }
    }
}

/// `Retry-After` in whole seconds, if the header is a delta-seconds value.
fn retry_after(response: &reqwest::Response) -> Option<u64> {
    response
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// A refused call as its bare server code (`CODE`, or `CODE:<secs>` with a
/// `Retry-After`), so the webview maps it with `errorText`.
pub async fn code_error(response: reqwest::Response) -> SessionError {
    let status = response.status().as_u16();
    let wait = retry_after(&response);
    let text = response.text().await.unwrap_or_default();
    match super::error_code(&text) {
        Some(code) => match wait {
            Some(secs) => SessionError::Server(status, format!("{code}:{secs}")),
            None => SessionError::Server(status, code),
        },
        None => super::server_error(status, &text),
    }
}

/// Parse a JSON answer (an empty body is `null`).
pub async fn json_body(response: reqwest::Response) -> Result<Value, SessionError> {
    let status = response.status().as_u16();
    let text = response.text().await.map_err(|e| network(&e))?;
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&text).map_err(|e| invalid_response(status, &e))
}

fn unknown_attempt() -> SessionError {
    SessionError::Server(404, "LOGIN_ATTEMPT_UNKNOWN".to_string())
}

fn attempt_url(base: &str, attempt_id: &str, step: &str) -> Result<String, SessionError> {
    // Ids are UUIDs; anything else never reaches the server.
    uuid::Uuid::parse_str(attempt_id).map_err(|_| unknown_attempt())?;
    Ok(format!("{base}/auth/login-attempts/{attempt_id}/{step}"))
}

/// Whether a `LoginAttempt` JSON is in a terminal state.
fn is_terminal(attempt: &Value) -> bool {
    attempt
        .get("state")
        .and_then(Value::as_str)
        .is_some_and(|s| TERMINAL_STATES.contains(&s))
}

impl SessionManager {
    /// POST a session-creating call with the desktop header.
    async fn post_desktop(
        &self,
        url: &str,
        body: &Value,
    ) -> Result<reqwest::Response, SessionError> {
        self.http
            .post(url)
            .header(DESKTOP_CLIENT.0, DESKTOP_CLIENT.1)
            .json(body)
            .send()
            .await
            .map_err(|e| network(&e))
    }

    /// Remember the verifier of the attempt in a `LoginStep` or `LoginAttempt`.
    fn keep_verifier(&self, base: &str, value: &Value, verifier: Zeroizing<String>) {
        let attempt = value.get("attempt").unwrap_or(value);
        if let Some(id) = attempt.get("attempt_id").and_then(Value::as_str) {
            if !is_terminal(attempt) {
                self.attempts.insert(id, base, verifier);
            }
        }
    }

    /// Username + password (F3): 202 `LoginStep` JSON or a bare code
    /// (`INVALID_CREDENTIALS` is the neutral answer). Never a session.
    pub async fn login(
        &self,
        server_url: &str,
        username: &str,
        password: &str,
    ) -> Result<Value, SessionError> {
        let base = normalize_server_url(server_url)?;
        let verifier = Zeroizing::new(new_secret());
        let body = json!({
            "username": username,
            "password": password,
            "client_challenge": s256_challenge(&verifier),
        });
        let response = self
            .post_desktop(&format!("{base}/auth/login"), &body)
            .await?;
        if !response.status().is_success() {
            return Err(code_error(response).await);
        }
        let step = json_body(response).await?;
        self.keep_verifier(&base, &step, verifier);
        Ok(step)
    }

    /// "Sign in with your phone": a QR attempt (`LoginAttempt` JSON).
    pub async fn create_qr_attempt(&self, server_url: &str) -> Result<Value, SessionError> {
        let base = normalize_server_url(server_url)?;
        let verifier = Zeroizing::new(new_secret());
        let body = json!({ "client_challenge": s256_challenge(&verifier) });
        let response = self
            .post_desktop(&format!("{base}/auth/login-attempts"), &body)
            .await?;
        if !response.status().is_success() {
            return Err(code_error(response).await);
        }
        let attempt = json_body(response).await?;
        self.keep_verifier(&base, &attempt, verifier);
        Ok(attempt)
    }

    /// One attempt call with the kept verifier. `forget` decides from the
    /// outcome whether the verifier is dropped.
    async fn attempt_call(
        &self,
        attempt_id: &str,
        step: &str,
        extra: Value,
    ) -> Result<(String, Result<Value, SessionError>), SessionError> {
        let (base, verifier) = self.attempts.get(attempt_id).ok_or_else(unknown_attempt)?;
        let mut body = json!({ "verifier": verifier.as_str() });
        if let (Some(target), Value::Object(fields)) = (body.as_object_mut(), extra) {
            target.extend(fields);
        }
        let response = self
            .post_desktop(&attempt_url(&base, attempt_id, step)?, &body)
            .await?;
        let status = response.status().as_u16();
        if !response.status().is_success() {
            let err = code_error(response).await;
            // Unknown or ended: the verifier is worthless now.
            if status == 404 || status == 410 {
                self.attempts.remove(attempt_id);
            }
            return Ok((base, Err(err)));
        }
        Ok((base, json_body(response).await))
    }

    /// Long-poll (`wait` 0..25 s) → `LoginAttempt` JSON.
    pub async fn poll_attempt(&self, attempt_id: &str, wait: u8) -> Result<Value, SessionError> {
        let (_, result) = self
            .attempt_call(attempt_id, "poll", json!({ "wait": wait.min(25) }))
            .await?;
        let attempt = result?;
        if is_terminal(&attempt) {
            self.attempts.remove(attempt_id);
        }
        Ok(attempt)
    }

    /// Collect an approved attempt: the tokens stay here, the webview gets
    /// the user. The verifier is forgotten whatever the outcome.
    pub async fn collect_attempt(&self, attempt_id: &str) -> Result<AuthResult, SessionError> {
        let outcome = self.attempt_call(attempt_id, "collect", json!({})).await;
        self.attempts.remove(attempt_id);
        let (base, result) = outcome?;
        let tokens: TokenResponse =
            serde_json::from_value(result?).map_err(|e| invalid_response(200, &e))?;
        self.adopt_tokens(&base, tokens).await
    }

    /// The TOTP step (F4). A wrong code (`INVALID_MFA`) keeps the attempt.
    pub async fn attempt_totp(
        &self,
        attempt_id: &str,
        code: &str,
    ) -> Result<AuthResult, SessionError> {
        let (base, result) = self
            .attempt_call(attempt_id, "totp", json!({ "code": code }))
            .await?;
        let value = result?;
        self.attempts.remove(attempt_id);
        let tokens: TokenResponse =
            serde_json::from_value(value).map_err(|e| invalid_response(200, &e))?;
        self.adopt_tokens(&base, tokens).await
    }

    /// Give the attempt up; the verifier is forgotten even if the server is unreachable.
    pub async fn cancel_attempt(&self, attempt_id: &str) -> Result<(), SessionError> {
        let outcome = self.attempt_call(attempt_id, "cancel", json!({})).await;
        self.attempts.remove(attempt_id);
        outcome?.1.map(|_| ())
    }

    /// Redeem a desktop SSO hand-off code (`POST /auth/oidc/desktop/exchange`)
    /// → `LoginStep` JSON (C27: OIDC is confirmed like a password). The flow's
    /// verifier is the attempt's collection secret and is kept under the
    /// attempt id.
    pub async fn exchange_oidc_code(
        &self,
        server_url: &str,
        code: &str,
        verifier: Zeroizing<String>,
    ) -> Result<Value, SessionError> {
        let base = normalize_server_url(server_url)?;
        let body = json!({ "code": code, "verifier": verifier.as_str() });
        let response = self
            .post_desktop(&format!("{base}/auth/oidc/desktop/exchange"), &body)
            .await?;
        if !response.status().is_success() {
            return Err(code_error(response).await);
        }
        let step = json_body(response).await?;
        self.keep_verifier(&base, &step, verifier);
        Ok(step)
    }

    #[cfg(test)]
    pub fn attempt_verifiers(&self) -> &AttemptVerifiers {
        &self.attempts
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{json, Value};
    use tokio::sync::RwLock;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    use super::*;
    use crate::session::store::MemoryTokenStore;
    use crate::AuthState;

    const ID: &str = "44444444-4444-4444-8444-444444444444";

    fn manager() -> SessionManager {
        SessionManager::new(
            reqwest::Client::new(),
            Arc::new(MemoryTokenStore::new()),
            Arc::new(RwLock::new(AuthState::default())),
        )
    }

    fn attempt(state: &str) -> Value {
        json!({
            "attempt_id": ID,
            "kind": "password",
            "state": state,
            "expires_at": "2026-11-03T12:10:00Z",
            "qr": null,
            "totp_available": true,
        })
    }

    fn body_of(req: &Request) -> Value {
        serde_json::from_slice(&req.body).unwrap()
    }

    async fn mount_me(server: &MockServer) {
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
    }

    /// Sign in with a password attempt and return the challenge the server saw.
    async fn started(server: &MockServer, m: &SessionManager) -> String {
        Mock::given(method("POST"))
            .and(path("/auth/login"))
            .and(header("x-kaiku-client", "desktop"))
            .respond_with(ResponseTemplate::new(202).set_body_json(
                json!({"status": "confirmation_required", "attempt": attempt("open")}),
            ))
            .mount(server)
            .await;
        m.login(&server.uri(), "alice", "pw").await.unwrap();
        let req = &server.received_requests().await.unwrap()[0];
        body_of(req)["client_challenge"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[tokio::test]
    async fn login_sends_challenge_and_stores_verifier() {
        let server = MockServer::start().await;
        let m = manager();
        let challenge = started(&server, &m).await;
        let req = &server.received_requests().await.unwrap()[0];
        let body = body_of(req);
        assert_eq!(challenge.len(), 43);
        assert!(body.get("mfa_code").is_none());
        assert_eq!(body["username"], "alice");
        let (_, verifier) = m.attempt_verifiers().get(ID).unwrap();
        assert_eq!(s256_challenge(&verifier), challenge);
    }

    #[tokio::test]
    async fn login_result_has_no_verifier() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/login"))
            .respond_with(
                ResponseTemplate::new(202)
                    .set_body_json(json!({"status": "totp_required", "attempt": attempt("open")})),
            )
            .mount(&server)
            .await;
        let m = manager();
        let step = m.login(&server.uri(), "alice", "pw").await.unwrap();
        let (_, verifier) = m.attempt_verifiers().get(ID).unwrap();
        assert!(!step.to_string().contains(verifier.as_str()));
        assert_eq!(step["status"], "totp_required");
    }

    #[tokio::test]
    async fn neutral_answer_is_a_bare_code() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/login"))
            .respond_with(
                ResponseTemplate::new(401)
                    .set_body_json(json!({"error": "INVALID_CREDENTIALS", "message": "x"})),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/auth/login-attempts"))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("Retry-After", "120")
                    .set_body_json(json!({"error": "TOO_MANY_ATTEMPTS", "message": "x"})),
            )
            .mount(&server)
            .await;
        let m = manager();
        let err = m.login(&server.uri(), "alice", "bad").await.unwrap_err();
        assert_eq!(err.to_string(), "INVALID_CREDENTIALS");
        let err = m.create_qr_attempt(&server.uri()).await.unwrap_err();
        assert_eq!(err.to_string(), "TOO_MANY_ATTEMPTS:120");
        assert_eq!(m.attempt_verifiers().len(), 0);
    }

    #[tokio::test]
    async fn poll_collect_and_totp_send_the_verifier() {
        let server = MockServer::start().await;
        let m = manager();
        started(&server, &m).await;
        let (_, verifier) = m.attempt_verifiers().get(ID).unwrap();
        Mock::given(method("POST"))
            .and(path(format!("/auth/login-attempts/{ID}/poll")))
            .respond_with(ResponseTemplate::new(200).set_body_json(attempt("claimed")))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(format!("/auth/login-attempts/{ID}/totp")))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({"error": "INVALID_MFA"})))
            .mount(&server)
            .await;
        m.poll_attempt(ID, 25).await.unwrap();
        let err = m.attempt_totp(ID, "123456").await.unwrap_err();
        assert_eq!(err.to_string(), "INVALID_MFA");
        // A wrong code keeps the attempt.
        assert!(m.attempt_verifiers().get(ID).is_some());
        let requests = server.received_requests().await.unwrap();
        let poll = body_of(&requests[1]);
        assert_eq!(poll["verifier"], verifier.as_str());
        assert_eq!(poll["wait"], 25);
        let totp = body_of(&requests[2]);
        assert_eq!(totp["verifier"], verifier.as_str());
        assert_eq!(totp["code"], "123456");
    }

    #[tokio::test]
    async fn collect_adopts_tokens_and_forgets_the_verifier() {
        let server = MockServer::start().await;
        let m = manager();
        started(&server, &m).await;
        mount_me(&server).await;
        Mock::given(method("POST"))
            .and(path(format!("/auth/login-attempts/{ID}/collect")))
            .and(header("x-kaiku-client", "desktop"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "at-1",
                "refresh_token": "rt-1",
                "expires_in": 900,
                "token_type": "Bearer",
                "session_id": "s-1",
                "assurance": "probation",
                "absolute_expires_at": "2027-02-01T00:00:00Z"
            })))
            .mount(&server)
            .await;
        let result = m.collect_attempt(ID).await.unwrap();
        assert_eq!(result.user["username"], "alice");
        assert_eq!(m.auth_info().unwrap().access_token, "at-1");
        assert!(m.attempt_verifiers().get(ID).is_none());
        assert!(!serde_json::to_string(&result).unwrap().contains("rt-1"));
    }

    #[tokio::test]
    async fn terminal_poll_state_forgets_the_verifier() {
        for state in ["denied", "reported", "cancelled", "expired"] {
            let server = MockServer::start().await;
            let m = manager();
            started(&server, &m).await;
            Mock::given(method("POST"))
                .and(path(format!("/auth/login-attempts/{ID}/poll")))
                .respond_with(ResponseTemplate::new(200).set_body_json(attempt(state)))
                .mount(&server)
                .await;
            let a = m.poll_attempt(ID, 0).await.unwrap();
            assert_eq!(a["state"], state);
            assert!(m.attempt_verifiers().get(ID).is_none(), "{state}");
        }
        // An ended attempt (410) also forgets it.
        let server = MockServer::start().await;
        let m = manager();
        started(&server, &m).await;
        Mock::given(method("POST"))
            .and(path(format!("/auth/login-attempts/{ID}/poll")))
            .respond_with(
                ResponseTemplate::new(410)
                    .set_body_json(json!({"error": "LOGIN_ATTEMPT_ENDED", "state": "expired"})),
            )
            .mount(&server)
            .await;
        assert_eq!(
            m.poll_attempt(ID, 0).await.unwrap_err().to_string(),
            "LOGIN_ATTEMPT_ENDED"
        );
        assert!(m.attempt_verifiers().get(ID).is_none());
    }

    #[tokio::test]
    async fn unknown_attempt_sends_nothing() {
        let server = MockServer::start().await;
        let m = manager();
        let err = m.poll_attempt(ID, 1).await.unwrap_err();
        assert_eq!(err.to_string(), "LOGIN_ATTEMPT_UNKNOWN");
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn cancel_forgets_the_verifier() {
        let server = MockServer::start().await;
        let m = manager();
        started(&server, &m).await;
        Mock::given(method("POST"))
            .and(path(format!("/auth/login-attempts/{ID}/cancel")))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        m.cancel_attempt(ID).await.unwrap();
        assert!(m.attempt_verifiers().get(ID).is_none());
    }

    #[tokio::test]
    async fn oidc_exchange_returns_login_step() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/oidc/desktop/exchange"))
            .and(header("x-kaiku-client", "desktop"))
            .respond_with(ResponseTemplate::new(202).set_body_json(
                json!({"status": "confirmation_required", "attempt": attempt("open")}),
            ))
            .mount(&server)
            .await;
        let m = manager();
        let step = m
            .exchange_oidc_code(&server.uri(), "c-1", Zeroizing::new("v-1".to_string()))
            .await
            .unwrap();
        assert_eq!(step["status"], "confirmation_required");
        let body = body_of(&server.received_requests().await.unwrap()[0]);
        assert_eq!(body, json!({"code": "c-1", "verifier": "v-1"}));
        assert_eq!(m.attempt_verifiers().get(ID).unwrap().1.as_str(), "v-1");
        // No session yet.
        assert!(m.auth_info().is_none());
    }

    #[test]
    fn prune_drops_entries_older_than_ten_minutes() {
        let v = AttemptVerifiers::default();
        v.insert("a", "https://chat.example.com", Zeroizing::new("x".into()));
        v.insert("b", "https://chat.example.com", Zeroizing::new("y".into()));
        v.backdate("a", ATTEMPT_TTL + Duration::from_secs(1));
        v.prune(Instant::now());
        assert!(v.get("a").is_none());
        assert!(v.get("b").is_some());
    }

    #[test]
    fn at_most_eight_verifiers_are_kept() {
        let v = AttemptVerifiers::default();
        for i in 0..10 {
            v.insert(&format!("a{i}"), "b", Zeroizing::new(format!("v{i}")));
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(v.len(), MAX_ATTEMPTS);
        assert!(v.get("a0").is_none());
        assert!(v.get("a9").is_some());
    }

    #[test]
    fn debug_output_redacts_secrets() {
        let v = AttemptVerifiers::default();
        v.insert("a", "b", Zeroizing::new("super-secret-verifier".into()));
        let text = format!("{v:?}");
        assert!(!text.contains("super-secret-verifier"));
        assert!(text.contains("REDACTED"));
    }
}
