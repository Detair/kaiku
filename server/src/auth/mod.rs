//! Authentication Service
//!
//! Handles local authentication, SSO/OIDC, MFA, and session management.

mod backup_codes;
pub(crate) mod error;
pub(crate) mod handlers;
pub mod jwt;
pub mod mfa_crypto;
mod middleware;
pub mod oidc;
mod password;

use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use axum::{middleware as axum_middleware, Router};
pub use error::{AuthError, AuthResult};
pub use jwt::Claims;
pub use middleware::{require_auth, AuthUser};
pub use password::{hash_password, verify_password};

use crate::api::AppState;
use crate::ratelimit::{check_ip_not_blocked, rate_limit_by_ip, with_category, RateLimitCategory};

/// Hash a token for secure storage using SHA256.
///
/// Used for storing refresh tokens - we never store the raw token.
#[must_use]
pub fn hash_token(token: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

/// Create authentication router.
///
/// Public routes (no auth required):
/// - POST /register - Register a new user
/// - POST /login - Login with username/password
/// - POST /refresh - Refresh access token
/// - POST /forgot-password - Request password reset email
/// - POST /reset-password - Reset password with token
/// - GET /oidc/providers - List OIDC providers
/// - GET /oidc/authorize/{provider} - Initiate OIDC flow
/// - GET /oidc/callback - OIDC callback
///
/// Protected routes (auth required):
/// - POST /logout - Invalidate session
/// - GET /me - Get current user profile
/// - POST /me - Update profile
/// - POST /me/avatar - Upload avatar
/// - POST /mfa/setup - Setup MFA
/// - POST /mfa/verify - Verify MFA (TOTP or backup code)
/// - POST /mfa/disable - Disable MFA
/// - POST /mfa/backup-codes - Generate MFA backup codes
pub fn router(state: AppState) -> Router<AppState> {
    // Login route with IP block check and rate limiting
    let login_route = Router::new()
        .route("/login", post(handlers::login))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            rate_limit_by_ip,
        ))
        .layer(axum_middleware::from_fn(with_category(
            RateLimitCategory::AuthLogin,
        )))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            check_ip_not_blocked,
        ));

    // Register route with rate limiting
    let register_route = Router::new()
        .route("/register", post(handlers::register))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            rate_limit_by_ip,
        ))
        .layer(axum_middleware::from_fn(with_category(
            RateLimitCategory::AuthRegister,
        )));

    // Refresh route with rate limiting
    let refresh_route = Router::new()
        .route("/refresh", post(handlers::refresh_token))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            rate_limit_by_ip,
        ))
        .layer(axum_middleware::from_fn(with_category(
            RateLimitCategory::AuthOther,
        )));

    // OIDC routes - authorize gets rate limiting to prevent abuse
    let oidc_authorize_route = Router::new()
        .route("/oidc/authorize/{provider}", get(handlers::oidc_authorize))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            rate_limit_by_ip,
        ))
        .layer(axum_middleware::from_fn(with_category(
            RateLimitCategory::AuthOther,
        )));

    // OIDC routes without rate limiting (providers list + callback from IdP)
    let oidc_other_routes = Router::new()
        .route("/oidc/providers", get(handlers::oidc_providers))
        .route("/oidc/callback", get(handlers::oidc_callback));

    let oidc_routes = oidc_authorize_route.merge(oidc_other_routes);

    // Password reset routes with rate limiting
    let forgot_password_route = Router::new()
        .route("/forgot-password", post(handlers::forgot_password))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            rate_limit_by_ip,
        ))
        .layer(axum_middleware::from_fn(with_category(
            RateLimitCategory::AuthPasswordReset,
        )));

    let reset_password_route = Router::new()
        .route("/reset-password", post(handlers::reset_password))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            rate_limit_by_ip,
        ))
        .layer(axum_middleware::from_fn(with_category(
            RateLimitCategory::AuthPasswordReset,
        )));

    // Merge all public routes
    let public_routes = login_route
        .merge(register_route)
        .merge(refresh_route)
        .merge(oidc_routes)
        .merge(forgot_password_route)
        .merge(reset_password_route);

    // Protected routes (auth required)
    let protected_routes = Router::new()
        .route("/logout", post(handlers::logout))
        .route("/me", get(handlers::get_profile))
        .route("/me", post(handlers::update_profile))
        .route(
            "/me/avatar",
            post(handlers::upload_avatar)
                .layer(DefaultBodyLimit::max(state.config.max_avatar_size)),
        )
        .route("/mfa/setup", post(handlers::mfa_setup))
        .route("/mfa/verify", post(handlers::mfa_verify))
        .route("/mfa/disable", post(handlers::mfa_disable))
        .route(
            "/mfa/backup-codes",
            post(handlers::mfa_generate_backup_codes),
        )
        .route(
            "/mfa/backup-codes/count",
            get(handlers::mfa_backup_code_count),
        )
        .layer(axum_middleware::from_fn_with_state(state, require_auth));

    public_routes.merge(protected_routes)
}
