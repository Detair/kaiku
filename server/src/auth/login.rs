//! Login, logout, refresh, password reset, and OIDC handlers.

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::{Extension, Json};
use axum_extra::extract::CookieJar;
use chrono::Utc;
use fred::prelude::*;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::client_class::ClientClass;
use super::ctx::AuthCtx;
use super::error::{AuthError, AuthResult};
use super::factors::{Factor, FactorRef};
use super::helpers::extract_refresh_token;
use super::login_attempts::types::LoginStep;
use super::mfa_crypto::{decrypt_mfa_secret, encrypt_mfa_secret};
use super::middleware::AuthUser;
use super::oidc::{
    append_collision_suffix, client_challenge_matches, client_return_url,
    generate_username_from_claims, handoff_key, is_valid_client_challenge, new_handoff_code,
    resolve_flow_redirects, FlowRedirectError, OidcFlowPurpose, OidcFlowState, OidcHandoffGrant,
    OidcUserInfo, OIDC_HANDOFF_TTL_SECS,
};
use super::password::verify_password;
use super::session_issue::SessionClient;
use super::types::{
    AuthResponse, LoginRequest, LogoutRequest, OidcAuthorizeQuery, OidcCallbackQuery,
    OidcHandoffExchangeRequest,
};
use super::{cookies, hash_token, queries};
use crate::api::AppState;
use crate::db::{
    self, find_session_by_token_hash, find_user_by_id, find_user_by_username,
    find_user_id_by_identity, get_auth_methods_allowed,
};
use crate::ratelimit::NormalizedIp;

/// F3's neutral answer: same status, body and work for a wrong password,
/// an unknown user and a closed password path.
#[must_use]
pub const fn neutral_answer() -> AuthError {
    AuthError::NeutralSignIn
}

/// Sign in with a password (F3, F4): never a session. A correct password
/// creates a `password` login attempt and answers 202 `LoginStep`
/// (`confirmation_required` with an active key device and `may_approve`,
/// else `totp_required` with an active TOTP); everything else gets the
/// neutral answer after one Argon2 verify.
///
/// POST /auth/login
#[utoipa::path(
    post,
    path = "/auth/login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 202, description = "Confirm on the phone or with TOTP", body = super::login_attempts::types::LoginStep),
        (status = 400, description = "CLIENT_OUTDATED (mfa_code, or a native client without client_challenge)"),
        (status = 401, description = "INVALID_CREDENTIALS (the neutral answer)"),
    ),
    security(()),
)]
#[tracing::instrument(skip(state, jar, headers, body, normalized_ip), fields(username = %body.username))]
pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    normalized_ip: Option<Extension<NormalizedIp>>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(body): Json<LoginRequest>,
) -> AuthResult<(StatusCode, CookieJar, Json<LoginStep>)> {
    // Helper macro to record failed auth (if rate limiter is configured)
    // SECURITY: Fails request with 503 if Valkey is down (fail-closed pattern)
    macro_rules! record_failed_auth {
        () => {
            if let (Some(ref rl), Some(Extension(ref nip))) = (&state.rate_limiter, &normalized_ip)
            {
                if rl.record_failed_auth(&nip.0).await.is_err()
                    && !rl.allow_during_outage(crate::ratelimit::RateLimitCategory::FailedAuth)
                {
                    return Err(AuthError::ServiceUnavailable);
                }
            }
        };
    }

    // v1.0: the code is never sent with the password (C27).
    if body.mfa_code.is_some() {
        return Err(AuthError::ClientOutdated);
    }
    let binding =
        super::login_attempts::handlers::binding(&headers, body.client_challenge.as_deref())?;

    // B.5: the per-account counter (unknown usernames count alike, DC15).
    let counter = crate::ratelimit::account::CounterKey::password(&body.username);
    let limiter = crate::ratelimit::account::AccountLimiter::new(&state.redis);
    match limiter.check(&counter).await {
        Ok(Ok(())) => {}
        Ok(Err(d)) => {
            tracing::warn!(
                retry_after = d.retry_after,
                "password sign-in delayed (B.5)"
            );
            return Err(AuthError::TooManyAttempts {
                retry_after: d.retry_after,
            });
        }
        Err(_) => return Err(AuthError::ServiceUnavailable),
    }

    // Always one Argon2 verify (a dummy hash for unknown users, the system
    // account and passwordless accounts), so the work is the same.
    let user_opt = find_user_by_username(&state.db, &body.username).await?;
    let stored_hash = user_opt
        .as_ref()
        .and_then(|u| u.password_hash.clone())
        .unwrap_or_else(|| super::password::DUMMY_PASSWORD_HASH.clone());
    let password = body.password.clone();
    let password_valid = tokio::task::spawn_blocking(move || {
        verify_password(&password, &stored_hash).unwrap_or(false)
    })
    .await
    .map_err(|e| AuthError::Internal(format!("password check failed: {e}")))?;

    let typed_account = user_opt.as_ref().filter(|u| !u.is_system).map(|u| u.id);
    let user = match user_opt {
        Some(u) if u.password_hash.is_some() && !u.is_system && password_valid => Some(u),
        _ => None,
    };
    let mut conn = state.db.acquire().await?;
    let step = match &user {
        Some(u) => {
            super::login_attempts::core::next_step(
                &mut conn,
                u.id,
                super::login_attempts::core::AttemptKind::Password,
            )
            .await?
        }
        None => None,
    };
    drop(conn);
    if !password_valid {
        account_failure(&state, &counter, typed_account).await?;
    }
    let (Some(user), Some((step, generation))) = (user, step) else {
        record_failed_auth!();
        crate::observability::metrics::record_auth_login_attempt(false);
        return Err(neutral_answer());
    };

    // A banned account cannot start a new session.
    if queries::is_user_globally_banned(&state.db, user.id).await? {
        crate::observability::metrics::record_auth_login_attempt(false);
        return Err(AuthError::AccountBanned);
    }

    let (status, jar, step) = start_attempt(
        &state,
        &headers,
        addr,
        jar,
        user.id,
        super::login_attempts::core::AttemptKind::Password,
        Some(FactorRef {
            factor: Factor::Password,
            generation,
        }),
        binding,
        step,
    )
    .await?;
    if let (Some(ref rl), Some(Extension(ref nip))) = (&state.rate_limiter, &normalized_ip) {
        let _ = rl.clear_failed_auth(&nip.0).await;
    }
    let _ = limiter.record_success(&counter).await;
    tracing::info!(user_id = %user.id, step = step.status, "Password correct; confirmation pending");
    Ok((status, jar, Json(step)))
}

