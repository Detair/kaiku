//! Rust-owned desktop session.
//!
//! [`SessionManager`] owns the access token, the refresh token and the
//! keyring entry. The webview only ever receives the access token
//! ([`AuthInfo`]); the refresh token never leaves Rust. Refreshes are
//! single-flight because the server rotates the refresh token on every use:
//! two parallel refreshes with the same token would invalidate the session.

pub mod store;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, PoisonError, RwLock as StdRwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
pub use store::{KeyringTokenStore, StoredSession, TokenStore};
use tokio::sync::{broadcast, Mutex, MutexGuard, Notify, RwLock};
use tracing::{info, warn};

use crate::{AuthState, User, UserStatus};

/// How long session restore waits for the server before reporting a network error.
///
/// Generous on purpose: the webview keeps the stored login and retries after a
/// `network_error`. An answer that arrives later is still committed by the
/// rotation task, so a slow start costs a retry, never the login.
const RESTORE_TIMEOUT: Duration = Duration::from_secs(15);

// ============================================================================
// Wire and result types
// ============================================================================

/// Registration request from the frontend.
#[derive(Deserialize)]
pub struct RegisterRequest {
    pub server_url: String,
    pub username: String,
    pub email: Option<String>,
    pub password: String,
    pub display_name: Option<String>,
}

impl std::fmt::Debug for RegisterRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegisterRequest")
            .field("server_url", &self.server_url)
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

/// Token pair returned by `/auth/login`, `/auth/register`, `/auth/refresh`
/// and the OIDC callback.
#[derive(Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
    #[serde(default)]
    pub setup_required: bool,
    #[serde(default)]
    pub password_change_required: bool,
}

/// What the webview needs to call the API itself: base URL and access token.
#[derive(Clone, Serialize, PartialEq, Eq)]
pub struct AuthInfo {
    pub server_url: String,
    pub access_token: String,
    pub expires_at_ms: u64,
}

impl std::fmt::Debug for AuthInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthInfo")
            .field("server_url", &self.server_url)
            .field("access_token", &"[REDACTED]")
            .field("expires_at_ms", &self.expires_at_ms)
            .finish()
    }
}

/// Result of login, registration and OIDC login.
#[derive(Debug, Serialize)]
pub struct AuthResult {
    /// Raw `/auth/me` JSON, so the webview gets every field.
    pub user: Value,
    pub setup_required: bool,
    pub password_change_required: bool,
    /// `false` when the OS keyring refused the refresh token: the user stays
    /// signed in for this run but has to sign in again after a restart.
    pub persisted: bool,
}

/// Result of a session restore attempt (`get_current_user`).
#[derive(Debug, Serialize)]
pub struct SessionRestoreResult {
    pub user: Option<Value>,
    /// `"session_expired"` or `"network_error"`; `None` on success or when
    /// there was no stored session.
    pub error_reason: Option<String>,
    pub password_change_required: bool,
}

impl SessionRestoreResult {
    fn empty(error_reason: Option<&str>) -> Self {
        Self {
            user: None,
            error_reason: error_reason.map(str::to_string),
            password_change_required: false,
        }
    }
}

/// Session failures. `Display` strings are shown to the user by the webview.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// The server could not be reached (transient).
    #[error("Connection failed: {0}")]
    Network(String),
    #[error("Invalid username or password")]
    InvalidCredentials,
    /// The TypeScript side matches this exact string.
    #[error("MFA_REQUIRED")]
    MfaRequired,
    /// The refresh token was rejected or there is no session: sign in again.
    #[error("Session expired")]
    Expired,
    /// Any other non-success response (status, user-facing message).
    #[error("{1}")]
    Server(u16, String),
    /// Rejected locally without contacting the server.
    #[error("{0}")]
    InvalidRequest(String),
}

/// Events the refresh scheduler reports to the webview.
#[derive(Debug, Clone)]
pub enum SessionEvent {
    TokenRefreshed(AuthInfo),
    Expired,
}

/// `/auth/me` fields the Rust side needs (`AuthState.user`).
#[derive(Deserialize)]
struct UserResponse {
    id: String,
    username: String,
    display_name: String,
    email: Option<String>,
    avatar_url: Option<String>,
    status: String,
    mfa_enabled: bool,
}

