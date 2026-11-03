//! Passkeys (`WebAuthn` L3, discoverable, UV required; spec B.1, F1, F7, C29).
//!
//! - `config`: the RP ID (`PUBLIC_URL`'s host), the web origin and the Android `apk-key-hash`
//!   origins.
//! - `rp`: `webauthn-rs` in Kaiku's profile (exact origins, UV, BE).
//! - `assetlinks`: `/.well-known/assetlinks.json` for the Android apps.
//! - `ceremony`: single-use ceremony state in Valkey (fail closed).
//! - `queries`: the `passkeys` table; revocation runs the L1a cascade.
//! - `rp_pin`: the RP-ID pin checked at startup (DC5).
//! - `step_up`: passkey step-up on L1a's action-bound grants.
//! - `login`: discoverable sign-in → a new full session.
//! - `manage`: list, rename and remove (never the last strong authenticator).
//! - `desktop`, `desktop_page`: the desktop flow in the system browser and the loopback hand-off
//!   page (F1, C29 steps 1–7).
//! - `register`: adding a passkey (`passkey.add`) and the core L1b calls.
//!
//! Limits (B.5): every passkey route has its own `AuthPasskey` request
//! limit (per IP for public routes, per user behind a session); none is
//! behind `check_ip_not_blocked`, and no passkey failure feeds `failed_auth`.

pub mod assetlinks;
pub mod ceremony;
pub mod config;
pub(crate) mod desktop;
pub mod desktop_page;
pub(crate) mod login;
pub(crate) mod manage;
pub mod queries;
pub mod register;
pub mod rp;
pub mod rp_pin;
pub(crate) mod step_up;
pub mod types;

use axum::middleware::{from_fn, from_fn_with_state};
use axum::routing::{get, patch, post};
use axum::Router;

use self::rp::{PasskeyFailure, PasskeyRp};
use crate::api::AppState;
use crate::auth::{AuthError, AuthResult};
use crate::ratelimit::{rate_limit_by_ip, rate_limit_by_user, with_category, RateLimitCategory};

/// At most 10 active passkeys per account (DC6).
pub const MAX_ACTIVE_PASSKEYS: usize = 10;

/// The configured RP, or 503 `PASSKEYS_UNAVAILABLE` (test configs).
pub(crate) fn rp_of(state: &AppState) -> AuthResult<&PasskeyRp> {
    state
        .config
        .passkeys
        .as_deref()
        .ok_or(AuthError::PasskeysUnavailable)
}

/// The client-facing code of a verification failure.
pub(crate) fn failure(f: PasskeyFailure) -> AuthError {
    match f {
        PasskeyFailure::Invalid
        | PasskeyFailure::CounterRegressed
        | PasskeyFailure::UserHandleMissing => AuthError::PasskeyInvalid,
        PasskeyFailure::NotDiscoverable => AuthError::PasskeyNotDiscoverable,
        PasskeyFailure::Internal(e) => AuthError::Internal(format!("passkey: {e}")),
    }
}

pub(crate) mod audit {
    use super::queries::PasskeyRow;
    use crate::api::AppState;

    /// B.1: a counter that went backwards is a signal (this audit row), never
    /// a refusal; the passkey stays active and the stored counter stays.
    pub async fn counter_regression(state: &AppState, row: &PasskeyRow, received: u32) {
        if let Err(e) = crate::permissions::write_audit_log(
            &state.db,
            row.user_id,
            "auth.passkey.counter_regression",
            Some("passkey"),
            Some(row.id),
            Some(serde_json::json!({
                "label": row.label,
                "stored": super::rp::stored_counter(&row.credential.0),
                "received": received,
            })),
            None,
        )
        .await
        {
            tracing::warn!(error = %e, "could not audit a passkey counter regression");
        }
    }
}

/// Public passkey routes: per-IP `AuthPasskey` limit only.
pub(crate) fn public_limits(router: Router<AppState>, state: &AppState) -> Router<AppState> {
    router
        .layer(from_fn_with_state(state.clone(), rate_limit_by_ip))
        .layer(from_fn(with_category(RateLimitCategory::AuthPasskey)))
}

/// Session routes: `require_auth`, then a per-user `AuthPasskey` limit.
pub(crate) fn session_limits(router: Router<AppState>, state: &AppState) -> Router<AppState> {
    router
        .layer(from_fn_with_state(state.clone(), rate_limit_by_user))
        .layer(from_fn(with_category(RateLimitCategory::AuthPasskey)))
        .layer(from_fn_with_state(state.clone(), crate::auth::require_auth))
}

/// The passkey routes, merged under `/auth`.
pub fn router(state: &AppState) -> Router<AppState> {
    let public = Router::new()
        .route("/passkeys/login/start", post(login::start))
        .route("/passkeys/login/finish", post(login::finish))
        .route("/passkeys/desktop/{flow_id}", get(desktop::page))
        .route(
            "/passkeys/desktop/{flow_id}/options",
            post(desktop::options),
        )
        .route("/passkeys/desktop/{flow_id}/finish", post(desktop::finish))
        .route("/passkeys/desktop/{flow_id}/cancel", post(desktop::cancel));
    // Public for `sign_in`, bearer-bound for `step_up` (C29 steps 2 and 6).
    let desktop_api = Router::new()
        .route("/passkeys/desktop/flows", post(desktop::start))
        .route("/passkeys/desktop/redeem", post(desktop::redeem))
        .layer(from_fn_with_state(
            state.clone(),
            crate::auth::optional_auth,
        ));
    let assets = Router::new()
        .route(
            "/passkeys/assets/ceremony.js",
            get(desktop_page::ceremony_js),
        )
        .route(
            "/passkeys/assets/ceremony.css",
            get(desktop_page::ceremony_css),
        );
    let session = Router::new()
        .route("/passkeys/step-up/start", post(step_up::start))
        .route("/passkeys/step-up/finish", post(step_up::finish))
        .route("/passkeys/register/start", post(register::start))
        .route("/passkeys/register/finish", post(register::finish))
        .route("/passkeys", get(manage::list))
        .route(
            "/passkeys/{id}",
            patch(manage::rename).delete(manage::remove),
        );
    public_limits(public.merge(desktop_api), state)
        .merge(session_limits(session, state))
        .merge(assets)
}