/// B.5: count a wrong password for `counter`; at every 20th a coalesced
/// `guessing` notice, at the 100th the password is `throttled`.
async fn account_failure(
    state: &AppState,
    counter: &crate::ratelimit::account::CounterKey,
    account: Option<Uuid>,
) -> AuthResult<()> {
    use crate::ratelimit::account::{AccountLimiter, NOTICE_EVERY, THROTTLE_AT};
    let n = AccountLimiter::new(&state.redis)
        .record_failure(counter)
        .await
        .map_err(|_| AuthError::ServiceUnavailable)?;
    let Some(user) = account else {
        return Ok(());
    };
    if n % NOTICE_EVERY != 0 && n != THROTTLE_AT {
        return Ok(());
    }
    let ctx = AuthCtx::from(state);
    let mut tx = state.db.begin().await?;
    crate::approvals::transition::lock_accounts(&mut tx, &[user]).await?;
    let mut events = Vec::new();
    if n % NOTICE_EVERY == 0 {
        events.push(
            crate::security_events::record(
                &mut tx,
                crate::security_events::NewSecurityEvent::new(
                    user,
                    crate::security_events::EventType::Guessing,
                    crate::security_events::Actor::System,
                    serde_json::json!({"factor": "password", "count": n}),
                ),
            )
            .await?,
        );
    }
    if n == THROTTLE_AT {
        tracing::warn!(%user, "password throttled after repeated failures (B.5)");
        super::factors::throttle_tx(&mut tx, &ctx, user, Factor::Password).await?;
    }
    tx.commit().await?;
    crate::security_events::spawn_delivery(state, events);
    crate::security_events::notices::spawn_pending_delivery(state);
    Ok(())
}

/// Create the `password`/`oidc` attempt of a correct first factor and its
/// `LoginStep` (202; the attempt cookie for a browser).
#[allow(clippy::too_many_arguments)]
pub(super) async fn start_attempt(
    state: &AppState,
    headers: &HeaderMap,
    addr: SocketAddr,
    jar: CookieJar,
    user_id: Uuid,
    kind: super::login_attempts::core::AttemptKind,
    first_factor: Option<FactorRef>,
    binding: super::login_attempts::core::CollectionBinding,
    step: super::login_attempts::core::NextStep,
) -> AuthResult<(StatusCode, CookieJar, LoginStep)> {
    use super::login_attempts::{core, handlers};
    let class = ClientClass::from_headers(headers);
    let client = SessionClient::from_request(state, headers, addr);
    let ctx = AuthCtx::from(state);
    let mut tx = state.db.begin().await?;
    if let Some(old) = handlers::cookie_secret(&jar, &state.config) {
        core::cancel_by_cookie(&mut tx, &ctx, &old).await?;
    }
    let created = core::create_tx(
        &mut tx,
        &ctx,
        core::NewAttempt {
            kind,
            user_id: Some(user_id),
            first_factor,
            client_class: class,
            client_label: core::client_label(class, client.user_agent.as_deref()),
            client,
            binding,
        },
    )
    .await?;
    tx.commit().await?;
    let jar = match &created.cookie {
        Some(secret) => jar.add(handlers::attempt_cookie(
            &secret.to_b64u(),
            core::ATTEMPT_TTL_SECS,
            &state.config,
        )),
        None => jar,
    };
    // A TOTP-only account sees no QR (nothing could scan it).
    let mut attempt = created.attempt;
    if step == core::NextStep::TotpRequired {
        attempt.qr = None;
    }
    Ok((
        StatusCode::ACCEPTED,
        jar,
        LoginStep {
            status: step.as_str().to_string(),
            attempt,
        },
    ))
}

/// Logout and invalidate session.
///
/// POST /auth/logout
#[utoipa::path(
    post,
    path = "/auth/logout",
    tag = "auth",
    request_body(content = LogoutRequest, description = "Optional; without a refresh token (body or HttpOnly cookie) the bearer token's session is ended"),
    responses(
        (status = 200, description = "Logged out successfully"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, jar, body), fields(user_id = %auth_user.id))]
pub async fn logout(
    State(state): State<AppState>,
    auth_user: AuthUser,
    jar: CookieJar,
    body: Option<Json<LogoutRequest>>,
) -> AuthResult<CookieJar> {
    // The session to end: the access token's own session, plus the one named
    // by a refresh token (JSON body for Tauri/Android, HttpOnly cookie for
    // browsers) when it belongs to the caller. Either alone is enough.
    let mut session_ids: Vec<Uuid> = auth_user.session_id.into_iter().collect();
    let body_token = body.and_then(|b| b.0.refresh_token);
    if let Ok(raw_token) = extract_refresh_token(body_token, &jar, &state.config) {
        let token_hash = hash_token(&raw_token);
        if let Some(session) = find_session_by_token_hash(&state.db, &token_hash).await? {
            if session.user_id == auth_user.id && !session_ids.contains(&session.id) {
                session_ids.push(session.id);
            }
        }
    }
    if session_ids.is_empty() {
        return Err(AuthError::InvalidToken);
    }

    // End the sessions in the revocation order (B.4): `ended_at`, denylist,
    // NOTIFY, commit. Signing out the current session is always allowed.
    let mut tx = state.db.begin().await?;
    super::revocation::end_sessions_tx(
        &mut tx,
        &AuthCtx::from(&state),
        auth_user.id,
        &session_ids,
        super::revocation::EndReason::Logout,
    )
    .await?;
    tx.commit().await?;

    tracing::info!(user_id = %auth_user.id, "User logged out");

    Ok(jar.add(cookies::build_clear_cookie(&state.config)))
}

// ============================================================================
// OIDC
// ============================================================================

/// Get available OIDC providers.
///
/// GET /auth/oidc/providers
#[utoipa::path(
    get,
    path = "/auth/oidc/providers",
    tag = "auth",
    responses(
        (status = 200, description = "List of available OIDC providers"),
    ),
    security(()),
)]
#[tracing::instrument(skip(state))]
pub async fn oidc_providers(State(state): State<AppState>) -> AuthResult<Json<serde_json::Value>> {
    let auth_methods = get_auth_methods_allowed(&state.db).await?;

    if !auth_methods.oidc {
        return Ok(Json(serde_json::json!({ "providers": [] })));
    }

    let oidc_manager = state
        .oidc_manager
        .as_ref()
        .ok_or(AuthError::OidcNotConfigured)?;

    let providers = oidc_manager.list_public().await;

    Ok(Json(serde_json::json!({ "providers": providers })))
}

