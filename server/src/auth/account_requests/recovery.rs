//! Self-service recovery (F6) and the fresh-OIDC request factor.
//!
//! Order (B.5): the start budget (3 `self` starts per account per delay
//! period, checked before any code is consumed) → the recovery code (an
//! invalid one answers like an unknown user, with the same work, and counts
//! nothing per account) → the valid code is consumed and the start counted →
//! the second factor (password `active`, TOTP in any state, or a fresh OIDC
//! sign-in); a failed second factor → 401 `REQUEST_FACTOR_INVALID
//! {recovery_codes_left}`.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::core::{self, NewRequest, Purpose, RequestKind, StartedRequest};
use super::types::{StartFactor, StartRequest};
use crate::api::AppState;
use crate::auth::ctx::AuthCtx;
use crate::auth::error::{AuthError, AuthResult};
use crate::auth::factors::{self, Factor, FactorRef, FactorState};

/// `self` starts per account per delay period (B.5).
pub const STARTS_PER_PERIOD: i64 = 3;

/// A fresh OIDC sign-in usable as a request factor.
#[derive(Debug, Clone)]
pub struct OidcFactor {
    pub user_id: Uuid,
    pub provider_slug: String,
    pub auth_time: DateTime<Utc>,
}

/// Start a recovery request (F6).
pub async fn start_recovery(
    state: &AppState,
    ctx: &AuthCtx<'_>,
    body: &StartRequest,
) -> AuthResult<StartedRequest> {
    let neutral = || AuthError::InvalidCredentials;
    let code = body.recovery_code.as_deref().unwrap_or("");
    // The request's shape first: no answer depends on the account (B.5).
    let lost_all = match body.lost.as_deref() {
        None | Some("none") => false,
        Some("all_key_devices") => true,
        Some(_) => {
            return Err(AuthError::Validation(
                "lost: none or all_key_devices".into(),
            ))
        }
    };
    let user = crate::db::find_user_by_username(&state.db, &body.username)
        .await?
        .filter(|u| !u.is_system);
    let Some(user) = user else {
        crate::auth::recovery_codes::dummy_check(code).await?;
        return Err(neutral());
    };
    // A banned account starts nothing (the neutral answer and work).
    if crate::auth::queries::is_user_globally_banned(&state.db, user.id).await? {
        crate::auth::recovery_codes::dummy_check(code).await?;
        return Err(neutral());
    }
    // The code first, without any lock and with constant work (Fable r2
    // M-A); an invalid one is the neutral answer for every account (Codex
    // r2 M6).
    let found = {
        let mut conn = state.db.acquire().await?;
        crate::auth::recovery_codes::find(&mut conn, user.id, code).await?
    };
    let Some((code_id, code_generation)) = found else {
        return Err(neutral());
    };
    // The second factor's counter waits before the code is consumed (B.5).
    match &body.second {
        Some(StartFactor::Totp { .. }) => crate::auth::totp_guard::pre_check(ctx, user.id).await?,
        Some(StartFactor::Password { .. }) => {
            match crate::ratelimit::account::AccountLimiter::new(ctx.redis)
                .check(&crate::ratelimit::account::CounterKey::password(
                    &user.username,
                ))
                .await
            {
                Ok(Ok(())) => {}
                Ok(Err(d)) => {
                    return Err(AuthError::TooManyAttempts {
                        retry_after: d.retry_after,
                    })
                }
                Err(_) => return Err(AuthError::ServiceUnavailable),
            }
        }
        _ => {}
    }
    let mut tx = state.db.begin().await?;
    // The subject with every possible canceller (Codex M7).
    crate::approvals::transition::lock_for_start(&mut tx, user.id, &[])
        .await
        .map_err(|e| match e {
            crate::approvals::transition::TransitionError::Db(d) => AuthError::Database(d),
            _ => AuthError::ServiceUnavailable,
        })?;
    // The credentials under the lock (Codex B2).
    let (hash_now, secret_now): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT password_hash, mfa_secret FROM users WHERE id = $1")
            .bind(user.id)
            .fetch_one(&mut *tx)
            .await?;
    // 1. The start budget, before the code is consumed.
    let starts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM account_request_starts
          WHERE user_id = $1 AND started_at > NOW() - make_interval(hours => $2)",
    )
    .bind(user.id)
    .bind(i32::try_from(ctx.config.account_request_delay_hours).unwrap_or(24))
    .fetch_one(&mut *tx)
    .await?;
    if starts >= STARTS_PER_PERIOD {
        tracing::warn!(user_id = %user.id, "recovery start budget exhausted (B.5)");
        return Err(neutral());
    }
    // 2. The code, consumed by id while unused and current.
    if !crate::auth::recovery_codes::consume_id_tx(&mut tx, user.id, code_id, code_generation)
        .await?
    {
        return Err(neutral());
    }
    sqlx::query("INSERT INTO account_request_starts (user_id) VALUES ($1)")
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    // 3. The second factor.
    let st = factors::states(&mut tx, user.id).await?;
    let mut used = vec![FactorRef {
        factor: Factor::RecoveryCodes,
        generation: st.recovery_codes_generation,
    }];
    let mut oidc = false;
    let ok = match &body.second {
        Some(StartFactor::Password { password }) => {
            let hash = st.has_password.then(|| hash_now.clone()).flatten();
            let valid = password_ok(hash, password).await?;
            let key = crate::ratelimit::account::CounterKey::password(&user.username);
            if valid {
                let _ = crate::ratelimit::account::AccountLimiter::new(ctx.redis)
                    .record_success(&key)
                    .await;
            } else {
                count_failure(state, &key, user.id);
            }
            used.push(FactorRef {
                factor: Factor::Password,
                generation: st.password.generation,
            });
            valid && st.password.state == FactorState::Active
        }
        Some(StartFactor::Totp { code }) => match &secret_now {
            // Any state (B.1: a throttled or compromised TOTP still proves
            // possession for a delayed request); the replay store applies.
            Some(secret) => {
                let valid = crate::auth::reauth::accept_totp_ctx(
                    ctx,
                    user.id,
                    secret,
                    &user.username,
                    code.trim(),
                )
                .await?;
                crate::auth::totp_guard::after(ctx, user.id, valid, None).await;
                used.push(FactorRef {
                    factor: Factor::Totp,
                    generation: st.totp.generation,
                });
                valid
            }
            None => false,
        },
        Some(StartFactor::Oidc {
            oidc_code,
            verifier,
        }) => {
            oidc = true;
            redeem_oidc_code(ctx, oidc_code, verifier.as_deref())
                .await
                .is_ok_and(|f| f.user_id == user.id)
        }
        None => false,
    };
    if !ok {
        let left = crate::auth::recovery_codes::left_conn(&mut tx, user.id).await?;
        // The code stays consumed and the start counted.
        tx.commit().await?;
        return Err(AuthError::RequestFactorInvalid {
            recovery_codes_left: left,
        });
    }
    let lost_device_ids: Vec<Uuid> = if lost_all {
        sqlx::query_scalar(
            "SELECT id FROM approval_devices WHERE user_id = $1 AND state = 'active' ORDER BY id",
        )
        .bind(user.id)
        .fetch_all(&mut *tx)
        .await?
    } else {
        Vec::new()
    };
    let started = core::insert_tx(
        &mut tx,
        ctx,
        NewRequest {
            user_id: user.id,
            kind: RequestKind::SelfService,
            purpose: Purpose::Recovery,
            factors: used,
            oidc,
            lost_device_ids,
            started_by: None,
            started_by_device: None,
            enrollment_id: None,
            pending: None,
            with_secret: true,
        },
    );
    let started = match started.await {
        Ok(s) => s,
        Err(e @ (AuthError::AccountRequestOpen | AuthError::NoIndependentCanceller)) => {
            // The consumed code and the counted start stay.
            tx.commit().await?;
            return Err(e);
        }
        Err(e) => return Err(e),
    };
    tx.commit().await?;
    Ok(started)
}

