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
    let user = crate::db::find_user_by_username(&state.db, &body.username)
        .await?
        .filter(|u| !u.is_system);
    let Some(user) = user else {
        crate::auth::recovery_codes::dummy_check(code).await?;
        return Err(neutral());
    };
    let lost_all = match body.lost.as_deref() {
        None | Some("none") => false,
        Some("all_key_devices") => true,
        Some(_) => {
            return Err(AuthError::Validation(
                "lost: none or all_key_devices".into(),
            ))
        }
    };
    let mut tx = state.db.begin().await?;
    crate::approvals::transition::lock_accounts(&mut tx, &[user.id]).await?;
    // 1. The start budget, before any code is consumed.
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
        crate::auth::recovery_codes::dummy_check(code).await?;
        return Err(neutral());
    }
    // 2. The recovery code (consumed when valid).
    if !crate::auth::recovery_codes::consume_tx(&mut tx, user.id, code).await? {
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
            let hash = st
                .has_password
                .then(|| user.password_hash.clone())
                .flatten();
            let valid = password_ok(hash, password).await?;
            if !valid {
                count_failure(
                    state,
                    &crate::ratelimit::account::CounterKey::password(&user.username),
                )
                .await;
            }
            used.push(FactorRef {
                factor: Factor::Password,
                generation: st.password.generation,
            });
            valid && st.password.state == FactorState::Active
        }
        Some(StartFactor::Totp { code }) => match &user.mfa_secret {
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
                if !valid {
                    count_failure(state, &crate::ratelimit::account::CounterKey::Totp(user.id))
                        .await;
                }
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

/// A failed second factor counts on its per-account counter (Task 26).
async fn count_failure(state: &AppState, key: &crate::ratelimit::account::CounterKey) {
    let _ = crate::ratelimit::account::AccountLimiter::new(&state.redis)
        .record_failure(key)
        .await;
}

/// Redeem a one-time request OIDC code. The OIDC request flow
/// (`/auth/account-requests/oidc/authorize/{provider}`) is not built yet:
/// every code is refused.
pub async fn redeem_oidc_code(
    _ctx: &AuthCtx<'_>,
    _code: &str,
    _verifier: Option<&str>,
) -> AuthResult<OidcFactor> {
    Err(AuthError::InvalidCredentials)
}