/// Initiate OIDC authorization.
///
/// GET /auth/oidc/authorize/:provider
#[utoipa::path(
    get,
    path = "/auth/oidc/authorize/{provider}",
    tag = "auth",
    params(
        ("provider" = String, Path, description = "OIDC provider name"),
        ("redirect_uri" = Option<String>, Query, description = "Optional desktop loopback return address (Tauri): `http://{127.0.0.1|localhost}[:port]/callback`, optionally with one per-flow value `?flow=<base64url, 22-128 chars>` that is handed back unchanged; the provider always returns to the server callback"),
        ("client_challenge" = Option<String>, Query, description = "Required with `redirect_uri`, refused without it: S256 challenge (unpadded base64url SHA-256, 43 chars) of the desktop's verifier. The loopback then receives `?code=` (one-time, 60 s), redeemed at `POST /auth/oidc/desktop/exchange`"),
        ("response" = Option<String>, Query, description = "`json` returns `{\"url\": ...}` instead of a 307 (always JSON for a desktop start)"),
        ("X-Kaiku-Client" = Option<String>, Header, description = "Must be `desktop` for a desktop start (`redirect_uri` or `client_challenge` given); otherwise 400 `OIDC_CLIENT_OUTDATED`"),
    ),
    responses(
        (status = 200, description = "Desktop start: `{\"url\": provider authorization URL}`"),
        (status = 307, description = "Browser: redirect to provider authorization URL"),
        (status = 400, description = "Invalid redirect_uri or client_challenge (`VALIDATION_ERROR`), or a desktop start without the desktop header or client_challenge (`OIDC_CLIENT_OUTDATED`)"),
        (status = 403, description = "Auth method disabled"),
    ),
    security(()),
)]
#[tracing::instrument(
    skip(state, headers, query),
    fields(provider = %provider, has_client_header = is_desktop_client(&headers))
)]
pub async fn oidc_authorize(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<OidcAuthorizeQuery>,
) -> Result<Response, AuthError> {
    // v1.0 (C27, C29 step 8): a native start names its app with
    // `X-Kaiku-Client` and binds the later attempt to `client_challenge`;
    // the desktop also names its loopback listener. Browsers cannot send the
    // header cross-site, so a crafted link never starts a native flow.
    let class = ClientClass::from_headers(&headers);
    let native = match class {
        ClientClass::Web => {
            if query.redirect_uri.is_some()
                || query.client_challenge.is_some()
                || query.loopback_port.is_some()
            {
                return Err(AuthError::OidcClientOutdated);
            }
            None
        }
        ClientClass::Desktop => {
            let (Some(port), Some(path), Some(_)) = (
                query.loopback_port,
                query.callback_path.as_deref(),
                query.client_challenge.as_deref(),
            ) else {
                // The 307 `?code=` hand-back to `redirect_uri` is gone.
                return Err(AuthError::OidcClientOutdated);
            };
            if port < 1024
                || path.len() != 43
                || crate::approvals::secrets::b64u_decode(path).is_none()
            {
                return Err(AuthError::Validation(
                    "loopback_port or callback_path".to_string(),
                ));
            }
            Some(super::oidc::NativeLogin::Desktop {
                flow_id: Uuid::new_v4(),
                loopback: super::passkeys::desktop_page::LoopbackTarget {
                    port,
                    callback_path: path.to_string(),
                },
            })
        }
        ClientClass::Android => {
            if query.client_challenge.is_none() {
                return Err(AuthError::OidcClientOutdated);
            }
            Some(super::oidc::NativeLogin::Android)
        }
    };
    let want_json = native.is_some() || query.response.as_deref() == Some("json");
    start_oidc_flow_with(
        &state,
        &provider,
        None,
        query.client_challenge.as_deref(),
        OidcFlowPurpose::Login,
        want_json,
        native,
    )
    .await
}

/// Header (`X-Kaiku-Client: desktop`) the desktop app sends on SSO starts.
pub const DESKTOP_CLIENT_HEADER: &str = "x-kaiku-client";

/// Whether the request carries `X-Kaiku-Client: desktop`.
fn is_desktop_client(headers: &HeaderMap) -> bool {
    headers
        .get(DESKTOP_CLIENT_HEADER)
        .is_some_and(|v| v.as_bytes() == b"desktop")
}

/// Flow-state TTL of a login or link flow.
const OIDC_STATE_TTL_SECS: i64 = 600;

/// Begin an OIDC authorization flow and redirect to the provider.
///
/// Shared by the public login flow, the authenticated identity-link flow and
/// the re-authentication flow; the only difference is what gets persisted in
/// the encrypted Redis flow state (see [`OidcFlowPurpose`]), so the callback
/// can tell them apart.
///
/// When `want_json` is set, the provider authorization URL is returned as a JSON
/// body (`{"url": ...}`) instead of a 307 redirect — for the browser link flow,
/// which fetches this as an authenticated XHR and opens the URL itself.
///
/// `client_challenge` is required for a desktop login (`redirect_uri` set) and
/// refused for every other flow; see [`OidcFlowState::client_challenge`].
pub async fn start_oidc_flow(
    state: &AppState,
    provider: &str,
    redirect_uri: Option<&str>,
    client_challenge: Option<&str>,
    purpose: OidcFlowPurpose,
    want_json: bool,
) -> Result<Response, AuthError> {
    start_oidc_flow_with(
        state,
        provider,
        redirect_uri,
        client_challenge,
        purpose,
        want_json,
        None,
    )
    .await
}

