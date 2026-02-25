//! Authentication Middleware

use axum::extract::{Request, State};
use axum::http::header::AUTHORIZATION;
use axum::middleware::Next;
use axum::response::Response;
use uuid::Uuid;

use super::error::AuthError;
use super::jwt::validate_access_token;
use crate::api::AppState;
use crate::db::{find_user_by_id, User};

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
        }
    }
}

/// Middleware to require authentication.
///
/// Extracts Bearer token from Authorization header, validates JWT,
/// loads user from database, and injects `AuthUser` into request extensions.
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
    mut request: Request,
    next: Next,
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

    // Parse user ID from claims
    let user_id: Uuid = claims.sub.parse().map_err(|_| AuthError::InvalidToken)?;

    // Load user from database
    let user = find_user_by_id(&state.db, user_id)
        .await?
        .ok_or(AuthError::UserNotFound)?;

    // Inject AuthUser into request extensions
    let auth_user = AuthUser::from(user);
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

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Self>()
            .cloned()
            .ok_or(AuthError::MissingAuthHeader)
    }
}

impl<S> axum::extract::OptionalFromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Option<Self>, Self::Rejection> {
        Ok(parts.extensions.get::<Self>().cloned())
    }
}
