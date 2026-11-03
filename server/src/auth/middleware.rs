//! Authentication Middleware

use axum::extract::{Request, State};
use axum::http::header::AUTHORIZATION;
use axum::middleware::Next;
use axum::response::Response;
use uuid::Uuid;

use super::assurance::Assurance;
use super::client_class::ClientClass;
use super::error::AuthError;
use super::jwt::validate_access_token;
use super::session_check::{check_claims, Refusal, RequestSession};
use super::strong::AuthenticatorRef;
use crate::api::AppState;
use crate::db::User;

/// The committed session behind a request (B.4 rev 5), as `require_auth`
/// read it.
#[derive(Debug, Clone)]
pub struct SessionMeta {
    pub id: Uuid,
    pub assurance: Assurance,
    pub auth_methods: Vec<String>,
    pub auth_time: chrono::DateTime<chrono::Utc>,
    pub client_class: ClientClass,
    pub authenticator: Option<AuthenticatorRef>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub absolute_expires_at: chrono::DateTime<chrono::Utc>,
}

impl From<&RequestSession> for SessionMeta {
    fn from(s: &RequestSession) -> Self {
        Self {
            id: s.session_id,
            assurance: s.assurance,
            auth_methods: s.auth_methods.clone(),
            auth_time: s.auth_time,
            client_class: s.client_class,
            authenticator: s.authenticator,
            created_at: s.created_at,
            absolute_expires_at: s.absolute_expires_at,
        }
    }
}

/// Authenticated user injected into request extensions.
///
/// This is a minimal struct containing only safe-to-expose user data.
/// Use this in handlers to access the current user.
#[derive(Debug, Clone)]
pub struct AuthUser {
    /// User ID.
    pub id: Uuid,
    /// Username.
    pub username: String,
    /// Display name.
    pub display_name: String,
    /// Email (if set).
    pub email: Option<String>,
    /// Avatar URL (if set).
    pub avatar_url: Option<String>,
    /// Whether MFA is enabled.
    pub mfa_enabled: bool,
    /// When the account is scheduled for permanent deletion (if requested).
    pub deletion_scheduled_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Whether the user must set a new password before using the rest of the API.
    pub must_change_password: bool,
    /// Session the access token belongs to (`sid` claim). Always set by
    /// `require_auth`; `None` only for an `AuthUser` built from a `User`
    /// outside the middleware.
    pub session_id: Option<Uuid>,
    /// The committed session row (assurance, methods, client class,
    /// authenticator). `None` only outside the middleware, treated as
    /// probation.
    pub session: Option<SessionMeta>,
}

impl AuthUser {
    /// The session's assurance; probation without a session.
    #[must_use]
    pub fn assurance(&self) -> Assurance {
        self.session
            .as_ref()
            .map_or(Assurance::Probation, |s| s.assurance)
    }
}

impl From<User> for AuthUser {
    fn from(user: User) -> Self {
        Self {
            id: user.id,
            username: user.username,
            display_name: user.display_name,
            email: user.email,
            avatar_url: user.avatar_url,
            mfa_enabled: user.mfa_secret.is_some(),
            deletion_scheduled_at: user.deletion_scheduled_at,
            must_change_password: user.must_change_password,
            session_id: None,
            session: None,
        }
    }
}

/// Middleware to require authentication.
///
/// Extracts Bearer token from Authorization header, validates JWT,
/// loads user from database, and injects `AuthUser` into request extensions.
///
/// Users that still have to replace an admin-issued temporary password are
/// rejected with [`AuthError::PasswordChangeRequired`]; the few routes they may
/// use are layered with [`require_auth_allow_password_change`] instead.
///
/// # Usage
///
/// Apply to routes that require authentication:
/// ```ignore
/// Router::new()
///     .route("/protected", get(handler))
///     .layer(axum::middleware::from_fn_with_state(state, require_auth))
/// ```
pub async fn require_auth(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, AuthError> {
    authenticate(&state, request, next, false).await
}

/// Like [`require_auth`], but also admits users whose password change is
/// still pending.
///
/// Only for the routes such a user needs to finish onboarding: changing the
/// password, reading their own profile and logging out.
pub async fn require_auth_allow_password_change(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, AuthError> {
    authenticate(&state, request, next, true).await
}

/// Like [`require_auth`] when an `Authorization` header is present.
///
/// A bad header is refused; without one the request passes unauthenticated. For
/// routes that are public for one purpose and bearer-bound for another (the
/// desktop passkey flows, C29); handlers take `Option<Extension<AuthUser>>`.
pub async fn optional_auth(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, AuthError> {
    if request.headers().contains_key(AUTHORIZATION) {
        authenticate(&state, request, next, false).await
    } else {
        Ok(next.run(request).await)
    }
}

async fn authenticate(
    state: &AppState,
    mut request: Request,
    next: Next,
    allow_password_change: bool,
) -> Result<Response, AuthError> {
    // Extract Authorization header
    let auth_header = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .ok_or(AuthError::MissingAuthHeader)?;

    // Parse Bearer token
    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or(AuthError::InvalidAuthHeader)?;

    // Validate JWT
    let claims = validate_access_token(token, &state.config.jwt_public_key)?;

    // The committed session row decides (B.4 rev 5): a denylist hit rejects
    // at once; a miss or an unreachable denylist falls through to the row,
    // read by primary key with the user and ban state (DC6). A token without
    // `sid`, an unknown, ended or expired session, another user's session
    // and the system account all answer 401 `INVALID_TOKEN`.
    let session =
        match check_claims(&state.db, &state.redis, &claims.sub, claims.sid.as_deref()).await {
            Ok(s) => s,
            Err(Refusal::Invalid) => return Err(AuthError::InvalidToken),
            Err(Refusal::Banned) => return Err(AuthError::AccountBanned),
            Err(Refusal::Db(e)) => return Err(AuthError::Database(e)),
        };

    if session.user.must_change_password && !allow_password_change {
        return Err(AuthError::PasswordChangeRequired);
    }

    // Inject AuthUser into request extensions
    let meta = SessionMeta::from(&session);
    let mut auth_user = AuthUser::from(session.user);
    auth_user.session_id = Some(meta.id);
    auth_user.session = Some(meta);
    request.extensions_mut().insert(auth_user);

    // Continue to handler
    Ok(next.run(request).await)
}

/// Extractor for authenticated user in handlers.
///
/// Use this to get the current user in protected endpoints:
///
/// ```ignore
/// async fn protected_handler(auth_user: AuthUser) -> impl IntoResponse {
///     format!("Hello, {}!", auth_user.username)
/// }
/// ```
impl<S> axum::extract::FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> impl std::future::Future<Output = Result<Self, Self::Rejection>> + Send {
        std::future::ready(
            parts
                .extensions
                .get::<Self>()
                .cloned()
                .ok_or(AuthError::MissingAuthHeader),
        )
    }
}

impl<S> axum::extract::OptionalFromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> impl std::future::Future<Output = Result<Option<Self>, Self::Rejection>> + Send {
        std::future::ready(Ok(parts.extensions.get::<Self>().cloned()))
    }
}