impl From<UserResponse> for User {
    fn from(r: UserResponse) -> Self {
        Self {
            id: r.id,
            username: r.username,
            display_name: r.display_name,
            avatar_url: r.avatar_url,
            email: r.email,
            mfa_enabled: r.mfa_enabled,
            status: match r.status.as_str() {
                "online" => UserStatus::Online,
                "away" => UserStatus::Away,
                "busy" => UserStatus::Busy,
                _ => UserStatus::Offline,
            },
        }
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn network(e: &reqwest::Error) -> SessionError {
    SessionError::Network(e.to_string())
}

/// The `error` code of a JSON error body, e.g. `MFA_REQUIRED`.
fn error_code(body: &str) -> Option<String> {
    serde_json::from_str::<Value>(body)
        .ok()?
        .get("error")?
        .as_str()
        .map(str::to_string)
}

fn server_error(status: u16, body: &str) -> SessionError {
    let message = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v.get("message")
                .or_else(|| v.get("error"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| format!("Server error ({status})"));
    SessionError::Server(status, message)
}

fn invalid_response(status: u16, e: &dyn std::fmt::Display) -> SessionError {
    SessionError::Server(status, format!("Invalid response from server: {e}"))
}

/// Trim and validate a server base URL (`http(s)://host[:port][/prefix]`).
fn normalize_server_url(raw: &str) -> Result<String, SessionError> {
    let trimmed = raw.trim().trim_end_matches('/');
    match url::Url::parse(trimmed) {
        Ok(u) if matches!(u.scheme(), "http" | "https") && u.has_host() => Ok(trimmed.to_string()),
        _ => Err(SessionError::InvalidRequest(
            "Invalid server URL".to_string(),
        )),
    }
}

fn parse_user(value: &Value) -> Result<User, SessionError> {
    UserResponse::deserialize(value)
        .map(User::from)
        .map_err(|e| invalid_response(200, &e))
}

fn flag(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// The only requests the webview may send with the refresh token attached
/// (the server identifies the current session by it). Exact matches only.
fn allowed_session_request(method: &str, path: &str) -> Option<reqwest::Method> {
    match (method, path) {
        ("GET", "/auth/sessions") => Some(reqwest::Method::GET),
        ("DELETE", "/auth/sessions") => Some(reqwest::Method::DELETE),
        ("POST", "/auth/me/password") => Some(reqwest::Method::POST),
        ("DELETE", p) => p
            .strip_prefix("/auth/sessions/")
            .filter(|id| id.len() == 36 && uuid::Uuid::parse_str(id).is_ok())
            .map(|_| reqwest::Method::DELETE),
        _ => None,
    }
}

// ============================================================================
// Session manager
// ============================================================================

#[derive(Clone)]
struct Session {
    server_url: String,
    access_token: String,
    refresh_token: String,
    expires_at_ms: u64,
    /// Raw `/auth/me` JSON; `None` until the profile was fetched.
    user: Option<Value>,
    password_change_required: bool,
}

impl Session {
    fn from_tokens(server_url: &str, tokens: &TokenResponse, user: Option<Value>) -> Self {
        Self {
            server_url: server_url.to_string(),
            access_token: tokens.access_token.clone(),
            refresh_token: tokens.refresh_token.clone(),
            expires_at_ms: now_ms().saturating_add(tokens.expires_in.saturating_mul(1000)),
            user,
            password_change_required: tokens.password_change_required,
        }
    }

    /// The stored login before its first rotation: only the refresh token is
    /// known, so the access token is empty and already expired.
    fn from_stored(stored: &StoredSession) -> Self {
        Self {
            server_url: stored.server_url.clone(),
            access_token: String::new(),
            refresh_token: stored.refresh_token.clone(),
            expires_at_ms: 0,
            user: None,
            password_change_required: false,
        }
    }

    fn auth_info(&self) -> AuthInfo {
        AuthInfo {
            server_url: self.server_url.clone(),
            access_token: self.access_token.clone(),
            expires_at_ms: self.expires_at_ms,
        }
    }

    fn stored(&self) -> StoredSession {
        StoredSession {
            server_url: self.server_url.clone(),
            refresh_token: self.refresh_token.clone(),
        }
    }
}

/// Owner of the desktop session: tokens, keyring, refresh.
///
/// A cheap handle around shared state so a token rotation can finish on its
/// own task even when the caller that started it is dropped (see
/// [`SessionManager::refresh_locked`]).
pub struct SessionManager {
    inner: Arc<SessionInner>,
}

impl std::ops::Deref for SessionManager {
    type Target = SessionInner;

    fn deref(&self) -> &SessionInner {
        &self.inner
    }
}

/// State shared by [`SessionManager`] and its rotation tasks.
pub struct SessionInner {
    http: reqwest::Client,
    store: Arc<dyn TokenStore>,
    /// Shared with `AppState.auth`, which older commands still read.
    legacy: Arc<RwLock<AuthState>>,
    session: StdRwLock<Option<Session>>,
    /// Serializes every token mutation (refresh, sign-in, sign-out).
    refresh_lock: Mutex<()>,
    /// Wakes the refresh scheduler when the session changes.
    changed: Notify,
    /// Bumped whenever a different session starts or the session ends, so a
    /// slow operation can tell that the session it started with is gone.
    generation: AtomicU64,
    /// Token refreshes and server-side session ends, from any code path.
    events: broadcast::Sender<SessionEvent>,
    /// Held by a running token rotation for its whole duration. Taken (and
    /// released) after `refresh_lock` by [`SessionManager::lock`], so a
    /// rotation whose caller was dropped still finishes before anyone else
    /// touches the tokens.
    rotation: Arc<Mutex<()>>,
}

/// Proof that the caller holds `refresh_lock`.
type LockGuard<'a> = MutexGuard<'a, ()>;

/// Why tokens are rotated; decides how the result is committed.
#[derive(Clone, Copy)]
enum Rotation {
    /// A signed-in session gets a new token pair: same identity, the webview
    /// hears about it through `SessionEvent`s.
    Refresh,
    /// App start: the stored refresh token becomes the session (new identity,
    /// legacy server URL set). No events: the restore result carries the
    /// outcome, and a rejected token only clears the store.
    Restore,
}

impl SessionInner {
    fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    fn publish(&self, event: SessionEvent) {
        // No subscriber (tests, before setup) is fine.
        let _ = self.events.send(event);
    }

    fn snapshot(&self) -> Option<Session> {
        self.session
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Replace the session. `new_identity` marks a different session (sign-in,
    /// restore, sign-out) as opposed to a token rotation of the same one.
    fn set_session(&self, session: Option<Session>, new_identity: bool) {
        *self.session.write().unwrap_or_else(PoisonError::into_inner) = session;
        if new_identity {
            self.generation.fetch_add(1, Ordering::SeqCst);
        }
        self.changed.notify_one();
    }

    /// Persist the refresh token; `false` if the keyring refused it.
    fn persist(&self, session: &Session) -> bool {
        match self.store.save(&session.stored()) {
            Ok(()) => true,
            Err(e) => {
                warn!(error = %e, "Could not save the login in the OS keyring; it will not survive a restart");
                false
            }
        }
    }

    /// Forget the session everywhere (caller holds `refresh_lock` or
    /// `rotation`). `expired` = the server rejected it (the webview is told
    /// via `SessionEvent::Expired`); `false` = user sign-out.
    async fn clear_session(&self, server_url: &str, expired: bool) {
        if let Err(e) = self.store.clear(server_url) {
            warn!(error = %e, "Could not remove the stored login from the OS keyring");
        }
        self.set_session(None, true);
        if expired {
            self.publish(SessionEvent::Expired);
        }
        let mut auth = self.legacy.write().await;
        auth.access_token = None;
        auth.user = None;
    }

    /// `POST /auth/refresh`. 401/403 → `Expired`; everything else that fails
    /// is transient and leaves the stored token alone.
    async fn call_refresh(
        &self,
        base: &str,
        refresh_token: &str,
    ) -> Result<TokenResponse, SessionError> {
        let response = self
            .http
            .post(format!("{base}/auth/refresh"))
            .json(&json!({ "refresh_token": refresh_token }))
            .send()
            .await
            .map_err(|e| network(&e))?;
        let status = response.status().as_u16();
        if status == 401 || status == 403 {
            return Err(SessionError::Expired);
        }
        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(server_error(status, &text));
        }
        response
            .json()
            .await
            .map_err(|e| invalid_response(status, &e))
    }

    /// Rotate the tokens of `current` and commit them (memory, keyring,
    /// legacy state, event). Runs on its own task holding `rotation`.
    async fn rotate(&self, current: Session, kind: Rotation) -> Result<Session, SessionError> {
        let tokens = match self
            .call_refresh(&current.server_url, &current.refresh_token)
            .await
        {
            Ok(tokens) => tokens,
            Err(SessionError::Expired) => {
                match kind {
                    Rotation::Refresh => {
                        info!("Refresh token rejected; session ended");
                        self.clear_session(&current.server_url, true).await;
                    }
                    Rotation::Restore => {
                        info!("Stored login was rejected by the server");
                        if let Err(e) = self.store.clear(&current.server_url) {
                            warn!(error = %e, "Could not remove the stored login from the OS keyring");
                        }
                    }
                }
                return Err(SessionError::Expired);
            }
            Err(e) => return Err(e),
        };

        let session = Session::from_tokens(&current.server_url, &tokens, current.user);
        self.persist(&session);
        self.set_session(Some(session.clone()), matches!(kind, Rotation::Restore));
        {
            let mut auth = self.legacy.write().await;
            auth.access_token = Some(tokens.access_token);
            if matches!(kind, Rotation::Restore) {
                auth.server_url = Some(session.server_url.clone());
            }
        }
        if matches!(kind, Rotation::Refresh) {
            self.publish(SessionEvent::TokenRefreshed(session.auth_info()));
        }
        Ok(session)
    }
}

impl SessionManager {
    pub fn new(
        http: reqwest::Client,
        store: Arc<dyn TokenStore>,
        legacy: Arc<RwLock<AuthState>>,
    ) -> Self {
        Self {
            inner: Arc::new(SessionInner {
                http,
                store,
                legacy,
                session: StdRwLock::new(None),
                refresh_lock: Mutex::new(()),
                changed: Notify::new(),
                generation: AtomicU64::new(0),
                events: broadcast::channel(16).0,
                rotation: Arc::new(Mutex::new(())),
            }),
        }
    }

    /// Take the token lock, after any rotation still running for a dropped
    /// caller has finished.
    async fn lock(&self) -> LockGuard<'_> {
        let guard = self.refresh_lock.lock().await;
        drop(self.rotation.lock().await);
        guard
    }

    #[cfg(test)]
    pub fn legacy_auth(&self) -> &Arc<RwLock<AuthState>> {
        &self.legacy
    }

    /// Session events (`TokenRefreshed`, `Expired`) for the webview bridge.
    pub fn subscribe(&self) -> broadcast::Receiver<SessionEvent> {
        self.events.subscribe()
    }

    /// Forget the session everywhere. `expired` = the server rejected it (the
    /// webview is told via `SessionEvent::Expired`); `false` = user sign-out.
    async fn end_session(&self, _lock: &LockGuard<'_>, server_url: &str, expired: bool) {
        self.inner.clear_session(server_url, expired).await;
    }

    /// Current base URL and access token, if signed in.
    pub fn auth_info(&self) -> Option<AuthInfo> {
        self.snapshot().map(|s| s.auth_info())
    }

    pub async fn login(
        &self,
        server_url: &str,
        username: &str,
        password: &str,
        mfa_code: Option<String>,
    ) -> Result<AuthResult, SessionError> {
        let base = normalize_server_url(server_url)?;
        let mut body = json!({ "username": username, "password": password });
        if let Some(code) = mfa_code {
            body["mfa_code"] = json!(code);
        }
        let response = self
            .http
            .post(format!("{base}/auth/login"))
            .json(&body)
            .send()
            .await
            .map_err(|e| network(&e))?;
        let status = response.status().as_u16();
        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(match status {
                401 => SessionError::InvalidCredentials,
                403 if error_code(&text).as_deref() == Some("MFA_REQUIRED") => {
                    SessionError::MfaRequired
                }
                _ => server_error(status, &text),
            });
        }
        let tokens: TokenResponse = response
            .json()
            .await
            .map_err(|e| invalid_response(status, &e))?;
        self.adopt_tokens(&base, tokens).await
    }

    pub async fn register(&self, req: RegisterRequest) -> Result<AuthResult, SessionError> {
        let base = normalize_server_url(&req.server_url)?;
        let response = self
            .http
            .post(format!("{base}/auth/register"))
            .json(&json!({
                "username": req.username,
                "email": req.email,
                "password": req.password,
                "display_name": req.display_name,
            }))
            .send()
            .await
            .map_err(|e| network(&e))?;
        let status = response.status().as_u16();
        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(if status == 409 {
                SessionError::Server(409, "Username or email already exists".to_string())
            } else {
                server_error(status, &text)
            });
        }
        let tokens: TokenResponse = response
            .json()
            .await
            .map_err(|e| invalid_response(status, &e))?;
        self.adopt_tokens(&base, tokens).await
    }

    /// Take over a fresh token pair (login, registration, OIDC): fetch the
    /// profile, store the refresh token, publish the session.
    pub async fn adopt_tokens(
        &self,
        server_url: &str,
        tokens: TokenResponse,
    ) -> Result<AuthResult, SessionError> {
        let base = normalize_server_url(server_url)?;
        let user = self.fetch_me(&base, &tokens.access_token).await?;
        let legacy_user = parse_user(&user)?;
        let password_change_required =
            tokens.password_change_required || flag(&user, "password_change_required");

        let _guard = self.lock().await;
        let mut session = Session::from_tokens(&base, &tokens, Some(user.clone()));
        session.password_change_required = password_change_required;
        let persisted = self.persist(&session);
        self.set_session(Some(session), true);
        {
            let mut auth = self.legacy.write().await;
            auth.server_url = Some(base);
            auth.access_token = Some(tokens.access_token);
            auth.user = Some(legacy_user);
        }
        info!("Signed in");
        Ok(AuthResult {
            user,
            setup_required: tokens.setup_required,
            password_change_required,
            persisted,
        })
    }

    async fn fetch_me(&self, base: &str, access_token: &str) -> Result<Value, SessionError> {
        let response = self
            .http
            .get(format!("{base}/auth/me"))
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| network(&e))?;
        let status = response.status().as_u16();
        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(server_error(status, &text));
        }
        response
            .json()
            .await
            .map_err(|e| invalid_response(status, &e))
    }