/// [`start_oidc_flow`] with a native sign-in start (C27, C29 step 8).
pub async fn start_oidc_flow_with(
    state: &AppState,
    provider: &str,
    redirect_uri: Option<&str>,
    client_challenge: Option<&str>,
    purpose: OidcFlowPurpose,
    want_json: bool,
    native: Option<super::oidc::NativeLogin>,
) -> Result<Response, AuthError> {
    let auth_methods = get_auth_methods_allowed(&state.db).await?;
    if !auth_methods.oidc {
        return Err(AuthError::AuthMethodDisabled);
    }

    let oidc_manager = state
        .oidc_manager
        .as_ref()
        .ok_or(AuthError::OidcNotConfigured)?;

    // Verify provider exists
    if oidc_manager.get_provider_row(provider).await.is_none() {
        return Err(AuthError::OidcProviderNotFound);
    }

    // Provider always returns to the server's callback; a desktop (Tauri)
    // loopback address is only where the server hands the result back.
    let redirects = resolve_flow_redirects(redirect_uri, state.config.public_url.as_deref())
        .map_err(|e| match e {
            FlowRedirectError::InvalidClientRedirect => {
                tracing::warn!(redirect_uri = ?redirect_uri, "Rejected non-localhost redirect_uri");
                AuthError::Validation(
                    "redirect_uri must be http://localhost or http://127.0.0.1".to_string(),
                )
            }
            FlowRedirectError::MissingPublicUrl => {
                tracing::error!("PUBLIC_URL env var is not set; required for OIDC");
                AuthError::Internal("Server misconfiguration: PUBLIC_URL is not set".to_string())
            }
        })?;

    // Desktop login hands back a one-time code bound to this challenge; an app
    // that sends none would expect tokens in the loopback URL and is refused.
    // The request factor binds its native code like a login (C27).
    let is_desktop_login = matches!(
        purpose,
        OidcFlowPurpose::Login | OidcFlowPurpose::RequestFactor
    ) && (redirects.client_redirect_uri.is_some() || native.is_some());
    let client_challenge = match (is_desktop_login, client_challenge) {
        (true, Some(challenge)) if is_valid_client_challenge(challenge) => {
            Some(challenge.to_string())
        }
        (true, Some(_)) => {
            return Err(AuthError::Validation(
                "client_challenge must be an unpadded base64url SHA-256 digest".to_string(),
            ))
        }
        (true, None) => return Err(AuthError::OidcClientOutdated),
        (false, Some(_)) => {
            return Err(AuthError::Validation(
                "client_challenge is only accepted for desktop sign-in".to_string(),
            ))
        }
        (false, None) => None,
    };

    // A request factor needs a fresh sign-in (`prompt=login`, `max_age=0`).
    let force_login = matches!(purpose, OidcFlowPurpose::RequestFactor);
    let (auth_url, csrf_state, nonce, pkce_verifier) = oidc_manager
        .generate_auth_url_with(provider, &redirects.provider_redirect_uri, force_login)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, provider = %provider, "Failed to generate OIDC auth URL");
            AuthError::Internal(format!("Failed to generate auth URL: {e}"))
        })?;

    // Store OIDC state in Redis with 600s TTL
    let state_hash = hex::encode(Sha256::digest(csrf_state.as_bytes()));
    let redis_key = format!("oidc:state:{state_hash}");
    let flow_state = OidcFlowState {
        slug: provider.to_string(),
        pkce_verifier,
        nonce,
        redirect_uri: redirects.provider_redirect_uri,
        created_at: Utc::now().timestamp(),
        purpose,
        client_redirect_uri: redirects.client_redirect_uri,
        client_challenge,
        native: native.clone(),
    };
    let ttl = OIDC_STATE_TTL_SECS;

    let flow_json =
        serde_json::to_string(&flow_state).map_err(|e| AuthError::Internal(e.to_string()))?;

    // Encrypt the flow state before storing (protects PKCE verifier at rest)
    let enc_key = state
        .config
        .mfa_encryption_key
        .as_ref()
        .ok_or_else(|| AuthError::Internal("MFA encryption not configured".to_string()))?;
    let enc_key_bytes = hex::decode(enc_key)
        .map_err(|_| AuthError::Internal("Invalid MFA encryption key".to_string()))?;
    let encrypted_flow = encrypt_mfa_secret(&flow_json, &enc_key_bytes)
        .map_err(|e| AuthError::Internal(format!("Failed to encrypt OIDC state: {e}")))?;

    state
        .redis
        .set::<(), _, _>(
            &redis_key,
            encrypted_flow.as_str(),
            Some(Expiration::EX(ttl)),
            None,
            false,
        )
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to store OIDC state in Redis");
            AuthError::Internal("Failed to store OIDC state".to_string())
        })?;

    tracing::info!(provider = %provider, purpose = flow_state.purpose.kind(), json = want_json, "Starting OIDC provider flow");
    if let Some(super::oidc::NativeLogin::Desktop { flow_id, .. }) = &native {
        Ok(Json(serde_json::json!({ "url": auth_url, "flow_id": flow_id })).into_response())
    } else if want_json {
        Ok(Json(serde_json::json!({ "url": auth_url })).into_response())
    } else {
        Ok(Redirect::temporary(&auth_url).into_response())
    }
}