async fn password_ok(hash: Option<String>, password: &str) -> AuthResult<bool> {
    let password = password.to_string();
    let hash = hash.unwrap_or_else(|| crate::auth::password::DUMMY_PASSWORD_HASH.clone());
    tokio::task::spawn_blocking(move || {
        crate::auth::verify_password(&password, &hash).unwrap_or(false)
    })
    .await
    .map_err(|e| AuthError::Internal(format!("password check failed: {e}")))
}

/// A failed password counts on its per-account counter with its 20/100
/// effects (Task 26; spawned: the caller holds the account lock).
fn count_failure(state: &AppState, key: &crate::ratelimit::account::CounterKey, user: Uuid) {
    let state = state.clone();
    let key = key.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::auth::login::account_failure(&state, &key, Some(user)).await {
            tracing::warn!(error = %e, "password failure effects failed");
        }
    });
}

/// A sign-in counts as fresh this long after its `auth_time` (C27).
pub const OIDC_FRESH_SECS: i64 = 300;

/// The Valkey record behind a one-time request `oidc_code` (60 s).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RequestOidcGrant {
    /// `None`: a stale or unlinked sign-in (the code redeems to nothing).
    pub user_id: Option<Uuid>,
    pub slug: String,
    pub auth_time: Option<DateTime<Utc>>,
    /// Native starts bind the code to this S256 challenge.
    pub client_challenge: Option<String>,
}