    /// Get a new access token. Single-flight: a caller that waited while
    /// another refresh (or a sign-in) replaced the token gets that token
    /// without a second server call.
    pub async fn refresh(&self) -> Result<AuthInfo, SessionError> {
        let seen = self.snapshot().map(|s| s.access_token);
        let guard = self.lock().await;
        let current = self.snapshot().ok_or(SessionError::Expired)?;
        if seen.as_deref() != Some(current.access_token.as_str()) {
            return Ok(current.auth_info());
        }
        self.refresh_locked(&guard, current).await
    }

    /// Rotate the tokens of a signed-in session. Caller holds `refresh_lock`.
    async fn refresh_locked(
        &self,
        lock: &LockGuard<'_>,
        current: Session,
    ) -> Result<AuthInfo, SessionError> {
        self.rotate_on_task(lock, current, Rotation::Refresh)
            .await
            .map(|session| session.auth_info())
    }

    /// Rotate the tokens of `current` on a task of its own. Caller holds
    /// `refresh_lock`.
    ///
    /// The server revokes the old refresh token as soon as it answers, so
    /// dropping the caller (aborted WebSocket loop, cancelled command, a
    /// restore that timed out) must not lose the new token: the task finishes
    /// and commits regardless, holding `rotation` until it is done, which
    /// [`Self::lock`] waits for.
    async fn rotate_on_task(
        &self,
        _lock: &LockGuard<'_>,
        current: Session,
        kind: Rotation,
    ) -> Result<Session, SessionError> {
        let rotation = self
            .rotation
            .clone()
            .try_lock_owned()
            .map_err(|_| SessionError::Network("token rotation already running".into()))?;
        let inner = self.inner.clone();
        tokio::spawn(async move {
            let _rotation = rotation;
            inner.rotate(current, kind).await
        })
        .await
        .unwrap_or_else(|e| Err(SessionError::Network(format!("token rotation failed: {e}"))))
    }