/// Handle OIDC callback.
///
/// GET /auth/oidc/callback
#[utoipa::path(
    get,
    path = "/auth/oidc/callback",
    tag = "auth",
    params(
        ("code" = Option<String>, Query, description = "Authorization code from provider (absent on provider error)"),
        ("state" = String, Query, description = "CSRF state parameter"),
        ("error" = Option<String>, Query, description = "Provider error; returned to the sign-in origin as OIDC_PROVIDER_ERROR"),
    ),
    responses(
        (status = 200, description = "Browser popup result page (postMessage to opener), success or generic error code"),
        (status = 307, description = "Redirect to the desktop loopback with the result or a generic error code"),
        (status = 400, description = "Invalid or unknown state"),
    ),
)]
#[tracing::instrument(skip(state, jar, query))]
pub async fn oidc_callback(
    State(state): State<AppState>,
    connect: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    jar: CookieJar,
    axum::extract::Query(query): axum::extract::Query<OidcCallbackQuery>,
) -> Result<Response, AuthError> {
    let addr = connect.map_or_else(|| SocketAddr::from(([0, 0, 0, 0], 0)), |c| c.0 .0);
    let oidc_manager = state
        .oidc_manager
        .as_ref()
        .ok_or(AuthError::OidcNotConfigured)?;

    // Lookup and delete OIDC state from Redis (one-time use)
    let state_hash = hex::encode(Sha256::digest(query.state.as_bytes()));
    let redis_key = format!("oidc:state:{state_hash}");

    // Atomically get and delete the state (one-time use, prevents replay)
    let encrypted_flow: Option<String> = state.redis.getdel(&redis_key).await.map_err(|e| {
        tracing::error!(error = %e, "Failed to read OIDC state from Redis");
        AuthError::Internal("Failed to read OIDC state".to_string())
    })?;

    let encrypted_flow = encrypted_flow.ok_or(AuthError::OidcStateMismatch)?;

    // Decrypt the flow state (PKCE verifier protected at rest)
    let enc_key = state
        .config
        .mfa_encryption_key
        .as_ref()
        .ok_or_else(|| AuthError::Internal("MFA encryption not configured".to_string()))?;
    let enc_key_bytes = hex::decode(enc_key)
        .map_err(|_| AuthError::Internal("Invalid MFA encryption key".to_string()))?;
    let flow_json = decrypt_mfa_secret(&encrypted_flow, &enc_key_bytes).map_err(|e| {
        tracing::error!(error = %e, "Failed to decrypt OIDC state");
        AuthError::OidcStateMismatch
    })?;

    let flow_state: OidcFlowState =
        serde_json::from_str(&flow_json).map_err(|e| AuthError::Internal(e.to_string()))?;

    // From here on the state is valid and consumed: failures go back to the
    // sign-in origin (desktop loopback / browser popup) with a generic code
    // instead of an HTTP error the origin never sees.
    let code = match (query.code.as_deref(), query.error.as_deref()) {
        (Some(code), None) => code,
        (_, provider_error) => {
            let provider_error: Option<String> =
                provider_error.map(|e| e.chars().take(64).collect());
            tracing::info!(provider = %flow_state.slug, provider_error = ?provider_error, "OIDC provider returned no authorization code");
            return Ok(flow_failure_response(&flow_state, OIDC_PROVIDER_ERROR));
        }
    };

    if matches!(flow_state.purpose, OidcFlowPurpose::RequestFactor) {
        return Ok(request_factor_callback(&state, &flow_state, code).await);
    }

    // Exchange code for tokens (also verifies ID token nonce for OIDC providers)
    let access_token = match oidc_manager
        .exchange_code(
            &flow_state.slug,
            code,
            &flow_state.pkce_verifier,
            &flow_state.redirect_uri,
            &flow_state.nonce,
        )
        .await
    {
        Ok((access_token, _id_token)) => access_token,
        Err(e) => {
            tracing::warn!(error = %e, provider = %flow_state.slug, "OIDC code exchange failed");
            return Ok(flow_failure_response(&flow_state, OIDC_EXCHANGE_FAILED));
        }
    };

    // Extract user info
    let user_info = match oidc_manager
        .extract_user_info(&flow_state.slug, &access_token)
        .await
    {
        Ok(user_info) => user_info,
        Err(e) => {
            tracing::warn!(error = %e, provider = %flow_state.slug, "Failed to extract OIDC user info");
            return Ok(flow_failure_response(&flow_state, OIDC_EXCHANGE_FAILED));
        }
    };

    // Composite external_id: "{provider_slug}:{subject}". Retained as the
    // user's primary-identity marker; the authoritative lookup is the
    // user_identities table keyed by (provider_slug, subject).
    //
    // Accounts can now attach additional identities (see handle_identity_link),
    // so users.external_id only ever holds the *first* identity — it is no
    // longer the identity-set uniqueness guarantee (user_identities is), and
    // it carries no UNIQUE constraint (demoted in TD-31).
    let external_id = format!("{}:{}", flow_state.slug, user_info.subject);

    // Link flow: attach this identity to the already-authenticated user instead
    // of logging in or creating an account.
    if let OidcFlowPurpose::Link {
        user_id: link_user_id,
        session_id: link_session_id,
    } = flow_state.purpose
    {
        return handle_identity_link(
            &state,
            &flow_state,
            &user_info,
            link_user_id,
            link_session_id,
        )
        .await;
    }

    // User resolution: resolve the external identity to an account.
    let existing_user =
        match find_user_id_by_identity(&state.db, &flow_state.slug, &user_info.subject).await? {
            Some(user_id) => find_user_by_id(&state.db, user_id).await?,
            None => None,
        };

    let user = if let Some(existing) = existing_user {
        // Existing user — login
        existing
    } else {
        // New user — check registration policy (fail-closed: deny if DB unreachable)
        let reg_policy_value = db::get_config_value(&state.db, "registration_policy")
            .await
            .map_err(|e| {
                tracing::error!(
                    error = %e,
                    provider = %flow_state.slug,
                    "Failed to read registration_policy config - denying OIDC registration (fail-closed)"
                );
                AuthError::Database(e)
            })?;
        let reg_policy = reg_policy_value.as_str().ok_or_else(|| {
            tracing::error!(
                actual_value = ?reg_policy_value,
                provider = %flow_state.slug,
                "registration_policy config value is not a string"
            );
            AuthError::Internal("Server configuration error".to_string())
        })?;
        if reg_policy != "open" {
            // Both "closed" and "invite_only" reject OIDC registration
            // (no mechanism to carry invite tokens through OIDC flow)
            return Err(AuthError::RegistrationDisabled);
        }

        // Generate username from claims
        let base_username = generate_username_from_claims(&user_info);

        let display_name = user_info
            .name
            .clone()
            .unwrap_or_else(|| base_username.clone());

        // Use a transaction for atomic first-user detection + user creation.
        // Retry on username collision (UNIQUE constraint violation).
        let mut username = base_username;
        let mut new_user = None;
        for attempt in 0..5u8 {
            if attempt > 0 {
                username = append_collision_suffix(&username);
            }

            let mut tx = state.db.begin().await.map_err(|e| {
                tracing::error!(error = %e, "Failed to start OIDC registration transaction");
                AuthError::Database(e)
            })?;

            // Lock setup_complete to serialize first-user detection (same pattern as local
            // register)
            queries::lock_setup_complete(&mut tx).await.map_err(|e| {
                tracing::error!(error = ?e, "Failed to lock setup_complete during OIDC registration");
                e
            })?;

            let user_count = queries::count_users(&mut tx).await?;
            let is_first_user = user_count == 0;

            match queries::insert_oidc_user(
                &mut tx,
                &username,
                &display_name,
                user_info.email.as_deref(),
                &external_id,
                user_info.avatar_url.as_deref(),
            )
            .await?
            {
                Some(user) => {
                    // Record the external identity (authoritative login key) in
                    // the same transaction as the user row.
                    match db::insert_user_identity(
                        &mut *tx,
                        user.id,
                        &flow_state.slug,
                        &user_info.subject,
                        user_info.email.as_deref(),
                    )
                    .await
                    {
                        Ok(_) => {}
                        // Lost a concurrent first-login race: another request
                        // registered this identity between our pre-loop lookup
                        // and here. Roll back and fall through to the post-loop
                        // re-resolve, which logs that account in.
                        Err(sqlx::Error::Database(ref db_err)) if db_err.is_unique_violation() => {
                            drop(tx);
                            break;
                        }
                        Err(e) => {
                            tracing::error!(error = %e, user_id = %user.id, provider = %flow_state.slug, "Failed to record OIDC identity");
                            return Err(AuthError::Database(e));
                        }
                    }

                    // Grant admin to first user
                    if is_first_user {
                        queries::grant_first_user_admin(&mut tx, user.id)
                            .await
                            .map_err(|e| {
                                tracing::error!(error = ?e, user_id = %user.id, "Failed to grant admin to first OIDC user");
                                e
                            })?;
                        tracing::info!(user_id = %user.id, "First user registered via OIDC and granted system admin");
                    }

                    tx.commit().await.map_err(AuthError::Database)?;

                    tracing::info!(
                        user_id = %user.id,
                        username = %user.username,
                        provider = %flow_state.slug,
                        "New user registered via OIDC"
                    );
                    new_user = Some(user);
                    break;
                }
                None => {
                    // Username collision — tx dropped (implicit rollback), retry with suffix
                    tracing::debug!(username = %username, "Username collision during OIDC registration, retrying");
                }
            }
        }

        match new_user {
            Some(user) => user,
            None => {
                // Either the username-retry loop exhausted, or we lost a
                // concurrent first-login race for this same identity (the
                // insert_user_identity unique violation broke out of the loop
                // above). Re-resolve the identity; if it now exists, log that
                // account in instead of failing.
                if let Some(user_id) =
                    find_user_id_by_identity(&state.db, &flow_state.slug, &user_info.subject)
                        .await?
                {
                    find_user_by_id(&state.db, user_id).await?.ok_or_else(|| {
                        AuthError::Internal("Identity resolved to a missing user".to_string())
                    })?
                } else {
                    tracing::error!(external_id = %external_id, "Failed to create OIDC user after 5 collision retries");
                    return Err(AuthError::Internal(
                        "Username generation failed after retries".to_string(),
                    ));
                }
            }
        }
    };

    // A banned account cannot start a new session.
    if queries::is_user_globally_banned(&state.db, user.id).await? {
        return Err(AuthError::AccountBanned);
    }

    // Best-effort: record that this identity was just used to log in.
    if let Err(e) =
        db::touch_user_identity_last_used(&state.db, &flow_state.slug, &user_info.subject).await
    {
        tracing::warn!(error = %e, provider = %flow_state.slug, "Failed to update identity last_used_at");
    }

    // v1.0 (F3 OIDC variant): no session; the account's second step.
    let mut conn = state.db.acquire().await?;
    let step = super::login_attempts::core::next_step(
        &mut conn,
        user.id,
        super::login_attempts::core::AttemptKind::Oidc,
    )
    .await?;
    drop(conn);

    match flow_state.native.clone() {
        Some(native) => {
            let Some(step_kind) = step else {
                return Ok(native_failure(&flow_state, &native, "INVALID_CREDENTIALS"));
            };
            let _ = step_kind;
            Ok(native_hand_back(&state, &flow_state, &native, user.id).await)
        }
        None if client_return_url(&flow_state).is_some() => {
            // A pre-v1.0 desktop state (`redirect_uri`): the app must update.
            Ok(flow_failure_response(&flow_state, OIDC_CLIENT_OUTDATED))
        }
        None => {
            let Some((step, _)) = step else {
                return Ok(flow_failure_response(&flow_state, "INVALID_CREDENTIALS"));
            };
            // Browser: an `oidc` attempt bound to the attempt cookie.
            let (_, jar, step) = start_attempt(
                &state,
                &headers,
                addr,
                jar,
                user.id,
                super::login_attempts::core::AttemptKind::Oidc,
                None,
                super::login_attempts::core::CollectionBinding::Cookie,
                step,
            )
            .await?;
            tracing::info!(user_id = %user.id, provider = %flow_state.slug, "OIDC first factor; confirmation pending");
            let payload = serde_json::json!({
                "type": "oidc-callback",
                "status": step.status,
                "attempt": step.attempt,
            });
            Ok((jar, axum::response::Html(popup_result_page(&payload, true))).into_response())
        }
    }
}

