//! Authentication Service
//!
//! Handles local authentication, SSO/OIDC, MFA, and session management.

pub mod account_requests;
pub mod assurance;
pub mod client_class;
pub(crate) mod cookies;
pub mod ctx;
pub(crate) mod error;
pub mod factors;
pub mod guards;
mod helpers;
pub(crate) mod identities;
pub mod jwt;
pub(crate) mod login;
pub mod login_attempts;
pub(crate) mod mfa;
pub mod mfa_crypto;
mod middleware;
pub mod oidc;
pub mod owner_rule;
pub mod passkeys;
pub(crate) mod password;
pub mod password_policy;
pub(crate) mod profile;
pub(crate) mod queries;
pub(crate) mod reauth;
pub mod recovery_codes;
pub mod recovery_contact;
pub mod refresh;
pub(crate) mod register;
pub mod registration;
pub mod repudiate;
pub mod revocation;
pub mod security_overview;
pub mod session_check;
pub mod session_issue;
pub(crate) mod sessions;
pub mod step_up;
pub mod strong;
pub mod timers;
pub mod types;
pub mod ua_parser;

use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, post};
use axum::{middleware as axum_middleware, Router};
pub use error::{AuthError, AuthResult};
pub use helpers::hash_token;
pub use jwt::Claims;
pub use middleware::{
    optional_auth, require_auth, require_auth_allow_password_change, AuthUser, SessionMeta,
};
pub use password::{hash_password, verify_password};

use crate::api::AppState;
use crate::ratelimit::{check_ip_not_blocked, rate_limit_by_ip, with_category, RateLimitCategory};

