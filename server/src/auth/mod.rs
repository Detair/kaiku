//! Authentication Service
//!
//! Handles local authentication, SSO/OIDC, MFA, and session management.

mod backup_codes;
pub(crate) mod cookies;
pub(crate) mod error;
pub mod geoip;
mod helpers;
pub(crate) mod identities;
pub mod jwt;
pub(crate) mod login;
pub(crate) mod mfa;
pub mod mfa_crypto;
mod middleware;
pub mod oidc;
mod password;
pub(crate) mod profile;
pub(crate) mod queries;
pub(crate) mod reauth;
pub(crate) mod register;
pub(crate) mod revocation;
pub(crate) mod sessions;
pub mod types;
pub mod ua_parser;

use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, post};
use axum::{middleware as axum_middleware, Router};
pub use error::{AuthError, AuthResult};
pub use helpers::hash_token;
pub use jwt::Claims;
pub use middleware::{require_auth, require_auth_allow_password_change, AuthUser};
pub use password::{hash_password, verify_password};

use crate::api::AppState;
use crate::ratelimit::{
    check_ip_not_blocked, rate_limit_by_ip, rate_limit_by_user, with_category, RateLimitCategory,
};

/// Create authentication router.
///
/// Public routes (no auth required):
/// - POST /register - Register a new user
/// - POST /login - Login with username/password
/// - POST /refresh - Refresh access token
/// - POST /forgot-password - Request password reset email
/// - POST /reset-password - Reset password with token
/// - POST /qr/redeem - Redeem a one-time QR login token
/// - GET /oidc/providers - List OIDC providers
/// - GET /oidc/authorize/{provider} - Initiate OIDC flow
/// - GET /oidc/callback - OIDC callback
/// - POST /oidc/desktop/exchange - Redeem a desktop hand-off code (rate limited)
///
/// Protected routes that stay usable while an admin-issued temporary password
/// has not been replaced yet (`require_auth_allow_password_change`):
/// - POST /logout - Invalidate session
/// - GET /me - Get current user profile
/// - POST /me/password - Change password (invalidates all sessions; rate limited)
///
/// Protected routes (auth required, password change completed):
/// - GET /sessions - List active sessions
/// - DELETE /sessions - Revoke all sessions except current
/// - DELETE /sessions/{id} - Revoke a specific session
/// - POST /me - Update profile
/// - POST /me/avatar - Upload avatar
/// - POST /mfa/setup - Setup MFA
/// - POST /mfa/verify - Verify MFA (TOTP or backup code; rate limited)
/// - POST /mfa/disable - Disable MFA (rate limited)
/// - POST /mfa/backup-codes - Generate MFA backup codes (current TOTP code; rate limited)
/// - POST /qr/create - Create a one-time QR login token (rate limited)
/// - GET /reauth/methods - Re-authentication methods of the caller (C24)
/// - POST /reauth/oidc/authorize/{provider} - Start an OIDC re-authentication that elevates the
///   session (rate limited)
pub fn router(state: AppState) -> Router<AppState> {
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
        .route("/refresh", post(login::refresh_token))
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

    // Password reset routes with rate limiting
    let forgot_password_route = Router::new()
        .route("/forgot-password", post(login::forgot_password))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            rate_limit_by_ip,
        ))
        .layer(axum_middleware::from_fn(with_category(
            RateLimitCategory::AuthPasswordReset,
        )));

    let reset_password_route = Router::new()
        .route("/reset-password", post(login::reset_password))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            rate_limit_by_ip,
        ))
        .layer(axum_middleware::from_fn(with_category(
            RateLimitCategory::AuthPasswordReset,
        )));

    // QR login redeem route with rate limiting (public — mobile isn't authenticated yet).
    // Uses AuthLogin category: like /login, this exchanges a credential for a full session.
    let qr_redeem_route = Router::new()
        .route("/qr/redeem", post(mfa::qr_redeem))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            rate_limit_by_ip,
        ))
        .layer(axum_middleware::from_fn(with_category(
            RateLimitCategory::AuthLogin,
        )));

    // Merge all public routes
    let public_routes = login_route
        .merge(register_route)
        .merge(refresh_route)
        .merge(oidc_routes)
        .merge(forgot_password_route)
        .merge(reset_password_route)
        .merge(qr_redeem_route);

    // Routes a user with a pending forced password change may still use.
    let password_change_routes = Router::new()
        .route("/logout", post(login::logout))
        .route("/me", get(profile::get_profile))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            require_auth_allow_password_change,
        ));

    // Routes that check a password or an MFA code (C24): per-user rate limit,
    // and refused while the client IP is blocked after failed attempts. Auth
    // runs first so the limit is keyed by the user.
    let password_route = with_reauth_limits(
        Router::new().route("/me/password", post(profile::update_password)),
        &state,
    )
    .layer(axum_middleware::from_fn_with_state(
        state.clone(),
        require_auth_allow_password_change,
    ));
    let mfa_reauth_routes = with_reauth_limits(
        Router::new()
            .route(
                "/reauth/oidc/authorize/{provider}",
                post(reauth::oidc_reauth_authorize),
            )
            .route("/mfa/verify", post(mfa::mfa_verify))
            .route("/mfa/disable", post(mfa::mfa_disable))
            .route("/mfa/backup-codes", post(mfa::mfa_generate_backup_codes))
            .route("/qr/create", post(mfa::qr_create)),
        &state,
    )
    .layer(axum_middleware::from_fn_with_state(
        state.clone(),
        require_auth,
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
        .route("/reauth/methods", get(reauth::get_reauth_methods))
        .route("/mfa/backup-codes/count", get(mfa::mfa_backup_code_count))
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
        .merge(password_change_routes)
        .merge(password_route)
        .merge(mfa_reauth_routes)
        .merge(protected_routes)
        .merge(link_authorize_route)
}

/// Limits for routes that re-check a password or an MFA code (C24).
///
/// Wrap the result in the auth layer: `rate_limit_by_user` needs `AuthUser`.
/// Outer to inner: IP block check, category, per-user rate limit.
pub(crate) fn with_reauth_limits<S>(router: Router<S>, state: &AppState) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            rate_limit_by_user,
        ))
        .layer(axum_middleware::from_fn(with_category(
            RateLimitCategory::AuthOther,
        )))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            check_ip_not_blocked,
        ))
}