/// A native OIDC sign-in failed after the provider: the desktop hand-off
/// page carries `error`, Android's callback `error=`.
fn native_failure(
    flow_state: &OidcFlowState,
    native: &super::oidc::NativeLogin,
    code: &str,
) -> Response {
    match native {
        super::oidc::NativeLogin::Desktop { flow_id, loopback } => {
            super::passkeys::desktop_page::handoff_page(
                loopback,
                *flow_id,
                super::passkeys::desktop_page::HandoffField::Error(code),
            )
        }
        super::oidc::NativeLogin::Android => {
            let _ = flow_state;
            android_callback("error", code)
        }
    }
}

/// `kaiku://auth/callback?<key>=<value>` for the Android Custom Tab
/// (no-store, no referrer).
fn android_callback(key: &str, value: &str) -> Response {
    let mut url = openidconnect::url::Url::parse("kaiku://auth/callback").expect("static URL");
    url.query_pairs_mut().append_pair(key, value);
    loopback_redirect(&url)
}

/// Store the one-time hand-off grant (60 s) and deliver only its code: the
/// desktop through the hand-off page's form POST (C29 step 8), Android
/// through `kaiku://auth/callback?code=…`.
async fn native_hand_back(
    state: &AppState,
    flow_state: &OidcFlowState,
    native: &super::oidc::NativeLogin,
    user_id: Uuid,
) -> Response {
    let Some(client_challenge) = flow_state.client_challenge.clone() else {
        return native_failure(flow_state, native, OIDC_CLIENT_OUTDATED);
    };
    let code = new_handoff_code();
    let grant = OidcHandoffGrant {
        user_id,
        slug: flow_state.slug.clone(),
        client_challenge,
        created_at: Utc::now().timestamp(),
    };
    let stored = match serde_json::to_string(&grant) {
        Ok(json) => state
            .redis
            .set::<(), _, _>(
                handoff_key(&code),
                json.as_str(),
                Some(Expiration::EX(OIDC_HANDOFF_TTL_SECS)),
                None,
                false,
            )
            .await
            .map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    };
    if let Err(e) = stored {
        tracing::error!(error = %e, "Failed to store OIDC hand-off code");
        return native_failure(flow_state, native, "SERVICE_UNAVAILABLE");
    }
    tracing::info!(user_id = %user_id, provider = %flow_state.slug, "OIDC native sign-in handed off");
    match native {
        super::oidc::NativeLogin::Desktop { flow_id, loopback } => {
            super::passkeys::desktop_page::handoff_page(
                loopback,
                *flow_id,
                super::passkeys::desktop_page::HandoffField::Code(&code),
            )
        }
        super::oidc::NativeLogin::Android => android_callback("code", &code),
    }
}