    /// Restore the session on app start from the stored refresh token.
    pub async fn restore(&self) -> SessionRestoreResult {
        self.restore_within(RESTORE_TIMEOUT).await
    }

    /// [`Self::restore`] with an explicit wait for the server's answer. A
    /// rotation that outlives the wait still commits (see
    /// [`Self::rotate_on_task`]); the next restore finds the session.
    async fn restore_within(&self, server_timeout: Duration) -> SessionRestoreResult {
        let (session, generation) = {
            let guard = self.lock().await;
            if let Some(existing) = self.snapshot() {
                (existing, self.generation())
            } else {
                let stored = match self.store.load() {
                    Ok(Some(stored)) => stored,
                    Ok(None) => return SessionRestoreResult::empty(None),
                    Err(e) => {
                        warn!(error = %e, "Could not read the stored login from the OS keyring");
                        return SessionRestoreResult::empty(None);
                    }
                };
                info!("Restoring session");
                let rotated = tokio::time::timeout(
                    server_timeout,
                    self.rotate_on_task(&guard, Session::from_stored(&stored), Rotation::Restore),
                )
                .await;
                let session = match rotated {
                    Ok(Ok(session)) => session,
                    Ok(Err(SessionError::Expired)) => {
                        return SessionRestoreResult::empty(Some("session_expired"));
                    }
                    Ok(Err(e)) => {
                        warn!(error = %e, "Session restore failed; keeping the stored login");
                        return SessionRestoreResult::empty(Some("network_error"));
                    }
                    Err(_) => {
                        warn!("Session restore timed out; a late answer is still committed");
                        return SessionRestoreResult::empty(Some("network_error"));
                    }
                };
                (session, self.generation())
            }
        };

        if let Some(user) = session.user {
            return SessionRestoreResult {
                user: Some(user),
                error_reason: None,
                password_change_required: session.password_change_required,
            };
        }

        // Tokens are valid but the profile is still missing.
        let user = match self
            .fetch_me(&session.server_url, &session.access_token)
            .await
        {
            Ok(user) => user,
            Err(e) => {
                warn!(error = %e, "Could not load the profile during session restore");
                return SessionRestoreResult::empty(Some("network_error"));
            }
        };
        let legacy_user = match parse_user(&user) {
            Ok(u) => u,
            Err(e) => {
                warn!(error = %e, "Unexpected profile response during session restore");
                return SessionRestoreResult::empty(Some("network_error"));
            }
        };
        let password_change_required =
            session.password_change_required || flag(&user, "password_change_required");

        // Sign-in or sign-out may have happened while the profile loaded:
        // only commit it to the session it was fetched for.
        let _guard = self.lock().await;
        if self.generation() != generation {
            info!("Session changed during restore; dropping the restored profile");
            return match self.snapshot() {
                Some(current) if current.user.is_some() => SessionRestoreResult {
                    user: current.user,
                    error_reason: None,
                    password_change_required: current.password_change_required,
                },
                _ => SessionRestoreResult::empty(None),
            };
        }
        {
            let mut guard = self.session.write().unwrap_or_else(PoisonError::into_inner);
            if let Some(current) = guard.as_mut() {
                current.user = Some(user.clone());
                current.password_change_required = password_change_required;
            }
        }
        self.legacy.write().await.user = Some(legacy_user);
        info!("Session restored");
        SessionRestoreResult {
            user: Some(user),
            error_reason: None,
            password_change_required,
        }
    }

    /// Sign out: revoke the refresh token on the server (best effort), forget
    /// it locally.
    pub async fn logout(&self) -> Result<(), SessionError> {
        let guard = self.lock().await;
        if let Some(session) = self.snapshot() {
            let result = self
                .http
                .post(format!("{}/auth/logout", session.server_url))
                .bearer_auth(&session.access_token)
                .json(&json!({ "refresh_token": session.refresh_token }))
                .send()
                .await;
            match result {
                Ok(r) if r.status().is_success() => {}
                Ok(r) => warn!(status = r.status().as_u16(), "Server-side logout failed"),
                Err(e) => warn!(error = %e, "Server-side logout failed"),
            }
            self.end_session(&guard, &session.server_url, false).await;
        } else {
            match self.store.load() {
                Ok(Some(stored)) => self.end_session(&guard, &stored.server_url, false).await,
                Ok(None) => self.set_session(None, true),
                Err(e) => warn!(error = %e, "Could not read the stored login from the OS keyring"),
            }
        }
        info!("Signed out");
        Ok(())
    }

