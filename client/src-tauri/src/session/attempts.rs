//! Login attempts (C27, SM1) on the desktop.
//!
//! The PC side of "Sign in with your phone", of password and OIDC sign-ins
//! that wait for a phone confirmation, and of the TOTP step. The collection
//! secret is a verifier drawn here: the server sees only `client_challenge =
//! S256(verifier)` at creation and the verifier in the body of every later
//! attempt call. Verifiers never leave Rust; the webview only names the
//! attempt id. A verifier is forgotten on any terminal state, when its
//! session is taken over, on cancel and ten minutes after it was drawn (the
//! attempt's maximum life, by a timer of its own). A verifier present here
//! also means the webview still wants the attempt: a collect or TOTP answer
//! that arrives after a cancel is never adopted.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
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
    /// Tells this entry from a later one under the same id (expiry timer).
    seq: u64,
    /// A collect or TOTP step is in flight (or adopting its session): only a
    /// cancel removes the entry then. A long-poll that wakes with `consumed`
    /// (the server's answer to that very step), a late 404/410 of that poll
    /// or the attempt's 10 minutes must not make its own session unwanted
    /// (Argon review, part 8 #1).
    busy: u32,
}

type Entries = Mutex<HashMap<String, Entry>>;

fn lock_entries(entries: &Entries) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
    entries.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One collect or TOTP step in flight; ends it when dropped.
struct StepGuard<'a> {
    verifiers: &'a AttemptVerifiers,
    attempt_id: &'a str,
}

impl Drop for StepGuard<'_> {
    fn drop(&mut self) {
        self.verifiers.end_step(self.attempt_id);
    }
}

/// Verifiers of the attempts this app started, by attempt id.
#[derive(Default)]
pub struct AttemptVerifiers {
    entries: Arc<Entries>,
    next_seq: AtomicU64,
}

impl std::fmt::Debug for AttemptVerifiers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let len = self.lock().len();
        f.debug_struct("AttemptVerifiers")
            .field("entries", &len)
            .field("verifiers", &"[REDACTED]")
            .finish()
    }
}