/// The request-factor callback (C27): the verified ID token's `auth_time`
/// must be at most 300 s old and its identity linked; the one-time
/// `oidc_code` (60 s) comes back by `postMessage` `oidc-request-callback`,
/// the desktop hand-off page or the Android hand-back. A stale or unlinked
/// sign-in still gets a code (no oracle), which redeems to nothing.
async fn request_factor_callback(
    state: &AppState,
    flow_state: &OidcFlowState,
    code: &str,
) -> Response {
    use super::account_requests::recovery::{request_code_key, RequestOidcGrant, OIDC_FRESH_SECS};
    let Some(manager) = state.oidc_manager.as_ref() else {
        return flow_failure_response(flow_state, OIDC_EXCHANGE_FAILED);
    };
    let claims = match manager
        .exchange_code_verified(
            &flow_state.slug,
            code,
            &flow_state.pkce_verifier,
            &flow_state.redirect_uri,
            &flow_state.nonce,
        )
        .await
    {
        Ok((_, Some((_, claims)))) => claims,
        Ok((_, None)) => {
            tracing::info!(provider = %flow_state.slug, "request factor needs an ID token");
            return flow_failure_response(flow_state, OIDC_EXCHANGE_FAILED);
        }
        Err(e) => {
            tracing::warn!(error = %e, provider = %flow_state.slug, "OIDC code exchange failed");
            return flow_failure_response(flow_state, OIDC_EXCHANGE_FAILED);
        }
    };
    let now = Utc::now();
    let fresh = claims.auth_time.is_some_and(|t| {
        (now - t).num_seconds() <= OIDC_FRESH_SECS && (t - now).num_seconds() <= 60
    });
    let user = match find_user_id_by_identity(&state.db, &flow_state.slug, &claims.subject).await {
        Ok(u) => u,
        Err(_) => return flow_failure_response(flow_state, "SERVICE_UNAVAILABLE"),
    };
    if !fresh {
        tracing::info!(provider = %flow_state.slug, "request factor: sign-in not fresh");
    }
    let grant = RequestOidcGrant {
        user_id: if fresh { user } else { None },
        slug: flow_state.slug.clone(),
        auth_time: claims.auth_time,
        client_challenge: flow_state.client_challenge.clone(),
    };
    let oidc_code = new_handoff_code();
    let stored = match serde_json::to_string(&grant) {
        Ok(json) => state
            .redis
            .set::<(), _, _>(
                request_code_key(&oidc_code),
                json.as_str(),
                Some(Expiration::EX(OIDC_HANDOFF_TTL_SECS)),
                None,
                false,
            )
            .await
            .map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    };
    if let Err(e) = stored {
        tracing::error!(error = %e, "Failed to store the request OIDC code");
        return flow_failure_response(flow_state, "SERVICE_UNAVAILABLE");
    }
    match &flow_state.native {
        Some(super::oidc::NativeLogin::Desktop { flow_id, loopback }) => {
            super::passkeys::desktop_page::handoff_page(
                loopback,
                *flow_id,
                super::passkeys::desktop_page::HandoffField::Code(&oidc_code),
            )
        }
        Some(super::oidc::NativeLogin::Android) => android_callback("code", &oidc_code),
        None => axum::response::Html(popup_result_page(
            &serde_json::json!({"type": "oidc-request-callback", "oidc_code": oidc_code}),
            true,
        ))
        .into_response(),
    }
}

/// Generic code: a desktop login state without a client challenge (started by
/// an older app or before the upgrade) cannot be handed back.
const OIDC_CLIENT_OUTDATED: &str = "OIDC_CLIENT_OUTDATED";

/// 307 to the desktop loopback: never cached, never sent on as a referrer.
fn loopback_redirect(url: &openidconnect::url::Url) -> Response {
    let mut response = Redirect::temporary(url.as_str()).into_response();
    let headers = response.headers_mut();
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    headers.insert(
        axum::http::header::REFERRER_POLICY,
        axum::http::HeaderValue::from_static("no-referrer"),
    );
    response
}

/// Redeem a desktop hand-off code.
///
/// POST /auth/oidc/desktop/exchange
///
/// The code is single use (consumed by the first attempt, also a failed one)
/// and valid for 60 s; `verifier` must hash (S256) to the `client_challenge`
/// of the authorize request. Returns the login token response with the
/// refresh token in the body (no cookie).
#[utoipa::path(
    post,
    path = "/auth/oidc/desktop/exchange",
    tag = "auth",
    request_body = OidcHandoffExchangeRequest,
    responses(
        (status = 200, description = "Signed in", body = AuthResponse),
        (status = 400, description = "Unknown, expired, used or wrongly verified code (`OIDC_HANDOFF_INVALID`)"),
        (status = 403, description = "Account banned (`ACCOUNT_BANNED`)"),
        (status = 429, description = "Rate limited"),
        (status = 503, description = "Valkey unavailable (`SERVICE_UNAVAILABLE`)"),
    ),
    security(()),
)]
#[tracing::instrument(skip(state, headers, body))]
pub async fn oidc_desktop_exchange(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<OidcHandoffExchangeRequest>,
) -> AuthResult<(StatusCode, Json<LoginStep>)> {
    if body.code.is_empty() || body.code.len() > 128 {
        return Err(AuthError::OidcHandoffInvalid);
    }
    let raw: Option<String> = state
        .redis
        .getdel(handoff_key(&body.code))
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to read OIDC hand-off code");
            AuthError::ServiceUnavailable
        })?;
    let grant: OidcHandoffGrant = raw
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .ok_or(AuthError::OidcHandoffInvalid)?;
    let age = Utc::now().timestamp() - grant.created_at;
    if !(0..=OIDC_HANDOFF_TTL_SECS).contains(&age)
        || !client_challenge_matches(&body.verifier, &grant.client_challenge)
    {
        tracing::info!(provider = %grant.slug, "OIDC hand-off code refused (expired or verifier mismatch)");
        return Err(AuthError::OidcHandoffInvalid);
    }

    let user = find_user_by_id(&state.db, grant.user_id)
        .await?
        .ok_or(AuthError::OidcHandoffInvalid)?;
    if queries::is_user_globally_banned(&state.db, user.id).await? {
        return Err(AuthError::AccountBanned);
    }
    let mut conn = state.db.acquire().await?;
    let step = super::login_attempts::core::next_step(
        &mut conn,
        user.id,
        super::login_attempts::core::AttemptKind::Oidc,
    )
    .await?;
    drop(conn);
    let Some((step, _)) = step else {
        return Err(neutral_answer());
    };
    // The exchange is the native app's own call: its attempt is bound to
    // the start's `client_challenge` (the verifier just proved it).
    let mut headers = headers;
    if ClientClass::from_headers(&headers) == ClientClass::Web {
        headers.insert(
            DESKTOP_CLIENT_HEADER,
            axum::http::HeaderValue::from_static("desktop"),
        );
    }
    let (status, _, step) = start_attempt(
        &state,
        &headers,
        addr,
        CookieJar::new(),
        user.id,
        super::login_attempts::core::AttemptKind::Oidc,
        None,
        super::login_attempts::core::CollectionBinding::S256(grant.client_challenge.clone()),
        step,
    )
    .await?;
    tracing::info!(user_id = %user.id, provider = %grant.slug, "OIDC first factor (native); confirmation pending");
    Ok((status, Json(step)))
}

