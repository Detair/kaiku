//! Authentication Service
//!
//! Handles local authentication, SSO/OIDC, MFA, and session management.

mod error;
mod handlers;
pub mod jwt;
pub mod mfa_crypto;
mod middleware;
mod oidc;
mod password;

use axum::{
    middleware as axum_middleware,
    routing::{get, post},
    Router,
};

use crate::api::AppState;
use crate::ratelimit::{check_ip_not_blocked, rate_limit_by_ip, with_category, RateLimitCategory};

pub use error::{AuthError, AuthResult};
pub use middleware::{require_auth, AuthUser};

/// Create authentication router.
///
/// Public routes (no auth required):
/// - POST /register - Register a new user
/// - POST /login - Login with username/password
/// - POST /refresh - Refresh access token
/// - GET /oidc/providers - List OIDC providers
/// - GET /oidc/authorize/:provider - Initiate OIDC flow
/// - GET /oidc/callback - OIDC callback
///
/// Protected routes (auth required):
/// - POST /logout - Invalidate session
/// - GET /me - Get current user profile
/// - POST /me - Update profile
/// - POST /mfa/setup - Setup MFA
/// - POST /mfa/verify - Verify MFA
/// - POST /mfa/disable - Disable MFA
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
        .route("/oidc/authorize/:provider", get(handlers::oidc_authorize))
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

    // Merge all public routes
    let public_routes = login_route
        .merge(register_route)
        .merge(refresh_route)
        .merge(oidc_routes);

    // Protected routes (auth required)
    let protected_routes = Router::new()
        .route("/logout", post(handlers::logout))
        .route("/me", get(handlers::get_profile))
        .route("/me", post(handlers::update_profile))
        .route("/mfa/setup", post(handlers::mfa_setup))
        .route("/mfa/verify", post(handlers::mfa_verify))
        .route("/mfa/disable", post(handlers::mfa_disable))
        .layer(axum_middleware::from_fn_with_state(state, require_auth));

    public_routes.merge(protected_routes)
}
