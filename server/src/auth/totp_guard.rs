//! The per-account TOTP counter on every TOTP check (B.5; review round 1).
//!
//! [`pre_check`] answers 429 `TOO_MANY_ATTEMPTS` while the counter's delay
//! runs; [`after`] records the outcome: a success resets the counter, every
//! 20th failure records a coalesced `guessing {factor: totp}`, the first at
//! or past the 100th in an epoch throttles TOTP and compromises the password generation that came
//! with it (F4). The effects run in their own transaction, spawned, because the
//! caller may hold the account lock in a transaction that rolls back on the
//! refusal; the spawned one waits for that lock.

use uuid::Uuid;

use super::ctx::AuthCtx;
use super::error::{AuthError, AuthResult};
use super::factors::{Factor, FactorRef, StateCause};
use crate::ratelimit::account::{AccountLimiter, CounterKey, NOTICE_EVERY};

/// 429 while the delay of the account's current TOTP generation runs
/// (fail closed on Valkey errors).
pub async fn pre_check(ctx: &AuthCtx<'_>, user: Uuid) -> AuthResult<()> {
    let generation = current_generation(ctx, user).await?;
    match AccountLimiter::new(ctx.redis)
        .check(&CounterKey::totp(user, generation))
        .await
    {
        Ok(Ok(())) => Ok(()),
        Ok(Err(d)) => {
            tracing::warn!(%user, retry_after = d.retry_after, "TOTP check delayed (B.5)");
            Err(AuthError::TooManyAttempts {
                retry_after: d.retry_after,
            })
        }
        Err(_) => Err(AuthError::ServiceUnavailable),
    }
}

/// What a TOTP check used.
///
/// The TOTP generation it checked against (always known: a success or
/// failure counts only on that generation's counter, Codex r4) and the
/// password generation that came with the code (compromised at the 100th
/// failure). The effects re-check both under the account lock.
#[derive(Debug, Clone, Copy)]
pub struct Used {
    pub totp_generation: i64,
    pub password: Option<FactorRef>,
}

/// Record the outcome of a TOTP check. With `state` the notices of the
/// effects are delivered at once (else by the minute sweep).
pub async fn after(
    ctx: &AuthCtx<'_>,
    state: Option<&crate::api::AppState>,
    user: Uuid,
    ok: bool,
    used: Used,
) {
    let limiter = AccountLimiter::new(ctx.redis);
    // Codex/Flash r3, Codex r4: the counter of the generation that was
    // checked, for a success too; a renewal's counter is a fresh one.
    let key = CounterKey::totp(user, used.totp_generation);
    if ok {
        let _ = limiter.reset_epoch(&key).await;
        return;
    }
    let Ok(failure) = limiter.count_failure(&key).await else {
        return;
    };
    let (n, epoch, throttle) = (failure.count, failure.epoch, failure.throttle);
    if !n.is_multiple_of(NOTICE_EVERY) && !throttle {
        return;
    }
    let db = ctx.db.clone();
    let redis = ctx.redis.clone();
    let config = ctx.config.clone();
    let state = state.cloned();
    tokio::spawn(async move {
        let ctx = AuthCtx {
            db: &db,
            redis: &redis,
            config: &config,
        };
        let result = effects(&ctx, user, n, epoch, throttle, used).await;
        if throttle {
            // Flash r5 / Codex r6: this claim is settled, never another one:
            // done after a commit, given back (the next failure throttles)
            // after an error.
            let limiter = AccountLimiter::new(&redis);
            let key = CounterKey::totp(user, used.totp_generation);
            let _ = if result.is_ok() {
                limiter.confirm_throttle(&key, &failure).await
            } else {
                limiter.release_throttle(&key, &failure).await
            };
        }
        match result {
            Ok(()) => {
                if let Some(state) = state {
                    // Flash r2: the guessing / throttle notices now.
                    crate::security_events::notices::spawn_pending_delivery(&state);
                }
            }
            Err(e) => tracing::warn!(error = %e, %user, "TOTP guessing effects failed"),
        }
    });
}

async fn effects(
    ctx: &AuthCtx<'_>,
    user: Uuid,
    n: u32,
    epoch: i64,
    throttle: bool,
    used: Used,
) -> AuthResult<()> {
    use crate::security_events::{record, Actor, EventType, NewSecurityEvent};
    let mut tx = ctx.db.begin().await?;
    crate::approvals::transition::lock_accounts(&mut tx, &[user]).await?;
    // Codex r2: under the lock, the failures still count (no success reset
    // them since) and the TOTP they were made against is still current;
    // else nothing of this applies to the account's present TOTP.
    // Codex r3: the same epoch (no success since) and the count still there.
    let generation = used.totp_generation;
    // An unreadable counter is an error (the claim is given back).
    let now = AccountLimiter::new(ctx.redis)
        .read(&CounterKey::totp(user, generation))
        .await
        .map_err(|_| AuthError::ServiceUnavailable)?;
    if !(now.1 == epoch && now.0 >= i64::from(n)) {
        tx.commit().await?;
        return Ok(());
    }
    let st = super::factors::states(&mut tx, user).await?;
    if generation != st.totp.generation {
        tx.commit().await?;
        return Ok(());
    }
    let password = used.password;
    if n.is_multiple_of(NOTICE_EVERY) {
        record(
            &mut tx,
            NewSecurityEvent::new(
                user,
                EventType::Guessing,
                Actor::System,
                serde_json::json!({"factor": "totp", "count": n}),
            ),
        )
        .await?;
    }
    // The first failure at or past the 100th in this epoch (Flash r5).
    if throttle {
        tracing::warn!(%user, "TOTP throttled after repeated failures (B.5)");
        super::factors::throttle_tx(&mut tx, ctx, user, Factor::Totp).await?;
        if let Some(p) = password.filter(|p| p.factor == Factor::Password) {
            super::factors::mark_compromised_tx(
                &mut tx,
                ctx,
                user,
                p,
                StateCause::Guessing,
                Actor::System,
            )
            .await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

/// [`pre_check`], the replay-checked verification and [`after`] in one.
pub async fn verify(
    ctx: &AuthCtx<'_>,
    user: Uuid,
    encrypted_secret: &str,
    username: &str,
    code: &str,
    totp_generation: i64,
) -> AuthResult<bool> {
    pre_check(ctx, user).await?;
    let ok = super::reauth::accept_totp_ctx(ctx, user, encrypted_secret, username, code).await?;
    after(
        ctx,
        None,
        user,
        ok,
        Used {
            totp_generation,
            password: None,
        },
    )
    .await;
    Ok(ok)
}

async fn current_generation(ctx: &AuthCtx<'_>, user: Uuid) -> AuthResult<i64> {
    Ok(
        sqlx::query_scalar("SELECT totp_generation FROM users WHERE id = $1")
            .bind(user)
            .fetch_optional(ctx.db)
            .await?
            .unwrap_or(1),
    )
}
