//! `POST /auth/login-attempts[/{id}/{poll,collect,cancel}]` (C27).

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use axum_extra::extract::cookie::{Cookie, SameSite};
use axum_extra::extract::CookieJar;
use uuid::Uuid;

use super::core::{self, CollectionBinding, CollectionSecret, Locked, NewAttempt};
use super::types::{AttemptRequest, CreateAttemptRequest, LoginAttempt};
use crate::api::AppState;
use crate::approvals::secrets::Secret256;
use crate::auth::client_class::ClientClass;
use crate::auth::ctx::AuthCtx;
use crate::auth::error::{AuthError, AuthResult};
use crate::auth::session_issue::SessionClient;
use crate::auth::types::AuthResponse;
use crate::config::Config;

/// The browser's attempt cookie (C27 common rules).
pub const ATTEMPT_COOKIE: &str = "__Secure-kaiku_attempt";
/// Its path: only the attempt routes receive it.
pub const ATTEMPT_COOKIE_PATH: &str = "/auth/login-attempts";

/// The attempt cookie with `value` and `max_age` seconds (0 clears it).
#[must_use]
pub fn attempt_cookie(value: &str, max_age: i64, config: &Config) -> Cookie<'static> {
    let mut c = Cookie::build((
        crate::auth::cookies::cookie_name(ATTEMPT_COOKIE, config),
        value.to_owned(),
    ))
    .http_only(true)
    .same_site(SameSite::Strict)
    .path(ATTEMPT_COOKIE_PATH)
    .secure(config.cookie_secure)
    .max_age(time::Duration::seconds(max_age))
    .build();
    if let Some(domain) = &config.cookie_domain {
        c.set_domain(domain.clone());
    }
    c
}

/// The browser's attempt cookie secret, if one is sent.
#[must_use]
pub fn cookie_secret(jar: &CookieJar, config: &Config) -> Option<Secret256> {
    jar.get(crate::auth::cookies::cookie_name(ATTEMPT_COOKIE, config))
        .and_then(|c| Secret256::from_b64u(c.value()))
}

/// The secret the request presents: native clients send the verifier in
/// the body, browsers the cookie.
pub fn presented(
    headers: &HeaderMap,
    jar: &CookieJar,
    config: &Config,
    body: &AttemptRequest,
) -> AuthResult<CollectionSecret> {
    if ClientClass::from_headers(headers) == ClientClass::Web {
        cookie_secret(jar, config)
            .map(CollectionSecret::Cookie)
            .ok_or(AuthError::LoginAttemptUnknown)
    } else {
        body.verifier
            .clone()
            .map(CollectionSecret::Verifier)
            .ok_or(AuthError::LoginAttemptUnknown)
    }
}

/// How a new attempt binds to its PC (C27): browsers must not send a
/// `client_challenge` (400 `VALIDATION_ERROR`); native clients must (400
/// `CLIENT_OUTDATED`).
pub fn binding(
    headers: &HeaderMap,
    client_challenge: Option<&str>,
) -> AuthResult<CollectionBinding> {
    match (ClientClass::from_headers(headers), client_challenge) {
        (ClientClass::Web, None) => Ok(CollectionBinding::Cookie),
        (ClientClass::Web, Some(_)) => Err(AuthError::Validation(
            "client_challenge: browsers get a cookie".into(),
        )),
        (_, Some(c)) => Ok(CollectionBinding::S256(c.to_string())),
        (_, None) => Err(AuthError::ClientOutdated),
    }
}

/// "Sign in with phone" (F2): a QR attempt.
#[utoipa::path(
    post,
    path = "/auth/login-attempts",
    tag = "auth",
    request_body = CreateAttemptRequest,
    responses((status = 201, body = LoginAttempt), (status = 400, description = "CLIENT_OUTDATED or VALIDATION_ERROR")),
)]
#[tracing::instrument(skip_all)]
pub async fn create(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    body: Option<Json<CreateAttemptRequest>>,
) -> AuthResult<(StatusCode, CookieJar, Json<LoginAttempt>)> {
    let body = body.map(|b| b.0).unwrap_or_default();
    let binding = binding(&headers, body.client_challenge.as_deref())?;
    let class = ClientClass::from_headers(&headers);
    let client = SessionClient::from_request(&state, &headers, addr);
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    // One live attempt per screen.
    if let Some(old) = cookie_secret(&jar, &state.config) {
        core::cancel_by_cookie(&mut tx, &ctx, &old).await?;
    }
    let created = core::create_tx(
        &mut tx,
        &ctx,
        NewAttempt {
            kind: core::AttemptKind::Qr,
            user_id: None,
            first_factor: None,
            client_class: class,
            client_label: core::client_label(class, client.user_agent.as_deref()),
            client,
            binding,
        },
    )
    .await?;
    tx.commit().await?;
    let jar = match &created.cookie {
        Some(secret) => jar.add(attempt_cookie(
            &secret.to_b64u(),
            core::ATTEMPT_TTL_SECS,
            &state.config,
        )),
        None => jar,
    };
    Ok((StatusCode::CREATED, jar, Json(created.attempt)))
}

