//! Device signatures and approvals (spec B.2, contract C26).
//!
//! **SECURITY CRITICAL.** See `AGENTS.md` in this directory.

pub mod cascade;
pub mod context;
pub mod core;
pub mod device_login;
pub mod devices;
pub mod enrollments;
pub mod error;
pub mod handlers;
pub mod secrets;
pub mod signing;
pub mod transition;
pub mod types;

#[cfg(test)]
mod golden_tests;

use axum::middleware::{from_fn, from_fn_with_state};
use axum::routing::{delete, get, post};
use axum::Router;

use crate::api::AppState;
use crate::ratelimit::{rate_limit_by_user, with_category, RateLimitCategory};

/// `/api/approvals` (bearer; `ApprovalLogin` per user).
pub fn approvals_router(state: &AppState) -> Router<AppState> {
    Router::new()
        .route("/claim", post(handlers::claim))
        .route("/step-up", post(handlers::step_up))
        .route("/{id}/answer", post(handlers::answer))
        .route("/{id}/deny", post(handlers::deny))
        .route("/{id}/release", post(handlers::release))
        .layer(from_fn_with_state(state.clone(), rate_limit_by_user))
        .layer(from_fn(with_category(RateLimitCategory::ApprovalLogin)))
}

/// `/api/approval-devices` (bearer).
pub fn devices_router() -> Router<AppState> {
    Router::new()
        .route("/", get(devices::handlers::list))
        .route("/{id}", delete(devices::handlers::remove))
        .route("/{id}/invalidated", post(devices::handlers::invalidated))
}

/// The server's display name (setup wizard `server_name`), shown before a
/// phone sends anything and signed in `login` contexts. Default "Kaiku".
pub async fn server_name(conn: &mut sqlx::PgConnection) -> sqlx::Result<String> {
    Ok(sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT value FROM server_config WHERE key = 'server_name'",
    )
    .fetch_optional(&mut *conn)
    .await?
    .and_then(|v| v.as_str().map(str::to_string))
    .filter(|s| !s.is_empty())
    .unwrap_or_else(|| "Kaiku".to_string()))
}

/// Public enrolment and device-key sign-in routes (`AuthFlow` per IP, never
/// IP-blocked) and the issuing key device's `/enrollments/{id}/challenge`,
/// merged into `/auth`.
pub fn flow_router(state: &AppState) -> Router<AppState> {
    use crate::ratelimit::rate_limit_by_ip;
    let public = Router::new()
        .route("/enrollments/open", post(enrollments::handlers::open))
        .route(
            "/enrollments/pop-challenge",
            post(enrollments::handlers::pop_challenge),
        )
        .route("/enrollments/claim", post(enrollments::handlers::claim))
        .route("/enrollments/status", post(enrollments::handlers::status))
        .route(
            "/device-login/challenge",
            post(device_login::challenge_handler),
        )
        .route("/device-login", post(device_login::sign_in_handler))
        .layer(from_fn_with_state(state.clone(), rate_limit_by_ip))
        .layer(from_fn(with_category(RateLimitCategory::AuthFlow)));
    let bearer = Router::new()
        .route("/enrollments", post(enrollments::handlers::issue_own))
        .route(
            "/enrollments/{id}",
            axum::routing::get(enrollments::handlers::get_issued)
                .delete(enrollments::handlers::cancel_issued),
        )
        .route(
            "/enrollments/{id}/confirm",
            post(enrollments::handlers::confirm_pc),
        )
        .route(
            "/enrollments/{id}/challenge",
            post(enrollments::handlers::issuer_challenge),
        )
        .layer(from_fn_with_state(state.clone(), rate_limit_by_user))
        .layer(from_fn(with_category(RateLimitCategory::ApprovalLogin)))
        .layer(from_fn_with_state(state.clone(), crate::auth::require_auth));
    public.merge(bearer)
}