impl AttemptVerifiers {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        lock_entries(&self.entries)
    }

    /// Keep `verifier` for `attempt_id` on server `base`. A timer drops it
    /// after [`ATTEMPT_TTL`] even if no other attempt call follows.
    pub fn insert(&self, attempt_id: &str, base: &str, verifier: Zeroizing<String>) {
        self.prune(Instant::now());
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        {
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
                    seq,
                    busy: 0,
                },
            );
        }
        self.expire_later(attempt_id, seq);
    }

    /// Drop the entry `seq` of `attempt_id` once its attempt is over for
    /// sure. Without a runtime (plain unit tests) only `prune` applies.
    fn expire_later(&self, attempt_id: &str, seq: u64) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let entries = Arc::downgrade(&self.entries);
        let id = attempt_id.to_string();
        // Measured from the insert, not from the task's first poll.
        let deadline = tokio::time::Instant::now() + ATTEMPT_TTL;
        runtime.spawn(async move {
            tokio::time::sleep_until(deadline).await;
            if let Some(entries) = entries.upgrade() {
                let mut entries = lock_entries(&entries);
                if entries
                    .get(&id)
                    .is_some_and(|e| e.seq == seq && e.busy == 0)
                {
                    entries.remove(&id);
                }
            }
        });
    }

    /// Server base and verifier of a live attempt.
    pub fn get(&self, attempt_id: &str) -> Option<(String, Zeroizing<String>)> {
        self.prune(Instant::now());
        self.lock()
            .get(attempt_id)
            .map(|e| (e.base.clone(), e.verifier.clone()))
    }

    /// Remove and return a live attempt's server base and verifier.
    pub fn take(&self, attempt_id: &str) -> Option<(String, Zeroizing<String>)> {
        self.prune(Instant::now());
        self.lock().remove(attempt_id).map(|e| (e.base, e.verifier))
    }

    /// Whether the attempt is still wanted (not cancelled, ended or expired).
    pub fn contains(&self, attempt_id: &str) -> bool {
        self.get(attempt_id).is_some()
    }

    /// Drop the entry unless a collect or TOTP step of it is in flight.
    pub fn remove(&self, attempt_id: &str) {
        let mut entries = self.lock();
        if entries.get(attempt_id).is_some_and(|e| e.busy == 0) {
            entries.remove(attempt_id);
        }
    }

    /// Drop the entry for good (its session was taken over or ended).
    pub fn finish(&self, attempt_id: &str) {
        self.lock().remove(attempt_id);
    }

    /// A collect or TOTP step starts (`None` once the attempt is unwanted).
    /// The step ends when the guard drops, also when the step's future is
    /// dropped mid-flight (an aborted command; Flash r3 p2 #5).
    fn begin_step<'a>(&'a self, attempt_id: &'a str) -> Option<StepGuard<'a>> {
        self.prune(Instant::now());
        self.lock().get_mut(attempt_id)?.busy += 1;
        Some(StepGuard {
            verifiers: self,
            attempt_id,
        })
    }

    /// The step is over (the entry may be gone after a cancel). An entry
    /// that outlived its 10 minutes while busy goes now, as its timer
    /// skipped it.
    fn end_step(&self, attempt_id: &str) {
        let mut entries = self.lock();
        let expired = entries.get_mut(attempt_id).is_some_and(|e| {
            e.busy = e.busy.saturating_sub(1);
            e.busy == 0 && e.created.elapsed() >= ATTEMPT_TTL
        });
        if expired {
            entries.remove(attempt_id);
        }
    }

    /// Drop entries older than [`ATTEMPT_TTL`] at `now` (not while busy).
    pub fn prune(&self, now: Instant) {
        self.lock()
            .retain(|_, e| e.busy > 0 || now.saturating_duration_since(e.created) < ATTEMPT_TTL);
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

/// A refused call as its bare server code, so the webview maps it with
/// `errorText`: `CODE`, `CODE:<secs>` with a `Retry-After`, and
/// `CODE:<secs?>:{json}` with the body's extra fields (`tries_left`,
/// `recovery_codes_left`, `state`, `reason`, …; never `message`).
pub async fn code_error(response: reqwest::Response) -> SessionError {
    let status = response.status().as_u16();
    let wait = retry_after(&response);
    let text = response.text().await.unwrap_or_default();
    code_error_from(status, wait, &text)
}

pub fn code_error_from(status: u16, wait: Option<u64>, text: &str) -> SessionError {
    let Some(code) = super::error_code(text) else {
        return super::server_error(status, text);
    };
    let extra = serde_json::from_str::<Value>(text).ok().and_then(|v| {
        let mut fields = v.as_object()?.clone();
        fields.remove("error");
        fields.remove("message");
        (!fields.is_empty()).then(|| Value::Object(fields).to_string())
    });
    let secs = wait.map(|s| s.to_string()).unwrap_or_default();
    SessionError::Server(
        status,
        match extra {
            Some(json) => format!("{code}:{secs}:{json}"),
            None if wait.is_some() => format!("{code}:{secs}"),
            None => code,
        },
    )
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

/// Fields of a session answer (`AuthResponse`).
const TOKEN_FIELDS: &[&str] = &["access_token", "refresh_token", "session_id"];

/// A server without the v1.0 sign-in steps (pre-L1b) answers `/auth/login`,
/// `/auth/oidc/desktop/exchange` and `/auth/register` with a token pair.
/// Those answers would hand the tokens to the webview, so they are refused
/// as `SERVER_OUTDATED` (Fable review M1); no sign-in step or pre-session
/// answer ever carries tokens.
pub fn refuse_token_answer(value: &Value) -> Result<(), SessionError> {
    let carries_tokens = value
        .as_object()
        .is_some_and(|o| TOKEN_FIELDS.iter().any(|k| o.contains_key(*k)));
    if carries_tokens {
        return Err(SessionError::Server(400, "SERVER_OUTDATED".to_string()));
    }
    Ok(())
}

fn unknown_attempt() -> SessionError {
    SessionError::Server(404, "LOGIN_ATTEMPT_UNKNOWN".to_string())
}

/// The attempt was cancelled while its session was being made.
fn cancelled_attempt() -> SessionError {
    code_error_from(
        410,
        None,
        r#"{"error":"LOGIN_ATTEMPT_ENDED","state":"cancelled"}"#,
    )
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
        refuse_token_answer(&step)?;
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
        refuse_token_answer(&attempt)?;
        self.keep_verifier(&base, &attempt, verifier);
        Ok(attempt)
    }

    /// One attempt call with the kept verifier. An unknown or ended attempt
    /// (404, 410) drops the verifier; any other refusal keeps it.
    async fn attempt_call(
        &self,
        attempt_id: &str,
        step: &str,
        extra: Value,
    ) -> Result<(String, Result<Value, SessionError>), SessionError> {
        let (base, verifier) = self.attempts.get(attempt_id).ok_or_else(unknown_attempt)?;
        let result = self
            .send_attempt_step(&base, attempt_id, step, &verifier, extra)
            .await?;
        if let Err(SessionError::Server(404 | 410, _)) = &result {
            // Unknown or ended: the verifier is worthless now.
            self.attempts.remove(attempt_id);
        }
        Ok((base, result))
    }

    /// POST `step` of an attempt with `verifier` (and `extra` body fields).
    async fn send_attempt_step(
        &self,
        base: &str,
        attempt_id: &str,
        step: &str,
        verifier: &str,
        extra: Value,
    ) -> Result<Result<Value, SessionError>, SessionError> {
        let mut body = json!({ "verifier": verifier });
        if let (Some(target), Value::Object(fields)) = (body.as_object_mut(), extra) {
            target.extend(fields);
        }
        let response = self
            .post_desktop(&attempt_url(base, attempt_id, step)?, &body)
            .await?;
        if !response.status().is_success() {
            return Ok(Err(code_error(response).await));
        }
        Ok(json_body(response).await)
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
    /// the user. A refusal other than 404/410 and a network failure keep the
    /// verifier, so the webview can retry within the collect window.
    pub async fn collect_attempt(&self, attempt_id: &str) -> Result<AuthResult, SessionError> {
        self.session_step(attempt_id, "collect", json!({})).await
    }

    /// The TOTP step (F4). A wrong code (`INVALID_MFA`) keeps the attempt.
    pub async fn attempt_totp(
        &self,
        attempt_id: &str,
        code: &str,
    ) -> Result<AuthResult, SessionError> {
        self.session_step(attempt_id, "totp", json!({ "code": code }))
            .await
    }

    /// A step that answers with a session (collect, TOTP). The entry is busy
    /// from before the request until the session is taken over or ended, so
    /// only a cancel can make that session unwanted.
    async fn session_step(
        &self,
        attempt_id: &str,
        step: &str,
        extra: Value,
    ) -> Result<AuthResult, SessionError> {
        let Some(step_guard) = self.attempts.begin_step(attempt_id) else {
            return Err(unknown_attempt());
        };
        let outcome = async {
            let (base, result) = self.attempt_call(attempt_id, step, extra).await?;
            self.adopt_attempt(attempt_id, &base, result?).await
        }
        .await;
        drop(step_guard);
        if let Err(SessionError::Server(404 | 410, _)) = &outcome {
            self.attempts.remove(attempt_id);
        }
        outcome
    }

    /// Take over the session an attempt produced, but only while the
    /// webview still wants that attempt: after a cancel (or the attempt's
    /// expiry) the new session is ended on the server instead (Codex review:
    /// a superseded sign-in must never replace the current one).
    async fn adopt_attempt(
        &self,
        attempt_id: &str,
        base: &str,
        value: Value,
    ) -> Result<AuthResult, SessionError> {
        let outcome = match serde_json::from_value::<TokenResponse>(value) {
            Ok(tokens) => {
                self.adopt_tokens_if(base, tokens, || self.attempts.contains(attempt_id))
                    .await
            }
            Err(e) => Err(invalid_response(200, &e)),
        };
        self.attempts.finish(attempt_id);
        outcome?.ok_or_else(cancelled_attempt)
    }

    /// Give the attempt up. The verifier is taken out before the server is
    /// asked, so a collect or TOTP step still in flight never adopts its
    /// session, and it is gone even if the server is unreachable.
    pub async fn cancel_attempt(&self, attempt_id: &str) -> Result<(), SessionError> {
        let (base, verifier) = self.attempts.take(attempt_id).ok_or_else(unknown_attempt)?;
        self.send_attempt_step(&base, attempt_id, "cancel", &verifier, json!({}))
            .await?
            .map(|_| ())
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
        refuse_token_answer(&step)?;
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
    use std::time::Duration;

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

    fn me_json() -> Value {
        json!({
            "id": "0192f0c0-0000-7000-8000-000000000001",
            "username": "alice",
            "display_name": "Alice",
            "email": null,
            "avatar_url": null,
            "status": "online",
            "mfa_enabled": false
        })
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
            r#"LOGIN_ATTEMPT_ENDED::{"state":"expired"}"#
        );
        assert!(m.attempt_verifiers().get(ID).is_none());
    }

    #[tokio::test]
    async fn bare_code_keeps_the_extra_error_fields() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/login"))
            .respond_with(ResponseTemplate::new(410).set_body_json(
                json!({"error": "LOGIN_ATTEMPT_ENDED", "message": "gone", "state": "denied"}),
            ))
            .mount(&server)
            .await;
        let err = manager().login(&server.uri(), "a", "b").await.unwrap_err();
        assert_eq!(
            err.to_string(),
            r#"LOGIN_ATTEMPT_ENDED::{"state":"denied"}"#
        );
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

    #[tokio::test]
    async fn a_pre_l1b_token_answer_to_login_is_server_outdated() {
        // Fable review M1: an old server answers with tokens; none reach the
        // webview, and no verifier is kept for them.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/login"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(tokens_body("at-old-1", "rt-old-1")),
            )
            .mount(&server)
            .await;
        let m = manager();
        let err = m.login(&server.uri(), "alice", "pw").await.unwrap_err();
        let text = format!("{err} {err:?}");
        assert_eq!(err.to_string(), "SERVER_OUTDATED");
        assert!(
            !text.contains("at-old-1") && !text.contains("rt-old-1"),
            "{text}"
        );
        assert_eq!(m.attempt_verifiers().len(), 0);
        assert!(m.auth_info().is_none());
    }

    #[tokio::test]
    async fn a_pre_l1b_token_answer_to_the_oidc_exchange_is_server_outdated() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/oidc/desktop/exchange"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(tokens_body("at-old-2", "rt-old-2")),
            )
            .mount(&server)
            .await;
        let err = manager()
            .exchange_oidc_code(&server.uri(), "c", Zeroizing::new("v".into()))
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "SERVER_OUTDATED");
        assert!(!format!("{err:?}").contains("at-old-2"));
    }

    #[tokio::test]
    async fn a_poll_that_wakes_consumed_keeps_the_totp_session() {
        // Argon review part 8 #1: "Phone not at hand?" leaves the PC's
        // long-poll running; the server wakes it with `consumed` when the
        // TOTP step succeeds. That must not make the new session unwanted.
        let server = MockServer::start().await;
        let m = Arc::new(manager());
        started(&server, &m).await;
        Mock::given(method("GET"))
            .and(path("/auth/me"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(me_json())
                    .set_delay(Duration::from_millis(300)),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(format!("/auth/login-attempts/{ID}/totp")))
            .respond_with(ResponseTemplate::new(200).set_body_json(tokens_body("at-t", "rt-t")))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(format!("/auth/login-attempts/{ID}/poll")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(attempt("consumed"))
                    .set_delay(Duration::from_millis(100)),
            )
            .mount(&server)
            .await;
        let (totp, poll) = tokio::join!(m.attempt_totp(ID, "123456"), async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            m.poll_attempt(ID, 25).await
        });
        assert_eq!(poll.unwrap()["state"], "consumed");
        totp.unwrap();
        assert!(m.auth_info().is_some());
        let logouts = server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|r| r.url.path() == "/auth/logout")
            .count();
        assert_eq!(logouts, 0);
        assert!(m.attempt_verifiers().get(ID).is_none());
    }

    #[tokio::test]
    async fn a_cancel_still_beats_a_busy_collect() {
        let server = MockServer::start().await;
        let m = Arc::new(manager());
        started(&server, &m).await;
        Mock::given(method("GET"))
            .and(path("/auth/me"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(me_json())
                    .set_delay(Duration::from_millis(300)),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(format!("/auth/login-attempts/{ID}/collect")))
            .respond_with(ResponseTemplate::new(200).set_body_json(tokens_body("at-c", "rt-c")))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(format!("/auth/login-attempts/{ID}/cancel")))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/auth/logout"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        let (collect, _) = tokio::join!(m.collect_attempt(ID), async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            m.cancel_attempt(ID).await
        });
        assert!(collect.is_err());
        assert!(m.auth_info().is_none());
    }

    #[tokio::test]
    async fn a_dropped_collect_is_no_longer_busy() {
        // Flash r3 p2 #5: a collect whose future is dropped mid-flight (the
        // command was aborted) leaves its entry removable and expirable.
        let server = MockServer::start().await;
        let m = Arc::new(manager());
        started(&server, &m).await;
        Mock::given(method("POST"))
            .and(path(format!("/auth/login-attempts/{ID}/collect")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(tokens_body("at-d", "rt-d"))
                    .set_delay(Duration::from_secs(30)),
            )
            .mount(&server)
            .await;
        let dropped = tokio::time::timeout(Duration::from_millis(100), m.collect_attempt(ID)).await;
        assert!(dropped.is_err(), "the collect was still in flight");
        m.attempt_verifiers().remove(ID);
        assert!(m.attempt_verifiers().get(ID).is_none());
    }

    #[test]
    fn a_step_that_ends_after_the_ttl_drops_its_entry() {
        let v = AttemptVerifiers::default();
        v.insert(ID, "https://chat.example.com", Zeroizing::new("ver".into()));
        let step = v.begin_step(ID).expect("wanted");
        v.backdate(ID, ATTEMPT_TTL + Duration::from_secs(1));
        // Busy: its 10 minutes do not count yet.
        v.prune(Instant::now());
        assert_eq!(v.len(), 1);
        drop(step);
        assert_eq!(v.len(), 0);
    }

    #[tokio::test]
    async fn a_sign_out_while_the_profile_loads_wins() {
        // Argon review part 8 #2: a sign-out (or another sign-in) that lands
        // during `/auth/me` is never undone by the older adoption.
        let server = MockServer::start().await;
        let m = Arc::new(manager());
        started(&server, &m).await;
        Mock::given(method("GET"))
            .and(path("/auth/me"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(me_json())
                    .set_delay(Duration::from_millis(300)),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(format!("/auth/login-attempts/{ID}/collect")))
            .respond_with(ResponseTemplate::new(200).set_body_json(tokens_body("at-g", "rt-g")))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/auth/logout"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        let (collect, _) = tokio::join!(m.collect_attempt(ID), async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            m.logout().await
        });
        assert!(collect.is_err());
        assert!(m.auth_info().is_none());
        // The abandoned pair was ended with its own bearer.
        let ended = server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .any(|r| {
                r.url.path() == "/auth/logout"
                    && r.headers.get("authorization").and_then(|v| v.to_str().ok())
                        == Some("Bearer at-g")
            });
        assert!(ended);
    }

    fn tokens_body(access: &str, refresh: &str) -> Value {
        json!({
            "access_token": access,
            "refresh_token": refresh,
            "expires_in": 900,
            "token_type": "Bearer",
            "session_id": "s-1",
            "assurance": "probation",
            "absolute_expires_at": "2027-02-01T00:00:00Z"
        })
    }

    #[tokio::test]
    async fn failed_collect_keeps_the_verifier_for_a_retry() {
        let server = MockServer::start().await;
        let m = manager();
        started(&server, &m).await;
        mount_me(&server).await;
        Mock::given(method("POST"))
            .and(path(format!("/auth/login-attempts/{ID}/collect")))
            .respond_with(
                ResponseTemplate::new(503)
                    .set_body_json(json!({"error": "SERVICE_UNAVAILABLE", "message": "x"})),
            )
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(format!("/auth/login-attempts/{ID}/collect")))
            .respond_with(ResponseTemplate::new(200).set_body_json(tokens_body("at-1", "rt-1")))
            .mount(&server)
            .await;
        let err = m.collect_attempt(ID).await.unwrap_err();
        assert_eq!(err.to_string(), "SERVICE_UNAVAILABLE");
        assert!(m.attempt_verifiers().get(ID).is_some());
        let result = m.collect_attempt(ID).await.unwrap();
        assert_eq!(result.user["username"], "alice");
        assert!(m.attempt_verifiers().get(ID).is_none());
    }

    /// A collect (or TOTP step) that answers after the webview cancelled
    /// the attempt never becomes the app's session: it is ended at once.
    #[tokio::test]
    async fn cancel_during_collect_or_totp_ends_the_new_session() {
        for step in ["collect", "totp"] {
            let server = MockServer::start().await;
            let m = manager();
            started(&server, &m).await;
            mount_me(&server).await;
            Mock::given(method("POST"))
                .and(path(format!("/auth/login-attempts/{ID}/{step}")))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(tokens_body("at-late", "rt-late"))
                        .set_delay(Duration::from_millis(300)),
                )
                .mount(&server)
                .await;
            Mock::given(method("POST"))
                .and(path(format!("/auth/login-attempts/{ID}/cancel")))
                .respond_with(ResponseTemplate::new(204))
                .mount(&server)
                .await;
            Mock::given(method("POST"))
                .and(path("/auth/logout"))
                .and(header("authorization", "Bearer at-late"))
                .respond_with(ResponseTemplate::new(204))
                .expect(1)
                .mount(&server)
                .await;
            let late = async {
                if step == "collect" {
                    m.collect_attempt(ID).await
                } else {
                    m.attempt_totp(ID, "123456").await
                }
            };
            let cancel = async {
                tokio::time::sleep(Duration::from_millis(50)).await;
                m.cancel_attempt(ID).await
            };
            let (late, cancelled) = tokio::join!(late, cancel);
            cancelled.unwrap();
            assert_eq!(
                late.unwrap_err().to_string(),
                r#"LOGIN_ATTEMPT_ENDED::{"state":"cancelled"}"#,
                "{step}"
            );
            assert!(m.auth_info().is_none(), "{step}");
            assert!(m.attempt_verifiers().get(ID).is_none(), "{step}");
            let logout = server
                .received_requests()
                .await
                .unwrap()
                .into_iter()
                .find(|r| r.url.path() == "/auth/logout")
                .expect("the late session is ended");
            assert_eq!(body_of(&logout)["refresh_token"], "rt-late");
        }
    }

    #[tokio::test]
    async fn cancel_forgets_the_verifier_before_the_server_answers() {
        let server = MockServer::start().await;
        let m = manager();
        started(&server, &m).await;
        Mock::given(method("POST"))
            .and(path(format!("/auth/login-attempts/{ID}/cancel")))
            .respond_with(ResponseTemplate::new(204).set_delay(Duration::from_millis(300)))
            .mount(&server)
            .await;
        let cancel = m.cancel_attempt(ID);
        let check = async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            m.attempt_verifiers().get(ID).is_none()
        };
        let (cancelled, gone_early) = tokio::join!(cancel, check);
        cancelled.unwrap();
        assert!(gone_early);
        let body = body_of(&server.received_requests().await.unwrap()[1]);
        assert!(body["verifier"].as_str().is_some_and(|v| !v.is_empty()));
    }

    /// Codex review: a verifier must not outlive its attempt just because no
    /// other attempt call follows (a hung poll, a closed screen).
    #[tokio::test(start_paused = true)]
    async fn expired_verifier_is_dropped_without_another_call() {
        let v = AttemptVerifiers::default();
        v.insert("a", "https://chat.example.com", Zeroizing::new("x".into()));
        tokio::time::advance(Duration::from_secs(ATTEMPT_TTL.as_secs() - 1)).await;
        tokio::task::yield_now().await;
        assert_eq!(v.len(), 1);
        tokio::time::advance(Duration::from_secs(2)).await;
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
        assert_eq!(v.len(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn expiry_of_an_old_entry_spares_a_newer_one_with_the_same_id() {
        let v = AttemptVerifiers::default();
        v.insert("a", "b", Zeroizing::new("old".into()));
        tokio::time::advance(Duration::from_mins(5)).await;
        v.insert("a", "b", Zeroizing::new("new".into()));
        tokio::time::advance(Duration::from_mins(5) + Duration::from_secs(1)).await;
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
        assert_eq!(v.len(), 1);
        assert_eq!(v.lock().get("a").unwrap().verifier.as_str(), "new");
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