/// The attempt's state; a new QR token when due; long-poll with `wait`.
#[utoipa::path(
    post,
    path = "/auth/login-attempts/{id}/poll",
    tag = "auth",
    params(("id" = Uuid, Path, description = "Attempt id")),
    request_body = AttemptRequest,
    responses((status = 200, body = LoginAttempt), (status = 404, description = "LOGIN_ATTEMPT_UNKNOWN")),
)]
#[tracing::instrument(skip_all, fields(attempt_id = %id))]
pub async fn poll(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
    body: Option<Json<AttemptRequest>>,
) -> AuthResult<Json<LoginAttempt>> {
    let body = body.map(|b| b.0).unwrap_or_default();
    let secret = presented(&headers, &jar, &state.config, &body)?;
    let ctx = AuthCtx::from(&state);
    Ok(Json(
        core::poll(&ctx, id, &secret, body.wait.unwrap_or(0)).await?,
    ))
}

/// Collect the session of an approved attempt (probation).
#[utoipa::path(
    post,
    path = "/auth/login-attempts/{id}/collect",
    tag = "auth",
    params(("id" = Uuid, Path, description = "Attempt id")),
    request_body = AttemptRequest,
    responses(
        (status = 200, body = AuthResponse),
        (status = 404, description = "LOGIN_ATTEMPT_UNKNOWN"),
        (status = 409, description = "LOGIN_ATTEMPT_NOT_READY"),
        (status = 410, description = "LOGIN_ATTEMPT_ENDED {state}"),
    ),
)]
#[tracing::instrument(skip_all, fields(attempt_id = %id))]
pub async fn collect(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
    body: Option<Json<AttemptRequest>>,
) -> AuthResult<(CookieJar, Json<AuthResponse>)> {
    let body = body.map(|b| b.0).unwrap_or_default();
    let secret = presented(&headers, &jar, &state.config, &body)?;
    let client = SessionClient::from_request(&state, &headers, addr);
    let ctx = AuthCtx::from(&state);
    let mut tries = 0;
    let collected = loop {
        let mut tx = state.db.begin().await?;
        let r = core::collect_tx(&mut tx, &ctx, id, &secret, &client).await;
        if let Some(v) = settle(tx, r, &mut tries).await? {
            break v;
        }
    };
    Ok(session_response(&state, &headers, jar, collected))
}