/// The Valkey key of a request `oidc_code` (only its hash is stored).
#[must_use]
pub fn request_code_key(code: &str) -> String {
    use sha2::Digest;
    format!(
        "oidc:request:{}",
        hex::encode(sha2::Sha256::digest(code.as_bytes()))
    )
}

/// Redeem a one-time request OIDC code (single use, also when it fails): a
/// native code needs the verifier of its `client_challenge`.
pub async fn redeem_oidc_code(
    ctx: &AuthCtx<'_>,
    code: &str,
    verifier: Option<&str>,
) -> AuthResult<OidcFactor> {
    use fred::interfaces::KeysInterface;
    let raw: Option<String> = ctx
        .redis
        .getdel(request_code_key(code))
        .await
        .map_err(|_| AuthError::ServiceUnavailable)?;
    let grant: RequestOidcGrant = raw
        .and_then(|r| serde_json::from_str(&r).ok())
        .ok_or(AuthError::InvalidCredentials)?;
    if let Some(challenge) = &grant.client_challenge {
        let ok =
            verifier.is_some_and(|v| crate::auth::oidc::client_challenge_matches(v, challenge));
        if !ok {
            return Err(AuthError::InvalidCredentials);
        }
    }
    match (grant.user_id, grant.auth_time) {
        (Some(user_id), Some(auth_time)) => Ok(OidcFactor {
            user_id,
            provider_slug: grant.slug,
            auth_time,
        }),
        _ => Err(AuthError::InvalidCredentials),
    }
}

/// `POST /auth/account-requests/oidc/authorize/{provider_id}` body (native
/// starts; a browser sends none).
#[derive(Debug, Default, serde::Deserialize, utoipa::ToSchema)]
pub struct OidcRequestAuthorize {
    #[serde(default)]
    pub loopback_port: Option<u16>,
    #[serde(default)]
    pub callback_path: Option<String>,
    #[serde(default)]
    pub client_challenge: Option<String>,
}

/// Start a fresh OIDC sign-in as a request factor → `{url}` (browser,
/// Android) or `{url, flow_id}` (desktop, as its sign-in start; C29 step 8).
#[utoipa::path(
    post,
    path = "/auth/account-requests/oidc/authorize/{provider_id}",
    tag = "auth",
    params(("provider_id" = String, Path, description = "Provider slug")),
    request_body = OidcRequestAuthorize,
    responses((status = 200, description = "`{url}` or `{url, flow_id}`"), (status = 400, description = "OIDC_CLIENT_OUTDATED or VALIDATION_ERROR")),
)]
#[tracing::instrument(skip_all, fields(provider = %provider))]
pub async fn oidc_request_authorize(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::extract::Path(provider): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
    body: Option<axum::Json<OidcRequestAuthorize>>,
) -> AuthResult<axum::response::Response> {
    use crate::auth::client_class::ClientClass;
    use crate::auth::oidc::NativeLogin;
    let body = body.map(|b| b.0).unwrap_or_default();
    let native = match ClientClass::from_headers(&headers) {
        ClientClass::Web => {
            if body.client_challenge.is_some() || body.loopback_port.is_some() {
                return Err(AuthError::OidcClientOutdated);
            }
            None
        }
        ClientClass::Desktop => {
            let (Some(port), Some(path), Some(_)) = (
                body.loopback_port,
                body.callback_path.as_deref(),
                body.client_challenge.as_deref(),
            ) else {
                return Err(AuthError::OidcClientOutdated);
            };
            if port < 1024
                || path.len() != 43
                || crate::approvals::secrets::b64u_decode(path).is_none()
            {
                return Err(AuthError::Validation(
                    "loopback_port or callback_path".into(),
                ));
            }
            Some(NativeLogin::Desktop {
                flow_id: Uuid::new_v4(),
                loopback: crate::auth::passkeys::desktop_page::LoopbackTarget {
                    port,
                    callback_path: path.to_string(),
                },
            })
        }
        ClientClass::Android => {
            if body.client_challenge.is_none() {
                return Err(AuthError::OidcClientOutdated);
            }
            Some(NativeLogin::Android)
        }
    };
    crate::auth::login::start_oidc_flow_with(
        &state,
        &provider,
        None,
        body.client_challenge.as_deref(),
        crate::auth::oidc::OidcFlowPurpose::RequestFactor,
        true,
        native,
    )
    .await
}