    /// Session-management requests that need the refresh token as
    /// `X-Refresh-Token`. Only the allowlisted method/path pairs are sent;
    /// anything else fails without a request.
    pub async fn session_request(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, SessionError> {
        let verb = allowed_session_request(method, path).ok_or_else(|| {
            SessionError::InvalidRequest(format!("Request not allowed: {method} {path}"))
        })?;
        // Hold the rotation lock for the whole request: the server identifies
        // the current session by the refresh token sent here, so a rotation
        // in between would make e.g. "revoke all other sessions" revoke this
        // one too.
        let guard = self.lock().await;
        let mut retried = false;
        loop {
            let session = self.snapshot().ok_or(SessionError::Expired)?;
            let mut request = self
                .http
                .request(verb.clone(), format!("{}{path}", session.server_url))
                .bearer_auth(&session.access_token)
                .header("X-Refresh-Token", &session.refresh_token);
            if let Some(body) = &body {
                request = request.json(body);
            }
            let response = request.send().await.map_err(|e| network(&e))?;
            let status = response.status().as_u16();
            if status == 401 && !retried {
                retried = true;
                self.refresh_locked(&guard, session).await?;
                continue;
            }
            let text = response.text().await.map_err(|e| network(&e))?;
            if !(200..300).contains(&status) {
                return Err(server_error(status, &text));
            }
            if text.trim().is_empty() {
                return Ok(Value::Null);
            }
            return serde_json::from_str(&text).map_err(|e| invalid_response(status, &e));
        }
    }
}

// ============================================================================
// Refresh scheduler
// ============================================================================

/// Timer settings for the refresh scheduler.
#[derive(Debug, Clone)]
pub struct SchedulerTiming {
    /// Refresh this long before the access token expires.
    pub lead: Duration,
    /// Never wake more often than this.
    pub min_delay: Duration,
    /// Wait after a transient failure (offline, 5xx).
    pub retry_delay: Duration,
    /// Re-check at least this often. Monotonic timers stop during system
    /// sleep, so long sleeps would miss the expiry after a wake-up.
    pub max_sleep: Duration,
}

impl Default for SchedulerTiming {
    fn default() -> Self {
        Self {
            lead: Duration::from_mins(1),
            min_delay: Duration::from_secs(5),
            retry_delay: Duration::from_secs(30),
            max_sleep: Duration::from_mins(1),
        }
    }
}

/// How long to sleep before the next check, or `None` to refresh now.
/// `hold_until_ms` delays the next attempt after a refresh (min delay) or a
/// transient failure (retry delay).
pub fn next_wake(
    expires_at_ms: u64,
    hold_until_ms: Option<u64>,
    now_ms: u64,
    timing: &SchedulerTiming,
) -> Option<Duration> {
    let lead_ms = u64::try_from(timing.lead.as_millis()).unwrap_or(u64::MAX);
    let due = expires_at_ms
        .saturating_sub(lead_ms)
        .max(hold_until_ms.unwrap_or(0));
    (due > now_ms)
        .then(|| Duration::from_millis(due - now_ms).clamp(timing.min_delay, timing.max_sleep))
}

fn add_ms(now: u64, d: Duration) -> u64 {
    now.saturating_add(u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Keep the access token fresh while signed in. Only a rejected refresh
/// token (`Expired`) ends the session; network errors and 5xx keep the stored
/// token and retry after `retry_delay`. Results reach the webview through
/// [`SessionManager::subscribe`], like refreshes from any other path.
pub async fn run_refresh_loop(mgr: Arc<SessionManager>, timing: SchedulerTiming) {
    let mut hold_until: Option<u64> = None;
    loop {
        let Some(info) = mgr.auth_info() else {
            hold_until = None;
            mgr.changed.notified().await;
            continue;
        };
        if let Some(wait) = next_wake(info.expires_at_ms, hold_until, now_ms(), &timing) {
            tokio::select! {
                () = tokio::time::sleep(wait) => {}
                () = mgr.changed.notified() => {}
            }
            continue;
        }
        match mgr.refresh().await {
            Ok(_) => hold_until = Some(add_ms(now_ms(), timing.min_delay)),
            Err(SessionError::Expired) => hold_until = None,
            Err(e) => {
                warn!(error = %e, "Token refresh failed; keeping the session and retrying");
                hold_until = Some(add_ms(now_ms(), timing.retry_delay));
            }
        }
    }
}

/// Start the background refresh for the app and forward every session event
/// to the webview: `auth:token_refreshed` (payload [`AuthInfo`]) and
/// `auth:session_expired` (no payload).
pub fn spawn_refresh_scheduler(app: tauri::AppHandle, mgr: Arc<SessionManager>) {
    use tauri::Emitter;
    let mut events = mgr.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!(skipped = n, "Session events lagged");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            };
            let result = match event {
                SessionEvent::TokenRefreshed(info) => app.emit("auth:token_refreshed", info),
                SessionEvent::Expired => app.emit("auth:session_expired", ()),
            };
            if let Err(e) = result {
                warn!(error = %e, "Could not deliver session event to the webview");
            }
        }
    });
    tauri::async_runtime::spawn(run_refresh_loop(mgr, SchedulerTiming::default()));
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{json, Value};
    use tokio::sync::RwLock;
    use wiremock::matchers::{body_json, header, header_exists, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::store::MemoryTokenStore;
    use super::*;
    use crate::AuthState;

    fn tokens(access: &str, refresh: &str) -> Value {
        json!({
            "access_token": access,
            "refresh_token": refresh,
            "expires_in": 900,
            "token_type": "Bearer",
            "setup_required": false
        })
    }

    fn me() -> Value {
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

    fn manager(store: &Arc<MemoryTokenStore>) -> SessionManager {
        SessionManager::new(
            reqwest::Client::new(),
            store.clone(),
            Arc::new(RwLock::new(AuthState::default())),
        )
    }

    async fn mount_login(server: &MockServer, access: &str, refresh: &str) {
        Mock::given(method("POST"))
            .and(path("/auth/login"))
            .respond_with(ResponseTemplate::new(200).set_body_json(tokens(access, refresh)))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path("/auth/me"))
            .respond_with(ResponseTemplate::new(200).set_body_json(me()))
            .mount(server)
            .await;
    }

    async fn logged_in(server: &MockServer, store: &Arc<MemoryTokenStore>) -> SessionManager {
        mount_login(server, "at-1", "rt-1").await;
        let m = manager(store);
        m.login(&server.uri(), "alice", "pw", None).await.unwrap();
        m
    }

    #[tokio::test]
    async fn login_returns_full_user_and_flags() {
        let server = MockServer::start().await;
        let mut body = tokens("at-1", "rt-1");
        body["setup_required"] = json!(true);
        body["password_change_required"] = json!(true);
        Mock::given(method("POST"))
            .and(path("/auth/login"))
            .and(body_json(json!({"username": "alice", "password": "pw"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
        let mut user = me();
        user["is_admin"] = json!(true);
        Mock::given(method("GET"))
            .and(path("/auth/me"))
            .and(header("authorization", "Bearer at-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(user))
            .mount(&server)
            .await;
        let store = Arc::new(MemoryTokenStore::new());
        let m = manager(&store);

        let result = m
            .login(&format!("{}/", server.uri()), "alice", "pw", None)
            .await
            .unwrap();

        assert_eq!(result.user["is_admin"], json!(true));
        assert!(result.setup_required);
        assert!(result.password_change_required);
        assert!(result.persisted);
        let stored = store.get().unwrap();
        assert_eq!(stored.refresh_token, "rt-1");
        assert_eq!(stored.server_url, server.uri());
        let info = m.auth_info().unwrap();
        assert_eq!(info.access_token, "at-1");
        assert_eq!(info.server_url, server.uri());
        // The refresh token is not part of anything the webview receives.
        let serialized =
            serde_json::to_string(&result).unwrap() + &serde_json::to_string(&info).unwrap();
        assert!(!serialized.contains("rt-1"));
        // Legacy commands still read the shared auth state.
        let auth = m.legacy_auth().read().await;
        assert_eq!(auth.access_token.as_deref(), Some("at-1"));
        assert_eq!(auth.user.as_ref().unwrap().username, "alice");
    }

    #[tokio::test]
    async fn login_maps_mfa_required() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/login"))
            .respond_with(
                ResponseTemplate::new(403).set_body_json(json!({"error": "MFA_REQUIRED"})),
            )
            .mount(&server)
            .await;
        let store = Arc::new(MemoryTokenStore::new());

        let err = manager(&store)
            .login(&server.uri(), "alice", "pw", None)
            .await
            .unwrap_err();

        assert!(matches!(err, SessionError::MfaRequired));
        assert_eq!(err.to_string(), "MFA_REQUIRED");
        assert!(store.get().is_none());
    }

    #[tokio::test]
    async fn login_maps_invalid_credentials() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/login"))
            .respond_with(
                ResponseTemplate::new(401).set_body_json(json!({"error": "INVALID_CREDENTIALS"})),
            )
            .mount(&server)
            .await;
        let store = Arc::new(MemoryTokenStore::new());

        let err = manager(&store)
            .login(&server.uri(), "alice", "bad", None)
            .await
            .unwrap_err();

        assert!(matches!(err, SessionError::InvalidCredentials));
    }

    #[tokio::test]
    async fn refresh_rotates_and_persists() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryTokenStore::new());
        let m = logged_in(&server, &store).await;
        Mock::given(method("POST"))
            .and(path("/auth/refresh"))
            .and(body_json(json!({"refresh_token": "rt-1"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(tokens("at-2", "rt-2")))
            .expect(1)
            .mount(&server)
            .await;

        let info = m.refresh().await.unwrap();

        assert_eq!(info.access_token, "at-2");
        assert_eq!(m.auth_info().unwrap().access_token, "at-2");
        assert_eq!(store.get().unwrap().refresh_token, "rt-2");
        assert_eq!(
            m.legacy_auth().read().await.access_token.as_deref(),
            Some("at-2")
        );
    }

    #[tokio::test]
    async fn dropped_refresh_caller_still_commits_rotation() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryTokenStore::new());
        let m = logged_in(&server, &store).await;
        Mock::given(method("POST"))
            .and(path("/auth/refresh"))
            .and(body_json(json!({"refresh_token": "rt-1"})))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(tokens("at-2", "rt-2"))
                    .set_delay(std::time::Duration::from_millis(300)),
            )
            .expect(1)
            .mount(&server)
            .await;

        // The caller gives up while the server is still answering.
        let dropped = tokio::time::timeout(std::time::Duration::from_millis(50), m.refresh()).await;
        assert!(dropped.is_err(), "refresh should still be in flight");

        // Anyone taking the lock next waits for the orphaned rotation.
        let info = m.refresh().await.unwrap();
        assert_eq!(info.access_token, "at-2");
        assert_eq!(m.auth_info().unwrap().access_token, "at-2");
        assert_eq!(store.get().unwrap().refresh_token, "rt-2");
        assert_eq!(
            m.legacy_auth().read().await.access_token.as_deref(),
            Some("at-2")
        );
    }

    #[tokio::test]
    async fn concurrent_refresh_calls_hit_server_once() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryTokenStore::new());
        let m = logged_in(&server, &store).await;
        Mock::given(method("POST"))
            .and(path("/auth/refresh"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(tokens("at-2", "rt-2"))
                    .set_delay(std::time::Duration::from_millis(100)),
            )
            .expect(1)
            .mount(&server)
            .await;

        let (a, b) = tokio::join!(m.refresh(), m.refresh());

        assert_eq!(a.unwrap().access_token, "at-2");
        assert_eq!(b.unwrap().access_token, "at-2");
        assert_eq!(store.get().unwrap().refresh_token, "rt-2");
    }

    #[tokio::test]
    async fn refresh_401_clears_store_and_returns_expired() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryTokenStore::new());
        let m = logged_in(&server, &store).await;
        Mock::given(method("POST"))
            .and(path("/auth/refresh"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let err = m.refresh().await.unwrap_err();

        assert!(matches!(err, SessionError::Expired));
        assert!(store.get().is_none());
        assert!(m.auth_info().is_none());
        assert!(m.legacy_auth().read().await.access_token.is_none());
    }

    #[tokio::test]
    async fn refresh_5xx_keeps_session() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryTokenStore::new());
        let m = logged_in(&server, &store).await;
        Mock::given(method("POST"))
            .and(path("/auth/refresh"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let err = m.refresh().await.unwrap_err();

        assert!(matches!(err, SessionError::Server(503, _)));
        assert_eq!(store.get().unwrap().refresh_token, "rt-1");
        assert_eq!(m.auth_info().unwrap().access_token, "at-1");
    }

    #[tokio::test]
    async fn restore_without_stored_session_returns_none() {
        let store = Arc::new(MemoryTokenStore::new());

        let result = manager(&store).restore().await;

        assert!(result.user.is_none());
        assert!(result.error_reason.is_none());
    }

    #[tokio::test]
    async fn restore_uses_stored_token() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryTokenStore::with_session(&server.uri(), "rt-old"));
        let mut body = tokens("at-2", "rt-2");
        body["password_change_required"] = json!(true);
        Mock::given(method("POST"))
            .and(path("/auth/refresh"))
            .and(body_json(json!({"refresh_token": "rt-old"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/auth/me"))
            .and(header("authorization", "Bearer at-2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(me()))
            .mount(&server)
            .await;
        let m = manager(&store);

        let result = m.restore().await;

        assert_eq!(result.user.unwrap()["username"], json!("alice"));
        assert!(result.error_reason.is_none());
        assert!(result.password_change_required);
        assert_eq!(store.get().unwrap().refresh_token, "rt-2");
        assert_eq!(m.auth_info().unwrap().access_token, "at-2");
    }

    #[test]
    fn restore_timeout_tolerates_a_slow_start() {
        // A short timeout dropped users on the login screen on a slow start
        // and could abandon a refresh the server was already rotating.
        assert!(RESTORE_TIMEOUT >= Duration::from_secs(15));
    }

    #[tokio::test]
    async fn restore_timed_out_rotation_still_commits() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryTokenStore::with_session(&server.uri(), "rt-old"));
        Mock::given(method("POST"))
            .and(path("/auth/refresh"))
            .and(body_json(json!({"refresh_token": "rt-old"})))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(tokens("at-2", "rt-2"))
                    .set_delay(std::time::Duration::from_millis(300)),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/auth/me"))
            .and(header("authorization", "Bearer at-2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(me()))
            .mount(&server)
            .await;
        let m = manager(&store);

        // The caller gives up while the server is already rotating: the old
        // refresh token is revoked the moment the server answers.
        let first = m.restore_within(Duration::from_millis(50)).await;
        assert!(first.user.is_none());
        assert_eq!(first.error_reason.as_deref(), Some("network_error"));

        // The retry waits for the orphaned rotation and finds the new session
        // instead of sending the revoked token again (`.expect(1)`).
        let second = m.restore().await;
        assert_eq!(second.user.unwrap()["username"], json!("alice"));
        assert_eq!(store.get().unwrap().refresh_token, "rt-2");
        assert_eq!(m.auth_info().unwrap().access_token, "at-2");
        let auth = m.legacy_auth().read().await;
        assert_eq!(auth.access_token.as_deref(), Some("at-2"));
        assert_eq!(auth.server_url.as_deref(), Some(server.uri().as_str()));
    }

    #[tokio::test]
    async fn restore_network_error_keeps_stored_token() {
        // Nothing listens on the discard port: connection refused.
        let store = Arc::new(MemoryTokenStore::with_session(
            "http://127.0.0.1:9",
            "rt-old",
        ));

        let result = manager(&store).restore().await;

        assert!(result.user.is_none());
        assert_eq!(result.error_reason.as_deref(), Some("network_error"));
        assert_eq!(
            store.get(),
            Some(StoredSession {
                server_url: "http://127.0.0.1:9".into(),
                refresh_token: "rt-old".into(),
            })
        );
    }

    #[tokio::test]
    async fn restore_rejected_token_reports_session_expired() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryTokenStore::with_session(&server.uri(), "rt-old"));
        Mock::given(method("POST"))
            .and(path("/auth/refresh"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let result = manager(&store).restore().await;

        assert!(result.user.is_none());
        assert_eq!(result.error_reason.as_deref(), Some("session_expired"));
        assert!(store.get().is_none());
    }

    #[tokio::test]
    async fn keyring_failure_still_logs_in_with_persisted_false() {
        let server = MockServer::start().await;
        mount_login(&server, "at-1", "rt-1").await;
        let store = Arc::new(MemoryTokenStore::new());
        store.set_fail_save(true);
        let m = manager(&store);

        let result = m.login(&server.uri(), "alice", "pw", None).await.unwrap();

        assert!(!result.persisted);
        assert_eq!(result.user["username"], json!("alice"));
        assert_eq!(m.auth_info().unwrap().access_token, "at-1");
        assert!(store.get().is_none());
    }

    #[tokio::test]
    async fn session_request_rejects_paths_outside_allowlist() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryTokenStore::new());
        let m = logged_in(&server, &store).await;
        let before = server.received_requests().await.unwrap().len();

        for (verb, p) in [
            ("GET", "/api/guilds"),
            ("DELETE", "/auth/sessions/../x"),
            ("POST", "/auth/sessions"),
            (
                "DELETE",
                "/auth/sessions/0192f0c0-0000-7000-8000-000000000001?x=1",
            ),
            (
                "DELETE",
                "/auth/sessions/0192f0c0-0000-7000-8000-000000000001/x",
            ),
            ("get", "/auth/sessions"),
        ] {
            let result = m.session_request(verb, p, None).await;
            assert!(result.is_err(), "{verb} {p} must be rejected");
        }

        assert_eq!(server.received_requests().await.unwrap().len(), before);
    }

    #[tokio::test]
    async fn session_request_sends_refresh_header() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryTokenStore::new());
        let m = logged_in(&server, &store).await;
        Mock::given(method("DELETE"))
            .and(path("/auth/sessions"))
            .and(header("x-refresh-token", "rt-1"))
            .and(header("authorization", "Bearer at-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"revoked_count": 2})))
            .expect(1)
            .mount(&server)
            .await;

        let value = m
            .session_request("DELETE", "/auth/sessions", None)
            .await
            .unwrap();

        assert_eq!(value["revoked_count"], json!(2));
    }

    #[tokio::test]
    async fn session_request_allows_revoking_one_session() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryTokenStore::new());
        let m = logged_in(&server, &store).await;
        Mock::given(method("DELETE"))
            .and(path("/auth/sessions/0192f0c0-0000-7000-8000-000000000001"))
            .and(header_exists("x-refresh-token"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let value = m
            .session_request(
                "DELETE",
                "/auth/sessions/0192f0c0-0000-7000-8000-000000000001",
                None,
            )
            .await
            .unwrap();

        assert_eq!(value, Value::Null);
    }

    #[tokio::test]
    async fn session_request_waits_for_concurrent_rotation() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryTokenStore::new());
        let m = logged_in(&server, &store).await;
        Mock::given(method("POST"))
            .and(path("/auth/refresh"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(tokens("at-2", "rt-2"))
                    .set_delay(std::time::Duration::from_millis(150)),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/auth/sessions"))
            .and(header("x-refresh-token", "rt-2"))
            .and(header("authorization", "Bearer at-2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"revoked_count": 1})))
            .expect(1)
            .mount(&server)
            .await;
        // A stale token would make the server revoke the current session too.
        Mock::given(method("DELETE"))
            .and(path("/auth/sessions"))
            .and(header("x-refresh-token", "rt-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"revoked_count": 2})))
            .expect(0)
            .mount(&server)
            .await;

        let (refreshed, revoked) = tokio::join!(m.refresh(), async {
            // Start while the rotation is in flight.
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            m.session_request("DELETE", "/auth/sessions", None).await
        });

        assert_eq!(refreshed.unwrap().access_token, "at-2");
        assert_eq!(revoked.unwrap()["revoked_count"], json!(1));
    }

    #[tokio::test]
    async fn session_request_rejected_refresh_ends_session_and_publishes_expiry() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryTokenStore::new());
        let m = logged_in(&server, &store).await;
        Mock::given(method("GET"))
            .and(path("/auth/sessions"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/auth/refresh"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&server)
            .await;
        let mut events = m.subscribe();

        let err = m
            .session_request("GET", "/auth/sessions", None)
            .await
            .unwrap_err();

        assert!(matches!(err, SessionError::Expired));
        assert!(matches!(events.try_recv(), Ok(SessionEvent::Expired)));
        assert!(m.auth_info().is_none());
        assert!(store.get().is_none());
    }

    #[tokio::test]
    async fn session_request_401_retry_publishes_new_token() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryTokenStore::new());
        let m = logged_in(&server, &store).await;
        Mock::given(method("GET"))
            .and(path("/auth/sessions"))
            .and(header("authorization", "Bearer at-1"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/auth/sessions"))
            .and(header("authorization", "Bearer at-2"))
            .and(header("x-refresh-token", "rt-2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"sessions": []})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/auth/refresh"))
            .respond_with(ResponseTemplate::new(200).set_body_json(tokens("at-2", "rt-2")))
            .expect(1)
            .mount(&server)
            .await;
        let mut events = m.subscribe();

        let value = m
            .session_request("GET", "/auth/sessions", None)
            .await
            .unwrap();

        assert_eq!(value["sessions"], json!([]));
        match events.try_recv() {
            Ok(SessionEvent::TokenRefreshed(info)) => assert_eq!(info.access_token, "at-2"),
            other => panic!("expected TokenRefreshed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn logout_during_delayed_restore_keeps_state_empty() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryTokenStore::with_session(&server.uri(), "rt-old"));
        Mock::given(method("POST"))
            .and(path("/auth/refresh"))
            .respond_with(ResponseTemplate::new(200).set_body_json(tokens("at-2", "rt-2")))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/auth/me"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(me())
                    .set_delay(std::time::Duration::from_millis(200)),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/auth/logout"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let m = manager(&store);

        let (restored, logged_out) = tokio::join!(m.restore(), async {
            // Sign out while the profile request is still pending.
            tokio::time::sleep(std::time::Duration::from_millis(80)).await;
            m.logout().await
        });

        logged_out.unwrap();
        assert!(restored.user.is_none());
        assert!(m.auth_info().is_none());
        assert!(store.get().is_none());
        let auth = m.legacy_auth().read().await;
        assert!(auth.user.is_none());
        assert!(auth.access_token.is_none());
    }

    #[tokio::test]
    async fn logout_revokes_on_server_and_clears_store() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryTokenStore::new());
        let m = logged_in(&server, &store).await;
        Mock::given(method("POST"))
            .and(path("/auth/logout"))
            .and(body_json(json!({"refresh_token": "rt-1"})))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        m.logout().await.unwrap();

        assert!(store.get().is_none());
        assert!(m.auth_info().is_none());
        assert!(m.legacy_auth().read().await.user.is_none());
    }

    #[test]
    fn next_wake_is_lead_before_expiry_and_bounded() {
        let timing = SchedulerTiming::default();
        let now = 1_000_000;
        // 15 min token: due 60 s before expiry, but re-check at least every max_sleep.
        assert_eq!(
            next_wake(now + 900_000, None, now, &timing),
            Some(timing.max_sleep)
        );
        assert_eq!(
            next_wake(now + 100_000, None, now, &timing),
            Some(std::time::Duration::from_secs(40))
        );
        // Within the lead: refresh now.
        assert_eq!(next_wake(now + 10_000, None, now, &timing), None);
        // After a transient failure: wait for the retry hold.
        assert_eq!(
            next_wake(now + 10_000, Some(now + 30_000), now, &timing),
            Some(std::time::Duration::from_secs(30))
        );
        // Never wake more often than the minimum delay.
        assert_eq!(
            next_wake(now + 10_000, Some(now + 1_000), now, &timing),
            Some(timing.min_delay)
        );
        assert!(timing.min_delay >= std::time::Duration::from_secs(5));
    }

    fn fast_timing() -> SchedulerTiming {
        SchedulerTiming {
            lead: std::time::Duration::from_mins(1),
            min_delay: std::time::Duration::from_millis(10),
            retry_delay: std::time::Duration::from_millis(50),
            max_sleep: std::time::Duration::from_millis(50),
        }
    }

    async fn expiring_session(
        server: &MockServer,
        store: &Arc<MemoryTokenStore>,
    ) -> Arc<SessionManager> {
        // expires_in 30 s < 60 s lead: the scheduler refreshes immediately.
        let mut body = tokens("at-1", "rt-1");
        body["expires_in"] = json!(30);
        Mock::given(method("POST"))
            .and(path("/auth/login"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path("/auth/me"))
            .respond_with(ResponseTemplate::new(200).set_body_json(me()))
            .mount(server)
            .await;
        let m = Arc::new(manager(store));
        m.login(&server.uri(), "alice", "pw", None).await.unwrap();
        m
    }

    #[tokio::test]
    async fn scheduler_retries_transient_failure_and_keeps_token() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryTokenStore::new());
        let m = expiring_session(&server, &store).await;
        Mock::given(method("POST"))
            .and(path("/auth/refresh"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/auth/refresh"))
            .respond_with(ResponseTemplate::new(200).set_body_json(tokens("at-2", "rt-2")))
            .mount(&server)
            .await;
        let mut rx = m.subscribe();
        let task = tokio::spawn(run_refresh_loop(m.clone(), fast_timing()));

        let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        task.abort();

        match event {
            SessionEvent::TokenRefreshed(info) => assert_eq!(info.access_token, "at-2"),
            SessionEvent::Expired => panic!("a 503 must not end the session"),
        }
        assert_eq!(store.get().unwrap().refresh_token, "rt-2");
    }

    #[tokio::test]
    async fn scheduler_emits_expired_on_rejected_refresh() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryTokenStore::new());
        let m = expiring_session(&server, &store).await;
        Mock::given(method("POST"))
            .and(path("/auth/refresh"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let mut rx = m.subscribe();
        let task = tokio::spawn(run_refresh_loop(m.clone(), fast_timing()));

        let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        task.abort();

        assert!(matches!(event, SessionEvent::Expired));
        assert!(store.get().is_none());
    }
}