/// Attach a freshly-authenticated external identity to an existing account
/// (the OIDC *link* flow). Unlike login, this issues no tokens.
///
/// Refuses to steal an identity already bound to a different account. A conflict
/// (already bound elsewhere, or the provider already linked to this account) is
/// reported back to the opener as a generic `IDENTITY_ALREADY_LINKED` code via
/// the same redirect/postMessage channel — not as an HTTP error.
async fn handle_identity_link(
    state: &AppState,
    flow_state: &OidcFlowState,
    user_info: &OidcUserInfo,
    link_user_id: Uuid,
    link_session_id: Option<Uuid>,
) -> Result<Response, AuthError> {
    // The session that started the flow must still be live, not revoked, of
    // an unbanned account and (for a system admin) elevated.
    if let Some(code) =
        super::identities::link_refusal(state, link_user_id, link_session_id).await?
    {
        tracing::info!(user_id = %link_user_id, provider = %flow_state.slug, error_code = code, "OIDC identity link refused: starting session no longer valid");
        return Ok(flow_failure_response(flow_state, code));
    }

    // Determine the link outcome. Genuine server faults (DB errors) propagate as
    // AuthError; a *conflict* instead becomes a generic error code carried back to
    // the opener via the same redirect/postMessage channel, so the popup always
    // closes rather than hanging on an HTTP error page. The code is intentionally
    // ownerless — it never reveals which account holds the identity.
    let error_code: Option<&'static str> = match find_user_id_by_identity(
        &state.db,
        &flow_state.slug,
        &user_info.subject,
    )
    .await?
    {
        // Already linked to this account — idempotent success.
        Some(existing) if existing == link_user_id => {
            tracing::info!(user_id = %link_user_id, provider = %flow_state.slug, "OIDC identity already linked to this account");
            None
        }
        // Bound to someone else — refuse.
        Some(_) => Some("IDENTITY_ALREADY_LINKED"),
        None => match db::insert_user_identity(
            &state.db,
            link_user_id,
            &flow_state.slug,
            &user_info.subject,
            user_info.email.as_deref(),
        )
        .await
        {
            Ok(identity) => {
                // Linked from an elevated session: may confirm re-authentication
                // right away (C24); otherwise only sessions started later can.
                if let Some(session_id) = link_session_id {
                    if crate::admin::queries::has_active_elevated_session(
                        &state.db,
                        link_user_id,
                        session_id,
                    )
                    .await
                    .map_err(|e| AuthError::Internal(format!("Elevation lookup failed: {e}")))?
                    {
                        sqlx::query(
                            "UPDATE user_identities SET linked_while_elevated = TRUE WHERE id = $1",
                        )
                        .bind(identity.id)
                        .execute(&state.db)
                        .await?;
                    }
                }
                tracing::info!(user_id = %link_user_id, provider = %flow_state.slug, "Linked OIDC identity to account");
                None
            }
            // Lost a race, or the account already has an identity for this
            // provider (the (user_id, provider_slug) UNIQUE constraint).
            Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                Some("IDENTITY_ALREADY_LINKED")
            }
            Err(e) => return Err(AuthError::Database(e)),
        },
    };

    // Token-less result response (mirrors the login callback's Tauri-vs-browser
    // split, minus the session/cookie since the caller is already authenticated).
    // Always completes the handshake — success or a generic error code.
    if let Some(code) = error_code {
        tracing::info!(user_id = %link_user_id, provider = %flow_state.slug, error_code = code, "OIDC identity link rejected");
        return Ok(flow_failure_response(flow_state, code));
    }

    if let Some(mut redirect_url) = client_return_url(flow_state) {
        redirect_url
            .query_pairs_mut()
            .append_pair("linked", &flow_state.slug);
        Ok(loopback_redirect(&redirect_url))
    } else {
        let payload = serde_json::json!({
            "type": "oidc-link-callback",
            "success": true,
            "provider_slug": flow_state.slug,
            "error_code": null,
        });
        Ok(axum::response::Html(popup_result_page(&payload, true)).into_response())
    }
}

/// Generic code: the provider returned an error instead of an authorization code.
const OIDC_PROVIDER_ERROR: &str = "OIDC_PROVIDER_ERROR";
/// Generic code: code exchange or user-info lookup with the provider failed.
const OIDC_EXCHANGE_FAILED: &str = "OIDC_EXCHANGE_FAILED";

/// Hand a failed flow back to its origin with a generic error code (never
/// provider text). Only used once the flow state has been validated, so the
/// return target is the stored, re-validated loopback — never callback input.
///
/// | flow  | desktop loopback       | browser popup (`postMessage`)                        |
/// |-------|------------------------|------------------------------------------------------|
/// | login | `?error=CODE`          | `{type: "oidc-callback", error}`                     |
/// | link  | `?link_error=CODE`     | `{type: "oidc-link-callback", success: false, error_code}` |
fn flow_failure_response(flow_state: &OidcFlowState, code: &'static str) -> Response {
    if let Some(native) = &flow_state.native {
        return native_failure(flow_state, native, code);
    }
    let is_link = match flow_state.purpose {
        OidcFlowPurpose::Link { .. } => true,
        OidcFlowPurpose::Login => false,
        OidcFlowPurpose::RequestFactor => {
            let payload = serde_json::json!({
                "type": "oidc-request-callback", "status": "error", "error": code,
            });
            return axum::response::Html(popup_result_page(&payload, false)).into_response();
        }
    };
    if let Some(mut redirect_url) = client_return_url(flow_state) {
        let key = if is_link { "link_error" } else { "error" };
        redirect_url.query_pairs_mut().append_pair(key, code);
        return loopback_redirect(&redirect_url);
    }
    let payload = if is_link {
        serde_json::json!({
            "type": "oidc-link-callback",
            "success": false,
            "provider_slug": flow_state.slug,
            "error_code": code,
        })
    } else {
        serde_json::json!({ "type": "oidc-callback", "status": "error", "error": code })
    };
    // The link UI handles failures itself (closes the popup); the login page
    // only reacts to success, so a failed login keeps the popup open with a hint.
    axum::response::Html(popup_result_page(&payload, is_link)).into_response()
}

/// Popup page that posts `payload` to the opener (same origin only). With
/// `close` it closes itself afterwards; otherwise it stays open showing that
/// sign-in failed.
fn popup_result_page(payload: &serde_json::Value, close: bool) -> String {
    let (close_js, fallback) = if close {
        ("window.close();", "You can close this window.")
    } else {
        (
            "",
            "Sign-in did not complete. Close this window and try again.",
        )
    };
    format!(
        r#"<!DOCTYPE html>
<html><body><script>
if (window.opener) {{
    window.opener.postMessage({payload}, window.location.origin);
    {close_js}
}}
document.body.innerText = "{fallback}";
</script></body></html>"#,
    )
}
