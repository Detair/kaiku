//! Authentication Service
//!
//! Handles local authentication, SSO/OIDC, MFA, and session management.

mod error;
mod handlers;
pub mod jwt;
mod middleware;
mod oidc;
mod password;

use axum::{
    middleware as axum_middleware,
    routing::{get, post},
    Router,
};

use crate::api::AppState;

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
    // Public routes (no auth required)
    let public_routes = Router::new()
        .route("/register", post(handlers::register))
        .route("/login", post(handlers::login))
        .route("/refresh", post(handlers::refresh_token))
        .route("/oidc/providers", get(handlers::oidc_providers))
        .route("/oidc/authorize/:provider", get(handlers::oidc_authorize))
        .route("/oidc/callback", get(handlers::oidc_callback));

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