/// Create authentication router.
///
/// Public routes (no auth required):
/// - POST /register - Register a new user
/// - POST /login - Login with username/password
/// - POST /refresh - Refresh access token
/// - POST /login-attempts[/{id}/poll|collect|cancel] - Sign in with phone (SM1, C27)
/// - GET /oidc/providers - List OIDC providers
/// - GET /oidc/authorize/{provider} - Initiate OIDC flow
/// - GET /oidc/callback - OIDC callback
/// - POST /oidc/desktop/exchange - Redeem a desktop hand-off code (rate limited)
///
/// Protected routes that stay usable while an admin-issued temporary password
/// has not been replaced yet (`require_auth_allow_password_change`):
/// - POST /logout - Invalidate session
/// - GET /me - Get current user profile
/// - POST/DELETE /me/password - Set or remove the password (full + step-up, C27)
///
/// Protected routes (auth required, password change completed):
/// - GET /sessions - List active sessions
/// - DELETE /sessions - Revoke all sessions except current
/// - DELETE /sessions/{id} - Revoke a specific session
/// - POST /me - Update profile
/// - POST /me/avatar - Upload avatar
/// - POST /mfa/setup, /mfa/verify - Set up TOTP (full + step-up `totp.set`, C27)
/// - POST /mfa/disable - Turn TOTP off (full + step-up `totp.remove`)
/// - POST /recovery-codes - New recovery codes (full session + step-up, C27)
/// - GET /recovery-codes/count - Unused recovery codes (any session)
/// - GET /reauth/methods - Step-up methods of the caller (C27)
pub fn router(state: AppState) -> Router<AppState> {
    // Phone setup and device-key sign-in (C26), merged under `/auth`.
    let flow_routes = crate::approvals::flow_router(&state);
    // Passkeys (C29): their own `AuthPasskey` limits, never IP-blocked.
    let passkey_routes = passkeys::router(&state);
    // Login route with IP block check and rate limiting
    let login_route = Router::new()
        .route("/login", post(login::login))
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
        .route("/register", post(register::register))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            rate_limit_by_ip,
        ))
        .layer(axum_middleware::from_fn(with_category(
            RateLimitCategory::AuthRegister,
        )));

    // Refresh route with rate limiting
    let refresh_route = Router::new()
        .route("/refresh", post(refresh::refresh_token))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            rate_limit_by_ip,
        ))
        .layer(axum_middleware::from_fn(with_category(
            RateLimitCategory::AuthOther,
        )));

    // OIDC routes - authorize gets rate limiting to prevent abuse
    let oidc_authorize_route = Router::new()
        .route("/oidc/authorize/{provider}", get(login::oidc_authorize))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            rate_limit_by_ip,
        ))
        .layer(axum_middleware::from_fn(with_category(
            RateLimitCategory::AuthOther,
        )));

    // OIDC callback with rate limiting (same as authorize)
    let oidc_callback_route = Router::new()
        .route("/oidc/callback", get(login::oidc_callback))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            rate_limit_by_ip,
        ))
        .layer(axum_middleware::from_fn(with_category(
            RateLimitCategory::AuthOther,
        )));

    // Desktop hand-off code exchange: IP block check and rate limiting like
    // the other public sign-in routes.
    let oidc_exchange_route = Router::new()
        .route("/oidc/desktop/exchange", post(login::oidc_desktop_exchange))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            rate_limit_by_ip,
        ))
        .layer(axum_middleware::from_fn(with_category(
            RateLimitCategory::AuthOther,
        )))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            check_ip_not_blocked,
        ));

    // OIDC providers list (no rate limiting needed)
    let oidc_providers_route = Router::new().route("/oidc/providers", get(login::oidc_providers));

    let oidc_routes = oidc_authorize_route
        .merge(oidc_callback_route)
        .merge(oidc_exchange_route)
        .merge(oidc_providers_route);

    // Merge all public routes
    let public_routes = login_route
        .merge(register_route)
        .merge(refresh_route)
        .merge(oidc_routes)
        .merge(login_attempts::router(&state))
        .merge(registration::router(&state))
        .merge(account_requests::public_router(&state));

    // Routes a user with a pending forced password change may still use.
    // `/me/password` needs a full session and a step-up grant (C27).
    let password_change_routes = Router::new()
        .route("/logout", post(login::logout))
        .route("/me", get(profile::get_profile))
        .route(
            "/me/password",
            post(profile::update_password).delete(profile::remove_password),
        )
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            require_auth_allow_password_change,
        ));

    // Protected routes (auth required)
    let protected_routes = Router::new()
        .route(
            "/sessions",
            get(sessions::list_sessions).delete(sessions::revoke_all_other_sessions),
        )
        .route("/sessions/{id}", delete(sessions::revoke_session))
        .route("/me", post(profile::update_profile))
        .route(
            "/me/avatar",
            post(profile::upload_avatar).layer(DefaultBodyLimit::max(state.config.max_avatar_size)),
        )
        .route("/me/identities", get(identities::list_identities))
        .route("/me/identities/{id}", delete(identities::unlink_identity))
        .route("/mfa/setup", post(mfa::mfa_setup))
        .route("/mfa/verify", post(mfa::mfa_verify))
        .route("/mfa/disable", post(mfa::mfa_disable))
        .route("/reauth/methods", get(reauth::get_reauth_methods))
        .route("/recovery-codes", post(recovery_codes::create))
        .route("/recovery-codes/count", get(recovery_codes::count))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            require_auth,
        ));

    // Identity link-authorize: like the public OIDC authorize, each call triggers
    // an outbound request to the provider + a Redis state write, so it needs the
    // same per-IP throttle on top of auth (the rest of the protected block is
    // cheap DB work and stays unthrottled to avoid breaking the settings UI).
    let link_authorize_route = Router::new()
        .route(
            "/me/identities/authorize/{provider}",
            get(identities::link_authorize),
        )
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            require_auth,
        ))
        .layer(axum_middleware::from_fn_with_state(state, rate_limit_by_ip))
        .layer(axum_middleware::from_fn(with_category(
            RateLimitCategory::AuthOther,
        )));

    public_routes
        .merge(flow_routes)
        .merge(passkey_routes)
        .merge(password_change_routes)
        .merge(protected_routes)
        .merge(link_authorize_route)
}