/// The TOTP step of a `password`/`oidc` attempt (F4, "Phone not at hand?").
#[utoipa::path(
    post,
    path = "/auth/login-attempts/{id}/totp",
    tag = "auth",
    params(("id" = Uuid, Path, description = "Attempt id")),
    request_body = AttemptRequest,
    responses(
        (status = 200, body = AuthResponse),
        (status = 401, description = "INVALID_MFA"),
        (status = 403, description = "TOTP_UNAVAILABLE"),
        (status = 404, description = "LOGIN_ATTEMPT_UNKNOWN"),
        (status = 410, description = "LOGIN_ATTEMPT_ENDED {state}"),
    ),
)]
#[tracing::instrument(skip_all, fields(attempt_id = %id))]
pub async fn totp(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
    body: Option<Json<AttemptRequest>>,
) -> AuthResult<(CookieJar, Json<AuthResponse>)> {
    let body = body.map(|b| b.0).unwrap_or_default();
    let secret = presented(&headers, &jar, &state.config, &body)?;
    let code = body
        .code
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty() && c.len() <= 16)
        .ok_or_else(|| AuthError::Validation("code: is required".into()))?
        .to_string();
    let client = SessionClient::from_request(&state, &headers, addr);
    let ctx = AuthCtx::from(&state);
    // B.5: the account's TOTP counter (every attempt kind names its account).
    let account: Option<(Option<Uuid>, Option<String>, Option<i64>)> = sqlx::query_as(
        "SELECT user_id, first_factor, first_factor_generation FROM login_attempts WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?;
    let (account, password_generation) = match account {
        Some((user, first, generation)) => (
            user,
            (first.as_deref() == Some("password"))
                .then_some(generation)
                .flatten()
                .map(|generation| crate::auth::factors::FactorRef {
                    factor: crate::auth::factors::Factor::Password,
                    generation,
                }),
        ),
        None => (None, None),
    };
    let totp_generation: Option<i64> = if let Some(user) = account {
        crate::auth::totp_guard::pre_check(&ctx, user).await?;
        sqlx::query_scalar("SELECT totp_generation FROM users WHERE id = $1")
            .bind(user)
            .fetch_optional(&state.db)
            .await?
    } else {
        None
    };
    let mut tries = 0;
    let outcome = loop {
        let mut tx = state.db.begin().await?;
        let r = core::totp_tx(&mut tx, &ctx, id, &secret, &code, &client).await;
        if let Some(v) = settle(tx, r, &mut tries).await? {
            break v;
        }
    };
    match outcome {
        core::TotpOutcome::Signed(collected) => {
            if let Some(user) = account {
                crate::auth::totp_guard::after(
                    &ctx,
                    Some(&state),
                    user,
                    true,
                    crate::auth::totp_guard::Used::default(),
                )
                .await;
            }
            Ok(session_response(&state, &headers, jar, collected))
        }
        core::TotpOutcome::WrongCode(events) => {
            if let Some(user) = account {
                crate::auth::totp_guard::after(
                    &ctx,
                    Some(&state),
                    user,
                    false,
                    crate::auth::totp_guard::Used {
                        totp_generation,
                        password: password_generation,
                    },
                )
                .await;
            }
            crate::security_events::spawn_delivery(&state, events);
            crate::security_events::notices::spawn_pending_delivery(&state);
            Err(AuthError::InvalidMfaCode)
        }
    }
}

/// The PC closed the screen.
#[utoipa::path(
    post,
    path = "/auth/login-attempts/{id}/cancel",
    tag = "auth",
    params(("id" = Uuid, Path, description = "Attempt id")),
    request_body = AttemptRequest,
    responses((status = 204), (status = 404, description = "LOGIN_ATTEMPT_UNKNOWN")),
)]
#[tracing::instrument(skip_all, fields(attempt_id = %id))]
pub async fn cancel(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
    body: Option<Json<AttemptRequest>>,
) -> AuthResult<(CookieJar, StatusCode)> {
    let body = body.map(|b| b.0).unwrap_or_default();
    let secret = presented(&headers, &jar, &state.config, &body)?;
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    core::cancel_tx(&mut tx, &ctx, id, &secret).await?;
    tx.commit().await?;
    let jar = if matches!(secret, CollectionSecret::Cookie(_)) {
        jar.add(attempt_cookie("", 0, &state.config))
    } else {
        jar
    };
    Ok((jar, StatusCode::NO_CONTENT))
}

/// Finish one try of a PC-side transition: commit a result; roll back a
/// `Retry` (the attempt's account changed while unlocked; at most 3 tries,
/// then 503); keep the expiry mark of a rule-4 failure.
pub async fn settle<T>(
    tx: sqlx::Transaction<'_, sqlx::Postgres>,
    r: AuthResult<Locked<T>>,
    tries: &mut u8,
) -> AuthResult<Option<T>> {
    match r {
        Ok(Locked::Done(v)) => {
            tx.commit().await?;
            Ok(Some(v))
        }
        Ok(Locked::Retry) => {
            tx.rollback().await?;
            *tries += 1;
            if *tries >= 3 {
                Err(AuthError::ServiceUnavailable)
            } else {
                Ok(None)
            }
        }
        Err(e @ AuthError::LoginAttemptEnded { .. }) => {
            tx.commit().await?;
            Err(e)
        }
        Err(e) => Err(e),
    }
}

/// The sign-in response: the refresh cookie for browsers, the refresh
/// token in the body for native clients; notices delivered after commit.
pub fn session_response(
    state: &AppState,
    headers: &HeaderMap,
    jar: CookieJar,
    collected: core::Collected,
) -> (CookieJar, Json<AuthResponse>) {
    crate::security_events::spawn_delivery(state, collected.event_ids.clone());
    let in_body = crate::auth::helpers::should_return_refresh_token(headers);
    let jar = jar
        .add(collected.issued.refresh_cookie(&state.config))
        .add(attempt_cookie("", 0, &state.config));
    (
        jar,
        Json(
            collected
                .issued
                .into_response(collected.password_change_required, in_body),
        ),
    )
}
